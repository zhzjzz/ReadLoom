use std::{
    path::Path,
    sync::Mutex,
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

const SCHEMA_VERSION: i64 = 1;

pub(crate) struct LocalStateStore {
    connection: Mutex<Connection>,
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

pub(crate) struct RecentDocumentRecord<'a> {
    pub path: &'a Path,
    pub document_kind: &'a str,
    pub display_title: &'a str,
    pub author: Option<&'a str>,
    pub fingerprint: Option<&'a str>,
}

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
            connection: Mutex::new(connection),
        })
    }

    #[cfg(test)]
    fn in_memory() -> Result<Self, AppError> {
        let mut connection = Connection::open_in_memory().map_err(storage_error)?;
        configure(&connection)?;
        migrate(&mut connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub(crate) fn record_recent(&self, record: RecentDocumentRecord<'_>) -> Result<(), AppError> {
        let path = record.path.to_string_lossy();
        let now = now_ms()?;
        self.connection
            .lock()
            .map_err(|_| storage_lock_error())?
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
        Ok(())
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
    fn deleting_a_recent_document_removes_only_its_history_record() {
        let directory = tempdir().expect("temporary directory");
        let first_path = directory.path().join("first.txt");
        let second_path = directory.path().join("second.txt");
        std::fs::write(&first_path, "first").expect("write first document");
        std::fs::write(&second_path, "second").expect("write second document");
        let store = LocalStateStore::in_memory().expect("in-memory state");
        for (path, title) in [(&first_path, "第一本"), (&second_path, "第二本")] {
            store
                .record_recent(RecentDocumentRecord {
                    path,
                    document_kind: "txt",
                    display_title: title,
                    author: None,
                    fingerprint: None,
                })
                .expect("record recent document");
        }

        store
            .delete_recent(&first_path)
            .expect("delete one recent record");

        let recent = store.recent_documents(10).expect("load recent documents");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].path, second_path.to_string_lossy());
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
            .record_recent(RecentDocumentRecord {
                path: &missing_path,
                document_kind: "epub",
                display_title: "已移动的书",
                author: Some("作者"),
                fingerprint: Some("fingerprint"),
            })
            .expect("record missing document");

        let recent = store.recent_documents(10).expect("load recent documents");

        assert_eq!(recent.len(), 1);
        assert!(!recent[0].available);
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
