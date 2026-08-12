#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

const READER_WINDOW_SIZE: usize = 96;
const BACKGROUND_RESULT_POLL_INTERVAL: Duration = Duration::from_millis(40);
const EPUB_IMAGE_MAX_WIDTH: u32 = 1_200;
const EPUB_IMAGE_MAX_HEIGHT: u32 = 1_200;
const LIBRARY_COVER_MAX_WIDTH: u32 = 360;
const LIBRARY_COVER_MAX_HEIGHT: u32 = 480;
const EPUB_DECODED_IMAGE_CACHE_BYTES: usize = 32 * 1024 * 1024;
const LIBRARY_COVER_CACHE_BYTES: usize = 24 * 1024 * 1024;

use readloom_core::{
    AppSettings, AppTheme, ChapterTitleStyle, DEFAULT_TXT_CHAPTER_PATTERN, EpubDocument,
    EpubImageResource, EpubReadingLocator, LibraryDocument, ParagraphKind, ReaderDocument,
    ReadingParagraph, ReadingSettings, ReadloomCore, SaveTextOptions, SearchHit, TextAlignment,
    TxtBlankLines, TxtLeadingIndent, WindowCloseAction,
};
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{
    CloseRequestResponse, ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer,
    SharedString, Timer, TimerMode, VecModel,
};

slint::include_modules!();

#[derive(Clone)]
enum OpenDocument {
    Txt(Arc<ReaderDocument>),
    Epub(Arc<EpubDocument>),
}

impl OpenDocument {
    fn path(&self) -> Option<&Path> {
        match self {
            Self::Txt(document) => document.path(),
            Self::Epub(document) => Some(document.path()),
        }
    }

    fn title(&self) -> &str {
        match self {
            Self::Txt(document) => document.title(),
            Self::Epub(document) => document.title(),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Txt(_) => "TXT",
            Self::Epub(_) => "EPUB",
        }
    }
}

struct DocumentLoadResult {
    request_id: u64,
    result: Result<(OpenDocument, usize), String>,
}

struct ChapterLoadResult {
    request_id: u64,
    document: Arc<EpubDocument>,
    paragraph_index: usize,
    result: Result<(), String>,
}

struct SearchTaskResult {
    request_id: u64,
    document_path: PathBuf,
    epub: bool,
    query: String,
    hits: Vec<SearchHit>,
}

struct CachedDecodedImage {
    image: Image,
    decoded_bytes: usize,
    last_used: u64,
}

struct DecodedImageCache {
    entries: HashMap<usize, CachedDecodedImage>,
    decoded_bytes: usize,
    maximum_bytes: usize,
    clock: u64,
}

impl DecodedImageCache {
    fn new(maximum_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            decoded_bytes: 0,
            maximum_bytes,
            clock: 0,
        }
    }

    fn get(&mut self, key: usize) -> Option<Image> {
        self.clock = self.clock.wrapping_add(1).max(1);
        let entry = self.entries.get_mut(&key)?;
        entry.last_used = self.clock;
        Some(entry.image.clone())
    }

    fn insert(&mut self, key: usize, image: Image) {
        if let Some(previous) = self.entries.remove(&key) {
            self.decoded_bytes = self.decoded_bytes.saturating_sub(previous.decoded_bytes);
        }
        self.clock = self.clock.wrapping_add(1).max(1);
        let decoded_bytes = decoded_image_bytes(&image);
        self.decoded_bytes = self.decoded_bytes.saturating_add(decoded_bytes);
        self.entries.insert(
            key,
            CachedDecodedImage {
                image,
                decoded_bytes,
                last_used: self.clock,
            },
        );
        while self.decoded_bytes > self.maximum_bytes {
            let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| *key)
            else {
                break;
            };
            let Some(removed) = self.entries.remove(&oldest_key) else {
                break;
            };
            self.decoded_bytes = self.decoded_bytes.saturating_sub(removed.decoded_bytes);
        }
    }
}

struct CachedLibraryCover {
    image: Image,
    decoded_bytes: usize,
    last_used: u64,
}

struct LibraryCoverCache {
    entries: HashMap<PathBuf, CachedLibraryCover>,
    decoded_bytes: usize,
    maximum_bytes: usize,
    clock: u64,
}

impl LibraryCoverCache {
    fn new(maximum_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            decoded_bytes: 0,
            maximum_bytes,
            clock: 0,
        }
    }

    fn get(&mut self, path: &Path) -> Option<Image> {
        self.clock = self.clock.wrapping_add(1).max(1);
        let entry = self.entries.get_mut(path)?;
        entry.last_used = self.clock;
        Some(entry.image.clone())
    }

    fn insert(&mut self, path: PathBuf, image: Image) {
        if let Some(previous) = self.entries.remove(&path) {
            self.decoded_bytes = self.decoded_bytes.saturating_sub(previous.decoded_bytes);
        }
        self.clock = self.clock.wrapping_add(1).max(1);
        let decoded_bytes = decoded_image_bytes(&image);
        self.decoded_bytes = self.decoded_bytes.saturating_add(decoded_bytes);
        self.entries.insert(
            path,
            CachedLibraryCover {
                image,
                decoded_bytes,
                last_used: self.clock,
            },
        );
        while self.decoded_bytes > self.maximum_bytes {
            let Some(oldest_path) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(path, _)| path.clone())
            else {
                break;
            };
            let Some(removed) = self.entries.remove(&oldest_path) else {
                break;
            };
            self.decoded_bytes = self.decoded_bytes.saturating_sub(removed.decoded_bytes);
        }
    }
}

thread_local! {
    static LIBRARY_COVER_CACHE: RefCell<LibraryCoverCache> =
        RefCell::new(LibraryCoverCache::new(LIBRARY_COVER_CACHE_BYTES));
}

struct ReaderParagraphModel {
    document: OpenDocument,
    epub_paragraphs: Option<Arc<Vec<ReadingParagraph>>>,
    epub_images: Option<Arc<Vec<EpubImageResource>>>,
    range: Range<usize>,
    decoded_images: RefCell<DecodedImageCache>,
}

impl ReaderParagraphModel {
    fn new(document: OpenDocument, range: Range<usize>) -> Self {
        let (epub_paragraphs, epub_images) = match &document {
            OpenDocument::Epub(document) => (Some(document.paragraphs()), Some(document.images())),
            OpenDocument::Txt(_) => (None, None),
        };
        Self {
            document,
            epub_paragraphs,
            epub_images,
            range,
            decoded_images: RefCell::new(DecodedImageCache::new(EPUB_DECODED_IMAGE_CACHE_BYTES)),
        }
    }

    fn paragraphs(&self) -> &[ReadingParagraph] {
        match &self.document {
            OpenDocument::Txt(document) => document.paragraphs(),
            OpenDocument::Epub(_) => self.epub_paragraphs.as_deref().map_or(&[], Vec::as_slice),
        }
    }

    fn window_paragraphs(&self) -> &[ReadingParagraph] {
        &self.paragraphs()[self.range.clone()]
    }

    fn paragraph_rows(&self) -> Vec<(usize, Option<usize>)> {
        let paragraphs = self.window_paragraphs();
        let mut rows = Vec::with_capacity(paragraphs.len().div_ceil(2));
        let mut index = 0;
        while index < paragraphs.len() {
            let first = &paragraphs[index];
            let second = (first.kind != ParagraphKind::Heading
                && first.kind != ParagraphKind::Blank)
                .then(|| index + 1)
                .filter(|next| {
                    paragraphs
                        .get(*next)
                        .is_some_and(|paragraph| paragraph.kind == ParagraphKind::Paragraph)
                });
            rows.push((index, second));
            index += if second.is_some() { 2 } else { 1 };
        }
        rows
    }

    fn paragraph_item(&self, paragraph: &ReadingParagraph) -> ParagraphItem {
        let image = match (&self.document, paragraph.image_index) {
            (OpenDocument::Epub(_), Some(image_index)) => {
                let cached = self.decoded_images.borrow_mut().get(image_index);
                if let Some(cached) = cached {
                    cached
                } else {
                    let image = self
                        .epub_images
                        .as_deref()
                        .and_then(|images| images.get(image_index))
                        .map(decode_epub_image)
                        .unwrap_or_default();
                    self.decoded_images
                        .borrow_mut()
                        .insert(image_index, image.clone());
                    image
                }
            }
            _ => Image::default(),
        };
        let has_image = image.size().width > 0;
        ParagraphItem {
            text: paragraph.text.clone().into(),
            kind: paragraph_kind_name(paragraph.kind),
            index: as_i32(paragraph.paragraph_index),
            chapter_index: as_i32(paragraph.chapter_index),
            image,
            has_image,
        }
    }
}

impl Model for ReaderParagraphModel {
    type Data = ParagraphItem;

    fn row_count(&self) -> usize {
        self.range.len()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        self.paragraphs()
            .get(self.range.start.checked_add(row)?)
            .map(|paragraph| self.paragraph_item(paragraph))
    }

