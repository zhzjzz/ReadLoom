use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    error::AppError,
    infrastructure::{
        filesystem::{commit_prepared_output, create_prepared_output, fingerprint_file},
        storage::local_state::LocalStateStore,
    },
};

use super::library_commands::import_library_file;

const BACKUP_VERSION: u8 = 1;
const MAXIMUM_BACKUP_BOOKS: usize = 2_000;
const MAXIMUM_BACKUP_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const MAXIMUM_BOOK_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAXIMUM_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
static RESTORE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateBooksBackupRequest {
    target_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RestoreBooksBackupRequest {
    backup_paths: Vec<String>,
    target_directory: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BooksBackupResult {
    target_path: String,
    book_count: usize,
    unique_content_count: usize,
    unavailable_skipped: usize,
    source_bytes: u64,
    backup_bytes: u64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BooksRestoreFailure {
    backup_path: String,
    file_name: String,
    message: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BooksRestoreResult {
    target_directory: String,
    restored: usize,
    duplicate_content_skipped: usize,
    existing_content_skipped: usize,
    restored_bytes: u64,
    failed: Vec<BooksRestoreFailure>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupManifest {
    format: String,
    version: u8,
    content_only: bool,
    books: Vec<BackupBook>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupBook {
    file_name: String,
    document_kind: String,
    size_bytes: u64,
    sha256: String,
    archive_path: String,
}

struct BackupSource {
    path: PathBuf,
    book: BackupBook,
}

#[tauri::command]
pub(crate) async fn create_books_backup(
    local_state: State<'_, LocalStateStore>,
    request: CreateBooksBackupRequest,
) -> Result<BooksBackupResult, AppError> {
    let local_state = local_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        create_backup(&local_state, Path::new(request.target_path.trim()))
    })
    .await
    .map_err(|_| AppError::internal("BACKUP_FAILED", "join backup worker"))?
}

#[tauri::command]
pub(crate) async fn restore_books_backup(
    local_state: State<'_, LocalStateStore>,
    request: RestoreBooksBackupRequest,
) -> Result<BooksRestoreResult, AppError> {
    if request.backup_paths.is_empty() || request.backup_paths.len() > 128 {
        return Err(invalid_backup_selection());
    }
    let local_state = local_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        restore_backups(
            &local_state,
            request
                .backup_paths
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            Path::new(request.target_directory.trim()),
        )
    })
    .await
    .map_err(|_| AppError::internal("BACKUP_RESTORE_FAILED", "join restore worker"))?
}

fn create_backup(
    local_state: &LocalStateStore,
    target: &Path,
) -> Result<BooksBackupResult, AppError> {
    validate_backup_target(target)?;
    let mut sources = Vec::new();
    let mut unavailable_skipped = 0_usize;
    let mut source_bytes = 0_u64;
    for path in local_state.library_document_paths()? {
        let metadata = match fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => metadata,
            _ => {
                unavailable_skipped += 1;
                continue;
            }
        };
        let kind = supported_kind(&path).ok_or_else(invalid_backup_source)?;
        if metadata.len() > MAXIMUM_BOOK_BYTES {
            return Err(backup_limit_exceeded());
        }
        source_bytes = source_bytes
            .checked_add(metadata.len())
            .ok_or_else(backup_limit_exceeded)?;
        if source_bytes > MAXIMUM_BACKUP_BYTES {
            return Err(backup_limit_exceeded());
        }
        let sha256 = sha256_file(&path)?;
        let file_name = path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .ok_or_else(invalid_backup_source)?;
        sources.push(BackupSource {
            path,
            book: BackupBook {
                file_name,
                document_kind: kind.to_owned(),
                size_bytes: metadata.len(),
                archive_path: format!("books/{sha256}.{kind}"),
                sha256,
            },
        });
    }
    if sources.is_empty() {
        return Err(AppError::validation(
            "BACKUP_LIBRARY_EMPTY",
            "书库中没有可备份的图书文件。",
            "先导入图书，或修复已移动的书籍路径。",
        ));
    }
    if sources.len() > MAXIMUM_BACKUP_BOOKS {
        return Err(backup_limit_exceeded());
    }

    let expected = target
        .exists()
        .then(|| fingerprint_file(target))
        .transpose()
        .map_err(|_| {
            AppError::validation(
                "BACKUP_TARGET_UNAVAILABLE",
                "无法读取现有备份文件。",
                "选择其他备份文件名后重试。",
            )
        })?;
    let (temporary_file, temporary_path) = create_prepared_output(target)?;
    let write_result = write_backup_archive(temporary_file, &sources);
    let unique_content_count = match write_result {
        Ok(value) => value,
        Err(error) => {
            let _ = fs::remove_file(&temporary_path);
            return Err(error);
        }
    };
    commit_prepared_output(target, &temporary_path, expected.as_ref())?;
    let backup_bytes = fs::metadata(target).map_err(|_| backup_failed())?.len();
    Ok(BooksBackupResult {
        target_path: target.to_string_lossy().into_owned(),
        book_count: sources.len(),
        unique_content_count,
        unavailable_skipped,
        source_bytes,
        backup_bytes,
    })
}

fn write_backup_archive(file: File, sources: &[BackupSource]) -> Result<usize, AppError> {
    let mut writer = ZipWriter::new(file);
    let mut written = HashSet::new();
    for source in sources {
        if !written.insert(source.book.sha256.clone()) {
            continue;
        }
        let method = if source.book.document_kind == "txt" {
            CompressionMethod::Deflated
        } else {
            CompressionMethod::Stored
        };
        writer
            .start_file(
                &source.book.archive_path,
                SimpleFileOptions::default().compression_method(method),
            )
            .map_err(|_| backup_failed())?;
        let mut input = File::open(&source.path).map_err(|_| invalid_backup_source())?;
        std::io::copy(&mut input, &mut writer).map_err(|_| backup_failed())?;
    }
    let manifest = BackupManifest {
        format: "readloom-books".to_owned(),
        version: BACKUP_VERSION,
        content_only: true,
        books: sources.iter().map(|source| source.book.clone()).collect(),
    };
    let manifest_bytes = serde_json::to_vec(&manifest)
        .map_err(|_| AppError::internal("BACKUP_FAILED", "serialize backup manifest"))?;
    writer
        .start_file(
            "manifest.json",
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .map_err(|_| backup_failed())?;
    writer
        .write_all(&manifest_bytes)
        .map_err(|_| backup_failed())?;
    let mut file = writer.finish().map_err(|_| backup_failed())?;
    file.flush().map_err(|_| backup_failed())?;
    file.sync_all().map_err(|_| backup_failed())?;
    Ok(written.len())
}

fn restore_backups(
    local_state: &LocalStateStore,
    backup_paths: Vec<PathBuf>,
    target_directory: &Path,
) -> Result<BooksRestoreResult, AppError> {
    let target_directory =
        fs::canonicalize(target_directory).map_err(|_| invalid_restore_directory())?;
    if !target_directory.is_dir() {
        return Err(invalid_restore_directory());
    }
    let mut result = BooksRestoreResult {
        target_directory: target_directory.to_string_lossy().into_owned(),
        restored: 0,
        duplicate_content_skipped: 0,
        existing_content_skipped: 0,
        restored_bytes: 0,
        failed: Vec::new(),
    };
    let mut seen = HashSet::new();
    for backup_path in backup_paths {
        let backup_display = backup_path.to_string_lossy().into_owned();
        let (mut archive, manifest) = match open_backup(&backup_path) {
            Ok(value) => value,
            Err(error) => {
                result.failed.push(BooksRestoreFailure {
                    backup_path: backup_display,
                    file_name: String::new(),
                    message: error.to_dto().message,
                });
                continue;
            }
        };
        for book in manifest.books {
            if !seen.insert(book.sha256.clone()) {
                result.duplicate_content_skipped += 1;
                continue;
            }
            match restore_book(local_state, &mut archive, &book, &target_directory) {
                Ok(RestoreBookOutcome::Restored(bytes)) => {
                    result.restored += 1;
                    result.restored_bytes = result.restored_bytes.saturating_add(bytes);
                }
                Ok(RestoreBookOutcome::Existing) => result.existing_content_skipped += 1,
                Err(error) => result.failed.push(BooksRestoreFailure {
                    backup_path: backup_display.clone(),
                    file_name: book.file_name,
                    message: error.to_dto().message,
                }),
            }
        }
    }
    Ok(result)
}

enum RestoreBookOutcome {
    Restored(u64),
    Existing,
}

fn restore_book(
    local_state: &LocalStateStore,
    archive: &mut ZipArchive<File>,
    book: &BackupBook,
    target_directory: &Path,
) -> Result<RestoreBookOutcome, AppError> {
    validate_manifest_book(book)?;
    let mut entry = archive
        .by_name(&book.archive_path)
        .map_err(|_| invalid_backup_archive())?;
    validate_archive_entry(&entry, book.size_bytes)?;
    let file_name = safe_file_name(&book.file_name, &book.document_kind)?;
    let target = unique_restore_target(target_directory, &file_name, &book.sha256)?;
    if target.exists() && sha256_file(&target)? == book.sha256 {
        import_library_file(local_state, &target)?;
        return Ok(RestoreBookOutcome::Existing);
    }
    let (mut output, temporary) = create_restore_temporary(target_directory)?;
    let restore_result = copy_and_verify(&mut entry, &mut output, book);
    if let Err(error) = restore_result {
        drop(output);
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    output.flush().map_err(|_| restore_failed())?;
    output.sync_all().map_err(|_| restore_failed())?;
    drop(output);
    fs::rename(&temporary, &target).map_err(|_| restore_failed())?;
    if let Err(error) = import_library_file(local_state, &target) {
        let _ = fs::remove_file(&target);
        return Err(error);
    }
    Ok(RestoreBookOutcome::Restored(book.size_bytes))
}

fn open_backup(path: &Path) -> Result<(ZipArchive<File>, BackupManifest), AppError> {
    let metadata = fs::metadata(path).map_err(|_| invalid_backup_archive())?;
    if !metadata.is_file() || metadata.len() > MAXIMUM_BACKUP_BYTES {
        return Err(backup_limit_exceeded());
    }
    let file = File::open(path).map_err(|_| invalid_backup_archive())?;
    let mut archive = ZipArchive::new(file).map_err(|_| invalid_backup_archive())?;
    if archive.len() > MAXIMUM_BACKUP_BOOKS + 1 {
        return Err(backup_limit_exceeded());
    }
    let manifest = {
        let mut entry = archive
            .by_name("manifest.json")
            .map_err(|_| invalid_backup_archive())?;
        if entry.size() > MAXIMUM_MANIFEST_BYTES || entry.encrypted() || entry.is_symlink() {
            return Err(invalid_backup_archive());
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .by_ref()
            .take(MAXIMUM_MANIFEST_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid_backup_archive())?;
        serde_json::from_slice::<BackupManifest>(&bytes).map_err(|_| invalid_backup_archive())?
    };
    if manifest.format != "readloom-books"
        || manifest.version != BACKUP_VERSION
        || !manifest.content_only
        || manifest.books.is_empty()
        || manifest.books.len() > MAXIMUM_BACKUP_BOOKS
    {
        return Err(invalid_backup_archive());
    }
    let total = manifest.books.iter().try_fold(0_u64, |total, book| {
        validate_manifest_book(book)?;
        total
            .checked_add(book.size_bytes)
            .ok_or_else(backup_limit_exceeded)
    })?;
    if total > MAXIMUM_BACKUP_BYTES {
        return Err(backup_limit_exceeded());
    }
    Ok((archive, manifest))
}

fn validate_manifest_book(book: &BackupBook) -> Result<(), AppError> {
    if book.size_bytes > MAXIMUM_BOOK_BYTES
        || !matches!(book.document_kind.as_str(), "txt" | "epub")
        || book.sha256.len() != 64
        || !book.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || book.archive_path != format!("books/{}.{}", book.sha256, book.document_kind)
    {
        return Err(invalid_backup_archive());
    }
    safe_file_name(&book.file_name, &book.document_kind).map(|_| ())
}

fn validate_archive_entry(
    entry: &zip::read::ZipFile<'_, File>,
    expected: u64,
) -> Result<(), AppError> {
    if !entry.is_file()
        || entry.encrypted()
        || entry.is_symlink()
        || entry.size() != expected
        || !matches!(
            entry.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        )
        || (entry.size() > 0
            && (entry.compressed_size() == 0
                || entry.size() / entry.compressed_size().max(1) > 20_000))
    {
        return Err(invalid_backup_archive());
    }
    Ok(())
}

fn copy_and_verify(
    input: &mut impl Read,
    output: &mut File,
    book: &BackupBook,
) -> Result<(), AppError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut written = 0_u64;
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|_| invalid_backup_archive())?;
        if read == 0 {
            break;
        }
        written = written
            .checked_add(read as u64)
            .ok_or_else(backup_limit_exceeded)?;
        if written > book.size_bytes || written > MAXIMUM_BOOK_BYTES {
            return Err(backup_limit_exceeded());
        }
        hasher.update(&buffer[..read]);
        output
            .write_all(&buffer[..read])
            .map_err(|_| restore_failed())?;
    }
    let digest = hasher.finalize();
    if written != book.size_bytes || hex_digest(&digest) != book.sha256 {
        return Err(AppError::validation(
            "BACKUP_HASH_MISMATCH",
            "备份中的图书内容校验失败。",
            "该备份可能已损坏，请选择其他备份。",
        ));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, AppError> {
    let mut input = File::open(path).map_err(|_| invalid_backup_source())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|_| invalid_backup_source())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    Ok(hex_digest(&digest))
}

fn hex_digest(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(value, "{byte:02x}");
    }
    value
}

fn safe_file_name(file_name: &str, kind: &str) -> Result<String, AppError> {
    let path = Path::new(file_name);
    if path.components().count() != 1
        || path.file_name().and_then(|value| value.to_str()) != Some(file_name)
        || supported_kind(path) != Some(kind)
        || file_name.len() > 240
        || file_name.ends_with(' ')
        || file_name.ends_with('.')
        || file_name
            .chars()
            .any(|character| character.is_control() || r#"<>:"/\|?*"#.contains(character))
        || is_windows_reserved_file_name(file_name)
    {
        return Err(invalid_backup_archive());
    }
    Ok(file_name.to_owned())
}

fn is_windows_reserved_file_name(file_name: &str) -> bool {
    let stem = file_name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || matches!(
            stem.strip_prefix("COM"),
            Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        )
        || matches!(
            stem.strip_prefix("LPT"),
            Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        )
}

fn unique_restore_target(
    directory: &Path,
    file_name: &str,
    expected_hash: &str,
) -> Result<PathBuf, AppError> {
    let original = directory.join(file_name);
    if !original.exists() || sha256_file(&original)? == expected_hash {
        return Ok(original);
    }
    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("book");
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("txt");
    for index in 2..=10_000 {
        let candidate = directory.join(format!("{stem} ({index}).{extension}"));
        if !candidate.exists() || sha256_file(&candidate)? == expected_hash {
            return Ok(candidate);
        }
    }
    Err(restore_failed())
}

fn create_restore_temporary(directory: &Path) -> Result<(File, PathBuf), AppError> {
    for _ in 0..256 {
        let id = RESTORE_ID.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            ".readloom-restore-{}-{id:016x}.tmp",
            std::process::id()
        ));
        match OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(restore_failed()),
        }
    }
    Err(restore_failed())
}

