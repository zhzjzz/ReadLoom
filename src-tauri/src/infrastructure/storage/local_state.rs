use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        epub_document::{EpubBookmark, EpubLocator},
        text_document::TextBookmark,
    },
    error::AppError,
};

const SCHEMA_VERSION: i64 = 4;

#[derive(Clone)]
pub(crate) struct LocalStateStore {
    connection: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecentDocument {
    pub path: String,
    pub document_kind: String,
    pub display_title: String,
    pub author: Option<String>,
    pub fingerprint: Option<String>,
    pub last_opened_at_ms: u64,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryDocument {
    pub path: String,
    pub document_kind: String,
    pub display_title: String,
    pub author: Option<String>,
    pub fingerprint: Option<String>,
    pub last_opened_at_ms: u64,
    pub available: bool,
    pub group_id: Option<String>,
    pub cover_key: Option<String>,
    #[serde(skip)]
    pub metadata_scanned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryCoverSource {
    pub path: std::path::PathBuf,
    pub resource_id: String,
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackgroundImageSource {
    pub path: std::path::PathBuf,
    pub key: String,
    pub media_type: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TextProgressLocator {
    version: u8,
    character_offset: usize,
    line_number: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryGroup {
    pub group_id: String,
    pub name: String,
    pub position: i64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibrarySnapshot {
    pub documents: Vec<LibraryDocument>,
    pub groups: Vec<LibraryGroup>,
}

pub(crate) struct RecentDocumentRecord<'a> {
    pub path: &'a Path,
    pub document_kind: &'a str,
    pub display_title: &'a str,
    pub author: Option<&'a str>,
    pub fingerprint: Option<&'a str>,
    pub cover_resource_id: Option<&'a str>,
    pub cover_media_type: Option<&'a str>,
}

pub(crate) type LibraryDocumentRecord<'a> = RecentDocumentRecord<'a>;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TextBookmarkLocator {
    version: u8,
    character_offset: usize,
    line_number: usize,
}

impl LocalStateStore {
    pub(crate) fn open(path: &Path) -> Result<Self, AppError> {
        let mut connection = Connection::open(path).map_err(storage_error)?;
        configure(&connection)?;
        migrate(&mut connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    #[cfg(test)]
    fn in_memory() -> Result<Self, AppError> {
        let mut connection = Connection::open_in_memory().map_err(storage_error)?;
        configure(&connection)?;
        migrate(&mut connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub(crate) fn record_document_opened(
        &self,
        record: RecentDocumentRecord<'_>,
    ) -> Result<(), AppError> {
        let path = record.path.to_string_lossy();
        let now = now_ms()?;
        let mut connection = self.connection.lock().map_err(|_| storage_lock_error())?;
        let transaction = connection.transaction().map_err(storage_error)?;
        transaction
            .execute(
                "INSERT INTO recent_documents
                   (path, document_kind, display_title, author, fingerprint, last_opened_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(path) DO UPDATE SET
                   document_kind = excluded.document_kind,
                   display_title = excluded.display_title,
                   author = excluded.author,
                   fingerprint = excluded.fingerprint,
                   last_opened_at_ms = excluded.last_opened_at_ms",
                params![
                    path.as_ref(),
                    record.document_kind,
                    record.display_title,
                    record.author,
                    record.fingerprint,
                    to_sql_integer(now)?,
                ],
            )
            .map_err(storage_error)?;
        upsert_library_entry(&transaction, &record, now)?;
        transaction.commit().map_err(storage_error)?;
        Ok(())
    }

    pub(crate) fn record_library_document(
        &self,
        record: LibraryDocumentRecord<'_>,
    ) -> Result<(), AppError> {
        let now = now_ms()?;
        let connection = self.connection.lock().map_err(|_| storage_lock_error())?;
        upsert_library_entry(&connection, &record, now)
    }

    pub(crate) fn recent_documents(&self, maximum: usize) -> Result<Vec<RecentDocument>, AppError> {
        let connection = self.connection.lock().map_err(|_| storage_lock_error())?;
        let mut statement = connection
            .prepare(
                "SELECT path, document_kind, display_title, author, fingerprint, last_opened_at_ms
                 FROM recent_documents ORDER BY last_opened_at_ms DESC LIMIT ?1",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([maximum.clamp(1, 100) as i64], |row| {
                let timestamp: i64 = row.get(5)?;
                let path: String = row.get(0)?;
                let available = Path::new(&path).is_file();
                Ok(RecentDocument {
                    path,
                    document_kind: row.get(1)?,
                    display_title: row.get(2)?,
                    author: row.get(3)?,
                    fingerprint: row.get(4)?,
                    last_opened_at_ms: timestamp.max(0) as u64,
                    available,
                })
            })
            .map_err(storage_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(storage_error)
    }

    pub(crate) fn delete_recent(&self, document_path: &Path) -> Result<(), AppError> {
        let path = document_path.to_string_lossy();
        self.connection
            .lock()
            .map_err(|_| storage_lock_error())?
            .execute(
                "DELETE FROM recent_documents WHERE path = ?1",
                [path.as_ref()],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub(crate) fn library_snapshot(&self, maximum: usize) -> Result<LibrarySnapshot, AppError> {
        let connection = self.connection.lock().map_err(|_| storage_lock_error())?;
        let mut document_statement = connection
            .prepare(
                "SELECT path, document_kind, display_title, author, fingerprint,
                        last_opened_at_ms, group_id, cover_key, metadata_scanned
                 FROM library_entries ORDER BY last_opened_at_ms DESC LIMIT ?1",
            )
            .map_err(storage_error)?;
        let document_rows = document_statement
            .query_map([maximum.clamp(1, 1000) as i64], |row| {
                let timestamp: i64 = row.get(5)?;
                let path: String = row.get(0)?;
                let available = Path::new(&path).is_file();
                Ok(LibraryDocument {
                    path,
                    document_kind: row.get(1)?,
                    display_title: row.get(2)?,
                    author: row.get(3)?,
                    fingerprint: row.get(4)?,
                    last_opened_at_ms: timestamp.max(0) as u64,
                    available,
                    group_id: row.get(6)?,
                    cover_key: row.get(7)?,
                    metadata_scanned: row.get::<_, i64>(8)? != 0,
                })
            })
            .map_err(storage_error)?;
        let documents = document_rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        drop(document_statement);

        let mut group_statement = connection
            .prepare(
                "SELECT group_id, name, position, created_at_ms, updated_at_ms
                 FROM library_groups ORDER BY position ASC, name COLLATE NOCASE ASC",
            )
            .map_err(storage_error)?;
        let group_rows = group_statement
            .query_map([], |row| {
                let created: i64 = row.get(3)?;
                let updated: i64 = row.get(4)?;
                Ok(LibraryGroup {
                    group_id: row.get(0)?,
                    name: row.get(1)?,
                    position: row.get(2)?,
                    created_at_ms: created.max(0) as u64,
                    updated_at_ms: updated.max(0) as u64,
                })
            })
            .map_err(storage_error)?;
        let groups = group_rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        Ok(LibrarySnapshot { documents, groups })
    }

    pub(crate) fn library_document_paths(&self) -> Result<Vec<std::path::PathBuf>, AppError> {
        let connection = self.connection.lock().map_err(|_| storage_lock_error())?;
        let mut statement = connection
            .prepare("SELECT path FROM library_entries ORDER BY path COLLATE NOCASE ASC")
            .map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| {
                row.get::<_, String>(0).map(std::path::PathBuf::from)
            })
            .map_err(storage_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(storage_error)
    }

    pub(crate) fn update_library_epub_metadata(
        &self,
        document_path: &Path,
        display_title: &str,
        author: Option<&str>,
        cover_resource_id: Option<&str>,
        cover_media_type: Option<&str>,
    ) -> Result<(), AppError> {
        let path = document_path.to_string_lossy();
        let cover_key = cover_resource_id
            .zip(cover_media_type)
            .map(|_| library_cover_key(document_path, None));
        self.connection
            .lock()
            .map_err(|_| storage_lock_error())?
            .execute(
                "UPDATE library_entries SET
                   display_title = ?2,
                   author = ?3,
                   cover_key = ?4,
                   cover_resource_id = ?5,
                   cover_media_type = ?6,
                   metadata_scanned = 1
                 WHERE path = ?1",
                params![
                    path.as_ref(),
                    display_title,
                    author,
                    cover_key,
                    cover_resource_id,
                    cover_media_type,
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub(crate) fn mark_library_metadata_scanned(
        &self,
        document_path: &Path,
    ) -> Result<(), AppError> {
        let path = document_path.to_string_lossy();
        self.connection
            .lock()
            .map_err(|_| storage_lock_error())?
            .execute(
                "UPDATE library_entries SET metadata_scanned = 1 WHERE path = ?1",
                [path.as_ref()],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub(crate) fn library_cover_source(
        &self,
        cover_key: &str,
    ) -> Result<Option<LibraryCoverSource>, AppError> {
        self.connection
            .lock()
            .map_err(|_| storage_lock_error())?
            .query_row(
                "SELECT path, cover_resource_id, cover_media_type
                 FROM library_entries
                 WHERE cover_key = ?1 AND cover_resource_id IS NOT NULL
                   AND cover_media_type IS NOT NULL",
                [cover_key],
                |row| {
                    Ok(LibraryCoverSource {
                        path: std::path::PathBuf::from(row.get::<_, String>(0)?),
                        resource_id: row.get(1)?,
                        media_type: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(storage_error)
    }

    pub(crate) fn create_library_group(
        &self,
        group_id: &str,
        name: &str,
    ) -> Result<LibraryGroup, AppError> {
        let now = now_ms()?;
        let timestamp = to_sql_integer(now)?;
        let connection = self.connection.lock().map_err(|_| storage_lock_error())?;
        connection
            .execute(
                "INSERT INTO library_groups
                   (group_id, name, position, created_at_ms, updated_at_ms)
                 SELECT ?1, ?2, COALESCE(MAX(position) + 1, 0), ?3, ?3
                 FROM library_groups",
                params![group_id, name, timestamp],
            )
            .map_err(storage_error)?;
        let position = connection
            .query_row(
                "SELECT position FROM library_groups WHERE group_id = ?1",
                [group_id],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        Ok(LibraryGroup {
            group_id: group_id.to_owned(),
            name: name.to_owned(),
            position,
            created_at_ms: now,
            updated_at_ms: now,
        })
    }

    pub(crate) fn rename_library_group(&self, group_id: &str, name: &str) -> Result<(), AppError> {
        let changed = self
            .connection
            .lock()
            .map_err(|_| storage_lock_error())?
            .execute(
                "UPDATE library_groups SET name = ?2, updated_at_ms = ?3 WHERE group_id = ?1",
                params![group_id, name, to_sql_integer(now_ms()?)?],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(AppError::validation(
                "LIBRARY_GROUP_NOT_FOUND",
                "书架分组不存在。",
                "刷新书库后重试。",
            ));
        }
        Ok(())
    }

    pub(crate) fn delete_library_group(&self, group_id: &str) -> Result<(), AppError> {
        let changed = self
            .connection
            .lock()
            .map_err(|_| storage_lock_error())?
            .execute("DELETE FROM library_groups WHERE group_id = ?1", [group_id])
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(AppError::validation(
                "LIBRARY_GROUP_NOT_FOUND",
                "书架分组不存在。",
                "刷新书库后重试。",
            ));
        }
        Ok(())
    }

    pub(crate) fn assign_library_group(
        &self,
        document_path: &Path,
        group_id: Option<&str>,
    ) -> Result<(), AppError> {
        let path = document_path.to_string_lossy();
        let changed = self
            .connection
            .lock()
            .map_err(|_| storage_lock_error())?
            .execute(
                "UPDATE library_entries SET group_id = ?2 WHERE path = ?1",
                params![path.as_ref(), group_id],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(AppError::validation(
                "LIBRARY_DOCUMENT_NOT_FOUND",
                "书库中没有这本书。",
                "刷新书库后重试。",
            ));
        }
        Ok(())
    }

    pub(crate) fn remove_library_document(&self, document_path: &Path) -> Result<(), AppError> {
        let path = document_path.to_string_lossy();
        self.connection
            .lock()
            .map_err(|_| storage_lock_error())?
            .execute(
                "DELETE FROM library_entries WHERE path = ?1",
                [path.as_ref()],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub(crate) fn remove_unavailable_library_documents(&self) -> Result<usize, AppError> {
        let mut connection = self.connection.lock().map_err(|_| storage_lock_error())?;
        let unavailable = {
            let mut statement = connection
                .prepare("SELECT path FROM library_entries")
                .map_err(storage_error)?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(storage_error)?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(storage_error)?
                .into_iter()
                .filter(|path| !Path::new(path).is_file())
                .collect::<Vec<_>>()
        };
        let transaction = connection.transaction().map_err(storage_error)?;
        for path in &unavailable {
            transaction
                .execute("DELETE FROM library_entries WHERE path = ?1", [path])
                .map_err(storage_error)?;
        }
        transaction.commit().map_err(storage_error)?;
        Ok(unavailable.len())
    }

    pub(crate) fn save_text_progress(
        &self,
        document_path: &Path,
        character_offset: usize,
        line_number: usize,
    ) -> Result<(), AppError> {
        let locator_json = serde_json::to_string(&TextProgressLocator {
            version: 1,
            character_offset,
            line_number,
        })
        .map_err(|_| AppError::internal("LOCAL_STATE_WRITE_FAILED", "serialize TXT locator"))?;
        let path = document_path.to_string_lossy();
        self.connection
            .lock()
            .map_err(|_| storage_lock_error())?
            .execute(
                "INSERT INTO reading_progress
                   (document_path, document_kind, fingerprint, locator_json, updated_at_ms)
                 VALUES (?1, 'txt', '', ?2, ?3)
                 ON CONFLICT(document_path) DO UPDATE SET
                   document_kind = excluded.document_kind,
                   fingerprint = excluded.fingerprint,
                   locator_json = excluded.locator_json,
                   updated_at_ms = excluded.updated_at_ms",
                params![path.as_ref(), locator_json, to_sql_integer(now_ms()?)?],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub(crate) fn load_text_progress(
        &self,
        document_path: &Path,
        maximum_offset: usize,
    ) -> Result<Option<usize>, AppError> {
        let path = document_path.to_string_lossy();
        let locator_json = self
            .connection
            .lock()
            .map_err(|_| storage_lock_error())?
            .query_row(
                "SELECT locator_json FROM reading_progress
                 WHERE document_path = ?1 AND document_kind = 'txt'",
                [path.as_ref()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(storage_error)?;
        let Some(locator_json) = locator_json else {
            return Ok(None);
        };
        let locator: TextProgressLocator = serde_json::from_str(&locator_json).map_err(|_| {
            AppError::internal("LOCAL_STATE_READ_FAILED", "deserialize TXT locator")
        })?;
        if locator.version != 1 || locator.line_number == 0 {
            return Ok(None);
        }
        Ok(Some(locator.character_offset.min(maximum_offset)))
    }

    pub(crate) fn set_background_image(
        &self,
        source: &BackgroundImageSource,
    ) -> Result<(), AppError> {
        let connection = self.connection.lock().map_err(|_| storage_lock_error())?;
        let value = serde_json::json!({
            "path": source.path.to_string_lossy(),
            "key": source.key,
            "mediaType": source.media_type,
        })
        .to_string();
        connection
            .execute(
                "INSERT INTO app_preferences (preference_key, value_json)
                 VALUES ('background_image', ?1)
                 ON CONFLICT(preference_key) DO UPDATE SET value_json = excluded.value_json",
                [value],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub(crate) fn background_image(&self) -> Result<Option<BackgroundImageSource>, AppError> {
        let value = self
            .connection
            .lock()
            .map_err(|_| storage_lock_error())?
            .query_row(
                "SELECT value_json FROM app_preferences WHERE preference_key = 'background_image'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(storage_error)?;
        let Some(value) = value else {
            return Ok(None);
        };
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct StoredBackground {
            path: String,
            key: String,
            media_type: String,
        }
        let stored: StoredBackground = serde_json::from_str(&value).map_err(|_| {
            AppError::internal("LOCAL_STATE_READ_FAILED", "deserialize background image")
        })?;
        Ok(Some(BackgroundImageSource {
            path: stored.path.into(),
            key: stored.key,
            media_type: stored.media_type,
        }))
    }

    pub(crate) fn background_image_source(
        &self,
        key: &str,
    ) -> Result<Option<BackgroundImageSource>, AppError> {
        Ok(self.background_image()?.filter(|source| source.key == key))
    }

    pub(crate) fn clear_background_image(&self) -> Result<(), AppError> {
        self.connection
            .lock()
            .map_err(|_| storage_lock_error())?
            .execute(
                "DELETE FROM app_preferences WHERE preference_key = 'background_image'",
                [],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub(crate) fn save_epub_progress(
        &self,
        document_path: &Path,
        locator: &EpubLocator,
    ) -> Result<(), AppError> {
        let locator_json = serde_json::to_string(locator).map_err(|_| {
            AppError::internal("LOCAL_STATE_WRITE_FAILED", "serialize EPUB locator")
        })?;
        let path = document_path.to_string_lossy();
        let now = now_ms()?;
        self.connection
            .lock()
            .map_err(|_| storage_lock_error())?
            .execute(
                "INSERT INTO reading_progress
                   (document_path, document_kind, fingerprint, locator_json, updated_at_ms)
                 VALUES (?1, 'epub', ?2, ?3, ?4)
                 ON CONFLICT(document_path) DO UPDATE SET
                   document_kind = excluded.document_kind,
                   fingerprint = excluded.fingerprint,
                   locator_json = excluded.locator_json,
                   updated_at_ms = excluded.updated_at_ms",
                params![
                    path.as_ref(),
                    locator.document_fingerprint,
                    locator_json,
                    to_sql_integer(now)?,
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub(crate) fn load_epub_progress(
        &self,
        document_path: &Path,
        fingerprint: &str,
    ) -> Result<Option<EpubLocator>, AppError> {
        let path = document_path.to_string_lossy();
        let row = self
            .connection
            .lock()
            .map_err(|_| storage_lock_error())?
            .query_row(
                "SELECT fingerprint, locator_json FROM reading_progress
                 WHERE document_path = ?1 AND document_kind = 'epub'",
                [path.as_ref()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(storage_error)?;
        let Some((stored_fingerprint, locator_json)) = row else {
            return Ok(None);
        };
        if stored_fingerprint != fingerprint {
            return Ok(None);
        }
        serde_json::from_str(&locator_json)
            .map(Some)
            .map_err(|_| AppError::internal("LOCAL_STATE_READ_FAILED", "deserialize EPUB locator"))
    }

    pub(crate) fn save_bookmark(
        &self,
        document_path: &Path,
        bookmark: &EpubBookmark,
    ) -> Result<(), AppError> {
        let locator_json = serde_json::to_string(&bookmark.locator).map_err(|_| {
            AppError::internal("LOCAL_STATE_WRITE_FAILED", "serialize EPUB bookmark")
        })?;
        let path = document_path.to_string_lossy();
        self.connection
            .lock()
            .map_err(|_| storage_lock_error())?
            .execute(
                "INSERT INTO bookmarks
                   (bookmark_id, document_path, document_kind, fingerprint, locator_json,
                    title, chapter_title, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, 'epub', ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(bookmark_id) DO UPDATE SET
                   locator_json = excluded.locator_json,
                   title = excluded.title,
                   chapter_title = excluded.chapter_title,
                   updated_at_ms = excluded.updated_at_ms",
                params![
                    bookmark.bookmark_id,
                    path.as_ref(),
                    bookmark.locator.document_fingerprint,
                    locator_json,
                    bookmark.title,
                    bookmark.chapter_title,
                    to_sql_integer(bookmark.created_at_ms)?,
                    to_sql_integer(bookmark.updated_at_ms)?,
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub(crate) fn bookmarks(
        &self,
        document_path: &Path,
        document_id: &str,
        fingerprint: &str,
    ) -> Result<Vec<EpubBookmark>, AppError> {
        let path = document_path.to_string_lossy();
        let connection = self.connection.lock().map_err(|_| storage_lock_error())?;
        let mut statement = connection
            .prepare(
                "SELECT bookmark_id, fingerprint, locator_json, title, chapter_title,
                        created_at_ms, updated_at_ms
                 FROM bookmarks WHERE document_path = ?1 AND document_kind = 'epub'
                 ORDER BY created_at_ms ASC",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([path.as_ref()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })
            .map_err(storage_error)?;
        let mut bookmarks = Vec::new();
        for row in rows {
            let (
                bookmark_id,
                stored_fingerprint,
                locator_json,
                title,
                chapter_title,
                created,
                updated,
            ) = row.map_err(storage_error)?;
            let mut locator: EpubLocator = serde_json::from_str(&locator_json).map_err(|_| {
                AppError::internal("LOCAL_STATE_READ_FAILED", "deserialize EPUB bookmark")
            })?;
            locator.document_id = document_id.to_owned();
            locator.document_fingerprint = fingerprint.to_owned();
            bookmarks.push(EpubBookmark {
                bookmark_id,
                locator: locator.normalized(),
                title,
                chapter_title,
                created_at_ms: created.max(0) as u64,
                updated_at_ms: updated.max(0) as u64,
                valid: stored_fingerprint == fingerprint,
            });
        }
        Ok(bookmarks)
    }

    pub(crate) fn delete_bookmark(
        &self,
        document_path: &Path,
        bookmark_id: &str,
    ) -> Result<(), AppError> {
        let path = document_path.to_string_lossy();
        self.connection
            .lock()
            .map_err(|_| storage_lock_error())?
            .execute(
                "DELETE FROM bookmarks WHERE bookmark_id = ?1 AND document_path = ?2",
                params![bookmark_id, path.as_ref()],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub(crate) fn save_text_bookmark(
        &self,
        document_path: &Path,
        bookmark: &TextBookmark,
    ) -> Result<(), AppError> {
        let locator_json = serde_json::to_string(&TextBookmarkLocator {
            version: 1,
            character_offset: bookmark.character_offset,
            line_number: bookmark.line_number,
        })
        .map_err(|_| AppError::internal("LOCAL_STATE_WRITE_FAILED", "serialize TXT bookmark"))?;
        let path = document_path.to_string_lossy();
        self.connection
            .lock()
            .map_err(|_| storage_lock_error())?
            .execute(
                "INSERT INTO bookmarks
                   (bookmark_id, document_path, document_kind, fingerprint, locator_json,
                    title, chapter_title, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, 'txt', '', ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(bookmark_id) DO UPDATE SET
                   document_path = excluded.document_path,
                   document_kind = excluded.document_kind,
                   fingerprint = excluded.fingerprint,
                   locator_json = excluded.locator_json,
                   title = excluded.title,
                   chapter_title = excluded.chapter_title,
                   updated_at_ms = excluded.updated_at_ms",
                params![
                    bookmark.bookmark_id,
                    path.as_ref(),
                    locator_json,
                    bookmark.title,
                    bookmark.preview,
                    to_sql_integer(bookmark.created_at_ms)?,
                    to_sql_integer(bookmark.updated_at_ms)?,
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub(crate) fn text_bookmarks(
        &self,
        document_path: &Path,
    ) -> Result<Vec<TextBookmark>, AppError> {
        let path = document_path.to_string_lossy();
        let connection = self.connection.lock().map_err(|_| storage_lock_error())?;
        let mut statement = connection
            .prepare(
                "SELECT bookmark_id, locator_json, title, chapter_title,
                        created_at_ms, updated_at_ms
                 FROM bookmarks WHERE document_path = ?1 AND document_kind = 'txt'
                 ORDER BY created_at_ms ASC",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([path.as_ref()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .map_err(storage_error)?;
        let mut bookmarks = Vec::new();
        for row in rows {
            let (bookmark_id, locator_json, title, preview, created, updated) =
                row.map_err(storage_error)?;
            let locator: TextBookmarkLocator =
                serde_json::from_str(&locator_json).map_err(|_| {
                    AppError::internal("LOCAL_STATE_READ_FAILED", "deserialize TXT bookmark")
                })?;
            if locator.version != 1 {
                continue;
            }
            bookmarks.push(TextBookmark {
                bookmark_id,
                character_offset: locator.character_offset,
                line_number: locator.line_number.max(1),
                title,
                preview,
                created_at_ms: created.max(0) as u64,
                updated_at_ms: updated.max(0) as u64,
            });
        }
        Ok(bookmarks)
    }
}

fn configure(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 3000;",
        )
        .map_err(storage_error)
}

fn migrate(connection: &mut Connection) -> Result<(), AppError> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(storage_error)?;
    if version > SCHEMA_VERSION {
        return Err(AppError::validation(
            "LOCAL_STATE_VERSION_UNSUPPORTED",
            "本地状态由更新版本的 Readloom 创建。",
            "请升级 Readloom 后重试。",
        ));
    }
    if version == SCHEMA_VERSION {
        return Ok(());
    }

    let transaction = connection.transaction().map_err(storage_error)?;
    if version < 1 {
        transaction
            .execute_batch(
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
            )
            .map_err(storage_error)?;
    }
    if version < 2 {
        transaction
            .execute_batch(
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
            )
            .map_err(storage_error)?;
    }
    if version < 3 {
        transaction
            .execute_batch(
                "ALTER TABLE library_entries ADD COLUMN cover_key TEXT;
                 ALTER TABLE library_entries ADD COLUMN cover_resource_id TEXT;
                 ALTER TABLE library_entries ADD COLUMN cover_media_type TEXT;
                 ALTER TABLE library_entries ADD COLUMN metadata_scanned INTEGER NOT NULL DEFAULT 0;
                 CREATE INDEX IF NOT EXISTS library_entries_cover_idx
                   ON library_entries(cover_key);",
            )
            .map_err(storage_error)?;
    }
    if version < 4 {
        transaction
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS app_preferences (
                   preference_key TEXT PRIMARY KEY NOT NULL,
                   value_json TEXT NOT NULL CHECK(length(value_json) <= 32768)
                 );",
            )
            .map_err(storage_error)?;
    }
    transaction
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

pub(crate) fn now_ms() -> Result<u64, AppError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .map_err(|_| AppError::internal("CLOCK_INVALID", "system time before Unix epoch"))
}

fn to_sql_integer(value: u64) -> Result<i64, AppError> {
    value.try_into().map_err(|_| {
        AppError::internal(
            "LOCAL_STATE_WRITE_FAILED",
            "timestamp exceeds SQLite integer",
        )
    })
}

fn upsert_library_entry(
    connection: &Connection,
    record: &LibraryDocumentRecord<'_>,
    timestamp: u64,
) -> Result<(), AppError> {
    let path = record.path.to_string_lossy();
    let cover_key = record
        .cover_resource_id
        .zip(record.cover_media_type)
        .map(|_| library_cover_key(record.path, record.fingerprint));
    connection
        .execute(
            "INSERT INTO library_entries
               (path, document_kind, display_title, author, fingerprint, last_opened_at_ms,
                cover_key, cover_resource_id, cover_media_type, metadata_scanned)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1)
             ON CONFLICT(path) DO UPDATE SET
               document_kind = excluded.document_kind,
               display_title = excluded.display_title,
               author = excluded.author,
               fingerprint = excluded.fingerprint,
               last_opened_at_ms = excluded.last_opened_at_ms,
               cover_key = excluded.cover_key,
               cover_resource_id = excluded.cover_resource_id,
               cover_media_type = excluded.cover_media_type,
               metadata_scanned = 1",
            params![
                path.as_ref(),
                record.document_kind,
                record.display_title,
                record.author,
                record.fingerprint,
                to_sql_integer(timestamp)?,
                cover_key,
                record.cover_resource_id,
                record.cover_media_type,
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn library_cover_key(path: &Path, fingerprint: Option<&str>) -> String {
    let mut input = path.to_string_lossy().into_owned();
    if let Some(fingerprint) = fingerprint {
        input.push('\0');
        input.push_str(fingerprint);
    }
    blake3::hash(input.as_bytes()).to_hex().to_string()
}

fn storage_error(_: rusqlite::Error) -> AppError {
    AppError::internal("LOCAL_STATE_FAILED", "SQLite local state operation")
}

fn storage_lock_error() -> AppError {
    AppError::internal("LOCAL_STATE_FAILED", "lock SQLite local state")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn locator() -> EpubLocator {
        EpubLocator {
            document_id: "doc-0000000000000001".to_owned(),
            document_fingerprint: "fingerprint-a".to_owned(),
            spine_index: 1,
            spine_href: "EPUB/chapter-2.xhtml".to_owned(),
            fragment: Some("middle".to_owned()),
            progression_in_chapter: 0.45,
            character_offset: Some(120),
            paragraph_index: Some(4),
        }
    }

    #[test]
    fn migration_preserves_unknown_legacy_txt_tables_and_is_idempotent() {
        let directory = tempdir().expect("temporary database directory");
        let path = directory.path().join("state.sqlite3");
        let connection = Connection::open(&path).expect("create legacy database");
        connection
            .execute_batch(
                "CREATE TABLE legacy_txt_state(path TEXT PRIMARY KEY, progress REAL);
                 INSERT INTO legacy_txt_state VALUES ('C:/books/legacy.txt', 0.5);",
            )
            .expect("seed legacy state");
        drop(connection);

        drop(LocalStateStore::open(&path).expect("first migration"));
        drop(LocalStateStore::open(&path).expect("idempotent migration"));

        let connection = Connection::open(&path).expect("reopen migrated database");
        let legacy_path: String = connection
            .query_row("SELECT path FROM legacy_txt_state", [], |row| row.get(0))
            .expect("legacy TXT data remains");
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version");
        assert_eq!(legacy_path, "C:/books/legacy.txt");
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn recent_history_and_library_entries_are_independent() {
        let directory = tempdir().expect("temporary directory");
        let first_path = directory.path().join("first.txt");
        let second_path = directory.path().join("second.txt");
        std::fs::write(&first_path, "first").expect("write first document");
        std::fs::write(&second_path, "second").expect("write second document");
        let store = LocalStateStore::in_memory().expect("in-memory state");
        for (path, title) in [(&first_path, "第一本"), (&second_path, "第二本")] {
            store
                .record_document_opened(RecentDocumentRecord {
                    path,
                    document_kind: "txt",
                    display_title: title,
                    author: None,
                    fingerprint: None,
                    cover_resource_id: None,
                    cover_media_type: None,
                })
                .expect("record recent document");
        }

        store
            .delete_recent(&first_path)
            .expect("delete one recent record");

        let recent = store.recent_documents(10).expect("load recent documents");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].path, second_path.to_string_lossy());
        let library = store.library_snapshot(10).expect("load library");
        assert_eq!(library.documents.len(), 2);

        store
            .remove_library_document(&second_path)
            .expect("remove one library entry");
        let recent = store.recent_documents(10).expect("reload history");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].path, second_path.to_string_lossy());
        let library = store.library_snapshot(10).expect("reload library");
        assert_eq!(library.documents.len(), 1);
        assert_eq!(library.documents[0].path, first_path.to_string_lossy());
        assert!(recent[0].available);
        assert_eq!(std::fs::read_to_string(&first_path).unwrap(), "first");
        assert_eq!(std::fs::read_to_string(&second_path).unwrap(), "second");
    }

    #[test]
    fn missing_recent_documents_remain_visible_but_are_marked_unavailable() {
        let directory = tempdir().expect("temporary directory");
        let missing_path = directory.path().join("moved.epub");
        let store = LocalStateStore::in_memory().expect("in-memory state");
        store
            .record_document_opened(RecentDocumentRecord {
                path: &missing_path,
                document_kind: "epub",
                display_title: "已移动的书",
                author: Some("作者"),
                fingerprint: Some("fingerprint"),
                cover_resource_id: None,
                cover_media_type: None,
            })
            .expect("record missing document");

        let recent = store.recent_documents(10).expect("load recent documents");

        assert_eq!(recent.len(), 1);
        assert!(!recent[0].available);
    }

    #[test]
    fn bulk_cleanup_removes_only_missing_library_entries() {
        let directory = tempdir().expect("temporary directory");
        let existing_path = directory.path().join("existing.txt");
        let missing_path = directory.path().join("moved.txt");
        std::fs::write(&existing_path, "still here").expect("write existing document");
        let store = LocalStateStore::in_memory().expect("in-memory state");
        for (path, title) in [(&existing_path, "仍存在"), (&missing_path, "已移动")] {
            store
                .record_library_document(LibraryDocumentRecord {
                    path,
                    document_kind: "txt",
                    display_title: title,
                    author: None,
                    fingerprint: None,
                    cover_resource_id: None,
                    cover_media_type: None,
                })
                .expect("record library document");
        }

        assert_eq!(
            store
                .remove_unavailable_library_documents()
                .expect("clean missing documents"),
            1
        );
        let snapshot = store.library_snapshot(10).expect("load cleaned library");
        assert_eq!(snapshot.documents.len(), 1);
        assert_eq!(snapshot.documents[0].path, existing_path.to_string_lossy());
        assert_eq!(
            std::fs::read_to_string(existing_path).unwrap(),
            "still here"
        );
    }

    #[test]
    fn version_one_history_is_migrated_into_the_independent_library() {
        let directory = tempdir().expect("temporary database directory");
        let path = directory.path().join("state.sqlite3");
        let connection = Connection::open(&path).expect("create version one database");
        connection
            .execute_batch(
                "CREATE TABLE recent_documents (
                   path TEXT PRIMARY KEY NOT NULL,
                   document_kind TEXT NOT NULL,
                   display_title TEXT NOT NULL,
                   author TEXT,
                   fingerprint TEXT,
                   last_opened_at_ms INTEGER NOT NULL
                 );
                 INSERT INTO recent_documents VALUES
                   ('C:/books/migrated.epub', 'epub', '迁移书籍', '作者', 'fingerprint', 42);
                 PRAGMA user_version = 1;",
            )
            .expect("seed version one state");
        drop(connection);

        let store = LocalStateStore::open(&path).expect("migrate version one state");
        let library = store.library_snapshot(10).expect("load migrated library");

        assert_eq!(library.documents.len(), 1);
        assert_eq!(library.documents[0].display_title, "迁移书籍");
        assert_eq!(library.documents[0].group_id, None);
    }

    #[test]
    fn groups_persist_assignments_and_release_books_when_deleted() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("grouped.txt");
        std::fs::write(&path, "grouped").expect("write grouped document");
        let store = LocalStateStore::in_memory().expect("in-memory state");
        store
            .record_document_opened(RecentDocumentRecord {
                path: &path,
                document_kind: "txt",
                display_title: "待分组",
                author: None,
                fingerprint: None,
                cover_resource_id: None,
                cover_media_type: None,
            })
            .expect("record document");
        let group = store
            .create_library_group("group-fiction", "小说")
            .expect("create group");
        store
            .assign_library_group(&path, Some(&group.group_id))
            .expect("assign group");

        let grouped = store.library_snapshot(10).expect("load grouped library");
        assert_eq!(grouped.groups, vec![group.clone()]);
        assert_eq!(
            grouped.documents[0].group_id.as_deref(),
            Some("group-fiction")
        );

        store
            .rename_library_group("group-fiction", "长篇小说")
            .expect("rename group");
        store
            .delete_library_group("group-fiction")
            .expect("delete group");
        let ungrouped = store.library_snapshot(10).expect("load ungrouped library");
        assert!(ungrouped.groups.is_empty());
        assert_eq!(ungrouped.documents[0].group_id, None);
    }

    #[test]
    fn epub_progress_round_trips_and_fingerprint_changes_invalidate_it() {
        let store = LocalStateStore::in_memory().expect("in-memory state");
        let path = Path::new("C:/books/readloom.epub");
        store
            .save_epub_progress(path, &locator())
            .expect("save progress");

        assert_eq!(
            store
                .load_epub_progress(path, "fingerprint-a")
                .expect("load progress"),
            Some(locator())
        );
        assert_eq!(
            store
                .load_epub_progress(path, "changed")
                .expect("load changed progress"),
            None
        );
    }

    #[test]
    fn txt_progress_round_trips_and_clamps_after_external_edits() {
        let store = LocalStateStore::in_memory().expect("in-memory state");
        let path = Path::new("C:/books/readloom.txt");
        store
            .save_text_progress(path, 120, 8)
            .expect("save TXT progress");

        assert_eq!(
            store.load_text_progress(path, 300).expect("load progress"),
            Some(120)
        );
        assert_eq!(
            store
                .load_text_progress(path, 40)
                .expect("load shortened progress"),
            Some(40)
        );
    }

    #[test]
    fn background_preference_round_trips_and_clears() {
        let store = LocalStateStore::in_memory().expect("in-memory state");
        let source = BackgroundImageSource {
            path: Path::new("C:/Readloom/backgrounds/background.png").to_path_buf(),
            key: "a".repeat(64),
            media_type: "image/png".to_owned(),
        };
        store
            .set_background_image(&source)
            .expect("save background preference");

        assert_eq!(store.background_image().unwrap(), Some(source.clone()));
        assert_eq!(
            store.background_image_source(&source.key).unwrap(),
            Some(source)
        );
        store
            .clear_background_image()
            .expect("clear background preference");
        assert_eq!(store.background_image().unwrap(), None);
    }

    #[test]
    fn epub_bookmarks_share_the_generic_bookmark_table_without_body_text() {
        let store = LocalStateStore::in_memory().expect("in-memory state");
        let path = Path::new("C:/books/readloom.epub");
        let bookmark = EpubBookmark {
            bookmark_id: "bm-1".to_owned(),
            locator: locator(),
            title: Some("重要位置".to_owned()),
            chapter_title: "第二章".to_owned(),
            created_at_ms: 100,
            updated_at_ms: 100,
            valid: true,
        };
        store.save_bookmark(path, &bookmark).expect("save bookmark");

        let loaded = store
            .bookmarks(path, "doc-0000000000000002", "fingerprint-a")
            .expect("load bookmarks");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].title.as_deref(), Some("重要位置"));
        assert_eq!(loaded[0].locator.document_id, "doc-0000000000000002");
        assert!(loaded[0].valid);
    }

    #[test]
    fn txt_bookmarks_round_trip_through_the_generic_bookmark_table() {
        let store = LocalStateStore::in_memory().expect("state store");
        let path = Path::new(r"C:\books\notes.txt");
        let bookmark = TextBookmark {
            bookmark_id: "tbm-1".to_owned(),
            character_offset: 18,
            line_number: 3,
            title: Some("关键段落".to_owned()),
            preview: "第三行关键正文".to_owned(),
            created_at_ms: 10,
            updated_at_ms: 11,
        };

        store
            .save_text_bookmark(path, &bookmark)
            .expect("save TXT bookmark");
        assert_eq!(
            store.text_bookmarks(path).expect("load TXT bookmarks"),
            vec![bookmark]
        );
    }
}