    fn model_tracker(&self) -> &dyn slint::ModelTracker {
        &()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct ReaderParagraphRowModel {
    paragraphs: ModelRc<ParagraphItem>,
    rows: Vec<(usize, Option<usize>)>,
    window_start: usize,
}

impl ReaderParagraphRowModel {
    fn row_for_paragraph(&self, paragraph_index: usize) -> usize {
        self.rows
            .partition_point(|(first, _)| self.window_start + *first <= paragraph_index)
            .saturating_sub(1)
    }
}

impl Model for ReaderParagraphRowModel {
    type Data = ParagraphRow;

    fn row_count(&self) -> usize {
        self.rows.len()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        let (first_index, second_index) = *self.rows.get(row)?;
        let first = self.paragraphs.row_data(first_index)?;
        let second = second_index.and_then(|index| self.paragraphs.row_data(index));
        Some(ParagraphRow {
            first,
            second: second.clone().unwrap_or_default(),
            has_second: second.is_some(),
        })
    }

    fn model_tracker(&self) -> &dyn slint::ModelTracker {
        &()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui = MainWindow::new()?;
    let core = Arc::new(ReadloomCore::open(&state_database_path())?);
    let settings = Rc::new(RefCell::new(core.load_settings()?));
    let current_document = Rc::new(RefCell::new(None::<OpenDocument>));
    let open_documents = Rc::new(RefCell::new(Vec::<OpenDocument>::new()));
    let (document_load_sender, document_load_receiver) = mpsc::channel::<DocumentLoadResult>();
    let (chapter_load_sender, chapter_load_receiver) = mpsc::channel::<ChapterLoadResult>();
    let (search_sender, search_receiver) = mpsc::channel::<SearchTaskResult>();
    let active_load_request = Rc::new(Cell::new(0_u64));
    let active_chapter_request = Rc::new(Cell::new(0_u64));
    let active_search_request = Rc::new(Cell::new(0_u64));
    let document_load_timer = Timer::default();
    apply_settings(&ui, &settings.borrow());
    apply_background(&ui, &core);
    load_library(&ui, &core, settings.borrow().library_columns as usize)?;
    {
        let weak = ui.as_weak();
        let core = core.clone();
        let open_documents = open_documents.clone();
        let document_load_sender = document_load_sender.clone();
        let active_load_request = active_load_request.clone();
        ui.on_open_document(move |path: SharedString| {
            if let Some(ui) = weak.upgrade() {
                let requested_path = Path::new(path.as_str());
                let already_open = {
                    let documents = open_documents.borrow();
                    documents
                        .iter()
                        .any(|document| document_path_matches(document, requested_path))
                };
                if already_open {
                    ui.invoke_select_open_document(path);
                    return;
                }
                let display_name = Path::new(path.as_str())
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(path.as_str());
                ui.set_status_text(format!("正在后台解析《{display_name}》…").into());
            }
            let path = PathBuf::from(path.as_str());
            let request_id = active_load_request.get().wrapping_add(1).max(1);
            active_load_request.set(request_id);
            let sender = document_load_sender.clone();
            let core = core.clone();
            thread::spawn(move || {
                let result = load_document_for_open(&core, &path);
                let _ = sender.send(DocumentLoadResult { request_id, result });
            });
        });
    }

    {
        let weak = ui.as_weak();
        let current_document = current_document.clone();
        ui.on_request_reader_window(move |target| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let Some(document) = current_document.borrow().as_ref().cloned() else {
                return;
            };
            let target = target.max(0) as usize;
            let start = ui.get_reader_window_start().max(0) as usize;
            let end = start.saturating_add(ui.get_paragraphs().row_count());
            if target < start || target >= end {
                set_reader_models(&ui, document, target);
            }
            update_reader_target_row(&ui, target);
        });
    }

    {
        let weak = ui.as_weak();
        let current_document = current_document.clone();
        let chapter_load_sender = chapter_load_sender.clone();
        let active_chapter_request = active_chapter_request.clone();
        ui.on_open_epub_location(move |chapter_index, paragraph_index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let active = current_document.borrow().as_ref().cloned();
            let Some(OpenDocument::Epub(document)) = active else {
                return;
            };
            let chapter_index = chapter_index.max(0) as usize;
            let paragraph_index = paragraph_index.max(0) as usize;
            let request_id = active_chapter_request.get().wrapping_add(1).max(1);
            active_chapter_request.set(request_id);
            ui.set_status_text("正在后台加载章节…".into());
            let sender = chapter_load_sender.clone();
            thread::spawn(move || {
                let result = document
                    .load_chapter(chapter_index)
                    .map_err(|error| error.to_string());
                let _ = sender.send(ChapterLoadResult {
                    request_id,
                    document,
                    paragraph_index,
                    result,
                });
            });
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        let current_document = current_document.clone();
        let open_documents = open_documents.clone();
        let active_load_request = active_load_request.clone();
        let active_chapter_request = active_chapter_request.clone();
        let active_search_request = active_search_request.clone();
        document_load_timer.start(TimerMode::Repeated, BACKGROUND_RESULT_POLL_INTERVAL, move || {
            while let Ok(message) = document_load_receiver.try_recv() {
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                match message.result {
                    Ok((opened, initial_index)) => {
                        upsert_open_document(&open_documents, opened.clone());
                        let _ =
                            load_library(&ui, &core, ui.get_settings_library_columns() as usize);
                        if message.request_id == active_load_request.get() {
                            activate_open_document(
                                &ui,
                                &core,
                                &current_document,
                                &open_documents,
                                &opened,
                                initial_index,
                            );
                        } else {
                            let active_path = current_document
                                .borrow()
                                .as_ref()
                                .and_then(|document| document.path().map(Path::to_path_buf));
                            refresh_open_tabs(
                                &ui,
                                &open_documents.borrow(),
                                active_path.as_deref(),
                            );
                        }
                    }
                    Err(error) if message.request_id == active_load_request.get() => {
                        ui.set_status_text(error.into());
                    }
                    Err(_) => {}
                }
            }
            while let Ok(message) = chapter_load_receiver.try_recv() {
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                let active_matches = current_document.borrow().as_ref().is_some_and(|active| {
                    matches!(active, OpenDocument::Epub(document) if Arc::ptr_eq(document, &message.document))
                });
                if message.request_id != active_chapter_request.get() || !active_matches {
                    continue;
                }
                match message.result {
                    Ok(()) => {
                        let paragraph_index = message
                            .paragraph_index
                            .min(message.document.paragraphs().len().saturating_sub(1));
                        set_reader_models(
                            &ui,
                            OpenDocument::Epub(message.document.clone()),
                            paragraph_index,
                        );
                        let chapter_index = message.document.active_chapter_index();
                        ui.set_active_chapter_index(as_i32(chapter_index));
                        ui.set_active_chapter_title(
                            message
                                .document
                                .chapters()
                                .get(chapter_index)
                                .map_or("", |chapter| chapter.title.as_str())
                                .into(),
                        );
                        ui.set_active_paragraph_index(as_i32(paragraph_index));
                        ui.set_status_text(
                            format!(
                                "{} · 第 {} / {} 章 · 本章 {} 段",
                                message.document.title(),
                                chapter_index + 1,
                                message.document.chapters().len(),
                                message.document.paragraphs().len()
                            )
                            .into(),
                        );
                        navigate_after_layout(&ui, paragraph_index);
                    }
                    Err(error) => ui.set_status_text(error.into()),
                }
            }
            while let Ok(message) = search_receiver.try_recv() {
                if message.request_id != active_search_request.get() {
                    continue;
                }
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                let active_matches = current_document.borrow().as_ref().is_some_and(|active| {
                    active.path().is_some_and(|path| path == message.document_path)
                });
                if !active_matches {
                    continue;
                }
                let results = search_items(message.hits, message.epub);
                let count = results.len();
                ui.set_search_results(ModelRc::new(VecModel::from(results)));
                ui.set_status_text(
                    format!("“{}” · {} 个结果", message.query, count).into(),
                );
            }
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        let current_document = current_document.clone();
        let open_documents = open_documents.clone();
        ui.on_show_open_documents(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let first = open_documents.borrow().first().cloned();
            if let Some(document) = first {
                let index = load_initial_index(&core, &document);
                activate_open_document(
                    &ui,
                    &core,
                    &current_document,
                    &open_documents,
                    &document,
                    index,
                );
            } else {
                *current_document.borrow_mut() = None;
                show_empty_reader(&ui);
                refresh_open_tabs(&ui, &[], None);
            }
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        let current_document = current_document.clone();
        let open_documents = open_documents.clone();
        ui.on_select_open_document(move |path| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let selected = open_documents
                .borrow()
                .iter()
                .find(|document| document_path_matches(document, Path::new(path.as_str())))
                .cloned();
            let Some(document) = selected else {
                return;
            };
            let index = load_initial_index(&core, &document);
            activate_open_document(
                &ui,
                &core,
                &current_document,
                &open_documents,
                &document,
                index,
            );
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        let current_document = current_document.clone();
        let open_documents = open_documents.clone();
        ui.on_close_open_document(move |path| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let closing_path = Path::new(path.as_str());
            let closing_active = current_document
                .borrow()
                .as_ref()
                .is_some_and(|document| document_path_matches(document, closing_path));
            open_documents
                .borrow_mut()
                .retain(|document| !document_path_matches(document, closing_path));
            if closing_active {
                let next = open_documents.borrow().first().cloned();
                if let Some(document) = next {
                    let index = load_initial_index(&core, &document);
                    activate_open_document(
                        &ui,
                        &core,
                        &current_document,
                        &open_documents,
                        &document,
                        index,
                    );
                } else {
                    *current_document.borrow_mut() = None;
                    show_empty_reader(&ui);
                }
            }
            let active_path = current_document
                .borrow()
                .as_ref()
                .and_then(|document| document.path().map(Path::to_path_buf));
            refresh_open_tabs(&ui, &open_documents.borrow(), active_path.as_deref());
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        ui.on_create_library_group(move |name: SharedString| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            match core.create_library_group(name.as_str()) {
                Ok(group) => {
                    if let Err(error) =
                        load_library(&ui, &core, ui.get_settings_library_columns() as usize)
                    {
                        ui.set_status_text(error.to_string().into());
                    } else {
                        ui.set_status_text(format!("已创建分组“{}”。", group.name).into());
                    }
                }
                Err(error) => ui.set_status_text(error.to_string().into()),
            }
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        ui.on_move_library_book(move |path, group_id, group_name| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let target = (group_id.as_str() != "ungrouped").then_some(group_id.as_str());
            match core.move_library_book(Path::new(path.as_str()), target) {
                Ok(true) => {
                    if let Err(error) =
                        load_library(&ui, &core, ui.get_settings_library_columns() as usize)
                    {
                        ui.set_status_text(error.to_string().into());
                    } else {
                        ui.set_status_text(format!("已移到“{}”。", group_name).into());
                    }
                }
                Ok(false) => ui.set_status_text("该书已不在书库中。".into()),
                Err(error) => ui.set_status_text(error.to_string().into()),
            }
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        ui.on_move_library_group(move |group_id, direction| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            match core.move_library_group(group_id.as_str(), direction) {
                Ok(true) => {
                    if let Err(error) =
                        load_library(&ui, &core, ui.get_settings_library_columns() as usize)
                    {
                        ui.set_status_text(error.to_string().into());
                    } else {
                        ui.set_status_text("分组位置已更新。".into());
                    }
                }
                Ok(false) => ui.set_status_text("该分组已经位于边界。".into()),
                Err(error) => ui.set_status_text(error.to_string().into()),
            }
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        ui.on_import_library_files(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let Some(paths) = rfd::FileDialog::new()
                .add_filter("Readloom 图书", &["txt", "epub"])
                .pick_files()
            else {
                return;
            };
            let (imported, failed) = import_library_documents(&core, paths);
            let _ = load_library(&ui, &core, ui.get_settings_library_columns() as usize);
            ui.set_status_text(format!("导入完成 · 成功 {imported} 本，失败 {failed} 本").into());
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        ui.on_import_library_directory(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let Some(directory) = rfd::FileDialog::new().pick_folder() else {
                return;
            };
            match collect_supported_documents(&directory, 2_000) {
                Ok(paths) => {
                    let scanned = paths.len();
                    let (imported, failed) = import_library_documents(&core, paths);
                    let _ = load_library(&ui, &core, ui.get_settings_library_columns() as usize);
                    ui.set_status_text(
                        format!(
                            "目录扫描完成 · 发现 {scanned} 本，成功 {imported} 本，失败 {failed} 本"
                        )
                        .into(),
                    );
                }
                Err(error) => ui.set_status_text(error.to_string().into()),
            }
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        ui.on_clean_invalid_library_books(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            match core.clean_invalid_library_entries() {
                Ok(count) => {
                    let _ = load_library(&ui, &core, ui.get_settings_library_columns() as usize);
                    ui.set_status_text(format!("已清理 {count} 条无效书库记录。").into());
                }
                Err(error) => ui.set_status_text(error.to_string().into()),
            }
        });
    }

    {
        let weak = ui.as_weak();
        let current_document = current_document.clone();
        let search_sender = search_sender.clone();
        let active_search_request = active_search_request.clone();
        ui.on_search_text(move |query: SharedString| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let query = query.trim().to_owned();
            if query.is_empty() {
                active_search_request.set(active_search_request.get().wrapping_add(1).max(1));
                ui.set_search_results(empty_model());
                ui.set_status_text("请输入搜索文字。".into());
                return;
            }
            let Some(document) = current_document.borrow().as_ref().cloned() else {
                return;
            };
            let Some(document_path) = document.path().map(Path::to_path_buf) else {
                return;
            };
            let request_id = active_search_request.get().wrapping_add(1).max(1);
            active_search_request.set(request_id);
            ui.set_status_text(format!("正在后台搜索“{query}”…").into());
            let sender = search_sender.clone();
            thread::spawn(move || {
                let epub = matches!(document, OpenDocument::Epub(_));
                let hits = match &document {
                    OpenDocument::Txt(document) => document.search(&query, 500),
                    OpenDocument::Epub(document) => document.search(&query, 500),
                };
                let _ = sender.send(SearchTaskResult {
                    request_id,
                    document_path,
                    epub,
                    query,
                    hits,
                });
            });
        });
    }

    {
        let weak = ui.as_weak();
        ui.on_back_to_library(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_reader_open(false);
                ui.set_settings_open(false);
                ui.set_edit_content("".into());
                ui.set_edit_mode(false);
                ui.set_search_results(empty_model());
                ui.set_status_text(
                    format!("已连接本地书库 · {} 本", ui.get_library_book_count()).into(),
                );
                ui.window().request_redraw();
            }
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        let current_document = current_document.clone();
        let pending_position = Rc::new(RefCell::new(None::<(OpenDocument, usize)>));
        let save_timer = Rc::new(Timer::default());
        ui.on_reading_position_changed(move |index| {
            if index < 0 {
                return;
            }
            if let Some(ui) = weak.upgrade()
                && let Some(document) = current_document.borrow().as_ref()
            {
                update_active_chapter(&ui, document, index as usize);
            }
            let active_document = current_document.borrow().as_ref().cloned();
            let Some(active_document) = active_document else {
                return;
            };
            *pending_position.borrow_mut() = Some((active_document, index as usize));
            let core = core.clone();
            let pending_position_for_timer = pending_position.clone();
            save_timer.start(
                TimerMode::SingleShot,
                Duration::from_millis(350),
                move || {
                    let Some((document, index)) = pending_position_for_timer.borrow_mut().take()
                    else {
                        return;
                    };
                    match &document {
                        OpenDocument::Txt(document) => {
                            let locator = document.locator_for_paragraph(index, 0);
                            let _ = core.save_text_locator(document, &locator);
                        }
                        OpenDocument::Epub(document) => {
                            let locator = document.locator_for_paragraph(index);
                            let _ = core.save_epub_locator(document, &locator);
                        }
                    }
                },
            );
        });
    }

    {
        let weak = ui.as_weak();
        let current_document = current_document.clone();
        ui.on_previous_chapter(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let active = current_document.borrow().as_ref().cloned();
            match active {
                Some(OpenDocument::Epub(document)) => {
                    if let Some(chapter) = document.active_chapter_index().checked_sub(1) {
                        ui.invoke_open_epub_location(as_i32(chapter), 0);
                    } else {
                        ui.set_status_text("已经是第一章。".into());
                    }
                }
                Some(document) => {
                    if let Some(target) =
                        chapter_target(&document, ui.get_active_paragraph_index(), -1)
                    {
                        navigate_after_layout(&ui, target);
                    } else {
                        ui.set_status_text("已经是第一章。".into());
                    }
                }
                None => {}
            }
        });
    }

    {
        let weak = ui.as_weak();
        let current_document = current_document.clone();
        ui.on_next_chapter(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let active = current_document.borrow().as_ref().cloned();
            match active {
                Some(OpenDocument::Epub(document)) => {
                    let chapter = document.active_chapter_index().saturating_add(1);
                    if chapter < document.chapters().len() {
                        ui.invoke_open_epub_location(as_i32(chapter), 0);
                    } else {
                        ui.set_status_text("已经是最后一章。".into());
                    }
                }
                Some(document) => {
                    if let Some(target) =
                        chapter_target(&document, ui.get_active_paragraph_index(), 1)
                    {
                        navigate_after_layout(&ui, target);
                    } else {
                        ui.set_status_text("已经是最后一章。".into());
                    }
                }
                None => {}
            }
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        let current_document = current_document.clone();
        ui.on_add_bookmark(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let index = ui.get_active_paragraph_index().max(0) as usize;
            let active = current_document.borrow().as_ref().cloned();
            let result = match active.as_ref() {
                Some(OpenDocument::Txt(document)) => core.add_text_bookmark(document, index),
                Some(OpenDocument::Epub(document)) => core.add_epub_bookmark(document, index),
                None => return,
            };
            match result {
                Ok(()) => {
                    if let Some(document) = active.as_ref() {
                        load_bookmarks(&ui, &core, document);
                    }
                    ui.set_status_text("书签已添加，并显示在右侧书签列表中。".into());
                }
                Err(error) => ui.set_status_text(error.to_string().into()),
            }
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        let current_document = current_document.clone();
        ui.on_delete_bookmark(move |bookmark_id| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            match core.delete_bookmark(bookmark_id.as_str()) {
                Ok(true) => {
                    if let Some(document) = current_document.borrow().as_ref() {
                        load_bookmarks(&ui, &core, document);
                    }
                    ui.set_status_text("书签已删除。".into());
                }
                Ok(false) => ui.set_status_text("该书签已经不存在。".into()),
                Err(error) => ui.set_status_text(error.to_string().into()),
            }
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        ui.on_refresh_library(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            match load_library(&ui, &core, ui.get_settings_library_columns() as usize) {
                Ok(()) => ui.set_status_text(
                    format!("书库已刷新 · {} 本", ui.get_library_book_count()).into(),
                ),
                Err(error) => ui.set_status_text(error.to_string().into()),
            }
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        let refresh_timer = Rc::new(Timer::default());
        ui.on_refresh_library_view(move || {
            let weak = weak.clone();
            let core = core.clone();
            refresh_timer.start(
                TimerMode::SingleShot,
                Duration::from_millis(80),
                move || {
                    let Some(ui) = weak.upgrade() else {
                        return;
                    };
                    if let Err(error) =
                        load_library(&ui, &core, ui.get_settings_library_columns() as usize)
                    {
                        ui.set_status_text(error.to_string().into());
                    }
                },
            );
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        ui.on_remove_library_book(move |path: SharedString, title: SharedString| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            match core.remove_from_library(Path::new(path.as_str())) {
                Ok(true) => {
                    if let Err(error) =
                        load_library(&ui, &core, ui.get_settings_library_columns() as usize)
                    {
                        ui.set_status_text(error.to_string().into());
                    } else {
                        ui.set_status_text(
                            format!("已移除《{}》及其本地阅读数据，原文件未删除。", title).into(),
                        );
                    }
                }
                Ok(false) => ui.set_status_text("该书已不在书库中。".into()),
                Err(error) => ui.set_status_text(error.to_string().into()),
            }
        });
    }

    {
        let weak = ui.as_weak();
        ui.on_open_file(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Readloom 图书", &["txt", "epub"])
                .pick_file()
            {
                ui.invoke_open_document(path.to_string_lossy().into_owned().into());
            }
        });
    }

    {
        let weak = ui.as_weak();
        let current_document = current_document.clone();
        ui.on_request_edit_mode(move |enabled| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let edit_content = match current_document.borrow().as_ref() {
                Some(OpenDocument::Txt(document)) if enabled => document.content().into(),
                Some(OpenDocument::Txt(_)) => SharedString::default(),
                _ => return,
            };
            ui.set_edit_content(edit_content);
            ui.set_edit_mode(enabled);
            ui.set_status_text(
                if enabled {
                    "TXT 编辑模式 · 保存时会保留原编码与主换行格式。"
                } else {
                    "已取消本次编辑。"
                }
                .into(),
            );
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        let current_document = current_document.clone();
        let open_documents = open_documents.clone();
        ui.on_save_edited_text(move |content: SharedString| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let saved = {
                let document = current_document.borrow();
                match document.as_ref() {
                    Some(OpenDocument::Txt(document)) => {
                        core.save_txt(document, content.as_str(), SaveTextOptions::PRESERVE)
                    }
                    _ => return,
                }
            };
            match saved {
                Ok(document) => {
                    let opened = OpenDocument::Txt(Arc::new(document));
                    let index = ui.get_active_paragraph_index().max(0) as usize;
                    upsert_open_document(&open_documents, opened.clone());
                    activate_open_document(
                        &ui,
                        &core,
                        &current_document,
                        &open_documents,
                        &opened,
                        index,
                    );
                    ui.set_status_text("TXT 已安全保存，并重新校验编码与文件指纹。".into());
                }
                Err(error) => ui.set_status_text(error.to_string().into()),
            }
        });
    }

    {
        let weak = ui.as_weak();
        let core = core.clone();
        let current_document = current_document.clone();
        let open_documents = open_documents.clone();
        ui.on_save_edited_text_as(move |content: SharedString| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let suggested_name = current_document
                .borrow()
                .as_ref()
                .and_then(|document| match document {
                    OpenDocument::Txt(document) => document
                        .path()
                        .and_then(|path| path.file_name())
                        .and_then(|name| name.to_str())
                        .map(str::to_owned),
                    OpenDocument::Epub(_) => None,
                })
                .unwrap_or_else(|| "未命名.txt".to_owned());
            let Some(path) = rfd::FileDialog::new()
                .add_filter("TXT 文本", &["txt"])
                .set_file_name(suggested_name)
                .save_file()
            else {
                return;
            };
            let previous_path = current_document
                .borrow()
                .as_ref()
                .and_then(|document| document.path().map(Path::to_path_buf));
            let saved = {
                let document = current_document.borrow();
                match document.as_ref() {
                    Some(OpenDocument::Txt(document)) => core.save_txt_as(
                        document,
                        &path,
                        content.as_str(),
                        SaveTextOptions::PRESERVE,
                    ),
                    _ => return,
                }
            };
            match saved {
                Ok(document) => {
                    if let Some(previous_path) = previous_path.as_deref() {
                        open_documents
                            .borrow_mut()
                            .retain(|open| !document_path_matches(open, previous_path));
                    }
                    let opened = OpenDocument::Txt(Arc::new(document));
                    upsert_open_document(&open_documents, opened.clone());
                    activate_open_document(
                        &ui,
                        &core,
                        &current_document,
                        &open_documents,
                        &opened,
                        0,
                    );
                    ui.set_status_text("TXT 已另存并切换到新文件。".into());
                    let _ = load_library(&ui, &core, ui.get_settings_library_columns() as usize);
                }
                Err(error) => ui.set_status_text(error.to_string().into()),
            }
        });
    }

    install_settings_handlers(&ui, &core, &settings, &current_document, &open_documents);
    install_window_behavior(&ui, &settings, &core, &current_document);
    let (_tray_icon, _tray_timer) = match install_tray(&ui) {
        Ok(value) => value,
        Err(error) => {
            ui.set_status_text(format!("系统托盘初始化失败：{error}").into());
            (None, Timer::default())
        }
    };

    ui.show()?;
    install_window_icon(&ui)?;
    ui.run()?;
    Ok(())
}

fn install_window_behavior(
    ui: &MainWindow,
    settings: &Rc<RefCell<AppSettings>>,
    core: &Arc<ReadloomCore>,
    current_document: &Rc<RefCell<Option<OpenDocument>>>,
) {
    {
        let weak = ui.as_weak();
        let settings = settings.clone();
        ui.window().on_close_requested(move || {
            if settings.borrow().close_action == WindowCloseAction::Tray {
                if let Some(ui) = weak.upgrade() {
                    let _ = ui.hide();
                    ui.set_status_text("窗口已隐藏到系统托盘。".into());
                }
                CloseRequestResponse::KeepWindowShown
            } else {
                let _ = slint::quit_event_loop();
                CloseRequestResponse::HideWindow
            }
        });
    }
    {
        let weak = ui.as_weak();
        let settings = settings.clone();
        let core = core.clone();
        let current_document = current_document.clone();
        let modifiers = Rc::new(Cell::new(winit::keyboard::ModifiersState::default()));
        let event_modifiers = modifiers.clone();
        ui.window().on_winit_window_event(move |window, event| {
            if let winit::event::WindowEvent::ModifiersChanged(value) = event {
                event_modifiers.set(value.state());
            }
            if settings.borrow().minimize_to_tray
                && matches!(event, winit::event::WindowEvent::Resized(size) if size.width == 0 || size.height == 0)
            {
                let _ = window.hide();
                EventResult::PreventDefault
            } else if let winit::event::WindowEvent::KeyboardInput { event, is_synthetic, .. } = event
                && !is_synthetic
                && event.state == winit::event::ElementState::Pressed
                && let Some(shortcut) = shortcut_from_key_event(&event.logical_key, modifiers.get())
                && let Some(ui) = weak.upgrade()
                && dispatch_shortcut(&ui, &core, &current_document, &settings.borrow(), &shortcut)
            {
                EventResult::PreventDefault
            } else {
                EventResult::Propagate
            }
        });
    }
}

fn install_window_icon(ui: &MainWindow) -> Result<(), Box<dyn std::error::Error>> {
    let size = 64_u32;
    let icon = winit::window::Icon::from_rgba(application_icon_rgba(size), size, size)?;
    ui.window()
        .with_winit_window(move |window| window.set_window_icon(Some(icon)));
    Ok(())
}

fn shortcut_from_key_event(
    key: &winit::keyboard::Key,
    modifiers: winit::keyboard::ModifiersState,
) -> Option<String> {
    use winit::keyboard::{Key, NamedKey};
    let key = match key {
        Key::Character(value) => value.to_uppercase(),
        Key::Named(NamedKey::F1) => "F1".to_owned(),
        Key::Named(NamedKey::F2) => "F2".to_owned(),
        Key::Named(NamedKey::F3) => "F3".to_owned(),
        Key::Named(NamedKey::F4) => "F4".to_owned(),
        Key::Named(NamedKey::F5) => "F5".to_owned(),
        Key::Named(NamedKey::F6) => "F6".to_owned(),
        Key::Named(NamedKey::F7) => "F7".to_owned(),
        Key::Named(NamedKey::F8) => "F8".to_owned(),
        Key::Named(NamedKey::F9) => "F9".to_owned(),
        Key::Named(NamedKey::F10) => "F10".to_owned(),
        Key::Named(NamedKey::F11) => "F11".to_owned(),
        Key::Named(NamedKey::F12) => "F12".to_owned(),
        Key::Named(NamedKey::ArrowLeft) => "Left".to_owned(),
        Key::Named(NamedKey::ArrowRight) => "Right".to_owned(),
        Key::Named(NamedKey::ArrowUp) => "Up".to_owned(),
        Key::Named(NamedKey::ArrowDown) => "Down".to_owned(),
        Key::Named(NamedKey::PageUp) => "PageUp".to_owned(),
        Key::Named(NamedKey::PageDown) => "PageDown".to_owned(),
        Key::Named(NamedKey::Home) => "Home".to_owned(),
        Key::Named(NamedKey::End) => "End".to_owned(),
        Key::Named(NamedKey::Space) => "Space".to_owned(),
        _ => return None,
    };
    let mut parts = Vec::new();
    if modifiers.control_key() {
        parts.push("Ctrl".to_owned());
    }
    if modifiers.alt_key() {
        parts.push("Alt".to_owned());
    }
    if modifiers.shift_key() {
        parts.push("Shift".to_owned());
    }
    if modifiers.super_key() {
        parts.push("Meta".to_owned());
    }
    parts.push(key);
    Some(parts.join("+"))
}

fn dispatch_shortcut(
    ui: &MainWindow,
    core: &ReadloomCore,
    current_document: &RefCell<Option<OpenDocument>>,
    settings: &AppSettings,
    shortcut: &str,
) -> bool {
    let action = readloom_core::ShortcutSettings::ACTIONS
        .into_iter()
        .find_map(|(action, _)| {
            settings
                .shortcuts
                .get(action)
                .filter(|configured| {
                    !configured.is_empty() && configured.eq_ignore_ascii_case(shortcut)
                })
                .map(|_| action)
        });
    let Some(action) = action else {
        return false;
    };
    match action {
        "open" => ui.invoke_open_file(),
        "save" if ui.get_edit_mode() => ui.invoke_save_edited_text(ui.get_edit_content()),
        "saveAs" if ui.get_edit_mode() => ui.invoke_save_edited_text_as(ui.get_edit_content()),
        "close" => ui.invoke_back_to_library(),
        "toggleEdit" if ui.get_reader_open() && ui.get_document_kind() == "TXT" => {
            ui.invoke_request_edit_mode(!ui.get_edit_mode());
        }
        "previousChapter" => navigate_chapter(ui, false),
        "nextChapter" => navigate_chapter(ui, true),
        "bookmark" => {
            let index = ui.get_active_paragraph_index().max(0) as usize;
            let result = match current_document.borrow().as_ref() {
                Some(OpenDocument::Txt(document)) => core.add_text_bookmark(document, index),
                Some(OpenDocument::Epub(document)) => core.add_epub_bookmark(document, index),
                None => return true,
            };
            match result {
                Ok(()) => ui.set_status_text("书签已添加到当前位置。".into()),
                Err(error) => ui.set_status_text(error.to_string().into()),
            }
        }
        "showLibrary" => ui.invoke_back_to_library(),
        "showSettings" => {
            ui.set_settings_open(true);
            ui.set_settings_section("reading.typography".into());
            ui.set_edit_content("".into());
            ui.set_edit_mode(false);
        }
        _ => ui.set_status_text("当前状态无法执行该快捷键。".into()),
    }
    true
}

fn navigate_chapter(ui: &MainWindow, forward: bool) {
    if forward {
        ui.invoke_next_chapter();
    } else {
        ui.invoke_previous_chapter();
    }
}

fn install_tray(
    ui: &MainWindow,
) -> Result<(Option<tray_icon::TrayIcon>, Timer), Box<dyn std::error::Error>> {
    let size = 32_u32;
    let rgba = application_icon_rgba(size);
    let icon = tray_icon::Icon::from_rgba(rgba, size, size)?;
    let tray = tray_icon::TrayIconBuilder::new()
        .with_tooltip("Readloom · 单击恢复窗口")
        .with_icon(icon)
        .with_menu_on_left_click(false)
        .build()?;
    let timer = Timer::default();
    let weak = ui.as_weak();
    timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
        while let Ok(event) = tray_icon::TrayIconEvent::receiver().try_recv() {
            if matches!(
                event,
                tray_icon::TrayIconEvent::Click { .. }
                    | tray_icon::TrayIconEvent::DoubleClick { .. }
            ) && let Some(ui) = weak.upgrade()
            {
                let _ = ui.show();
                ui.window().with_winit_window(|window| {
                    window.set_minimized(false);
                    window.focus_window();
                });
                ui.set_status_text("已从系统托盘恢复窗口。".into());
            }
        }
    });
    Ok((Some(tray), timer))
}

fn application_icon_rgba(size: u32) -> Vec<u8> {
    let mut rgba = vec![0_u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let index = ((y * size + x) * 4) as usize;
            let base_x = x * 32 / size;
            let base_y = y * 32 / size;
            let rounded = (3..29).contains(&base_x)
                && (3..29).contains(&base_y)
                && !(!(6..26).contains(&base_x) && !(6..26).contains(&base_y));
            if rounded {
                rgba[index..index + 4].copy_from_slice(&[31, 34, 40, 255]);
            }
            let r_mark = ((10..=13).contains(&base_x) && (8..=24).contains(&base_y))
                || ((13..=21).contains(&base_x) && (base_y == 8 || base_y == 15))
                || ((19..=22).contains(&base_x) && (9..=14).contains(&base_y))
                || ((16..=22).contains(&base_x)
                    && base_y >= 16
                    && base_x.saturating_sub(14) == base_y.saturating_sub(15));
            if r_mark {
                rgba[index..index + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
    }
    rgba
}

fn load_library(
    ui: &MainWindow,
    core: &ReadloomCore,
    columns: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = core.library_snapshot(500)?;
    let total_book_count = snapshot.documents.len();
    let mut counts = HashMap::<Option<String>, i32>::new();
    for document in &snapshot.documents {
        *counts.entry(document.group_id.clone()).or_default() += 1;
    }
    let mut shelves = vec![LibraryShelf {
        id: "all".into(),
        name: "全部图书".into(),
        count: as_i32(snapshot.documents.len()),
    }];
    shelves.push(LibraryShelf {
        id: "ungrouped".into(),
        name: "未分组".into(),
        count: counts.get(&None).copied().unwrap_or_default(),
    });
    shelves.extend(snapshot.groups.iter().map(|group| {
        LibraryShelf {
            id: group.group_id.clone().into(),
            name: group.name.clone().into(),
            count: counts
                .get(&Some(group.group_id.clone()))
                .copied()
                .unwrap_or_default(),
        }
    }));
    let requested_group = ui.get_library_group_filter();
    let group_index = shelves
        .iter()
        .position(|shelf| shelf.id.as_str() == requested_group.as_str())
        .unwrap_or(0);
    ui.set_library_group_filter_index(as_i32(group_index));
    ui.set_library_group_filter(shelves[group_index].id.clone());
    ui.set_library_group_label(shelves[group_index].name.clone());

    let documents = filter_library_documents(
        snapshot.documents,
        ui.get_library_search_query().as_str(),
        ui.get_library_type_filter().as_str(),
        shelves[group_index].id.as_str(),
        ui.get_library_sort_mode().as_str(),
    );
    let books = documents
        .into_iter()
        .map(|document| {
            let group_index = shelves
                .iter()
                .position(|shelf| match document.group_id.as_deref() {
                    Some(group_id) => shelf.id.as_str() == group_id,
                    None => shelf.id.as_str() == "ungrouped",
                })
                .unwrap_or(1);
            let cover_path = document
                .cover_key
                .as_deref()
                .and_then(|key| core.library_cover_path(key));
            let cover = cover_path
                .as_deref()
                .and_then(load_library_cover_image_cached);
            let has_cover = cover.is_some();
            LibraryBook {
                title: document.display_title.into(),
                subtitle: document
                    .author
                    .unwrap_or_else(|| document.path.clone())
                    .into(),
                path: document.path.into(),
                kind: document.document_kind.to_uppercase().into(),
                available: document.available,
                group_index: as_i32(group_index),
                group_name: shelves[group_index].name.clone(),
                cover: cover.unwrap_or_default(),
                has_cover,
            }
        })
        .collect::<Vec<_>>();
    let book_count = books.len();
    let columns = columns.clamp(3, 5);
    let book_rows = books
        .chunks(columns)
        .map(|books| LibraryBookRow {
            first: books[0].clone(),
            second: books.get(1).cloned().unwrap_or_default(),
            third: books.get(2).cloned().unwrap_or_default(),
            fourth: books.get(3).cloned().unwrap_or_default(),
            fifth: books.get(4).cloned().unwrap_or_default(),
            has_second: books.len() > 1,
            has_third: books.len() > 2,
            has_fourth: books.len() > 3,
            has_fifth: books.len() > 4,
        })
        .collect::<Vec<_>>();
    ui.set_library_shelves(ModelRc::new(VecModel::from(shelves)));
    ui.set_library_book_rows(ModelRc::new(VecModel::from(book_rows)));
    ui.set_library_book_count(as_i32(book_count));
    ui.set_status_text(
        if book_count == total_book_count {
            format!("已连接本地书库 · {book_count} 本")
        } else {
            format!("书库筛选 · 显示 {book_count} / {total_book_count} 本")
        }
        .into(),
    );
    Ok(())
}

fn filter_library_documents(
    mut documents: Vec<LibraryDocument>,
    query: &str,
    type_filter: &str,
    group_filter: &str,
    sort_mode: &str,
) -> Vec<LibraryDocument> {
    let query = query.trim().to_lowercase();
    documents.retain(|document| {
        let query_matches = query.is_empty()
            || document.display_title.to_lowercase().contains(&query)
            || document.path.to_lowercase().contains(&query)
            || document
                .author
                .as_deref()
                .is_some_and(|author| author.to_lowercase().contains(&query));
        let type_matches = match type_filter {
            "epub" | "txt" => document.document_kind.eq_ignore_ascii_case(type_filter),
            "missing" => !document.available,
            _ => true,
        };
        let group_matches = match group_filter {
            "all" => true,
            "ungrouped" => document.group_id.is_none(),
            group_id => document.group_id.as_deref() == Some(group_id),
        };
        query_matches && type_matches && group_matches
    });
    if sort_mode == "title" {
        documents.sort_by_cached_key(|document| document.display_title.to_lowercase());
    } else {
        documents.sort_by_key(|document| std::cmp::Reverse(document.last_opened_at_ms));
    }
    documents
}

fn import_library_documents(
    core: &ReadloomCore,
    paths: impl IntoIterator<Item = PathBuf>,
) -> (usize, usize) {
    let mut imported = 0;
    let mut failed = 0;
    for path in paths {
        let result = if is_epub(&path) {
            core.open_epub(&path).map(|_| ())
        } else {
            core.open_txt(&path).map(|_| ())
        };
        if result.is_ok() {
            imported += 1;
        } else {
            failed += 1;
        }
    }
    (imported, failed)
}

fn collect_supported_documents(
    root: &Path,
    maximum: usize,
) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut documents = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                pending.push(path);
                continue;
            }
            let supported = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("txt") || extension.eq_ignore_ascii_case("epub")
                });
            if supported {
                documents.push(path);
                if documents.len() >= maximum {
                    documents.sort();
                    return Ok(documents);
                }
            }
        }
    }
    documents.sort();
    Ok(documents)
}

fn document_path_matches(document: &OpenDocument, path: &Path) -> bool {
    document.path().is_some_and(|open_path| open_path == path)
}

fn upsert_open_document(open_documents: &RefCell<Vec<OpenDocument>>, document: OpenDocument) {
    let mut documents = open_documents.borrow_mut();
    if let Some(index) = document.path().and_then(|path| {
        documents
            .iter()
            .position(|open| document_path_matches(open, path))
    }) {
        documents[index] = document;
    } else {
        documents.push(document);
    }
}

fn refresh_open_tabs(ui: &MainWindow, documents: &[OpenDocument], active_path: Option<&Path>) {
    let tabs = documents
        .iter()
        .filter_map(|document| {
            let path = document.path()?;
            Some(OpenDocumentTab {
                title: document.title().into(),
                path: path.to_string_lossy().into_owned().into(),
                kind: document.kind().into(),
                active: active_path.is_some_and(|active| active == path),
            })
        })
        .collect::<Vec<_>>();
    ui.set_open_document_tabs(ModelRc::new(VecModel::from(tabs)));
}

fn load_initial_index(core: &ReadloomCore, document: &OpenDocument) -> usize {
    match document {
        OpenDocument::Txt(document) => core
            .load_text_locator(document)
            .ok()
            .flatten()
            .map_or(0, |locator| document.resolve_locator(&locator)),
        OpenDocument::Epub(document) => core
            .load_epub_locator(document)
            .ok()
            .flatten()
            .and_then(|locator| document.load_locator(&locator).ok())
            .unwrap_or(0),
    }
}

fn load_document_for_open(
    core: &ReadloomCore,
    path: &Path,
) -> Result<(OpenDocument, usize), String> {
    if is_epub(path) {
        let document = core.open_epub(path).map_err(|error| error.to_string())?;
        let stored_locator = core.load_epub_locator(&document).ok().flatten();
        let initial_index = stored_locator
            .as_ref()
            .map_or(Ok(0), |locator| document.load_locator(locator))
            .map_err(|error| error.to_string())?;
        if stored_locator.is_some_and(|locator| locator.version == 1) {
            let migrated = document.locator_for_paragraph(initial_index);
            let _ = core.save_epub_locator(&document, &migrated);
        }
        return Ok((OpenDocument::Epub(Arc::new(document)), initial_index));
    }

    let document = core.open_txt(path).map_err(|error| error.to_string())?;
    let initial_index = core
        .load_text_locator(&document)
        .ok()
        .flatten()
        .map_or(0, |locator| document.resolve_locator(&locator));
    Ok((OpenDocument::Txt(Arc::new(document)), initial_index))
}

fn populate_open_document(ui: &MainWindow, document: &OpenDocument, initial_index: usize) {
    match document {
        OpenDocument::Txt(document) => populate_reader(ui, document, initial_index),
        OpenDocument::Epub(document) => populate_epub_reader(ui, document, initial_index),
    }
}

fn activate_open_document(
    ui: &MainWindow,
    core: &ReadloomCore,
    current_document: &RefCell<Option<OpenDocument>>,
    open_documents: &RefCell<Vec<OpenDocument>>,
    document: &OpenDocument,
    initial_index: usize,
) {
    populate_open_document(ui, document, initial_index);
    *current_document.borrow_mut() = Some(document.clone());
    refresh_open_tabs(ui, &open_documents.borrow(), document.path());
    load_bookmarks(ui, core, document);
    navigate_after_layout(ui, initial_index);
}

fn show_empty_reader(ui: &MainWindow) {
    ui.set_reader_open(true);
    ui.set_settings_open(false);
    ui.set_edit_mode(false);
    ui.set_document_title("".into());
    ui.set_document_author("".into());
    ui.set_document_kind("".into());
    ui.set_document_path("".into());
    ui.set_edit_content("".into());
    ui.set_reader_side_panel("".into());
    ui.set_chapters(empty_model());
    ui.set_document_paragraph_count(0);
    ui.set_reader_window_start(0);
    ui.set_paragraphs(empty_model());
    ui.set_paragraph_rows(empty_model());
    ui.set_search_results(empty_model());
    ui.set_bookmarks(empty_model());
    ui.set_status_text("当前没有打开的书籍。".into());
}

fn load_bookmarks(ui: &MainWindow, core: &ReadloomCore, document: &OpenDocument) {
    let Some(path) = document.path() else {
        ui.set_bookmarks(empty_model());
        return;
    };
    match core.bookmarks_for_path(path) {
        Ok(stored) => {
            let bookmarks = stored
                .into_iter()
                .map(|bookmark| {
                    let chapter_index = bookmark.chapter_index.unwrap_or_default();
                    let paragraph_index = match document {
                        OpenDocument::Epub(document) if bookmark.locator_version == 1 => document
                            .local_paragraph_index(&EpubReadingLocator {
                                version: bookmark.locator_version,
                                chapter_index,
                                paragraph_index: bookmark.paragraph_index,
                                character_offset_in_paragraph: 0,
                            })
                            .unwrap_or(bookmark.paragraph_index),
                        _ => bookmark.paragraph_index,
                    };
                    BookmarkItem {
                        id: bookmark.bookmark_id.into(),
                        label: bookmark
                            .title
                            .filter(|title| !title.trim().is_empty())
                            .unwrap_or_else(|| bookmark.chapter_title.clone())
                            .into(),
                        detail: format!(
                            "{} · 第 {} 段",
                            bookmark.chapter_title,
                            paragraph_index + 1
                        )
                        .into(),
                        chapter_index: as_i32(chapter_index),
                        paragraph_index: as_i32(paragraph_index),
                    }
                })
                .collect::<Vec<_>>();
            ui.set_bookmarks(ModelRc::new(VecModel::from(bookmarks)));
        }
        Err(error) => {
            ui.set_bookmarks(empty_model());
            ui.set_status_text(error.to_string().into());
        }
    }
}

fn search_items(hits: Vec<SearchHit>, epub: bool) -> Vec<SearchItem> {
    hits.into_iter()
        .enumerate()
        .map(|(index, hit)| SearchItem {
            label: if epub {
                format!(
                    "结果 {} · 第 {} 章 · 段落 {}",
                    index + 1,
                    hit.chapter_index + 1,
                    hit.paragraph_index + 1
                )
                .into()
            } else {
                format!("结果 {} · 段落 {}", index + 1, hit.paragraph_index + 1).into()
            },
            preview: hit.preview.into(),
            chapter_index: as_i32(hit.chapter_index),
            paragraph_index: as_i32(hit.paragraph_index),
        })
        .collect()
}

fn populate_reader(ui: &MainWindow, document: &Arc<ReaderDocument>, initial_index: usize) {
    let chapters = document
        .chapters()
        .iter()
        .enumerate()
        .map(|(chapter_index, chapter)| ChapterItem {
            title: chapter.title.clone().into(),
            chapter_index: as_i32(chapter_index),
            paragraph_index: as_i32(chapter.paragraph_index),
            line_number: as_i32(chapter.line_number),
        })
        .collect::<Vec<_>>();
    ui.set_document_title(document.title().into());
    ui.set_document_author("".into());
    ui.set_document_kind("TXT".into());
    ui.set_edit_content("".into());
    ui.set_edit_mode(false);
    ui.set_reader_side_panel("".into());
    ui.set_document_encoding(document.encoding().label(document.has_bom()).into());
    ui.set_document_line_ending(document.line_ending().label().into());
    ui.set_document_path(
        document
            .path()
            .map_or_else(String::new, |path| path.to_string_lossy().into_owned())
            .into(),
    );
    ui.set_chapters(ModelRc::new(VecModel::from(chapters)));
    set_reader_models(ui, OpenDocument::Txt(document.clone()), initial_index);
    ui.set_search_results(empty_model());
    ui.set_active_paragraph_index(as_i32(initial_index));
    let active_chapter = document
        .paragraphs()
        .get(initial_index)
        .map_or(0, |paragraph| paragraph.chapter_index);
    ui.set_active_chapter_index(as_i32(active_chapter));
    ui.set_active_chapter_title(
        document
            .chapters()
            .get(active_chapter)
            .map_or("", |chapter| chapter.title.as_str())
            .into(),
    );
    ui.set_reader_open(true);
    ui.set_settings_open(false);
    ui.set_status_text(
        format!(
            "{} · {} 个阅读段落 · 已恢复到第 {} 段",
            document.title(),
            document.paragraphs().len(),
            initial_index + 1
        )
        .into(),
    );
    ui.window().request_redraw();
}

fn populate_epub_reader(ui: &MainWindow, document: &Arc<EpubDocument>, initial_index: usize) {
    let chapters = document
        .chapters()
        .iter()
        .enumerate()
        .map(|(chapter_index, chapter)| ChapterItem {
            title: chapter.title.clone().into(),
            chapter_index: as_i32(chapter_index),
            paragraph_index: 0,
            line_number: as_i32(chapter.spine_index + 1),
        })
        .collect::<Vec<_>>();
    ui.set_document_title(document.title().into());
    ui.set_document_author(document.author().unwrap_or("").into());
    ui.set_document_kind("EPUB".into());
    ui.set_edit_content("".into());
    ui.set_edit_mode(false);
    ui.set_reader_side_panel("".into());
    ui.set_document_encoding("EPUB".into());
    ui.set_document_line_ending("安全布局".into());
    ui.set_document_path(document.path().to_string_lossy().into_owned().into());
    ui.set_chapters(ModelRc::new(VecModel::from(chapters)));
    set_reader_models(ui, OpenDocument::Epub(document.clone()), initial_index);
    ui.set_search_results(empty_model());
    ui.set_active_paragraph_index(as_i32(initial_index));
    let active_chapter = document.active_chapter_index();
    ui.set_active_chapter_index(as_i32(active_chapter));
    ui.set_active_chapter_title(
        document
            .chapters()
            .get(active_chapter)
            .map_or("", |chapter| chapter.title.as_str())
            .into(),
    );
    ui.set_reader_open(true);
    ui.set_settings_open(false);
    ui.set_status_text(
        format!(
            "{} · 第 {} / {} 章 · 本章 {} 段 · 已恢复到第 {} 段",
            document.title(),
            active_chapter + 1,
            document.chapters().len(),
            document.paragraphs().len(),
            initial_index + 1
        )
        .into(),
    );
    ui.window().request_redraw();
}

fn paragraph_kind_name(kind: ParagraphKind) -> SharedString {
    match kind {
        ParagraphKind::Heading => "heading",
        ParagraphKind::Paragraph => "paragraph",
        ParagraphKind::Blank => "blank",
        ParagraphKind::Image => "image",
    }
    .into()
}

fn set_reader_models(ui: &MainWindow, document: OpenDocument, target: usize) {
    let paragraph_count = match &document {
        OpenDocument::Txt(document) => document.paragraphs().len(),
        OpenDocument::Epub(document) => document.paragraphs().len(),
    };
    let start = target.min(paragraph_count.saturating_sub(1));
    let end = start
        .saturating_add(READER_WINDOW_SIZE)
        .min(paragraph_count);
    let paragraph_model = ReaderParagraphModel::new(document, start..end);
    let rows = paragraph_model.paragraph_rows();
    let paragraphs = ModelRc::new(paragraph_model);
    let paragraph_rows = ModelRc::new(ReaderParagraphRowModel {
        paragraphs: paragraphs.clone(),
        rows,
        window_start: start,
    });
    ui.set_document_paragraph_count(as_i32(paragraph_count));
    ui.set_reader_window_start(as_i32(start));
    ui.set_paragraphs(paragraphs);
    ui.set_paragraph_rows(paragraph_rows);
}

fn update_reader_target_row(ui: &MainWindow, paragraph_index: usize) {
    let paragraph_rows = ui.get_paragraph_rows();
    let window_start = ui.get_reader_window_start().max(0) as usize;
    let row = paragraph_rows
        .as_any()
        .downcast_ref::<ReaderParagraphRowModel>()
        .map_or(paragraph_index.saturating_sub(window_start) / 2, |model| {
            model.row_for_paragraph(paragraph_index)
        });
    ui.set_reader_target_row(as_i32(row));
}

fn decode_epub_image(resource: &EpubImageResource) -> Image {
    let Ok(decoded) = image::load_from_memory(&resource.bytes) else {
        return Image::default();
    };
    dynamic_image_to_slint(decoded, EPUB_IMAGE_MAX_WIDTH, EPUB_IMAGE_MAX_HEIGHT)
}

fn load_library_cover_image(path: &Path) -> Option<Image> {
    let reader = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    let decoded = reader.decode().ok()?;
    let image = dynamic_image_to_slint(decoded, LIBRARY_COVER_MAX_WIDTH, LIBRARY_COVER_MAX_HEIGHT);
    (image.size().width > 0).then_some(image)
}

fn load_library_cover_image_cached(path: &Path) -> Option<Image> {
    if let Some(cached) = LIBRARY_COVER_CACHE.with(|cache| cache.borrow_mut().get(path)) {
        return Some(cached);
    }
    let image = load_library_cover_image(path)?;
    LIBRARY_COVER_CACHE.with(|cache| {
        cache.borrow_mut().insert(path.to_path_buf(), image.clone());
    });
    Some(image)
}

fn dynamic_image_to_slint(
    decoded: image::DynamicImage,
    maximum_width: u32,
    maximum_height: u32,
) -> Image {
    let decoded = if decoded.width() > maximum_width || decoded.height() > maximum_height {
        decoded.resize(
            maximum_width,
            maximum_height,
            image::imageops::FilterType::Triangle,
        )
    } else {
        decoded
    };
    let rgba = decoded.into_rgba8();
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
    );
    Image::from_rgba8(buffer)
}

fn decoded_image_bytes(image: &Image) -> usize {
    let size = image.size();
    (size.width as usize)
        .saturating_mul(size.height as usize)
        .saturating_mul(4)
}

fn navigate_after_layout(ui: &MainWindow, index: usize) {
    let weak = ui.as_weak();
    Timer::single_shot(Duration::from_millis(1), move || {
        if let Some(ui) = weak.upgrade() {
            ui.invoke_navigate_to(as_i32(index));
        }
    });
}

fn update_active_chapter(ui: &MainWindow, document: &OpenDocument, paragraph_index: usize) {
    match document {
        OpenDocument::Txt(document) => {
            let chapter_index = document
                .paragraphs()
                .get(paragraph_index)
                .map_or(0, |paragraph| paragraph.chapter_index);
            ui.set_active_chapter_index(as_i32(chapter_index));
            ui.set_active_chapter_title(
                document
                    .chapters()
                    .get(chapter_index)
                    .map_or("", |chapter| chapter.title.as_str())
                    .into(),
            );
        }
        OpenDocument::Epub(document) => {
            let chapter_index = document.active_chapter_index();
            ui.set_active_chapter_index(as_i32(chapter_index));
            ui.set_active_chapter_title(
                document
                    .chapters()
                    .get(chapter_index)
                    .map_or("", |chapter| chapter.title.as_str())
                    .into(),
            );
        }
    }
}

fn chapter_target(
    document: &OpenDocument,
    active_paragraph_index: i32,
    direction: i32,
) -> Option<usize> {
    let active_paragraph_index = active_paragraph_index.max(0) as usize;
    match document {
        OpenDocument::Txt(document) => {
            let active_chapter = document
                .paragraphs()
                .get(active_paragraph_index)
                .map_or(0, |paragraph| paragraph.chapter_index);
            let target_chapter = active_chapter.checked_add_signed(direction as isize)?;
            document
                .chapters()
                .get(target_chapter)
                .map(|chapter| chapter.paragraph_index)
        }
        OpenDocument::Epub(document) => {
            let _ = (document, active_paragraph_index, direction);
            None
        }
    }
}

fn apply_settings(ui: &MainWindow, settings: &AppSettings) {
    ui.set_settings_theme(
        match settings.theme {
            AppTheme::Light => "light",
            AppTheme::Dark => "dark",
            AppTheme::System => "system",
        }
        .into(),
    );
    ui.set_settings_effective_theme(
        if settings.theme == AppTheme::Dark
            || (settings.theme == AppTheme::System && windows_prefers_dark())
        {
            "dark"
        } else {
            "light"
        }
        .into(),
    );
    ui.set_settings_library_columns(settings.library_columns);
    ui.set_settings_background_opacity(settings.background_opacity);
    ui.set_settings_minimize_to_tray(settings.minimize_to_tray);
    ui.set_settings_close_action(
        match settings.close_action {
            WindowCloseAction::Exit => "exit",
            WindowCloseAction::Tray => "tray",
        }
        .into(),
    );
    ui.set_settings_font_id(settings.reading.font_family.clone().into());
    ui.set_settings_font_family(settings.reading.resolved_font_family().into());
    ui.set_settings_font_size(settings.reading.font_size);
    ui.set_settings_font_weight(settings.reading.font_weight);
    ui.set_settings_letter_spacing(settings.reading.letter_spacing);
    ui.set_settings_first_line_indent(settings.reading.first_line_indent);
    ui.set_settings_line_height(settings.reading.line_height);
    ui.set_settings_paragraph_spacing(settings.reading.paragraph_spacing);
    ui.set_settings_content_width(settings.reading.content_width);
    ui.set_settings_horizontal_margin(settings.reading.horizontal_margin);
    ui.set_settings_vertical_margin(settings.reading.vertical_margin);
    ui.set_settings_text_alignment(
        match settings.reading.text_alignment {
            TextAlignment::Left => "left",
            TextAlignment::Justify => "justify",
        }
        .into(),
    );
    ui.set_settings_reading_columns(settings.reading.columns);
    ui.set_settings_txt_leading_indent(
        match settings.txt.leading_indent {
            TxtLeadingIndent::Clean => "clean",
            TxtLeadingIndent::Preserve => "preserve",
        }
        .into(),
    );
    ui.set_settings_txt_blank_lines(
        match settings.txt.blank_lines {
            TxtBlankLines::Preserve => "preserve",
            TxtBlankLines::Single => "single",
            TxtBlankLines::Remove => "remove",
        }
        .into(),
    );
    ui.set_settings_merge_wrapped_lines(settings.txt.merge_wrapped_lines);
    ui.set_settings_chapter_title_style(
        match settings.txt.chapter_title_style {
            ChapterTitleStyle::Prominent => "prominent",
            ChapterTitleStyle::Compact => "compact",
            ChapterTitleStyle::Plain => "plain",
        }
        .into(),
    );
    ui.set_settings_epub_publisher_styles(settings.epub.use_publisher_styles);
    ui.set_settings_epub_override_font(settings.epub.override_font);
    ui.set_settings_epub_override_font_size(settings.epub.override_font_size);
    ui.set_settings_epub_override_indent(settings.epub.override_indent);
    ui.set_settings_epub_override_line_height(settings.epub.override_line_height);
    ui.set_settings_epub_override_paragraph_spacing(settings.epub.override_paragraph_spacing);
    ui.set_settings_epub_embedded_fonts(settings.epub.use_embedded_fonts);
    ui.set_settings_chapter_pattern(settings.books.txt_chapter_pattern.clone().into());
    ui.set_settings_backup_path(settings.data.backup_path.clone().into());
    ui.set_shortcut_open(settings.shortcuts.open.clone().into());
    ui.set_shortcut_save(settings.shortcuts.save.clone().into());
    ui.set_shortcut_save_as(settings.shortcuts.save_as.clone().into());
    ui.set_shortcut_close(settings.shortcuts.close.clone().into());
    ui.set_shortcut_toggle_edit(settings.shortcuts.toggle_edit.clone().into());
    ui.set_shortcut_previous_chapter(settings.shortcuts.previous_chapter.clone().into());
    ui.set_shortcut_next_chapter(settings.shortcuts.next_chapter.clone().into());
    ui.set_shortcut_bookmark(settings.shortcuts.bookmark.clone().into());
    ui.set_shortcut_show_library(settings.shortcuts.show_library.clone().into());
    ui.set_shortcut_show_settings(settings.shortcuts.show_settings.clone().into());
    ui.window().request_redraw();
}

#[cfg(windows)]
fn windows_prefers_dark() -> bool {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let subkey = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
        .encode_utf16()
        .collect::<Vec<_>>();
    let value_name = "AppsUseLightTheme\0".encode_utf16().collect::<Vec<_>>();
    let mut value = 1_u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value_name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    };
    status == 0 && value == 0
}

#[cfg(not(windows))]
fn windows_prefers_dark() -> bool {
    false
}

fn apply_background(ui: &MainWindow, core: &ReadloomCore) {
    let path = core.background_image_path().ok().flatten();
    ui.set_settings_has_background(path.is_some());
    if let Some(path) = path
        && let Ok(image) = slint::Image::load_from_path(&path)
    {
        ui.set_settings_background_image(image);
    } else {
        ui.set_settings_background_image(slint::Image::default());
    }
}

fn install_settings_handlers(
    ui: &MainWindow,
    core: &Arc<ReadloomCore>,
    settings: &Rc<RefCell<AppSettings>>,
    current_document: &Rc<RefCell<Option<OpenDocument>>>,
    open_documents: &Rc<RefCell<Vec<OpenDocument>>>,
) {
    {
        let weak = ui.as_weak();
        let core = core.clone();
        let settings = settings.clone();
        let current_document = current_document.clone();
        let open_documents = open_documents.clone();
        ui.on_update_setting(move |key, value| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let key = key.as_str();
            let value = value.as_str();
            {
                let mut settings = settings.borrow_mut();
                match key {
                    "theme" => {
                        settings.theme = match value {
                            "dark" => AppTheme::Dark,
                            "system" => AppTheme::System,
                            _ => AppTheme::Light,
                        }
                    }
                    "font" => settings.reading.font_family = value.to_owned(),
                    "libraryColumns" => settings.library_columns = value.parse().unwrap_or(4),
                    "minimizeToTray" => settings.minimize_to_tray = value == "true",
                    "closeAction" => {
                        settings.close_action = if value == "tray" {
                            WindowCloseAction::Tray
                        } else {
                            WindowCloseAction::Exit
                        }
                    }
                    "alignment" => {
                        settings.reading.text_alignment = if value == "left" {
                            TextAlignment::Left
                        } else {
                            TextAlignment::Justify
                        }
                    }
                    "readingColumns" => settings.reading.columns = value.parse().unwrap_or(1),
                    "txtIndent" => {
                        settings.txt.leading_indent = if value == "preserve" {
                            TxtLeadingIndent::Preserve
                        } else {
                            TxtLeadingIndent::Clean
                        }
                    }
                    "txtBlankLines" => {
                        settings.txt.blank_lines = match value {
                            "preserve" => TxtBlankLines::Preserve,
                            "remove" => TxtBlankLines::Remove,
                            _ => TxtBlankLines::Single,
                        }
                    }
                    "mergeWrappedLines" => settings.txt.merge_wrapped_lines = value == "true",
                    "chapterTitleStyle" => {
                        settings.txt.chapter_title_style = match value {
                            "compact" => ChapterTitleStyle::Compact,
                            "plain" => ChapterTitleStyle::Plain,
                            _ => ChapterTitleStyle::Prominent,
                        }
                    }
                    "epubPublisher" => settings.epub.use_publisher_styles = value == "true",
                    "epubEmbedded" => settings.epub.use_embedded_fonts = value == "true",
                    "epubFont" => settings.epub.override_font = value == "true",
                    "epubFontSize" => settings.epub.override_font_size = value == "true",
                    "epubIndent" => settings.epub.override_indent = value == "true",
                    "epubLineHeight" => settings.epub.override_line_height = value == "true",
                    "epubParagraphSpacing" => {
                        settings.epub.override_paragraph_spacing = value == "true"
                    }
                    "reading.reset" => {
                        let defaults = AppSettings::default();
                        settings.reading = ReadingSettings::default();
                        settings.txt = defaults.txt;
                        settings.epub = defaults.epub;
                    }
                    _ => return,
                }
            }
            let structural = matches!(key, "txtIndent" | "txtBlankLines" | "mergeWrappedLines");
            if persist_settings(&ui, &core, &settings, "设置已保存并实时生效。") {
                if key == "libraryColumns" {
                    let _ = load_library(&ui, &core, settings.borrow().library_columns as usize);
                }
                if key == "readingColumns" {
                    navigate_after_layout(&ui, ui.get_active_paragraph_index().max(0) as usize);
                }
                if structural {
                    refresh_open_txt(&ui, &core, &current_document, &open_documents);
                }
            }
        });
    }
    {
        let weak = ui.as_weak();
        let core = core.clone();
        let settings = settings.clone();
        let save_timer = Rc::new(Timer::default());
        ui.on_change_setting(move |key, delta| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            {
                let mut settings = settings.borrow_mut();
                match key.as_str() {
                    "backgroundOpacity" => settings.background_opacity += delta,
                    "fontSize" => settings.reading.font_size += delta.round() as i32,
                    "fontWeight" => settings.reading.font_weight += delta.round() as i32,
                    "letterSpacing" => settings.reading.letter_spacing += delta,
                    "firstLineIndent" => settings.reading.first_line_indent += delta,
                    "lineHeight" => settings.reading.line_height += delta,
                    "paragraphSpacing" => settings.reading.paragraph_spacing += delta,
                    "contentWidth" => settings.reading.content_width += delta.round() as i32,
                    "horizontalMargin" => {
                        settings.reading.horizontal_margin += delta.round() as i32
                    }
                    "verticalMargin" => settings.reading.vertical_margin += delta.round() as i32,
                    _ => return,
                }
            }
            apply_settings(&ui, &settings.borrow());
            ui.set_status_text("排版预览已更新，稍后自动保存。".into());
            let weak = weak.clone();
            let core = core.clone();
            let settings = settings.clone();
            save_timer.start(
                TimerMode::SingleShot,
                Duration::from_millis(240),
                move || {
                    let Some(ui) = weak.upgrade() else {
                        return;
                    };
                    match core.save_settings(&settings.borrow()) {
                        Ok(saved) => {
                            *settings.borrow_mut() = saved;
                            apply_settings(&ui, &settings.borrow());
                            ui.set_status_text("排版数值已保存。".into());
                        }
                        Err(error) => ui.set_status_text(error.to_string().into()),
                    }
                },
            );
        });
    }
    {
        let weak = ui.as_weak();
        let core = core.clone();
        ui.on_choose_background(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let Some(path) = rfd::FileDialog::new()
                .add_filter("背景图片", &["png", "jpg", "jpeg", "webp"])
                .pick_file()
            else {
                return;
            };
            match core.set_background_image(&path) {
                Ok(_) => {
                    apply_background(&ui, &core);
                    ui.set_status_text("背景图片已复制到应用数据目录。".into());
                }
                Err(error) => ui.set_status_text(error.to_string().into()),
            }
        });
    }
    {
        let weak = ui.as_weak();
        let core = core.clone();
        ui.on_clear_background(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            match core.clear_background_image() {
                Ok(()) => {
                    apply_background(&ui, &core);
                    ui.set_status_text("背景图片已清除。".into());
                }
                Err(error) => ui.set_status_text(error.to_string().into()),
            }
        });
    }
    {
        let weak = ui.as_weak();
        let core = core.clone();
        let settings = settings.clone();
        ui.on_set_shortcut(move |action, shortcut| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            if !settings
                .borrow_mut()
                .shortcuts
                .set(action.as_str(), shortcut.as_str())
            {
                return;
            }
            persist_settings(&ui, &core, &settings, "快捷键已保存。");
        });
    }
    {
        let weak = ui.as_weak();
        let core = core.clone();
        let settings = settings.clone();
        ui.on_clear_shortcuts(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            settings.borrow_mut().shortcuts = Default::default();
            persist_settings(&ui, &core, &settings, "全部快捷键已清除。");
        });
    }
    {
        let weak = ui.as_weak();
        let core = core.clone();
        let settings = settings.clone();
        let current_document = current_document.clone();
        let open_documents = open_documents.clone();
        ui.on_set_chapter_pattern(move |pattern| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            settings.borrow_mut().books.txt_chapter_pattern = pattern.to_string();
            if persist_settings(&ui, &core, &settings, "TXT 章节识别规则已保存。") {
                ui.set_settings_pattern_error("".into());
                refresh_open_txt(&ui, &core, &current_document, &open_documents);
            } else {
                ui.set_settings_pattern_error("正则表达式无效，仍使用上一次有效规则。".into());
            }
        });
    }
    {
        let weak = ui.as_weak();
        let core = core.clone();
        let settings = settings.clone();
        ui.on_reset_chapter_pattern(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            settings.borrow_mut().books.txt_chapter_pattern =
                DEFAULT_TXT_CHAPTER_PATTERN.to_owned();
            ui.set_settings_pattern_error("".into());
            persist_settings(&ui, &core, &settings, "TXT 章节识别规则已恢复默认。");
        });
    }
    {
        let weak = ui.as_weak();
        let core = core.clone();
        let settings = settings.clone();
        ui.on_choose_backup_path(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let Some(path) = rfd::FileDialog::new().pick_folder() else {
                return;
            };
            settings.borrow_mut().data.backup_path = path.to_string_lossy().into_owned();
            persist_settings(&ui, &core, &settings, "默认备份文件夹已保存。");
        });
    }
    {
        let weak = ui.as_weak();
        let core = core.clone();
        let settings = settings.clone();
        ui.on_create_backup(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let configured = PathBuf::from(&settings.borrow().data.backup_path);
            let mut dialog = rfd::FileDialog::new()
                .add_filter("Readloom 内容备份", &["readloom-backup"])
                .set_file_name(format!(
                    "readloom-{}.readloom-backup",
                    unix_timestamp_seconds()
                ));
            if configured.is_dir() {
                dialog = dialog.set_directory(&configured);
            } else if let Some(parent) = configured.parent()
                && !parent.as_os_str().is_empty()
            {
                dialog = dialog.set_directory(parent);
            }
            let Some(path) = dialog.save_file() else {
                return;
            };
            if let Some(parent) = path.parent() {
                settings.borrow_mut().data.backup_path = parent.to_string_lossy().into_owned();
                let _ = core.save_settings(&settings.borrow());
                ui.set_settings_backup_path(parent.to_string_lossy().into_owned().into());
            }
            ui.set_settings_backup_result("正在备份，请稍候…".into());
            ui.set_status_text("正在备份，请稍候…".into());
            let weak = weak.clone();
            let core = core.clone();
            Timer::single_shot(Duration::from_millis(20), move || {
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                match core.create_books_backup(&path) {
                    Ok(summary) => {
                        let message = format!(
                            "备份完成：扫描 {} 本，写入 {} 份唯一内容。",
                            summary.source_books, summary.unique_contents
                        );
                        ui.set_settings_backup_result(message.clone().into());
                        ui.set_status_text(message.into());
                    }
                    Err(error) => {
                        ui.set_settings_backup_result(error.to_string().into());
                        ui.set_status_text(error.to_string().into());
                    }
                }
            });
        });
    }
    {
        let weak = ui.as_weak();
        let core = core.clone();
        ui.on_restore_backup(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let Some(backups) = rfd::FileDialog::new()
                .add_filter("Readloom 内容备份", &["readloom-backup"])
                .pick_files()
            else {
                return;
            };
            let Some(output) = rfd::FileDialog::new().pick_folder() else {
                return;
            };
            restore_backups_with_progress(&ui, core.clone(), backups, output);
        });
    }
    {
        let weak = ui.as_weak();
        let core = core.clone();
        ui.on_restore_backup_directory(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let Some(directory) = rfd::FileDialog::new().pick_folder() else {
                return;
            };
            let mut backups = match std::fs::read_dir(&directory) {
                Ok(entries) => entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| {
                        path.extension()
                            .and_then(|extension| extension.to_str())
                            .is_some_and(|extension| {
                                extension.eq_ignore_ascii_case("readloom-backup")
                            })
                    })
                    .collect::<Vec<_>>(),
                Err(error) => {
                    ui.set_settings_backup_result(error.to_string().into());
                    return;
                }
            };
            backups.sort();
            if backups.is_empty() {
                ui.set_settings_backup_result("该文件夹内没有 .readloom-backup 文件。".into());
                return;
            }
            let Some(output) = rfd::FileDialog::new().pick_folder() else {
                return;
            };
            restore_backups_with_progress(&ui, core.clone(), backups, output);
        });
    }
}