fn supported_kind(path: &Path) -> Option<&'static str> {
    match path
        .extension()?
        .to_string_lossy()
        .to_ascii_lowercase()
        .as_str()
    {
        "epub" => Some("epub"),
        "txt" => Some("txt"),
        _ => None,
    }
}

fn validate_backup_target(target: &Path) -> Result<(), AppError> {
    if target.as_os_str().is_empty()
        || target.extension().and_then(|value| value.to_str()) != Some("readloom-backup")
    {
        return Err(AppError::validation(
            "INVALID_BACKUP_TARGET",
            "备份文件必须使用 .readloom-backup 扩展名。",
            "重新选择备份位置后重试。",
        ));
    }
    Ok(())
}

fn invalid_backup_selection() -> AppError {
    AppError::validation(
        "INVALID_BACKUP_SELECTION",
        "请选择 1 到 128 个 Readloom 备份文件。",
        "重新选择备份文件后重试。",
    )
}

fn invalid_backup_source() -> AppError {
    AppError::validation(
        "BACKUP_SOURCE_UNAVAILABLE",
        "备份期间无法读取某本图书。",
        "确认书籍仍在原位置且未被其他程序占用后重试。",
    )
}

fn invalid_backup_archive() -> AppError {
    AppError::validation(
        "INVALID_READLOOM_BACKUP",
        "所选文件不是有效且完整的 Readloom 图书备份。",
        "请选择未修改、未损坏的 .readloom-backup 文件。",
    )
}

