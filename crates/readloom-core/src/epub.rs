use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

use quick_xml::{
    Reader, XmlVersion,
    encoding::Decoder,
    escape::{resolve_predefined_entity, unescape},
    events::{BytesRef, BytesStart, Event},
};
use rbook::Epub;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use zip::{CompressionMethod, ZipArchive};

use crate::{CoreError, ParagraphKind, ReadingParagraph, SearchHit};

const MAXIMUM_EPUB_BYTES: u64 = 512 * 1024 * 1024;
const MAXIMUM_EPUB_ENTRIES: usize = 4_096;
const MAXIMUM_EPUB_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
const MAXIMUM_EPUB_EXPANDED_BYTES: u64 = 768 * 1024 * 1024;
const MAXIMUM_EPUB_IMAGE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpubChapter {
    pub title: String,
    pub paragraph_index: usize,
    pub spine_index: usize,
    pub resource_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpubImageResource {
    pub media_type: String,
    pub bytes: Vec<u8>,
    pub alt_text: String,
}

#[derive(Debug, Clone)]
struct EpubChapterSource {
    resource_href: String,
}

#[derive(Debug)]
struct LoadedEpubChapter {
    chapter_index: usize,
    paragraphs: Arc<Vec<ReadingParagraph>>,
    images: Arc<Vec<EpubImageResource>>,
}

struct ExtractedBlock {
    kind: ParagraphKind,
    text: String,
    image_href: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EpubReadingLocator {
    pub version: u8,
    pub chapter_index: usize,
    pub paragraph_index: usize,
    #[serde(default)]
    pub character_offset_in_paragraph: usize,
}

#[derive(Debug)]
pub struct EpubDocument {
    path: PathBuf,
    title: String,
    author: Option<String>,
    fingerprint: String,
    chapters: Vec<EpubChapter>,
    chapter_sources: Vec<EpubChapterSource>,
    loaded_chapter: RwLock<LoadedEpubChapter>,
    chapter_load_generation: AtomicU64,
    cover: Option<EpubImageResource>,
}

impl EpubDocument {
    pub(crate) fn open(path: &Path) -> Result<Self, CoreError> {
        validate_archive(path)?;
        let epub = Epub::options()
            .strict(false)
            .open(path)
            .map_err(|_| invalid_epub("无法解析 EPUB 的书籍结构。"))?;
        let version = epub.package().version_str();
        if !matches!(version.split('.').next(), Some("2" | "3")) {
            return Err(invalid_epub("Readloom 目前仅支持 EPUB 2 和 EPUB 3。"));
        }
        let title = epub
            .metadata()
            .title()
            .map(|value| value.value().trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("未命名 EPUB")
                    .to_owned()
            });
        let author = epub
            .metadata()
            .creators()
            .map(|value| value.value().trim().to_owned())
            .find(|value| !value.is_empty());
        let manifest = epub.manifest();
        let cover_entry = manifest.cover_image().or_else(|| {
            manifest
                .images()
                .filter_map(|entry| {
                    let href = entry.href().path().decode();
                    manifest_cover_score(entry.id(), &href).map(|score| (score, entry))
                })
                .min_by_key(|(score, _)| *score)
                .map(|(_, entry)| entry)
        });
        let cover = cover_entry.and_then(|entry| {
            let media_type = entry.media_type().to_ascii_lowercase();
            let bytes = entry.read_bytes().ok()?;
            validated_image_resource(&media_type, bytes, "封面".to_owned())
        });
        let mut chapters = Vec::new();
        let mut chapter_sources = Vec::new();
        let navigation_titles = epub
            .toc()
            .contents()
            .map(|root| {
                root.flatten()
                    .filter_map(|entry| {
                        let href = entry.href()?;
                        let path = href.path().decode().trim_start_matches('/').to_owned();
                        let label = entry.label().trim().to_owned();
                        (!path.is_empty() && !label.is_empty()).then_some((path, label))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
            .into_iter()
            .collect::<HashMap<_, _>>();

        for spine in epub.spine().iter().filter(|entry| entry.is_linear()) {
            let entry = spine
                .manifest_entry()
                .ok_or_else(|| invalid_epub("EPUB 阅读顺序引用了不存在的章节。"))?;
            if !matches!(entry.media_type(), "application/xhtml+xml" | "text/html") {
                continue;
            }
            let resource_id = entry
                .href()
                .path()
                .decode()
                .trim_start_matches('/')
                .to_owned();
            let resource_href = entry.href().path().decode().to_string();
            let chapter_index = chapters.len();
            let chapter_title = navigation_titles
                .get(&resource_id)
                .cloned()
                .unwrap_or_else(|| format!("第 {} 章", chapter_index + 1));
            chapters.push(EpubChapter {
                title: chapter_title,
                paragraph_index: 0,
                spine_index: spine.order(),
                resource_id: resource_id.clone(),
            });
            chapter_sources.push(EpubChapterSource { resource_href });
        }
        if chapters.is_empty() {
            return Err(invalid_epub("EPUB 没有可阅读的流式正文。"));
        }
        let (loaded_chapter, initial_heading, heuristic_cover) =
            load_chapter_from_epub(&epub, &chapter_sources[0], 0, &chapters[0])?;
        if let Some(initial_heading) = initial_heading {
            chapters[0].title = initial_heading;
        }
        // Some older EPUB files omit formal cover metadata. Only accept an image
        // from a resource/section explicitly identified as a cover; choosing the
        // first arbitrary image can incorrectly promote an advert or QR code.
        let cover = cover.or(heuristic_cover);
        Ok(Self {
            path: path.to_owned(),
            title,
            author,
            fingerprint: fingerprint_file(path)?,
            chapters,
            chapter_sources,
            loaded_chapter: RwLock::new(loaded_chapter),
            chapter_load_generation: AtomicU64::new(0),
            cover,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn paragraphs(&self) -> Arc<Vec<ReadingParagraph>> {
        self.loaded_chapter
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .paragraphs
            .clone()
    }

    pub fn chapters(&self) -> &[EpubChapter] {
        &self.chapters
    }

    pub fn images(&self) -> Arc<Vec<EpubImageResource>> {
        self.loaded_chapter
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .images
            .clone()
    }

    pub fn cover(&self) -> Option<&EpubImageResource> {
        self.cover.as_ref()
    }

    pub fn active_chapter_index(&self) -> usize {
        self.loaded_chapter
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .chapter_index
    }

    pub fn load_chapter(&self, chapter_index: usize) -> Result<(), CoreError> {
        let chapter_index = chapter_index.min(self.chapters.len().saturating_sub(1));
        let generation = self
            .chapter_load_generation
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        if self.active_chapter_index() == chapter_index {
            return Ok(());
        }
        let epub = open_epub_publication(&self.path)?;
        let (loaded, _, _) = load_chapter_from_epub(
            &epub,
            &self.chapter_sources[chapter_index],
            chapter_index,
            &self.chapters[chapter_index],
        )?;
        if self.chapter_load_generation.load(Ordering::Acquire) == generation {
            *self
                .loaded_chapter
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = loaded;
        }
        Ok(())
    }

    pub fn load_locator(&self, locator: &EpubReadingLocator) -> Result<usize, CoreError> {
        if !matches!(locator.version, 1 | 2) {
            return Ok(0);
        }
        let chapter_index = locator
            .chapter_index
            .min(self.chapters.len().saturating_sub(1));
        let paragraph_index = self.local_paragraph_index(locator)?;
        self.load_chapter(chapter_index)?;
        Ok(paragraph_index.min(self.paragraphs().len().saturating_sub(1)))
    }

    pub fn local_paragraph_index(&self, locator: &EpubReadingLocator) -> Result<usize, CoreError> {
        if locator.version == 1 {
            self.legacy_local_paragraph_index(
                locator
                    .chapter_index
                    .min(self.chapters.len().saturating_sub(1)),
                locator.paragraph_index,
            )
        } else {
            Ok(locator.paragraph_index)
        }
    }

    pub fn search(&self, query: &str, maximum: usize) -> Vec<SearchHit> {
        let query = query.trim();
        if query.is_empty() || maximum == 0 {
            return Vec::new();
        }
        self.search_publication(query, maximum.min(5_000))
            .unwrap_or_default()
    }

    pub fn locator_for_paragraph(&self, paragraph_index: usize) -> EpubReadingLocator {
        let paragraphs = self.paragraphs();
        let paragraph_index = paragraph_index.min(paragraphs.len().saturating_sub(1));
        EpubReadingLocator {
            version: 2,
            chapter_index: self.active_chapter_index(),
            paragraph_index,
            character_offset_in_paragraph: 0,
        }
    }

    pub fn resolve_locator(&self, locator: &EpubReadingLocator) -> usize {
        let paragraphs = self.paragraphs();
        if !matches!(locator.version, 1 | 2) || paragraphs.is_empty() {
            return 0;
        }
        if locator.chapter_index == self.active_chapter_index() {
            locator
                .paragraph_index
                .min(paragraphs.len().saturating_sub(1))
        } else {
            0
        }
    }

    fn legacy_local_paragraph_index(
        &self,
        target_chapter: usize,
        global_paragraph_index: usize,
    ) -> Result<usize, CoreError> {
        let epub = open_epub_publication(&self.path)?;
        let mut preceding_paragraphs = 0usize;
        for source in self.chapter_sources.iter().take(target_chapter) {
            let entry = epub
                .manifest()
                .by_href(&source.resource_href)
                .ok_or_else(|| invalid_epub("EPUB 章节资源已经不存在。"))?;
            let source = entry
                .read_str()
                .map_err(|_| invalid_epub("无法读取 EPUB 章节正文。"))?;
            preceding_paragraphs =
                preceding_paragraphs.saturating_add(extract_blocks(&source)?.len());
        }
        Ok(global_paragraph_index.saturating_sub(preceding_paragraphs))
    }

    fn search_publication(&self, query: &str, maximum: usize) -> Result<Vec<SearchHit>, CoreError> {
        let epub = open_epub_publication(&self.path)?;
        let mut hits = Vec::new();
        for (chapter_index, source) in self.chapter_sources.iter().enumerate() {
            let entry = epub
                .manifest()
                .by_href(&source.resource_href)
                .ok_or_else(|| invalid_epub("EPUB 章节资源已经不存在。"))?;
            let source = entry
                .read_str()
                .map_err(|_| invalid_epub("无法读取 EPUB 章节正文。"))?;
            for (paragraph_index, block) in extract_blocks(&source)?.into_iter().enumerate() {
                let Some(byte_offset) = block.text.find(query) else {
                    continue;
                };
                hits.push(SearchHit {
                    paragraph_index,
                    chapter_index,
                    character_offset_in_paragraph: block.text[..byte_offset].encode_utf16().count(),
                    preview: block.text,
                });
                if hits.len() >= maximum {
                    return Ok(hits);
                }
            }
        }
        Ok(hits)
    }
}

fn open_epub_publication(path: &Path) -> Result<Epub, CoreError> {
    Epub::options()
        .strict(false)
        .open(path)
        .map_err(|_| invalid_epub("无法重新打开 EPUB 的书籍结构。"))
}

fn load_chapter_from_epub(
    epub: &Epub,
    chapter_source: &EpubChapterSource,
    chapter_index: usize,
    chapter: &EpubChapter,
) -> Result<(LoadedEpubChapter, Option<String>, Option<EpubImageResource>), CoreError> {
    let manifest = epub.manifest();
    let entry = manifest
        .by_href(&chapter_source.resource_href)
        .ok_or_else(|| invalid_epub("EPUB 章节资源已经不存在。"))?;
    let source = entry
        .read_str()
        .map_err(|_| invalid_epub("无法读取 EPUB 章节正文。"))?;
    if source.len() as u64 > MAXIMUM_EPUB_ENTRY_BYTES {
        return Err(invalid_epub("EPUB 单个章节过大，已拒绝完整载入。"));
    }
    let blocks = extract_blocks(&source)?;
    if blocks.is_empty() {
        return Err(invalid_epub("EPUB 当前章节没有可阅读的流式正文。"));
    }
    let heading = blocks
        .iter()
        .find(|block| block.kind == ParagraphKind::Heading && !block.text.is_empty())
        .map(|block| block.text.clone());
    let cover_section = looks_like_cover(&chapter.title, &chapter.resource_id, "");
    let mut heuristic_cover = None;
    let mut images = Vec::new();
    let mut image_indices = HashMap::<String, usize>::new();
    let mut paragraphs = Vec::with_capacity(blocks.len());
    let mut source_offset = 0usize;
    let mut source_offset_utf16 = 0usize;
    for block in blocks {
        let mut kind = block.kind;
        let mut text = block.text;
        let image_index = if let Some(image_href) = block.image_href {
            let resolved_href = resolve_manifest_href(&chapter_source.resource_href, &image_href);
            resolved_href
                .as_ref()
                .and_then(|href| {
                    if let Some(index) = image_indices.get(href).copied() {
                        if heuristic_cover.is_none()
                            && (cover_section
                                || looks_like_cover(&chapter.title, &chapter.resource_id, &text))
                        {
                            heuristic_cover = images.get(index).cloned();
                        }
                        return Some(index);
                    }
                    let image_entry = manifest.by_href(href)?;
                    let media_type = image_entry.media_type().to_ascii_lowercase();
                    let bytes = image_entry.read_bytes().ok()?;
                    let image = validated_image_resource(&media_type, bytes, text.clone())?;
                    if heuristic_cover.is_none()
                        && (cover_section
                            || looks_like_cover(
                                &chapter.title,
                                &chapter.resource_id,
                                &image.alt_text,
                            ))
                    {
                        heuristic_cover = Some(image.clone());
                    }
                    let index = images.len();
                    images.push(image);
                    image_indices.insert(href.clone(), index);
                    Some(index)
                })
                .or_else(|| {
                    kind = ParagraphKind::Paragraph;
                    text = format!("[图片无法显示：{}]", text);
                    None
                })
        } else {
            None
        };
        let source_end = source_offset + text.len();
        let source_end_utf16 = source_offset_utf16 + text.encode_utf16().count();
        let paragraph_index = paragraphs.len();
        paragraphs.push(ReadingParagraph {
            kind,
            text,
            source_start: source_offset,
            source_end,
            source_start_utf16: source_offset_utf16,
            source_end_utf16,
            line_number: paragraph_index + 1,
            chapter_index,
            paragraph_index,
            image_index,
        });
        source_offset = source_end.saturating_add(1);
        source_offset_utf16 = source_end_utf16.saturating_add(1);
    }
    Ok((
        LoadedEpubChapter {
            chapter_index,
            paragraphs: Arc::new(paragraphs),
            images: Arc::new(images),
        },
        heading,
        heuristic_cover,
    ))
}

fn fingerprint_file(path: &Path) -> Result<String, CoreError> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn extract_blocks(source: &str) -> Result<Vec<ExtractedBlock>, CoreError> {
    let mut reader = Reader::from_str(source);
    reader.config_mut().check_end_names = true;
    let mut blocks = Vec::new();
    let mut current: Option<(Vec<u8>, ParagraphKind, String)> = None;
    let mut hidden_depth = 0usize;
    let mut all_visible_text = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let qualified_name = element.name();
                let name = local_name(qualified_name.as_ref());
                if hidden_depth > 0 || is_hidden(name) {
                    hidden_depth = hidden_depth.saturating_add(1);
                } else if name == b"img" {
                    if let Some(block) = image_block(&element, reader.decoder())? {
                        blocks.push(block);
                    }
                } else if current.is_none()
                    && let Some(kind) = block_kind(name)
                {
                    current = Some((name.to_vec(), kind, String::new()));
                }
            }
            Ok(Event::End(element)) if hidden_depth > 0 => {
                hidden_depth -= 1;
                let _ = element;
            }
            Ok(Event::End(element)) => {
                let qualified_name = element.name();
                let name = local_name(qualified_name.as_ref());
                if current
                    .as_ref()
                    .is_some_and(|(start, _, _)| start.as_slice() == name)
                {
                    let (_, kind, text) = current.take().expect("checked current block");
                    let text = collapse_whitespace(&text);
                    if !text.is_empty() {
                        blocks.push(ExtractedBlock {
                            kind,
                            text,
                            image_href: None,
                        });
                    }
                }
            }
            Ok(Event::Empty(element)) if hidden_depth == 0 => {
                if local_name(element.name().as_ref()) == b"img" {
                    if let Some(block) = image_block(&element, reader.decoder())? {
                        blocks.push(block);
                    }
                } else if local_name(element.name().as_ref()) == b"br"
                    && let Some((_, _, text)) = current.as_mut()
                {
                    text.push(' ');
                }
            }
            Ok(Event::Text(value)) if hidden_depth == 0 => {
                let decoded = value
                    .xml_content(XmlVersion::Implicit1_0)
                    .map_err(|_| invalid_epub("EPUB 章节包含无法解码的文本。"))?;
                let decoded = unescape(&decoded)
                    .map_err(|_| invalid_epub("EPUB 章节包含无效的字符实体。"))?;
                all_visible_text.push_str(&decoded);
                all_visible_text.push(' ');
                if let Some((_, _, text)) = current.as_mut() {
                    text.push_str(&decoded);
                }
            }
            Ok(Event::CData(value)) if hidden_depth == 0 => {
                let decoded = value
                    .decode()
                    .map_err(|_| invalid_epub("EPUB 章节包含无法解码的文本。"))?;
                all_visible_text.push_str(&decoded);
                all_visible_text.push(' ');
                if let Some((_, _, text)) = current.as_mut() {
                    text.push_str(&decoded);
                }
            }
            Ok(Event::GeneralRef(reference)) if hidden_depth == 0 => {
                let replacement = resolve_reference(&reference)?;
                all_visible_text.push_str(&replacement);
                if let Some((_, _, text)) = current.as_mut() {
                    text.push_str(&replacement);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(invalid_epub("EPUB 章节 XHTML 结构无效。")),
        }
    }
    if blocks.is_empty() {
        let text = collapse_whitespace(&all_visible_text);
        if !text.is_empty() {
            blocks.push(ExtractedBlock {
                kind: ParagraphKind::Paragraph,
                text,
                image_href: None,
            });
        }
    }
    Ok(blocks)
}

fn image_block(
    element: &BytesStart<'_>,
    decoder: Decoder,
) -> Result<Option<ExtractedBlock>, CoreError> {
    let mut source = None;
    let mut alt = None;
    for attribute in element.attributes().with_checks(false) {
        let attribute = attribute.map_err(|_| invalid_epub("EPUB 图片属性无效。"))?;
        let value = attribute
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, decoder)
            .map_err(|_| invalid_epub("EPUB 图片属性无法解码。"))?
            .into_owned();
        match local_name(attribute.key.as_ref()) {
            b"src" => source = Some(value),
            b"alt" => alt = Some(value),
            _ => {}
        }
    }
    let Some(source) = source.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    Ok(Some(ExtractedBlock {
        kind: ParagraphKind::Image,
        text: alt
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "图片".to_owned()),
        image_href: Some(source),
    }))
}

fn resolve_manifest_href(base: &str, reference: &str) -> Option<String> {
    let reference = reference.split(['?', '#']).next()?.trim();
    if reference.is_empty()
        || reference.starts_with("//")
        || reference.contains(['\\', '\0'])
        || reference
            .split('/')
            .next()
            .is_some_and(|segment| segment.contains(':'))
    {
        return None;
    }
    let mut segments = if reference.starts_with('/') {
        Vec::new()
    } else {
        base.trim_start_matches('/')
            .split('/')
            .collect::<Vec<_>>()
            .into_iter()
            .take(
                base.trim_start_matches('/')
                    .split('/')
                    .count()
                    .saturating_sub(1),
            )
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    for segment in reference.trim_start_matches('/').split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            value => segments.push(value.to_owned()),
        }
    }
    (!segments.is_empty()).then(|| format!("/{}", segments.join("/")))
}

fn validated_image_resource(
    media_type: &str,
    bytes: Vec<u8>,
    alt_text: String,
) -> Option<EpubImageResource> {
    if bytes.is_empty() || bytes.len() > MAXIMUM_EPUB_IMAGE_BYTES {
        return None;
    }
    let signature_matches = match media_type {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" | "image/jpg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        _ => false,
    };
    signature_matches.then(|| EpubImageResource {
        media_type: media_type.to_owned(),
        bytes,
        alt_text,
    })
}

fn resolve_reference(reference: &BytesRef<'_>) -> Result<String, CoreError> {
    if let Some(character) = reference
        .resolve_char_ref()
        .map_err(|_| invalid_epub("EPUB 章节包含无效的字符引用。"))?
    {
        return Ok(character.to_string());
    }
    let name = reference
        .decode()
        .map_err(|_| invalid_epub("EPUB 章节包含无法解码的字符引用。"))?;
    resolve_predefined_entity(&name)
        .map(str::to_owned)
        .ok_or_else(|| invalid_epub("EPUB 章节包含未知字符实体。"))
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or_default()
}

fn is_hidden(name: &[u8]) -> bool {
    matches!(
        name,
        b"script" | b"style" | b"iframe" | b"object" | b"embed" | b"template" | b"svg"
    )
}

fn block_kind(name: &[u8]) -> Option<ParagraphKind> {
    match name {
        b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" => Some(ParagraphKind::Heading),
        b"p" | b"li" | b"blockquote" | b"pre" | b"dd" | b"dt" => Some(ParagraphKind::Paragraph),
        _ => None,
    }
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn validate_archive(path: &Path) -> Result<(), CoreError> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > MAXIMUM_EPUB_BYTES {
        return Err(invalid_epub("EPUB 文件无效或超过 512 MiB。"));
    }
    let mut archive = ZipArchive::new(File::open(path)?)
        .map_err(|_| invalid_epub("EPUB 不是有效的 ZIP 容器。"))?;
    if archive.is_empty() || archive.len() > MAXIMUM_EPUB_ENTRIES {
        return Err(invalid_epub("EPUB 条目数量异常。"));
    }
    {
        let mut mimetype = archive
            .by_index(0)
            .map_err(|_| invalid_epub("EPUB 缺少 mimetype。"))?;
        if mimetype.name() != "mimetype" || mimetype.compression() != CompressionMethod::Stored {
            return Err(invalid_epub("EPUB 的 mimetype 必须是首个未压缩条目。"));
        }
        let mut value = Vec::new();
        mimetype
            .by_ref()
            .take(32)
            .read_to_end(&mut value)
            .map_err(CoreError::Io)?;
        if value != b"application/epub+zip" {
            return Err(invalid_epub("EPUB 的 mimetype 内容无效。"));
        }
    }
    let mut names = HashSet::new();
    let mut expanded = 0u64;
    let mut has_container = false;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| invalid_epub("无法读取 EPUB 容器条目。"))?;
        let name = entry.name();
        if std::str::from_utf8(entry.name_raw()).is_err()
            || entry.encrypted()
            || entry.is_symlink()
            || !is_safe_archive_path(name)
        {
            return Err(invalid_epub("EPUB 包含不安全、加密或无效的内部路径。"));
        }
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "meta-inf/rights.xml" | "meta-inf/encryption.xml"
        ) {
            return Err(invalid_epub("Readloom 不支持加密或 DRM 保护的 EPUB。"));
        }
        has_container |= name.eq_ignore_ascii_case("META-INF/container.xml");
        if !matches!(
            entry.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) || entry.size() > MAXIMUM_EPUB_ENTRY_BYTES
            || (entry.size() > 0
                && (entry.compressed_size() == 0
                    || entry.size() / entry.compressed_size().max(1) > 1_000))
        {
            return Err(invalid_epub("EPUB 使用了不受支持或异常的压缩数据。"));
        }
        expanded = expanded
            .checked_add(entry.size())
            .ok_or_else(|| invalid_epub("EPUB 解压大小溢出。"))?;
        if expanded > MAXIMUM_EPUB_EXPANDED_BYTES {
            return Err(invalid_epub("EPUB 解压后的总大小超过安全限制。"));
        }
        let collision_key = name
            .trim_end_matches('/')
            .nfkc()
            .flat_map(char::to_lowercase)
            .collect::<String>();
        if !names.insert(collision_key) {
            return Err(invalid_epub("EPUB 包含大小写或 Unicode 冲突的条目。"));
        }
    }
    if !has_container {
        return Err(invalid_epub("EPUB 缺少 META-INF/container.xml。"));
    }
    Ok(())
}

fn is_safe_archive_path(name: &str) -> bool {
    let name = name.trim_end_matches('/');
    if name.is_empty()
        || name.len() > 1_024
        || name.starts_with('/')
        || name.contains(['\\', '\0'])
        || (name.len() > 1 && name.as_bytes()[1] == b':')
    {
        return false;
    }
    let mut depth = 0usize;
    for segment in name.split('/') {
        if segment.is_empty() || segment == ".." || segment.len() > 255 {
            return false;
        }
        if segment != "." {
            depth += 1;
        }
    }
    depth > 0
}

fn looks_like_cover(chapter_title: &str, resource_id: &str, alt_text: &str) -> bool {
    chapter_title.contains("封面")
        || alt_text.contains("封面")
        || chapter_title.to_ascii_lowercase().contains("cover")
        || resource_id.to_ascii_lowercase().contains("cover")
        || alt_text.to_ascii_lowercase().contains("cover")
}

fn manifest_cover_score(id: &str, href: &str) -> Option<u8> {
    fn score(value: &str) -> Option<u8> {
        let value = value.to_ascii_lowercase();
        let basename = value.rsplit('/').next().unwrap_or(value.as_str());
        let stem = basename.rsplit_once('.').map_or(basename, |(stem, _)| stem);
        if stem == "cover" {
            return Some(0);
        }
        if stem
            .split(|character: char| !character.is_ascii_alphanumeric())
            .any(|token| token == "cover")
        {
            return Some(1);
        }
        (stem.starts_with("cover") || stem.ends_with("cover")).then_some(2)
    }

    score(href).into_iter().chain(score(id)).min()
}

fn invalid_epub(message: &str) -> CoreError {
    CoreError::Validation(message.to_owned())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::TempDir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;

    #[test]
    fn opens_epub_into_a_closed_paragraph_layout() {
        let (_directory, path) = minimal_epub();

        let document = EpubDocument::open(&path).expect("open minimal epub");

        assert_eq!(document.title(), "阅织 EPUB 测试");
        assert_eq!(document.author(), Some("测试作者"));
        assert_eq!(document.chapters()[0].title, "第一章");
        assert_eq!(document.paragraphs()[1].text, "你好，Readloom。 & 安全阅读");
        assert_eq!(document.search("Readloom", 10)[0].paragraph_index, 1);
        let locator = document.locator_for_paragraph(1);
        assert_eq!(document.resolve_locator(&locator), 1);
    }

    #[test]
    fn exposes_manifest_cover_and_inline_images() {
        let (_directory, path) = minimal_epub();

        let document = EpubDocument::open(&path).expect("open image epub");

        assert_eq!(
            document.cover().map(|image| image.media_type.as_str()),
            Some("image/png")
        );
        assert_eq!(document.images().len(), 1);
        assert!(document.paragraphs().iter().any(|paragraph| {
            paragraph.kind == ParagraphKind::Image && paragraph.image_index == Some(0)
        }));
    }

    #[test]
    fn cover_fallback_requires_explicit_cover_semantics() {
        assert!(looks_like_cover("封面", "text/chapter.xhtml", ""));
        assert!(looks_like_cover("Introduction", "OEBPS/cover.xhtml", ""));
        assert!(looks_like_cover("第 1 章", "text/one.xhtml", "封面图"));
        assert!(!looks_like_cover("制作说明", "text/info.xhtml", "二维码"));
    }

    #[test]
    fn recognizes_legacy_guide_cover_when_cover_page_is_non_linear() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("legacy-guide-cover.epub");
        let file = File::create(&path).expect("create epub");
        let mut writer = ZipWriter::new(file);
        let entries = [
            (
                "mimetype",
                "application/epub+zip",
                CompressionMethod::Stored,
            ),
            (
                "META-INF/container.xml",
                r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
                CompressionMethod::Deflated,
            ),
            (
                "OEBPS/content.opf",
                r#"<?xml version="1.0" encoding="UTF-8"?><package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">legacy-cover</dc:identifier><dc:title>旧版封面测试</dc:title><dc:language>zh-CN</dc:language></metadata><manifest><item id="front-image" href="Images/cover.jpg" media-type="image/jpeg"/><item id="front-page" href="Text/cover.xhtml" media-type="application/xhtml+xml"/><item id="chapter" href="Text/chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="front-page" linear="no"/><itemref idref="chapter"/></spine><guide><reference type="cover" title="封面" href="Text/cover.xhtml"/></guide></package>"#,
                CompressionMethod::Deflated,
            ),
            (
                "OEBPS/Text/cover.xhtml",
                r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><body><img src="../Images/cover.jpg" alt="封面"/></body></html>"#,
                CompressionMethod::Deflated,
            ),
            (
                "OEBPS/Text/chapter.xhtml",
                r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><body><h1>第一章</h1><p>正文。</p></body></html>"#,
                CompressionMethod::Deflated,
            ),
        ];
        for (name, content, compression) in entries {
            writer
                .start_file(
                    name,
                    SimpleFileOptions::default().compression_method(compression),
                )
                .expect("start file");
            writer.write_all(content.as_bytes()).expect("write file");
        }
        writer
            .start_file(
                "OEBPS/Images/cover.jpg",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .expect("start cover");
        writer
            .write_all(&[0xff, 0xd8, 0xff, 0xd9])
            .expect("write cover");
        writer.finish().expect("finish epub");

        let document = EpubDocument::open(&path).expect("open legacy cover epub");

        assert_eq!(
            document.cover().map(|cover| cover.media_type.as_str()),
            Some("image/jpeg")
        );
    }

    #[test]
    fn rejects_epub_with_parent_traversal_entry() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("unsafe.epub");
        let file = File::create(&path).expect("create epub");
        let mut writer = ZipWriter::new(file);
        writer
            .start_file(
                "mimetype",
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .expect("mimetype");
        writer
            .write_all(b"application/epub+zip")
            .expect("write mimetype");
        writer
            .start_file("../escape.xhtml", SimpleFileOptions::default())
            .expect("unsafe entry");
        writer.write_all(b"bad").expect("unsafe bytes");
        writer.finish().expect("finish epub");

        let error = EpubDocument::open(&path).expect_err("unsafe path should fail");
        assert!(error.to_string().contains("不安全"));
    }

    #[test]
    fn epub_navigation_labels_supply_chapter_titles_when_xhtml_has_no_heading() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("navigation.epub");
        let file = File::create(&path).expect("create epub");
        let mut writer = ZipWriter::new(file);
        let entries = [
            (
                "mimetype",
                "application/epub+zip",
                CompressionMethod::Stored,
            ),
            (
                "META-INF/container.xml",
                r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
                CompressionMethod::Deflated,
            ),
            (
                "EPUB/package.opf",
                r#"<?xml version="1.0" encoding="UTF-8"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">nav-test</dc:identifier><dc:title>目录标题测试</dc:title><dc:language>zh-CN</dc:language><meta property="dcterms:modified">2026-08-11T00:00:00Z</meta></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="one"/><itemref idref="two"/></spine></package>"#,
                CompressionMethod::Deflated,
            ),
            (
                "EPUB/nav.xhtml",
                r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><body><nav epub:type="toc" xmlns:epub="http://www.idpf.org/2007/ops"><ol><li><a href="one.xhtml">风起</a></li><li><a href="two.xhtml">云涌</a></li></ol></nav></body></html>"#,
                CompressionMethod::Deflated,
            ),
            (
                "EPUB/one.xhtml",
                r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><body><p>第一章正文。</p></body></html>"#,
                CompressionMethod::Deflated,
            ),
            (
                "EPUB/two.xhtml",
                r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><body><p>第二章正文。</p></body></html>"#,
                CompressionMethod::Deflated,
            ),
        ];
        for (name, content, compression) in entries {
            writer
                .start_file(
                    name,
                    SimpleFileOptions::default().compression_method(compression),
                )
                .expect("start file");
            writer.write_all(content.as_bytes()).expect("write file");
        }
        writer.finish().expect("finish epub");

        let document = EpubDocument::open(&path).expect("open navigation epub");

        assert_eq!(
            document
                .chapters()
                .iter()
                .map(|chapter| chapter.title.as_str())
                .collect::<Vec<_>>(),
            ["风起", "云涌"]
        );
    }

    #[test]
    fn opening_epub_keeps_only_the_initial_chapter_body_resident() {
        let (_directory, path) = two_chapter_epub();

        let document = EpubDocument::open(&path).expect("open two-chapter epub");

        assert_eq!(document.chapters().len(), 2);
        assert_eq!(
            document
                .paragraphs()
                .iter()
                .map(|paragraph| paragraph.text.as_str())
                .collect::<Vec<_>>(),
            ["第一章", "第一章正文。"]
        );
        assert!(
            document
                .paragraphs()
                .iter()
                .all(|paragraph| paragraph.chapter_index == 0)
        );
    }

    #[test]
    fn epub_chapters_replace_the_resident_body_on_demand() {
        let (_directory, path) = two_chapter_epub();
        let document = EpubDocument::open(&path).expect("open two-chapter epub");

        document.load_chapter(1).expect("load second chapter");

        assert_eq!(document.active_chapter_index(), 1);
        assert_eq!(
            document
                .paragraphs()
                .iter()
                .map(|paragraph| paragraph.text.as_str())
                .collect::<Vec<_>>(),
            ["第二章", "第二章正文。"]
        );
        let locator = document.locator_for_paragraph(1);
        assert_eq!(locator.version, 2);
        assert_eq!(locator.chapter_index, 1);
        assert_eq!(locator.paragraph_index, 1);
    }

    #[test]
    fn epub_search_streams_unloaded_chapters_without_changing_the_resident_chapter() {
        let (_directory, path) = two_chapter_epub();
        let document = EpubDocument::open(&path).expect("open two-chapter epub");

        let hits = document.search("第二章正文", 10);

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chapter_index, 1);
        assert_eq!(hits[0].paragraph_index, 1);
        assert_eq!(document.active_chapter_index(), 0);
        assert_eq!(document.paragraphs()[1].text, "第一章正文。");
    }

    #[test]
    fn legacy_global_epub_locator_is_migrated_to_a_chapter_local_position() {
        let (_directory, path) = two_chapter_epub();
        let document = EpubDocument::open(&path).expect("open two-chapter epub");
        let legacy = EpubReadingLocator {
            version: 1,
            chapter_index: 1,
            paragraph_index: 3,
            character_offset_in_paragraph: 0,
        };

        assert_eq!(
            document
                .local_paragraph_index(&legacy)
                .expect("localize legacy locator"),
            1
        );
        assert_eq!(document.active_chapter_index(), 0);

        let local_index = document.load_locator(&legacy).expect("load legacy locator");

        assert_eq!(local_index, 1);
        assert_eq!(document.active_chapter_index(), 1);
        assert_eq!(document.paragraphs()[local_index].text, "第二章正文。");
        assert_eq!(document.locator_for_paragraph(local_index).version, 2);
    }

    #[test]
    fn repeated_manifest_image_references_share_one_resource() {
        let (directory, path) = repeated_image_epub(500);

        let document = EpubDocument::open(&path).expect("open repeated-image epub");

        assert_eq!(document.images().len(), 1);
        let paragraphs = document.paragraphs();
        let image_paragraphs = paragraphs
            .iter()
            .filter(|paragraph| paragraph.kind == ParagraphKind::Image)
            .collect::<Vec<_>>();
        assert_eq!(image_paragraphs.len(), 500);
        assert!(
            image_paragraphs
                .iter()
                .all(|paragraph| paragraph.image_index == Some(0))
        );

        drop(directory);
    }

    fn repeated_image_epub(references: usize) -> (TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("repeated-image.epub");
        let file = File::create(&path).expect("create epub");
        let mut writer = ZipWriter::new(file);
        let entries = [
            (
                "mimetype",
                "application/epub+zip",
                CompressionMethod::Stored,
            ),
            (
                "META-INF/container.xml",
                r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
                CompressionMethod::Deflated,
            ),
            (
                "EPUB/package.opf",
                r#"<?xml version="1.0" encoding="UTF-8"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">repeated-image</dc:identifier><dc:title>重复图片测试</dc:title><dc:language>zh-CN</dc:language><meta property="dcterms:modified">2026-08-12T00:00:00Z</meta></metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/><item id="shared" href="shared.png" media-type="image/png"/></manifest><spine><itemref idref="chapter"/></spine></package>"#,
                CompressionMethod::Deflated,
            ),
        ];
        for (name, content, compression) in entries {
            writer
                .start_file(
                    name,
                    SimpleFileOptions::default().compression_method(compression),
                )
                .expect("start file");
            writer.write_all(content.as_bytes()).expect("write file");
        }
        let images = (0..references)
            .map(|_| r#"<img src="shared.png" alt="共享插图"/>"#)
            .collect::<String>();
        writer
            .start_file(
                "EPUB/chapter.xhtml",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .expect("start chapter");
        writer
            .write_all(
                format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><body><h1>第一章</h1>{images}</body></html>"#
                )
                .as_bytes(),
            )
            .expect("write chapter");
        writer
            .start_file(
                "EPUB/shared.png",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .expect("start image");
        writer
            .write_all(&[
                137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0,
                1, 8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 8, 215, 99, 248,
                207, 192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68,
                174, 66, 96, 130,
            ])
            .expect("write image");
        writer.finish().expect("finish epub");
        (directory, path)
    }

    fn two_chapter_epub() -> (TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("two-chapters.epub");
        let file = File::create(&path).expect("create epub");
        let mut writer = ZipWriter::new(file);
        let entries = [
            (
                "mimetype",
                "application/epub+zip",
                CompressionMethod::Stored,
            ),
            (
                "META-INF/container.xml",
                r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
                CompressionMethod::Deflated,
            ),
            (
                "EPUB/package.opf",
                r#"<?xml version="1.0" encoding="UTF-8"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">lazy-chapters</dc:identifier><dc:title>章节懒加载测试</dc:title><dc:language>zh-CN</dc:language><meta property="dcterms:modified">2026-08-12T00:00:00Z</meta></metadata><manifest><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="one"/><itemref idref="two"/></spine></package>"#,
                CompressionMethod::Deflated,
            ),
            (
                "EPUB/one.xhtml",
                r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><body><h1>第一章</h1><p>第一章正文。</p></body></html>"#,
                CompressionMethod::Deflated,
            ),
            (
                "EPUB/two.xhtml",
                r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><body><h1>第二章</h1><p>第二章正文。</p></body></html>"#,
                CompressionMethod::Deflated,
            ),
        ];
        for (name, content, compression) in entries {
            writer
                .start_file(
                    name,
                    SimpleFileOptions::default().compression_method(compression),
                )
                .expect("start file");
            writer.write_all(content.as_bytes()).expect("write file");
        }
        writer.finish().expect("finish epub");
        (directory, path)
    }

    fn minimal_epub() -> (TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("fixture.epub");
        let file = File::create(&path).expect("create epub");
        let mut writer = ZipWriter::new(file);
        let entries = [
            (
                "mimetype",
                "application/epub+zip",
                CompressionMethod::Stored,
            ),
            (
                "META-INF/container.xml",
                r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
                CompressionMethod::Deflated,
            ),
            (
                "EPUB/package.opf",
                r#"<?xml version="1.0" encoding="UTF-8"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="pub-id">urn:readloom:test</dc:identifier><dc:title>阅织 EPUB 测试</dc:title><dc:creator>测试作者</dc:creator><dc:language>zh-CN</dc:language><meta property="dcterms:modified">2026-08-11T00:00:00Z</meta></metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/></manifest><spine><itemref idref="chapter"/></spine></package>"#,
                CompressionMethod::Deflated,
            ),
            (
                "EPUB/chapter.xhtml",
                r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><body><h1>第一章</h1><p>你好，Readloom。 &amp; 安全阅读</p><img src="cover.png" alt="章节插图"/><script>不可见</script></body></html>"#,
                CompressionMethod::Deflated,
            ),
        ];
        for (name, content, compression) in entries {
            writer
                .start_file(
                    name,
                    SimpleFileOptions::default().compression_method(compression),
                )
                .expect("start file");
            writer.write_all(content.as_bytes()).expect("write file");
        }
        writer
            .start_file(
                "EPUB/cover.png",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .expect("start cover");
        writer
            .write_all(&[
                137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0,
                1, 8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 8, 215, 99, 248,
                207, 192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68,
                174, 66, 96, 130,
            ])
            .expect("write cover");
        writer.finish().expect("finish epub");
        (directory, path)
    }
}