fn restore_backups_with_progress(
    ui: &MainWindow,
    core: Arc<ReadloomCore>,
    backups: Vec<PathBuf>,
    output: PathBuf,
) {
    ui.set_settings_backup_result(format!("正在读取并恢复 {} 份备份…", backups.len()).into());
    ui.set_status_text("正在恢复备份，请稍候…".into());
    let weak = ui.as_weak();
    Timer::single_shot(Duration::from_millis(20), move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        match core.restore_books_backups(&backups, &output) {
            Ok(summary) => {
                let message = format!(
                    "恢复完成：恢复 {} 本，跨备份跳过 {} 份重复内容。",
                    summary.restored_books, summary.skipped_duplicates
                );
                ui.set_settings_backup_result(message.clone().into());
                ui.set_status_text(message.into());
                let _ = load_library(&ui, &core, ui.get_settings_library_columns() as usize);
            }
            Err(error) => {
                ui.set_settings_backup_result(error.to_string().into());
                ui.set_status_text(error.to_string().into());
            }
        }
    });
}

fn unix_timestamp_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn refresh_open_txt(
    ui: &MainWindow,
    core: &ReadloomCore,
    current_document: &RefCell<Option<OpenDocument>>,
    open_documents: &RefCell<Vec<OpenDocument>>,
) {
    let path = current_document
        .borrow()
        .as_ref()
        .and_then(|document| match document {
            OpenDocument::Txt(document) => document.path().map(Path::to_path_buf),
            OpenDocument::Epub(_) => None,
        });
    let Some(path) = path else {
        return;
    };
    let index = ui.get_active_paragraph_index().max(0) as usize;
    match core.open_txt(&path) {
        Ok(document) => {
            let index = index.min(document.paragraphs().len().saturating_sub(1));
            let opened = OpenDocument::Txt(Arc::new(document));
            upsert_open_document(open_documents, opened.clone());
            activate_open_document(ui, core, current_document, open_documents, &opened, index);
        }
        Err(error) => ui.set_status_text(error.to_string().into()),
    }
}

