use std::{
    fs::{self, OpenOptions},
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    AppSettings, EpubDocument, EpubDraft, EpubReadingLocator, ImageMediaType, ReaderDocument,
    TextReadingLocator, TxtDraft, ValidatedImageAsset,
    text_codec::{LineEnding, SaveTextOptions, decode_text, encode_text},
};

const SCHEMA_VERSION: i64 = 4;
const MAXIMUM_TEXT_BYTES: u64 = 160 * 1024 * 1024;
const MAXIMUM_BACKGROUND_BYTES: u64 = 20 * 1024 * 1024;
const MAXIMUM_EPUB_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
const MAXIMUM_EPUB_IMAGE_DIMENSION: u32 = 16_384;
const MAXIMUM_EPUB_IMAGE_PIXELS: u64 = 64_000_000;
static SAVE_ARTIFACT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{0}")]
    Validation(String),
    #[error("local state operation failed")]
    Storage(#[from] rusqlite::Error),
    #[error("file operation failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryDocument {
    pub path: String,
    pub document_kind: String,
    pub display_title: String,
    pub author: Option<String>,
    pub last_opened_at_ms: u64,
    pub available: bool,
    pub group_id: Option<String>,
    pub cover_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryGroup {
    pub group_id: String,
    pub name: String,
    pub position: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibrarySnapshot {
    pub documents: Vec<LibraryDocument>,
    pub groups: Vec<LibraryGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBookmark {
    pub bookmark_id: String,
    pub document_kind: String,
    pub title: Option<String>,
    pub chapter_title: String,
    pub locator_version: u8,
    pub chapter_index: Option<usize>,
    pub paragraph_index: usize,
    pub created_at_ms: u64,
}

pub struct ReadloomCore {
    connection: Mutex<Connection>,
    application_data_dir: PathBuf,
}

impl ReadloomCore {
    pub fn open(database_path: &Path) -> Result<Self, CoreError> {
        if let Some(parent) = database_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut connection = Connection::open(database_path)?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 3000;",
        )?;
        migrate(&mut connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
            application_data_dir: database_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf(),
        })
    }

    pub fn background_image_path(&self) -> Result<Option<PathBuf>, CoreError> {
        let path = self
            .lock_connection()?
            .query_row(
                "SELECT value_json FROM app_preferences WHERE preference_key = 'background_image_v1'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(path.map(PathBuf::from).filter(|path| path.is_file()))
    }

    pub fn library_cover_path(&self, key: &str) -> Option<PathBuf> {
        if key.is_empty()
            || key.len() > 96
            || key.contains(['/', '\\', '\0'])
            || key == "."
            || key == ".."
        {
            return None;
        }
        let path = self.application_data_dir.join("covers").join(key);
        path.is_file().then_some(path)
    }

    pub fn set_background_image(&self, source: &Path) -> Result<PathBuf, CoreError> {
        let metadata = fs::metadata(source)?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAXIMUM_BACKGROUND_BYTES {
            return Err(CoreError::Validation(
                "背景图片必须是小于 20 MiB 的 PNG、JPEG 或 WebP 文件。".to_owned(),
            ));
        }
        let bytes = fs::read(source)?;
        let extension = image_extension(&bytes).ok_or_else(|| {
            CoreError::Validation("背景图片格式无效，仅支持 PNG、JPEG 或 WebP。".to_owned())
        })?;
        let directory = self.application_data_dir.join("backgrounds");
        fs::create_dir_all(&directory)?;
        let target = directory.join(format!(
            "background-{}.{}",
            fingerprint_bytes(&bytes),
            extension
        ));
        if !target.exists() {
            write_new_file_safely(&target, &bytes)?;
        }
        self.lock_connection()?.execute(
            "INSERT INTO app_preferences (preference_key, value_json)
             VALUES ('background_image_v1', ?1)
             ON CONFLICT(preference_key) DO UPDATE SET value_json = excluded.value_json",
            [target.to_string_lossy().into_owned()],
        )?;
        Ok(target)
    }

    pub fn clear_background_image(&self) -> Result<(), CoreError> {
        let old_path = self.background_image_path()?;
        self.lock_connection()?.execute(
            "DELETE FROM app_preferences WHERE preference_key = 'background_image_v1'",
            [],
        )?;
        if let Some(path) = old_path
            && path.starts_with(self.application_data_dir.join("backgrounds"))
        {
            let _ = fs::remove_file(path);
        }
        Ok(())
    }

    pub fn validate_epub_image(&self, source: &Path) -> Result<ValidatedImageAsset, CoreError> {
        let metadata = fs::metadata(source)?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAXIMUM_EPUB_IMAGE_BYTES {
            return Err(CoreError::Validation(
                "EPUB 图片必须是小于 20 MiB 的 PNG、JPEG、GIF 或 WebP 文件。".to_owned(),
            ));
        }
        let bytes = fs::read(source)?;
        let media_type = match image::guess_format(&bytes) {
            Ok(image::ImageFormat::Png) if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => {
                ImageMediaType::Png
            }
            Ok(image::ImageFormat::Jpeg) if bytes.starts_with(&[0xff, 0xd8, 0xff]) => {
                ImageMediaType::Jpeg
            }
            Ok(image::ImageFormat::Gif)
                if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") =>
            {
                ImageMediaType::Gif
            }
            Ok(image::ImageFormat::WebP)
                if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") =>
            {
                ImageMediaType::Webp
            }
            _ => {
                return Err(CoreError::Validation(
                    "图片内容无效，仅支持 PNG、JPEG、GIF 或 WebP。".to_owned(),
                ));
            }
        };
        let format = match media_type {
            ImageMediaType::Png => image::ImageFormat::Png,
            ImageMediaType::Jpeg => image::ImageFormat::Jpeg,
            ImageMediaType::Gif => image::ImageFormat::Gif,
            ImageMediaType::Webp => image::ImageFormat::WebP,
        };
        let mut reader = image::ImageReader::with_format(Cursor::new(&bytes), format);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAXIMUM_EPUB_IMAGE_DIMENSION);
        limits.max_image_height = Some(MAXIMUM_EPUB_IMAGE_DIMENSION);
        limits.max_alloc = Some(256 * 1024 * 1024);
        reader.limits(limits);
        let decoded = reader.decode().map_err(|_| {
            CoreError::Validation("图片无法安全解码，可能已损坏或尺寸过大。".to_owned())
        })?;
        let (width, height) = (decoded.width(), decoded.height());
        if width == 0
            || height == 0
            || u64::from(width) * u64::from(height) > MAXIMUM_EPUB_IMAGE_PIXELS
        {
            return Err(CoreError::Validation(
                "图片尺寸过大；像素总量必须不超过 6400 万。".to_owned(),
            ));
        }
        Ok(ValidatedImageAsset {
            digest: blake3::hash(&bytes).to_hex().to_string(),
            bytes: bytes.into(),
            media_type,
            width,
            height,
        })
    }

    pub fn open_txt(&self, path: &Path) -> Result<ReaderDocument, CoreError> {
        let canonical_path = fs::canonicalize(path)?;
        let metadata = fs::metadata(&canonical_path)?;
        if !metadata.is_file() {
            return Err(CoreError::Validation("所选路径不是普通文件。".to_owned()));
        }
        if metadata.len() > MAXIMUM_TEXT_BYTES {
            return Err(CoreError::Validation(
                "TXT 超过 160 MiB，当前原生阅读切片拒绝完整载入。".to_owned(),
            ));
        }
        let bytes = fs::read(&canonical_path)?;
        let decoded = decode_text(&bytes)?;
        let title = canonical_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| CoreError::Validation("TXT 文件名不是有效文本。".to_owned()))?
            .to_owned();
        let settings = self.load_settings()?;
        let document = ReaderDocument::from_opened(
            canonical_path.clone(),
            title.clone(),
            decoded,
            fingerprint_bytes(&bytes),
            &settings.txt,
            &settings.books.txt_chapter_pattern,
        );
        self.record_opened_txt(&canonical_path, &title)?;
        Ok(document)
    }

    pub fn save_txt(
        &self,
        document: &ReaderDocument,
        content: &str,
        options: SaveTextOptions,
    ) -> Result<ReaderDocument, CoreError> {
        let path = document
            .path()
            .ok_or_else(|| CoreError::Validation("内存 TXT 没有可保存的路径。".to_owned()))?;
        let expected = document.source_fingerprint().ok_or_else(|| {
            CoreError::Validation("TXT 缺少打开时的文件指纹，已拒绝覆盖。".to_owned())
        })?;
        let current = fs::read(path)?;
        if fingerprint_bytes(&current) != expected {
            return Err(CoreError::Validation(
                "TXT 已被其他程序修改；为避免覆盖，Readloom 已取消保存。".to_owned(),
            ));
        }
        let bytes = encode_for_document(document, content, options)?;
        replace_file_safely(path, &bytes, expected)?;
        self.open_txt(path)
            .map(|saved| saved.with_encoding_hint(options.encoding.unwrap_or(document.encoding())))
    }

    pub fn save_txt_as(
        &self,
        document: &ReaderDocument,
        target: &Path,
        content: &str,
        options: SaveTextOptions,
    ) -> Result<ReaderDocument, CoreError> {
        let parent = target
            .parent()
            .ok_or_else(|| CoreError::Validation("保存路径没有有效的父目录。".to_owned()))?;
        let parent = fs::canonicalize(parent)?;
        let file_name = target
            .file_name()
            .ok_or_else(|| CoreError::Validation("保存文件名无效。".to_owned()))?;
        let target = parent.join(file_name);
        if target
            .extension()
            .and_then(|value| value.to_str())
            .is_none_or(|value| !value.eq_ignore_ascii_case("txt"))
        {
            return Err(CoreError::Validation(
                "TXT 另存为路径必须使用 .txt 扩展名。".to_owned(),
            ));
        }
        if document.path().is_some_and(|source| source == target) {
            return self.save_txt(document, content, options);
        }
        let bytes = encode_for_document(document, content, options)?;
        if target.exists() {
            let expected = fingerprint_bytes(&fs::read(&target)?);
            replace_file_safely(&target, &bytes, &expected)?;
        } else {
            write_new_file_safely(&target, &bytes)?;
        }
        self.open_txt(&target)
            .map(|saved| saved.with_encoding_hint(options.encoding.unwrap_or(document.encoding())))
    }

    pub fn save_txt_draft(
        &self,
        document: &ReaderDocument,
        draft: &TxtDraft,
    ) -> Result<ReaderDocument, CoreError> {
        self.save_txt(document, &draft.materialize(), SaveTextOptions::PRESERVE)
    }

    pub fn save_txt_draft_as(
        &self,
        document: &ReaderDocument,
        target: &Path,
        draft: &TxtDraft,
    ) -> Result<ReaderDocument, CoreError> {
        self.save_txt_as(
            document,
            target,
            &draft.materialize(),
            SaveTextOptions::PRESERVE,
        )
    }

    pub fn open_epub(&self, path: &Path) -> Result<EpubDocument, CoreError> {
        let canonical_path = fs::canonicalize(path)?;
        let document = EpubDocument::open(&canonical_path)?;
        self.record_opened_epub(&document)?;
        Ok(document)
    }

    pub fn save_epub_draft(&self, draft: &EpubDraft) -> Result<EpubDocument, CoreError> {
        let document = crate::epub_edit::save_epub_draft(draft, draft.source_path())?;
        self.record_opened_epub(&document)?;
        Ok(document)
    }

    pub fn save_epub_draft_as(
        &self,
        draft: &EpubDraft,
        target: &Path,
    ) -> Result<EpubDocument, CoreError> {
        let document = crate::epub_edit::save_epub_draft(draft, target)?;
        self.record_opened_epub(&document)?;
        Ok(document)
    }

    pub fn load_settings(&self) -> Result<AppSettings, CoreError> {
        let value = self
            .lock_connection()?
            .query_row(
                "SELECT value_json FROM app_preferences
                 WHERE preference_key IN ('readloom.app-settings.v2', 'app_settings_v1')
                 ORDER BY CASE preference_key WHEN 'readloom.app-settings.v2' THEN 0 ELSE 1 END
                 LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(value) = value else {
            return Ok(AppSettings::default());
        };
        let settings = serde_json::from_str::<AppSettings>(&value)
            .map_err(|_| CoreError::Validation("保存的应用设置无效。".to_owned()))?;
        settings.normalized().map_err(CoreError::Validation)
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<AppSettings, CoreError> {
        let normalized = settings
            .clone()
            .normalized()
            .map_err(CoreError::Validation)?;
        let value = serde_json::to_string(&normalized)
            .map_err(|_| CoreError::Validation("无法序列化应用设置。".to_owned()))?;
        let connection = self.lock_connection()?;
        connection.execute(
            "INSERT INTO app_preferences (preference_key, value_json)
             VALUES ('readloom.app-settings.v2', ?1)
             ON CONFLICT(preference_key) DO UPDATE SET value_json = excluded.value_json",
            [value],
        )?;
        connection.execute(
            "DELETE FROM app_preferences WHERE preference_key = 'app_settings_v1'",
            [],
        )?;
        Ok(normalized)
    }

    pub fn library_snapshot(&self, maximum: usize) -> Result<LibrarySnapshot, CoreError> {
        let connection = self.lock_connection()?;
        let mut documents_statement = connection.prepare(
            "SELECT path, document_kind, display_title, author, last_opened_at_ms,
                    group_id, cover_key
             FROM library_entries ORDER BY last_opened_at_ms DESC LIMIT ?1",
        )?;
        let documents = documents_statement
            .query_map([maximum.clamp(1, 1_000) as i64], |row| {
                let path: String = row.get(0)?;
                let opened: i64 = row.get(4)?;
                Ok(LibraryDocument {
                    available: Path::new(&path).is_file(),
                    path,
                    document_kind: row.get(1)?,
                    display_title: row.get(2)?,
                    author: row.get(3)?,
                    last_opened_at_ms: opened.max(0) as u64,
                    group_id: row.get(5)?,
                    cover_key: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(documents_statement);
        let mut groups_statement = connection.prepare(
            "SELECT group_id, name, position
             FROM library_groups ORDER BY position ASC, name COLLATE NOCASE ASC",
        )?;
        let groups = groups_statement
            .query_map([], |row| {
                Ok(LibraryGroup {
                    group_id: row.get(0)?,
                    name: row.get(1)?,
                    position: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(LibrarySnapshot { documents, groups })
    }

    pub fn create_library_group(&self, name: &str) -> Result<LibraryGroup, CoreError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 60 {
            return Err(CoreError::Validation(
                "分组名称必须包含 1～60 个字符。".to_owned(),
            ));
        }
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction()?;
        let duplicate = transaction
            .query_row(
                "SELECT 1 FROM library_groups WHERE name = ?1 COLLATE NOCASE LIMIT 1",
                [name],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if duplicate {
            return Err(CoreError::Validation("已经存在同名分组。".to_owned()));
        }
        let position = transaction.query_row(
            "SELECT COALESCE(MIN(position), 1) - 1 FROM library_groups",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        let timestamp = now_ms()?;
        let group_id = blake3::hash(format!("library-group\0{name}\0{timestamp}").as_bytes())
            .to_hex()
            .to_string();
        transaction.execute(
            "INSERT INTO library_groups
               (group_id, name, position, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![&group_id, name, position, timestamp],
        )?;
        transaction.commit()?;
        Ok(LibraryGroup {
            group_id,
            name: name.to_owned(),
            position,
        })
    }

    pub fn move_library_book(
        &self,
        path: &Path,
        group_id: Option<&str>,
    ) -> Result<bool, CoreError> {
        let requested_path = path.to_string_lossy().into_owned();
        let canonical_path = fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .into_owned();
        let group_id = group_id.map(str::trim).filter(|value| !value.is_empty());
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction()?;
        if let Some(group_id) = group_id {
            let exists = transaction
                .query_row(
                    "SELECT 1 FROM library_groups WHERE group_id = ?1 LIMIT 1",
                    [group_id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !exists {
                return Err(CoreError::Validation("目标分组已不存在。".to_owned()));
            }
        }
        let updated = transaction.execute(
            "UPDATE library_entries SET group_id = ?1 WHERE path = ?2 OR path = ?3",
            params![group_id, &requested_path, &canonical_path],
        )?;
        transaction.commit()?;
        Ok(updated > 0)
    }

    pub fn delete_library_group(&self, group_id: &str) -> Result<bool, CoreError> {
        let group_id = group_id.trim();
        if group_id.is_empty() {
            return Err(CoreError::Validation("分组标识不能为空。".to_owned()));
        }
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction()?;
        let deleted =
            transaction.execute("DELETE FROM library_groups WHERE group_id = ?1", [group_id])?;
        transaction.commit()?;
        Ok(deleted > 0)
    }

    pub fn reorder_library_group(
        &self,
        group_id: &str,
        target_index: usize,
    ) -> Result<bool, CoreError> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction()?;
        let mut group_ids = {
            let mut statement = transaction.prepare(
                "SELECT group_id FROM library_groups
                 ORDER BY position ASC, name COLLATE NOCASE ASC",
            )?;
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let Some(current_index) = group_ids.iter().position(|id| id == group_id) else {
            return Ok(false);
        };
        let target_index = target_index.min(group_ids.len().saturating_sub(1));
        if current_index == target_index {
            return Ok(false);
        }
        let moved_group_id = group_ids.remove(current_index);
        group_ids.insert(target_index, moved_group_id);
        let timestamp = now_ms()?;
        for (position, group_id) in group_ids.iter().enumerate() {
            transaction.execute(
                "UPDATE library_groups SET position = ?1, updated_at_ms = ?2 WHERE group_id = ?3",
                params![position as i64, timestamp, group_id],
            )?;
        }
        transaction.commit()?;
        Ok(true)
    }

    pub fn clean_invalid_library_entries(&self) -> Result<usize, CoreError> {
        let mut connection = self.lock_connection()?;
        let invalid_paths = {
            let mut statement = connection.prepare("SELECT path FROM library_entries")?;
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|path| !Path::new(path).is_file())
                .collect::<Vec<_>>()
        };
        let transaction = connection.transaction()?;
        for path in &invalid_paths {
            transaction.execute("DELETE FROM library_entries WHERE path = ?1", [path])?;
            transaction.execute(
                "DELETE FROM reading_progress WHERE document_path = ?1",
                [path],
            )?;
            transaction.execute("DELETE FROM bookmarks WHERE document_path = ?1", [path])?;
            transaction.execute("DELETE FROM recent_documents WHERE path = ?1", [path])?;
        }
        transaction.commit()?;
        Ok(invalid_paths.len())
    }

    /// Removes a book and its document-scoped application state without touching the source file.
    pub fn remove_from_library(&self, path: &Path) -> Result<bool, CoreError> {
        let requested_path = path.to_string_lossy().into_owned();
        let canonical_path = fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .into_owned();
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction()?;
        let removed = transaction.execute(
            "DELETE FROM library_entries WHERE path = ?1 OR path = ?2",
            params![&requested_path, &canonical_path],
        )?;
        transaction.execute(
            "DELETE FROM reading_progress WHERE document_path = ?1 OR document_path = ?2",
            params![&requested_path, &canonical_path],
        )?;
        transaction.execute(
            "DELETE FROM bookmarks WHERE document_path = ?1 OR document_path = ?2",
            params![&requested_path, &canonical_path],
        )?;
        transaction.execute(
            "DELETE FROM recent_documents WHERE path = ?1 OR path = ?2",
            params![&requested_path, &canonical_path],
        )?;
        transaction.commit()?;
        Ok(removed > 0)
    }

    pub fn save_text_locator(
        &self,
        document: &ReaderDocument,
        locator: &TextReadingLocator,
    ) -> Result<(), CoreError> {
        let path = document
            .path()
            .ok_or_else(|| CoreError::Validation("内存 TXT 没有可持久化的路径。".to_owned()))?;
        if locator.version != 1 || locator.line_number == 0 {
            return Err(CoreError::Validation("TXT 阅读位置无效。".to_owned()));
        }
        let locator_json = serde_json::to_string(locator)
            .map_err(|_| CoreError::Validation("无法保存 TXT 阅读位置。".to_owned()))?;
        self.lock_connection()?.execute(
            "INSERT INTO reading_progress
               (document_path, document_kind, fingerprint, locator_json, updated_at_ms)
             VALUES (?1, 'txt', '', ?2, ?3)
             ON CONFLICT(document_path) DO UPDATE SET
               document_kind = excluded.document_kind,
               fingerprint = excluded.fingerprint,
               locator_json = excluded.locator_json,
               updated_at_ms = excluded.updated_at_ms",
            params![path.to_string_lossy().as_ref(), locator_json, now_ms()?],
        )?;
        Ok(())
    }

    pub fn add_text_bookmark(
        &self,
        document: &ReaderDocument,
        paragraph_index: usize,
    ) -> Result<(), CoreError> {
        let path = document
            .path()
            .ok_or_else(|| CoreError::Validation("内存 TXT 无法添加书签。".to_owned()))?;
        let locator = document.locator_for_paragraph(paragraph_index, 0);
        let chapter_title = document
            .chapters()
            .get(locator.chapter_index.unwrap_or_default())
            .map_or("全文", |chapter| chapter.title.as_str());
        self.insert_bookmark(
            path,
            "txt",
            document.source_fingerprint().unwrap_or_default(),
            &serde_json::to_string(&locator)
                .map_err(|_| CoreError::Validation("无法序列化 TXT 书签。".to_owned()))?,
            chapter_title,
        )
    }

    pub fn add_epub_bookmark(
        &self,
        document: &EpubDocument,
        paragraph_index: usize,
    ) -> Result<(), CoreError> {
        let locator = document.locator_for_paragraph(paragraph_index);
        let chapter_title = document
            .chapters()
            .get(locator.chapter_index)
            .map_or("EPUB", |chapter| chapter.title.as_str());
        self.insert_bookmark(
            document.path(),
            "epub",
            document.fingerprint(),
            &serde_json::to_string(&locator)
                .map_err(|_| CoreError::Validation("无法序列化 EPUB 书签。".to_owned()))?,
            chapter_title,
        )
    }

    fn insert_bookmark(
        &self,
        path: &Path,
        kind: &str,
        fingerprint: &str,
        locator_json: &str,
        chapter_title: &str,
    ) -> Result<(), CoreError> {
        let now = now_ms()?;
        let bookmark_id = blake3::hash(
            format!("{}\0{kind}\0{now}\0{locator_json}", path.to_string_lossy()).as_bytes(),
        )
        .to_hex()
        .to_string();
        self.lock_connection()?.execute(
            "INSERT INTO bookmarks (
               bookmark_id, document_path, document_kind, fingerprint, locator_json,
               title, chapter_title, created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?7)",
            params![
                bookmark_id,
                path.to_string_lossy(),
                kind,
                fingerprint,
                locator_json,
                chapter_title.chars().take(512).collect::<String>(),
                now,
            ],
        )?;
        Ok(())
    }

    pub fn bookmarks_for_path(&self, path: &Path) -> Result<Vec<StoredBookmark>, CoreError> {
        let requested_path = path.to_string_lossy().into_owned();
        let canonical_path = fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .into_owned();
        let connection = self.lock_connection()?;
        let mut statement = connection.prepare(
            "SELECT bookmark_id, document_kind, locator_json, title, chapter_title, created_at_ms
             FROM bookmarks WHERE document_path = ?1 OR document_path = ?2
             ORDER BY created_at_ms ASC, bookmark_id ASC",
        )?;
        let rows = statement
            .query_map(params![requested_path, canonical_path], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(
                |(bookmark_id, document_kind, locator_json, title, chapter_title, created_at)| {
                    let (locator_version, chapter_index, paragraph_index) = if document_kind
                        == "epub"
                    {
                        serde_json::from_str::<EpubReadingLocator>(&locator_json).map(|locator| {
                            (
                                locator.version,
                                Some(locator.chapter_index),
                                locator.paragraph_index,
                            )
                        })
                    } else {
                        serde_json::from_str::<TextReadingLocator>(&locator_json).map(|locator| {
                            (
                                locator.version,
                                locator.chapter_index,
                                locator
                                    .paragraph_index
                                    .unwrap_or_else(|| locator.line_number.saturating_sub(1)),
                            )
                        })
                    }
                    .map_err(|_| CoreError::Validation("保存的书签位置无效。".to_owned()))?;
                    Ok(StoredBookmark {
                        bookmark_id,
                        document_kind,
                        title,
                        chapter_title,
                        locator_version,
                        chapter_index,
                        paragraph_index,
                        created_at_ms: created_at.max(0) as u64,
                    })
                },
            )
            .collect()
    }

    pub fn delete_bookmark(&self, bookmark_id: &str) -> Result<bool, CoreError> {
        Ok(self.lock_connection()?.execute(
            "DELETE FROM bookmarks WHERE bookmark_id = ?1",
            [bookmark_id],
        )? > 0)
    }

    pub fn load_text_locator(
        &self,
        document: &ReaderDocument,
    ) -> Result<Option<TextReadingLocator>, CoreError> {
        let path = document
            .path()
            .ok_or_else(|| CoreError::Validation("内存 TXT 没有可恢复的路径。".to_owned()))?;
        let locator_json = self
            .lock_connection()?
            .query_row(
                "SELECT locator_json FROM reading_progress
                 WHERE document_path = ?1 AND document_kind = 'txt'",
                [path.to_string_lossy().as_ref()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(locator_json) = locator_json else {
            return Ok(None);
        };
        let locator: TextReadingLocator = serde_json::from_str(&locator_json)
            .map_err(|_| CoreError::Validation("保存的 TXT 阅读位置无效。".to_owned()))?;
        if locator.version != 1 || locator.line_number == 0 {
            return Ok(None);
        }
        Ok(Some(locator))
    }

    pub fn save_epub_locator(
        &self,
        document: &EpubDocument,
        locator: &EpubReadingLocator,
    ) -> Result<(), CoreError> {
        if !matches!(locator.version, 1 | 2) {
            return Err(CoreError::Validation("EPUB 阅读位置无效。".to_owned()));
        }
        let locator_json = serde_json::to_string(locator)
            .map_err(|_| CoreError::Validation("无法保存 EPUB 阅读位置。".to_owned()))?;
        self.lock_connection()?.execute(
            "INSERT INTO reading_progress
               (document_path, document_kind, fingerprint, locator_json, updated_at_ms)
             VALUES (?1, 'epub', ?2, ?3, ?4)
             ON CONFLICT(document_path) DO UPDATE SET
               document_kind = excluded.document_kind,
               fingerprint = excluded.fingerprint,
               locator_json = excluded.locator_json,
               updated_at_ms = excluded.updated_at_ms",
            params![
                document.path().to_string_lossy().as_ref(),
                document.fingerprint(),
                locator_json,
                now_ms()?
            ],
        )?;
        Ok(())
    }

    pub fn load_epub_locator(
        &self,
        document: &EpubDocument,
    ) -> Result<Option<EpubReadingLocator>, CoreError> {
        let stored = self
            .lock_connection()?
            .query_row(
                "SELECT fingerprint, locator_json FROM reading_progress
                 WHERE document_path = ?1 AND document_kind = 'epub'",
                [document.path().to_string_lossy().as_ref()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((fingerprint, locator_json)) = stored else {
            return Ok(None);
        };
        if fingerprint != document.fingerprint() {
            return Ok(None);
        }
        let locator = serde_json::from_str::<EpubReadingLocator>(&locator_json)
            .map_err(|_| CoreError::Validation("保存的 EPUB 阅读位置无效。".to_owned()))?;
        Ok(matches!(locator.version, 1 | 2).then_some(locator))
    }

    fn record_opened_txt(&self, path: &Path, title: &str) -> Result<(), CoreError> {
        let path = path.to_string_lossy();
        let now = now_ms()?;
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO recent_documents
               (path, document_kind, display_title, author, fingerprint, last_opened_at_ms)
             VALUES (?1, 'txt', ?2, NULL, NULL, ?3)
             ON CONFLICT(path) DO UPDATE SET
               document_kind = excluded.document_kind,
               display_title = excluded.display_title,
               last_opened_at_ms = excluded.last_opened_at_ms",
            params![path.as_ref(), title, now],
        )?;
        transaction.execute(
            "INSERT INTO library_entries
               (path, document_kind, display_title, author, fingerprint, last_opened_at_ms,
                group_id, cover_key, cover_resource_id, cover_media_type, metadata_scanned)
             VALUES (?1, 'txt', ?2, NULL, NULL, ?3, NULL, NULL, NULL, NULL, 1)
             ON CONFLICT(path) DO UPDATE SET
               document_kind = excluded.document_kind,
               display_title = excluded.display_title,
               last_opened_at_ms = excluded.last_opened_at_ms",
            params![path.as_ref(), title, now],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn record_opened_epub(&self, document: &EpubDocument) -> Result<(), CoreError> {
        let path = document.path().to_string_lossy();
        let now = now_ms()?;
        let (cover_key, cover_media_type) = if let Some(cover) = document.cover() {
            let extension = match cover.media_type.as_str() {
                "image/png" => "png",
                "image/jpeg" | "image/jpg" => "jpg",
                "image/webp" => "webp",
                "image/gif" => "gif",
                _ => "img",
            };
            let key = format!("{}.{}", document.fingerprint(), extension);
            let directory = self.application_data_dir.join("covers");
            fs::create_dir_all(&directory)?;
            let target = directory.join(&key);
            if !target.is_file() {
                fs::write(&target, &cover.bytes)?;
            }
            (Some(key), Some(cover.media_type.clone()))
        } else {
            (None, None)
        };
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO recent_documents
               (path, document_kind, display_title, author, fingerprint, last_opened_at_ms)
             VALUES (?1, 'epub', ?2, ?3, ?4, ?5)
             ON CONFLICT(path) DO UPDATE SET
               document_kind = excluded.document_kind,
               display_title = excluded.display_title,
               author = excluded.author,
               fingerprint = excluded.fingerprint,
               last_opened_at_ms = excluded.last_opened_at_ms",
            params![
                path.as_ref(),
                document.title(),
                document.author(),
                document.fingerprint(),
                now
            ],
        )?;
        transaction.execute(
            "INSERT INTO library_entries
               (path, document_kind, display_title, author, fingerprint, last_opened_at_ms,
                group_id, cover_key, cover_resource_id, cover_media_type, metadata_scanned)
             VALUES (?1, 'epub', ?2, ?3, ?4, ?5, NULL, ?6, NULL, ?7, 1)
             ON CONFLICT(path) DO UPDATE SET
               document_kind = excluded.document_kind,
               display_title = excluded.display_title,
               author = excluded.author,
               fingerprint = excluded.fingerprint,
               last_opened_at_ms = excluded.last_opened_at_ms,
               cover_key = excluded.cover_key,
               cover_media_type = excluded.cover_media_type,
               metadata_scanned = 1",
            params![
                path.as_ref(),
                document.title(),
                document.author(),
                document.fingerprint(),
                now,
                cover_key,
                cover_media_type,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn lock_connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>, CoreError> {
        self.connection
            .lock()
            .map_err(|_| CoreError::Validation("本地状态暂时不可用。".to_owned()))
    }
}

fn migrate(connection: &mut Connection) -> Result<(), CoreError> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(CoreError::Validation(
            "本地状态由更新版本的 Readloom 创建。".to_owned(),
        ));
    }
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    let transaction = connection.transaction()?;
    if version < 1 {
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS recent_documents (
               path TEXT PRIMARY KEY NOT NULL,
               document_kind TEXT NOT NULL CHECK(document_kind IN ('txt', 'epub')),
               display_title TEXT NOT NULL,
               author TEXT,
               fingerprint TEXT,
               last_opened_at_ms INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS recent_documents_opened_idx
               ON recent_documents(last_opened_at_ms DESC);
             CREATE TABLE IF NOT EXISTS reading_progress (
               document_path TEXT PRIMARY KEY NOT NULL,
               document_kind TEXT NOT NULL CHECK(document_kind IN ('txt', 'epub')),
               fingerprint TEXT NOT NULL,
               locator_json TEXT NOT NULL CHECK(length(locator_json) <= 16384),
               updated_at_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS bookmarks (
               bookmark_id TEXT PRIMARY KEY NOT NULL,
               document_path TEXT NOT NULL,
               document_kind TEXT NOT NULL CHECK(document_kind IN ('txt', 'epub')),
               fingerprint TEXT NOT NULL,
               locator_json TEXT NOT NULL CHECK(length(locator_json) <= 16384),
               title TEXT CHECK(title IS NULL OR length(title) <= 256),
               chapter_title TEXT NOT NULL CHECK(length(chapter_title) <= 512),
               created_at_ms INTEGER NOT NULL,
               updated_at_ms INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS bookmarks_document_idx
               ON bookmarks(document_path, created_at_ms);",
        )?;
    }
    if version < 2 {
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS library_groups (
               group_id TEXT PRIMARY KEY NOT NULL,
               name TEXT NOT NULL COLLATE NOCASE UNIQUE CHECK(length(name) BETWEEN 1 AND 64),
               position INTEGER NOT NULL,
               created_at_ms INTEGER NOT NULL,
               updated_at_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS library_entries (
               path TEXT PRIMARY KEY NOT NULL,
               document_kind TEXT NOT NULL CHECK(document_kind IN ('txt', 'epub')),
               display_title TEXT NOT NULL,
               author TEXT,
               fingerprint TEXT,
               last_opened_at_ms INTEGER NOT NULL,
               group_id TEXT REFERENCES library_groups(group_id) ON DELETE SET NULL
             );
             CREATE INDEX IF NOT EXISTS library_entries_opened_idx
               ON library_entries(last_opened_at_ms DESC);
             CREATE INDEX IF NOT EXISTS library_entries_group_idx
               ON library_entries(group_id, last_opened_at_ms DESC);
             INSERT OR IGNORE INTO library_entries
               (path, document_kind, display_title, author, fingerprint, last_opened_at_ms)
             SELECT path, document_kind, display_title, author, fingerprint, last_opened_at_ms
             FROM recent_documents;",
        )?;
    }
    if version < 3 {
        transaction.execute_batch(
            "ALTER TABLE library_entries ADD COLUMN cover_key TEXT;
             ALTER TABLE library_entries ADD COLUMN cover_resource_id TEXT;
             ALTER TABLE library_entries ADD COLUMN cover_media_type TEXT;
             ALTER TABLE library_entries ADD COLUMN metadata_scanned INTEGER NOT NULL DEFAULT 0;
             CREATE INDEX IF NOT EXISTS library_entries_cover_idx ON library_entries(cover_key);",
        )?;
    }
    if version < 4 {
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS app_preferences (
               preference_key TEXT PRIMARY KEY NOT NULL,
               value_json TEXT NOT NULL CHECK(length(value_json) <= 32768)
             );",
        )?;
    }
    transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    transaction.commit()?;
    Ok(())
}

fn now_ms() -> Result<i64, CoreError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CoreError::Validation("系统时间无效。".to_owned()))?
        .as_millis();
    Ok(millis.min(i64::MAX as u128) as i64)
}

fn fingerprint_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

fn encode_for_document(
    document: &ReaderDocument,
    content: &str,
    options: SaveTextOptions,
) -> Result<Vec<u8>, CoreError> {
    let encoding = options.encoding.unwrap_or(document.encoding());
    let has_bom = options.has_bom.unwrap_or(document.has_bom());
    let line_ending = options.line_ending.unwrap_or_else(|| {
        if document.primary_line_ending() == LineEnding::None {
            LineEnding::Lf
        } else {
            document.primary_line_ending()
        }
    });
    encode_text(content, encoding, has_bom, line_ending)
}

fn write_new_file_safely(target: &Path, bytes: &[u8]) -> Result<(), CoreError> {
    let parent = target
        .parent()
        .ok_or_else(|| CoreError::Validation("TXT 路径没有有效的父目录。".to_owned()))?;
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CoreError::Validation("TXT 文件名不是有效文本。".to_owned()))?;
    let (mut temporary, temporary_path) = create_save_artifact(parent, file_name, "tmp")?;
    if let Err(error) = temporary
        .write_all(bytes)
        .and_then(|_| temporary.flush())
        .and_then(|_| temporary.sync_all())
    {
        drop(temporary);
        let _ = fs::remove_file(&temporary_path);
        return Err(error.into());
    }
    drop(temporary);
    if target.exists() {
        let _ = fs::remove_file(&temporary_path);
        return Err(CoreError::Validation(
            "另存为目标在确认后已被创建，请重新选择文件名。".to_owned(),
        ));
    }
    fs::rename(&temporary_path, target)?;
    if fingerprint_bytes(&fs::read(target)?) != fingerprint_bytes(bytes) {
        let _ = fs::remove_file(target);
        return Err(CoreError::Validation(
            "另存为后的 TXT 校验失败。".to_owned(),
        ));
    }
    Ok(())
}

fn replace_file_safely(target: &Path, bytes: &[u8], expected: &str) -> Result<(), CoreError> {
    if fs::metadata(target)?.permissions().readonly() {
        return Err(CoreError::Validation(
            "目标 TXT 是只读文件，请取消只读属性后再保存。".to_owned(),
        ));
    }
    let parent = target
        .parent()
        .ok_or_else(|| CoreError::Validation("TXT 路径没有有效的父目录。".to_owned()))?;
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CoreError::Validation("TXT 文件名不是有效文本。".to_owned()))?;
    let (mut temporary, temporary_path) = create_save_artifact(parent, file_name, "tmp")?;
    if let Err(error) = temporary
        .write_all(bytes)
        .and_then(|_| temporary.flush())
        .and_then(|_| temporary.sync_all())
    {
        drop(temporary);
        let _ = fs::remove_file(&temporary_path);
        return Err(error.into());
    }
    drop(temporary);
    if fingerprint_bytes(&fs::read(target)?) != expected {
        let _ = fs::remove_file(&temporary_path);
        return Err(CoreError::Validation(
            "TXT 在保存过程中被其他程序修改，已取消覆盖。".to_owned(),
        ));
    }
    let (_, backup_path) = create_save_artifact(parent, file_name, "backup")?;
    fs::remove_file(&backup_path)?;
    if let Err(error) = fs::rename(target, &backup_path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(error.into());
    }
    if let Err(error) = fs::rename(&temporary_path, target) {
        let _ = fs::rename(&backup_path, target);
        let _ = fs::remove_file(&temporary_path);
        return Err(error.into());
    }
    if fingerprint_bytes(&fs::read(target)?) != fingerprint_bytes(bytes) {
        let _ = fs::remove_file(target);
        let _ = fs::rename(&backup_path, target);
        return Err(CoreError::Validation(
            "保存后的 TXT 校验失败，原文件已恢复。".to_owned(),
        ));
    }
    let _ = fs::remove_file(backup_path);
    Ok(())
}

pub(crate) fn create_save_artifact(
    parent: &Path,
    file_name: &str,
    kind: &str,
) -> Result<(fs::File, PathBuf), CoreError> {
    for _ in 0..128 {
        let id = SAVE_ARTIFACT_ID.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".{file_name}.readloom-{kind}-{}-{id:016x}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
        {
            Ok(file) => return Ok((file, path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(CoreError::Validation(
        "无法创建不冲突的 TXT 临时文件。".to_owned(),
    ))
}

pub(crate) fn install_validated_file(
    target: &Path,
    temporary_path: &Path,
    expected_target: Option<&str>,
    document_kind: &str,
) -> Result<(), CoreError> {
    if target.exists() && fs::metadata(target)?.permissions().readonly() {
        let _ = fs::remove_file(temporary_path);
        return Err(CoreError::Validation(format!(
            "目标 {document_kind} 是只读文件，请取消只读属性后再保存。"
        )));
    }
    let temporary_fingerprint = crate::epub::fingerprint_file(temporary_path)?;
    match expected_target {
        Some(expected) => {
            if !target.exists() || crate::epub::fingerprint_file(target)? != expected {
                let _ = fs::remove_file(temporary_path);
                return Err(CoreError::Validation(format!(
                    "{document_kind} 在保存过程中被其他程序修改，已取消覆盖。"
                )));
            }
            let parent = target.parent().ok_or_else(|| {
                CoreError::Validation(format!("{document_kind} 路径没有有效的父目录。"))
            })?;
            let file_name = target
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| CoreError::Validation(format!("{document_kind} 文件名无效。")))?;
            let (_, backup_path) = create_save_artifact(parent, file_name, "backup")?;
            fs::remove_file(&backup_path)?;
            if let Err(error) = fs::rename(target, &backup_path) {
                let _ = fs::remove_file(temporary_path);
                return Err(error.into());
            }
            if let Err(error) = fs::rename(temporary_path, target) {
                let _ = fs::rename(&backup_path, target);
                let _ = fs::remove_file(temporary_path);
                return Err(error.into());
            }
            if crate::epub::fingerprint_file(target)? != temporary_fingerprint {
                let _ = fs::remove_file(target);
                let _ = fs::rename(&backup_path, target);
                return Err(CoreError::Validation(format!(
                    "保存后的 {document_kind} 校验失败，原文件已恢复。"
                )));
            }
            let _ = fs::remove_file(backup_path);
        }
        None => {
            if target.exists() {
                let _ = fs::remove_file(temporary_path);
                return Err(CoreError::Validation(
                    "另存为目标在确认后已被创建，请重新选择文件名。".to_owned(),
                ));
            }
            fs::rename(temporary_path, target)?;
            if crate::epub::fingerprint_file(target)? != temporary_fingerprint {
                let _ = fs::remove_file(target);
                return Err(CoreError::Validation(format!(
                    "另存为后的 {document_kind} 校验失败。"
                )));
            }
        }
    }
    Ok(())
}
