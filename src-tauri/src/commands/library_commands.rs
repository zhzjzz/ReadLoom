use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    error::AppError,
    formats::epub::parser::parse_epub_document,
    infrastructure::storage::local_state::{
        LibraryDocumentRecord, LibraryGroup, LibrarySnapshot, LocalStateStore, now_ms,
    },
    infrastructure::{archive::archive_limits::ArchiveLimits, filesystem::fingerprint_file},
};

const MAXIMUM_IMPORT_FILES: usize = 2_000;
const MAXIMUM_DIRECTORY_DEPTH: usize = 32;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListLibraryRequest {
    maximum: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateLibraryGroupRequest {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenameLibraryGroupRequest {
    group_id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteLibraryGroupRequest {
    group_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssignLibraryGroupRequest {
    path: String,
    group_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoveLibraryDocumentRequest {
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportLibraryDocumentsRequest {
    paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportLibraryDirectoryRequest {
    directory: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewLibraryDocumentsRequest {
    paths: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryImportCandidate {
    path: String,
    file_name: String,
    document_kind: String,
    size_bytes: u64,
    already_imported: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryImportPreview {
    root_path: Option<String>,
    candidates: Vec<LibraryImportCandidate>,
    total_size_bytes: u64,
    importable: usize,
    already_imported: usize,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryImportFailure {
    path: String,
    code: String,
    message: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryImportResult {
    imported: usize,
    skipped: usize,
    failed: Vec<LibraryImportFailure>,
}

#[tauri::command]
pub(crate) async fn list_library(
    local_state: State<'_, LocalStateStore>,
    request: ListLibraryRequest,
) -> Result<LibrarySnapshot, AppError> {
    let maximum = request.maximum.unwrap_or(500);
    let local_state = local_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || load_library_snapshot(&local_state, maximum))
        .await
        .map_err(|_| AppError::internal("LOCAL_STATE_FAILED", "join library list worker"))?
}

fn load_library_snapshot(
    local_state: &LocalStateStore,
    maximum: usize,
) -> Result<LibrarySnapshot, AppError> {
    let initial = local_state.library_snapshot(maximum)?;
    let pending = initial
        .documents
        .iter()
        .filter(|document| {
            document.available && document.document_kind == "epub" && !document.metadata_scanned
        })
        .map(|document| PathBuf::from(&document.path))
        .collect::<Vec<_>>();
    if pending.is_empty() {
        return Ok(initial);
    }
    for path in pending {
        if refresh_epub_metadata(local_state, &path).is_err() {
            let _ = local_state.mark_library_metadata_scanned(&path);
        }
    }
    local_state.library_snapshot(maximum)
}

#[tauri::command]
pub(crate) async fn import_library_documents(
    local_state: State<'_, LocalStateStore>,
    request: ImportLibraryDocumentsRequest,
) -> Result<LibraryImportResult, AppError> {
    if request.paths.len() > MAXIMUM_IMPORT_FILES {
        return Err(import_limit_exceeded());
    }
    let local_state = local_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        import_paths(&local_state, request.paths.into_iter().map(PathBuf::from))
    })
    .await
    .map_err(|_| AppError::internal("LOCAL_STATE_FAILED", "join library import worker"))
}

#[tauri::command]
pub(crate) async fn preview_library_directory(
    local_state: State<'_, LocalStateStore>,
    request: ImportLibraryDirectoryRequest,
) -> Result<LibraryImportPreview, AppError> {
    let local_state = local_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = validated_directory(request.directory)?;
        let files = collect_library_files(&root)?;
        preview_paths(&local_state, Some(&root), files)
    })
    .await
    .map_err(|_| AppError::internal("LOCAL_STATE_FAILED", "join library preview worker"))?
}

#[tauri::command]
pub(crate) async fn preview_library_documents(
    local_state: State<'_, LocalStateStore>,
    request: PreviewLibraryDocumentsRequest,
) -> Result<LibraryImportPreview, AppError> {
    if request.paths.len() > MAXIMUM_IMPORT_FILES {
        return Err(import_limit_exceeded());
    }
    let local_state = local_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        preview_paths(
            &local_state,
            None,
            request.paths.into_iter().map(PathBuf::from).collect(),
        )
    })
    .await
    .map_err(|_| AppError::internal("LOCAL_STATE_FAILED", "join library preview worker"))?
}

#[tauri::command]
pub(crate) fn create_library_group(
    local_state: State<'_, LocalStateStore>,
    request: CreateLibraryGroupRequest,
) -> Result<LibraryGroup, AppError> {
    let name = validated_group_name(request.name)?;
    local_state.create_library_group(&new_group_id(), &name)
}

#[tauri::command]
pub(crate) fn rename_library_group(
    local_state: State<'_, LocalStateStore>,
    request: RenameLibraryGroupRequest,
) -> Result<(), AppError> {
    let group_id = validated_group_id(request.group_id)?;
    let name = validated_group_name(request.name)?;
    local_state.rename_library_group(&group_id, &name)
}

#[tauri::command]
pub(crate) fn delete_library_group(
    local_state: State<'_, LocalStateStore>,
    request: DeleteLibraryGroupRequest,
) -> Result<(), AppError> {
    local_state.delete_library_group(&validated_group_id(request.group_id)?)
}

#[tauri::command]
pub(crate) fn assign_library_group(
    local_state: State<'_, LocalStateStore>,
    request: AssignLibraryGroupRequest,
) -> Result<(), AppError> {
    let path = validated_path(request.path)?;
    let group_id = request.group_id.map(validated_group_id).transpose()?;
    local_state.assign_library_group(&path, group_id.as_deref())
}

#[tauri::command]
pub(crate) fn remove_library_document(
    local_state: State<'_, LocalStateStore>,
    request: RemoveLibraryDocumentRequest,
) -> Result<(), AppError> {
    local_state.remove_library_document(&validated_path(request.path)?)
}

#[tauri::command]
pub(crate) fn remove_unavailable_library_documents(
    local_state: State<'_, LocalStateStore>,
) -> Result<usize, AppError> {
    local_state.remove_unavailable_library_documents()
}

fn validated_group_name(name: String) -> Result<String, AppError> {
    let name = name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(AppError::validation(
            "INVALID_LIBRARY_GROUP_NAME",
            "分组名称不能为空且不能超过 64 个字符。",
            "请输入简短的书架名称。",
        ));
    }
    Ok(name)
}

fn validated_group_id(group_id: String) -> Result<String, AppError> {
    if !group_id.starts_with("group-") || group_id.len() > 80 {
        return Err(AppError::validation(
            "LIBRARY_GROUP_NOT_FOUND",
            "书架分组标识无效。",
            "刷新书库后重试。",
        ));
    }
    Ok(group_id)
}

fn validated_path(path: String) -> Result<PathBuf, AppError> {
    if path.trim().is_empty() {
        return Err(AppError::validation(
            "LIBRARY_DOCUMENT_PATH_EMPTY",
            "书库文件路径不能为空。",
            "刷新书库后重试。",
        ));
    }
    Ok(Path::new(&path).to_path_buf())
}

fn new_group_id() -> String {
    let mut bytes = [0_u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        return format!("group-{}", now_ms().unwrap_or_default());
    }
    let mut value = String::with_capacity(38);
    value.push_str("group-");
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

fn import_paths(
    local_state: &LocalStateStore,
    paths: impl IntoIterator<Item = PathBuf>,
) -> LibraryImportResult {
    let mut result = LibraryImportResult {
        imported: 0,
        skipped: 0,
        failed: Vec::new(),
    };
    let mut unique = HashSet::new();
    for path in paths {
        let comparable = path.to_string_lossy().to_lowercase();
        if !unique.insert(comparable) {
            result.skipped += 1;
            continue;
        }
        match import_library_file(local_state, &path) {
            Ok(()) => result.imported += 1,
            Err(error) => {
                let dto = error.to_dto();
                result.failed.push(LibraryImportFailure {
                    path: path.to_string_lossy().into_owned(),
                    code: dto.code.to_owned(),
                    message: dto.message,
                });
            }
        }
    }
    result
}

fn preview_paths(
    local_state: &LocalStateStore,
    root: Option<&Path>,
    paths: Vec<PathBuf>,
) -> Result<LibraryImportPreview, AppError> {
    let existing = local_state
        .library_document_paths()?
        .into_iter()
        .map(|path| path.to_string_lossy().to_lowercase())
        .collect::<HashSet<_>>();
    let mut unique = HashSet::new();
    let mut candidates = Vec::with_capacity(paths.len());
    let mut total_size_bytes = 0_u64;
    for path in paths {
        let canonical = fs::canonicalize(&path).map_err(|_| invalid_import_path())?;
        let key = canonical.to_string_lossy().to_lowercase();
        if !unique.insert(key.clone()) {
            continue;
        }
        let metadata = fs::metadata(&canonical).map_err(|_| invalid_import_path())?;
        if !metadata.is_file() || !is_supported_library_file(&canonical) {
            return Err(invalid_import_path());
        }
        total_size_bytes = total_size_bytes
            .checked_add(metadata.len())
            .ok_or_else(import_limit_exceeded)?;
        let file_name = canonical
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .ok_or_else(invalid_import_path)?;
        candidates.push(LibraryImportCandidate {
            path: canonical.to_string_lossy().into_owned(),
            file_name,
            document_kind: normalized_extension(&canonical).unwrap_or_default(),
            size_bytes: metadata.len(),
            already_imported: existing.contains(&key),
        });
    }
    candidates.sort_by(|left, right| {
        left.file_name
            .to_lowercase()
            .cmp(&right.file_name.to_lowercase())
            .then_with(|| left.path.cmp(&right.path))
    });
    let already_imported = candidates
        .iter()
        .filter(|item| item.already_imported)
        .count();
    Ok(LibraryImportPreview {
        root_path: root.map(|path| path.to_string_lossy().into_owned()),
        importable: candidates.len().saturating_sub(already_imported),
        already_imported,
        candidates,
        total_size_bytes,
    })
}

pub(crate) fn import_library_file(
    local_state: &LocalStateStore,
    path: &Path,
) -> Result<(), AppError> {
    let canonical_path = fs::canonicalize(path).map_err(|_| invalid_import_path())?;
    if !canonical_path.is_file() {
        return Err(invalid_import_path());
    }
    match normalized_extension(&canonical_path).as_deref() {
        Some("epub") => {
            let parsed = parse_epub_document(&canonical_path, ArchiveLimits::default())?;
            let fingerprint = fingerprint_file(&canonical_path)
                .map_err(|_| AppError::internal("INTERNAL", "fingerprint library EPUB"))?;
            let author = parsed.metadata.creators.join("、");
            let cover_resource_id = parsed.cover_resource_id.as_deref();
            let cover_media_type = cover_resource_id.and_then(|resource_id| {
                parsed
                    .manifest
                    .iter()
                    .find(|item| item.resource_id == resource_id)
                    .map(|item| item.media_type.as_str())
            });
            local_state.record_library_document(LibraryDocumentRecord {
                path: &canonical_path,
                document_kind: "epub",
                display_title: &parsed.metadata.title,
                author: (!author.is_empty()).then_some(author.as_str()),
                fingerprint: Some(&fingerprint.blake3),
                cover_resource_id,
                cover_media_type,
            })
        }
        Some("txt") => {
            let title = canonical_path
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .ok_or_else(invalid_import_path)?;
            local_state.record_library_document(LibraryDocumentRecord {
                path: &canonical_path,
                document_kind: "txt",
                display_title: &title,
                author: None,
                fingerprint: None,
                cover_resource_id: None,
                cover_media_type: None,
            })
        }
        _ => Err(AppError::validation(
            "UNSUPPORTED_LIBRARY_FILE",
            "书库批量导入仅支持 EPUB 和 TXT 文件。",
            "请选择 EPUB、TXT 文件或包含这些文件的文件夹。",
        )),
    }
}

fn refresh_epub_metadata(local_state: &LocalStateStore, path: &Path) -> Result<(), AppError> {
    let parsed = parse_epub_document(path, ArchiveLimits::default())?;
    let author = parsed.metadata.creators.join("、");
    let cover_resource_id = parsed.cover_resource_id.as_deref();
    let cover_media_type = cover_resource_id.and_then(|resource_id| {
        parsed
            .manifest
            .iter()
            .find(|item| item.resource_id == resource_id)
            .map(|item| item.media_type.as_str())
    });
    local_state.update_library_epub_metadata(
        path,
        &parsed.metadata.title,
        (!author.is_empty()).then_some(author.as_str()),
        cover_resource_id,
        cover_media_type,
    )
}

fn validated_directory(directory: String) -> Result<PathBuf, AppError> {
    let path = fs::canonicalize(directory.trim()).map_err(|_| invalid_import_directory())?;
    if path.is_dir() {
        Ok(path)
    } else {
        Err(invalid_import_directory())
    }
}

fn collect_library_files(root: &Path) -> Result<Vec<PathBuf>, AppError> {
    let mut files = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0_usize)];
    while let Some((directory, depth)) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|_| invalid_import_directory())?;
        for entry in entries {
            let entry = entry.map_err(|_| invalid_import_directory())?;
            let file_type = entry.file_type().map_err(|_| invalid_import_directory())?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() && depth < MAXIMUM_DIRECTORY_DEPTH {
                pending.push((entry.path(), depth + 1));
            } else if file_type.is_file() && is_supported_library_file(&entry.path()) {
                files.push(entry.path());
                if files.len() > MAXIMUM_IMPORT_FILES {
                    return Err(import_limit_exceeded());
                }
            }
        }
    }
    files.sort_by(|left, right| left.to_string_lossy().cmp(&right.to_string_lossy()));
    Ok(files)
}

fn normalized_extension(path: &Path) -> Option<String> {
    path.extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
}

fn is_supported_library_file(path: &Path) -> bool {
    matches!(normalized_extension(path).as_deref(), Some("epub" | "txt"))
}

fn invalid_import_path() -> AppError {
    AppError::validation(
        "LIBRARY_IMPORT_FILE_UNAVAILABLE",
        "无法读取选中的图书文件。",
        "确认文件仍存在且拥有读取权限后重试。",
    )
}

fn invalid_import_directory() -> AppError {
    AppError::validation(
        "LIBRARY_IMPORT_DIRECTORY_UNAVAILABLE",
        "无法读取选中的图书目录。",
        "确认目录仍存在且拥有读取权限后重试。",
    )
}

fn import_limit_exceeded() -> AppError {
    AppError::validation(
        "LIBRARY_IMPORT_LIMIT_EXCEEDED",
        format!("一次最多导入 {MAXIMUM_IMPORT_FILES} 本图书。"),
        "请选择较小的目录或分批导入。",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epub_test_fixtures::minimal_epub2;

    #[test]
    fn trims_group_names_and_rejects_empty_names() {
        assert_eq!(validated_group_name("  小说  ".to_owned()).unwrap(), "小说");
        assert!(validated_group_name("   ".to_owned()).is_err());
    }

    #[test]
    fn group_ids_are_namespaced_and_fixed_length() {
        let group_id = new_group_id();
        assert!(group_id.starts_with("group-"));
        assert!(validated_group_id(group_id).is_ok());
    }

    #[test]
    fn directory_scan_recurses_and_only_collects_epub_and_txt() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(directory.path().join("one.TXT"), "one").unwrap();
        fs::write(nested.join("two.epub"), "not parsed during scan").unwrap();
        fs::write(nested.join("ignored.pdf"), "ignored").unwrap();

        let files = collect_library_files(directory.path()).unwrap();

        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|path| path.ends_with("one.TXT")));
        assert!(files.iter().any(|path| path.ends_with("two.epub")));
    }

    #[test]
    fn preview_marks_existing_books_and_reports_sizes_without_parsing_epub() {
        let directory = tempfile::tempdir().unwrap();
        let existing = directory.path().join("existing.txt");
        let incoming = directory.path().join("incoming.epub");
        fs::write(&existing, "existing").unwrap();
        fs::write(&incoming, "preview does not parse EPUB").unwrap();
        let store = LocalStateStore::open(&directory.path().join("state.sqlite3")).unwrap();
        import_library_file(&store, &existing).unwrap();

        let preview = preview_paths(&store, None, vec![existing, incoming]).unwrap();

        assert_eq!(preview.candidates.len(), 2);
        assert_eq!(preview.importable, 1);
        assert_eq!(preview.already_imported, 1);
        assert!(preview.total_size_bytes > 0);
        assert!(
            preview.candidates.iter().any(
                |candidate| candidate.file_name == "existing.txt" && candidate.already_imported
            )
        );
    }

    #[test]
    fn importing_an_epub_records_metadata_and_a_cover_key_without_opening_history() {
        let fixture = minimal_epub2();
        let directory = tempfile::tempdir().unwrap();
        let store = LocalStateStore::open(&directory.path().join("state.sqlite3")).unwrap();

        import_library_file(&store, fixture.path()).unwrap();

        let library = store.library_snapshot(10).unwrap();
        assert_eq!(library.documents[0].display_title, "阅织 EPUB 2 测试");
        assert_eq!(library.documents[0].author.as_deref(), Some("第二版作者"));
        assert!(library.documents[0].cover_key.is_some());
        assert!(store.recent_documents(10).unwrap().is_empty());
    }
}