fn persist_settings(
    ui: &MainWindow,
    core: &ReadloomCore,
    settings: &RefCell<AppSettings>,
    message: &str,
) -> bool {
    let result = core.save_settings(&settings.borrow());
    match result {
        Ok(saved) => {
            *settings.borrow_mut() = saved;
            apply_settings(ui, &settings.borrow());
            apply_background(ui, core);
            ui.set_status_text(message.into());
            true
        }
        Err(error) => {
            if let Ok(saved) = core.load_settings() {
                *settings.borrow_mut() = saved;
                apply_settings(ui, &settings.borrow());
            }
            ui.set_status_text(error.to_string().into());
            false
        }
    }
}

fn empty_model<T: Clone + 'static>() -> ModelRc<T> {
    ModelRc::new(VecModel::<T>::from(Vec::new()))
}

fn as_i32(value: usize) -> i32 {
    value.min(i32::MAX as usize) as i32
}

fn is_epub(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("epub"))
}

fn state_database_path() -> PathBuf {
    if let Some(path) = std::env::var_os("READLOOM_STATE_DB") {
        return PathBuf::from(path);
    }
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("app.readloom.desktop")
        .join("readloom-state.sqlite3")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_books_keep_the_slint_presentation_model_bounded() {
        let ui = MainWindow::new().expect("create Slint test window");
        let content = (0..20_000)
            .map(|index| format!("这是用于窗口化验证的第 {index} 段。\n"))
            .collect::<String>();
        let document = Arc::new(ReaderDocument::from_text("长篇测试", content));
        populate_reader(&ui, &document, 10_000);

        assert!(
            ui.get_paragraphs().row_count() <= 128,
            "Slint must never receive the full long-book model: {} rows",
            ui.get_paragraphs().row_count()
        );
        let start = ui.get_reader_window_start().max(0) as usize;
        let end = start + ui.get_paragraphs().row_count();
        assert!(start <= 10_000 && 10_000 < end);
    }

    #[test]
    fn reader_scroll_uses_stable_bounded_scroll_views() {
        let source = include_str!("../../../ui/readloom.slint");
        assert!(source.contains("reader-list := ScrollView"));
        assert!(source.contains("double-reader-list := ScrollView"));
        assert!(!source.contains("reader-list := ListView"));
        assert!(!source.contains("double-reader-list := ListView"));
        assert!(source.contains("reader-column := VerticalLayout"));
        assert!(source.contains("double-reader-column := VerticalLayout"));
    }

    #[test]
    fn epub_images_are_downscaled_before_becoming_slint_images() {
        let source = image::RgbaImage::from_pixel(1_600, 800, image::Rgba([24, 48, 72, 255]));
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .expect("encode test png");
        let encoded = encoded.into_inner();
        let resource = EpubImageResource {
            media_type: "image/png".to_owned(),
            bytes: encoded.clone(),
            alt_text: "大图".to_owned(),
        };

        let decoded = decode_epub_image(&resource);

        assert_eq!(decoded.size().width, 1_200);
        assert_eq!(decoded.size().height, 600);

        let directory = tempfile::tempdir().expect("temporary directory");
        let cover_path = directory.path().join("cover.png");
        std::fs::write(&cover_path, encoded).expect("write cover fixture");
        let cover = load_library_cover_image(&cover_path).expect("decode cover thumbnail");
        assert_eq!(cover.size().width, 360);
        assert_eq!(cover.size().height, 180);
    }

    #[test]
    fn reader_image_cache_is_a_bounded_lru() {
        let image = || Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(1, 1));
        let mut cache = DecodedImageCache::new(8);
        cache.insert(1, image());
        cache.insert(2, image());
        assert!(cache.get(1).is_some());

        cache.insert(3, image());

        assert!(cache.entries.contains_key(&1));
        assert!(!cache.entries.contains_key(&2));
        assert!(cache.entries.contains_key(&3));
        assert!(cache.decoded_bytes <= cache.maximum_bytes);
    }

    #[test]
    fn library_cover_cache_reuses_images_and_evicts_by_decoded_bytes() {
        let image = || Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(1, 1));
        let mut cache = LibraryCoverCache::new(8);
        cache.insert(PathBuf::from("one"), image());
        cache.insert(PathBuf::from("two"), image());
        assert!(cache.get(Path::new("one")).is_some());

        cache.insert(PathBuf::from("three"), image());

        assert!(cache.entries.contains_key(Path::new("one")));
        assert!(!cache.entries.contains_key(Path::new("two")));
        assert!(cache.entries.contains_key(Path::new("three")));
        assert!(cache.decoded_bytes <= cache.maximum_bytes);
    }

    #[test]
    fn pane_drag_only_commits_layout_on_pointer_release() {
        let source = include_str!("../../../ui/readloom.slint");

        assert_eq!(source.matches("root.preview-pane-resize(").count(), 4);
        assert_eq!(source.matches("root.commit-pane-resize(").count(), 4);
        assert_eq!(source.matches("PointerEventKind.up").count(), 4);
        assert!(!source.contains("root.queue-pane-resize("));
        assert!(!source.contains("root.workspace-pane-width = Math.max"));
        assert!(!source.contains("root.reader-tools-pane-width = Math.max"));
        assert!(!source.contains("root.epub-search-panel-width = Math.max"));
        assert!(!source.contains("root.settings-navigation-width = Math.max"));
    }

    #[test]
    fn reader_state_does_not_duplicate_text_or_bounded_view_models() {
        let ui_source = include_str!("../../../ui/readloom.slint");
        let rust_source = include_str!("main.rs");

        assert!(!ui_source.contains("document-content"));
        assert!(!ui_source.contains("epub-search-panel-open"));
        assert!(ui_source.contains("callback request-edit-mode(bool);"));
        assert!(!rust_source.contains(&["struct Reader", "ViewCache"].concat()));
        assert!(!rust_source.contains(&["populate_open_document", "_cached"].concat()));
    }

    #[test]
    fn outline_list_uses_the_remaining_navigation_column_height() {
        let source = include_str!("../../../ui/readloom.slint");
        assert!(!source.contains("height: Math.min(224px"));
        assert!(source.contains("accessible-id: \"txt-outline\";\n                                    vertical-stretch: 1;"));
    }

    #[test]
    fn directory_import_scan_is_recursive_filtered_and_bounded() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let nested = directory.path().join("nested");
        std::fs::create_dir(&nested).expect("create nested directory");
        std::fs::write(directory.path().join("甲.TXT"), "第一章\n正文").expect("write txt");
        std::fs::write(nested.join("乙.epub"), b"not parsed by the scanner").expect("write epub");
        std::fs::write(nested.join("忽略.md"), "not a book").expect("write ignored file");

        let all = collect_supported_documents(directory.path(), 10).expect("scan directory");
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|path| path.ends_with("甲.TXT")));
        assert!(all.iter().any(|path| path.ends_with("乙.epub")));

        let bounded = collect_supported_documents(directory.path(), 1).expect("bounded scan");
        assert_eq!(bounded.len(), 1);
    }

    #[test]
    fn application_icon_has_an_opaque_dark_tile_and_white_letter() {
        let rgba = application_icon_rgba(64);
        assert_eq!(rgba.len(), 64 * 64 * 4);
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] == 0));
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] == 255));
        assert!(
            rgba.chunks_exact(4)
                .any(|pixel| pixel[0] < 50 && pixel[1] < 50 && pixel[2] < 50)
        );
        assert!(
            rgba.chunks_exact(4)
                .any(|pixel| pixel[0] > 240 && pixel[1] > 240 && pixel[2] > 240)
        );
    }

    #[test]
    fn library_view_filters_search_type_group_and_missing_sources() {
        let documents = vec![
            LibraryDocument {
                path: "C:/books/江湖.txt".to_owned(),
                document_kind: "txt".to_owned(),
                display_title: "江湖夜雨".to_owned(),
                author: Some("作者甲".to_owned()),
                last_opened_at_ms: 10,
                available: true,
                group_id: Some("novels".to_owned()),
                cover_key: None,
            },
            LibraryDocument {
                path: "C:/books/History.epub".to_owned(),
                document_kind: "epub".to_owned(),
                display_title: "历史随笔".to_owned(),
                author: Some("作者乙".to_owned()),
                last_opened_at_ms: 20,
                available: false,
                group_id: None,
                cover_key: None,
            },
        ];

        let by_query = filter_library_documents(documents.clone(), "江湖", "all", "all", "recent");
        assert_eq!(by_query.len(), 1);
        assert_eq!(by_query[0].document_kind, "txt");

        let by_group = filter_library_documents(documents.clone(), "", "txt", "novels", "recent");
        assert_eq!(by_group.len(), 1);
        assert_eq!(by_group[0].display_title, "江湖夜雨");

        let missing = filter_library_documents(documents, "", "missing", "ungrouped", "title");
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].display_title, "历史随笔");
    }
}