fn invalid_restore_directory() -> AppError {
    AppError::validation(
        "INVALID_RESTORE_DIRECTORY",
        "备份恢复目录不存在或不可用。",
        "选择一个可写文件夹后重试。",
    )
}

fn backup_limit_exceeded() -> AppError {
    AppError::validation(
        "BACKUP_LIMIT_EXCEEDED",
        "备份中的图书数量或总大小超出安全限制。",
        "拆分书库后分批备份或恢复。",
    )
}

fn backup_failed() -> AppError {
    AppError::validation(
        "BACKUP_FAILED",
        "无法创建完整的图书内容备份。",
        "检查目标目录权限和磁盘空间后重试。",
    )
}

fn restore_failed() -> AppError {
    AppError::validation(
        "BACKUP_RESTORE_FAILED",
        "无法安全恢复备份中的图书。",
        "检查恢复目录权限和磁盘空间后重试。",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_round_trip_deduplicates_content_and_omits_reader_state() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("books");
        let restored = directory.path().join("restored");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&restored).unwrap();
        let first = source.join("first.txt");
        let duplicate = source.join("duplicate.txt");
        let highly_compressible_text = vec![b'A'; 2 * 1024 * 1024];
        fs::write(&first, &highly_compressible_text).unwrap();
        fs::write(&duplicate, &highly_compressible_text).unwrap();
        let store = LocalStateStore::open(&directory.path().join("state.sqlite3")).unwrap();
        import_library_file(&store, &first).unwrap();
        import_library_file(&store, &duplicate).unwrap();
        let backup = directory.path().join("books.readloom-backup");

        let created = create_backup(&store, &backup).unwrap();
        assert_eq!(created.book_count, 2);
        assert_eq!(created.unique_content_count, 1);
        assert!(created.backup_bytes < created.source_bytes);

        let restore_store =
            LocalStateStore::open(&directory.path().join("restored.sqlite3")).unwrap();
        let result =
            restore_backups(&restore_store, vec![backup.clone(), backup], &restored).unwrap();
        assert_eq!(result.restored, 1);
        assert_eq!(result.duplicate_content_skipped, 3);
        assert_eq!(
            restore_store.library_snapshot(10).unwrap().documents.len(),
            1
        );

        let file = File::open(directory.path().join("books.readloom-backup")).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut manifest = String::new();
        archive
            .by_name("manifest.json")
            .unwrap()
            .read_to_string(&mut manifest)
            .unwrap();
        assert!(!manifest.contains("bookmark"));
        assert!(!manifest.contains("progress"));
        assert!(!manifest.contains("settings"));
    }

    #[test]
    fn restore_rejects_unsafe_manifest_file_names() {
        let book = BackupBook {
            file_name: "..\\escape.txt".to_owned(),
            document_kind: "txt".to_owned(),
            size_bytes: 1,
            sha256: "a".repeat(64),
            archive_path: format!("books/{}.txt", "a".repeat(64)),
        };
        assert!(validate_manifest_book(&book).is_err());
        for reserved in ["CON.txt", "nul.TXT", "COM1.report.txt", "LPT9.epub"] {
            assert!(
                safe_file_name(reserved, supported_kind(Path::new(reserved)).unwrap()).is_err()
            );
        }
        for unsafe_name in ["trailing.txt.", "trailing.txt ", "bad:name.txt"] {
            assert!(safe_file_name(unsafe_name, "txt").is_err());
        }
    }
}
