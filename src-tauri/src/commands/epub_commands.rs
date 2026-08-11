use std::path::{Path, PathBuf};

use serde::Deserialize;
use tauri::State;

use crate::{
    application::{epub_document_service::EpubDocumentService, epub_edit_service::EpubEditService},
    domain::epub_document::{
        EpubBookmark, EpubLocator, EpubSearchRequest, EpubSearchResult, OpenedEpubDocument, TocNode,
    },
    error::AppError,
    infrastructure::storage::local_state::{
        LocalStateStore, RecentDocument, RecentDocumentRecord, now_ms,
    },
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenEpubDocumentRequest {
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloseEpubDocumentRequest {
    document_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveEpubProgressRequest {
    locator: EpubLocator,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveEpubBookmarkRequest {
    locator: EpubLocator,
    bookmark_id: Option<String>,
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteEpubBookmarkRequest {
    document_id: String,
    bookmark_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CancelEpubSearchRequest {
    document_id: String,
    request_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListRecentDocumentsRequest {
    maximum: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteRecentDocumentRequest {
    path: String,
}

#[tauri::command]
pub(crate) fn open_epub_document(
    state: State<'_, EpubDocumentService>,
    local_state: State<'_, LocalStateStore>,
    request: OpenEpubDocumentRequest,
) -> Result<OpenedEpubDocument, AppError> {
    if request.path.trim().is_empty() {
        return Err(AppError::validation(
            "INVALID_EPUB",
            "EPUB 文件路径不能为空。",
            "请通过文件选择器选择 EPUB 文件。",
        ));
    }
    let mut opened = state.open(&PathBuf::from(request.path))?;
    let context = state.session_context(&opened.document_id)?;
    if let Ok(Some(mut locator)) =
        local_state.load_epub_progress(&context.path, &context.file_fingerprint)
    {
        locator.document_id = context.document_id.clone();
        locator.document_fingerprint = context.file_fingerprint.clone();
        if let Ok((_, locator)) = state.validate_locator(locator) {
            opened.initial_locator = Some(locator);
        }
    }
    opened.bookmarks = local_state
        .bookmarks(
            &context.path,
            &context.document_id,
            &context.file_fingerprint,
        )
        .unwrap_or_default();
    let author = opened.document.metadata.creators.join("、");
    let cover_resource_id = opened.document.cover_resource_id.as_deref();
    let cover_media_type = cover_resource_id.and_then(|resource_id| {
        opened
            .document
            .manifest
            .iter()
            .find(|item| item.resource_id == resource_id)
            .map(|item| item.media_type.as_str())
    });
    let _ = local_state.record_document_opened(RecentDocumentRecord {
        path: &context.path,
        document_kind: "epub",
        display_title: &opened.document.metadata.title,
        author: (!author.is_empty()).then_some(author.as_str()),
        fingerprint: Some(&context.file_fingerprint),
        cover_resource_id,
        cover_media_type,
    });
    Ok(opened)
}

#[tauri::command]
pub(crate) fn close_epub_document(
    state: State<'_, EpubDocumentService>,
    edits: State<'_, EpubEditService>,
    request: CloseEpubDocumentRequest,
) -> Result<(), AppError> {
    edits.close_document(&request.document_id);
    state.close(&request.document_id)
}

#[tauri::command]
pub(crate) fn save_epub_progress(
    state: State<'_, EpubDocumentService>,
    local_state: State<'_, LocalStateStore>,
    request: SaveEpubProgressRequest,
) -> Result<EpubLocator, AppError> {
    let (path, locator) = state.validate_locator(request.locator)?;
    local_state.save_epub_progress(&path, &locator)?;
    Ok(locator)
}

#[tauri::command]
pub(crate) fn save_epub_bookmark(
    state: State<'_, EpubDocumentService>,
    local_state: State<'_, LocalStateStore>,
    request: SaveEpubBookmarkRequest,
) -> Result<EpubBookmark, AppError> {
    let title = request.title.map(|value| value.trim().to_owned());
    if title
        .as_ref()
        .is_some_and(|value| value.chars().count() > 128)
    {
        return Err(AppError::validation(
            "BOOKMARK_TITLE_TOO_LONG",
            "书签标题过长。",
            "请将标题缩短到 128 个字符以内。",
        ));
    }
    let (path, locator) = state.validate_locator(request.locator)?;
    let context = state.session_context(&locator.document_id)?;
    let chapter_title = toc_label(&context.parsed.toc, &locator.spine_href)
        .unwrap_or_else(|| format!("第 {} 章", locator.spine_index + 1));
    let timestamp = now_ms()?;
    let bookmark = EpubBookmark {
        bookmark_id: request.bookmark_id.unwrap_or_else(new_bookmark_id),
        locator,
        title: title.filter(|value| !value.is_empty()),
        chapter_title,
        created_at_ms: timestamp,
        updated_at_ms: timestamp,
        valid: true,
    };
    local_state.save_bookmark(&path, &bookmark)?;
    Ok(bookmark)
}

#[tauri::command]
pub(crate) fn delete_epub_bookmark(
    state: State<'_, EpubDocumentService>,
    local_state: State<'_, LocalStateStore>,
    request: DeleteEpubBookmarkRequest,
) -> Result<(), AppError> {
    if !request.bookmark_id.starts_with("bm-") || request.bookmark_id.len() > 80 {
        return Err(AppError::validation(
            "BOOKMARK_NOT_FOUND",
            "书签标识无效。",
            "刷新书签列表后重试。",
        ));
    }
    let context = state.session_context(&request.document_id)?;
    local_state.delete_bookmark(&context.path, &request.bookmark_id)
}

#[tauri::command]
pub(crate) async fn search_epub_document(
    state: State<'_, EpubDocumentService>,
    request: EpubSearchRequest,
) -> Result<Vec<EpubSearchResult>, AppError> {
    let service = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.search(request))
        .await
        .map_err(|_| AppError::internal("EPUB_SEARCH_FAILED", "join EPUB search worker"))?
}

#[tauri::command]
pub(crate) fn cancel_epub_search(
    state: State<'_, EpubDocumentService>,
    request: CancelEpubSearchRequest,
) {
    state.cancel_search(&request.document_id, &request.request_id);
}

#[tauri::command]
pub(crate) fn list_recent_documents(
    local_state: State<'_, LocalStateStore>,
    request: ListRecentDocumentsRequest,
) -> Result<Vec<RecentDocument>, AppError> {
    local_state.recent_documents(request.maximum.unwrap_or(20))
}

#[tauri::command]
pub(crate) fn delete_recent_document(
    local_state: State<'_, LocalStateStore>,
    request: DeleteRecentDocumentRequest,
) -> Result<(), AppError> {
    if request.path.trim().is_empty() {
        return Err(AppError::validation(
            "RECENT_DOCUMENT_PATH_EMPTY",
            "最近文件路径不能为空。",
            "刷新最近文件列表后重试。",
        ));
    }
    local_state.delete_recent(Path::new(&request.path))
}

fn toc_label(nodes: &[TocNode], resource_id: &str) -> Option<String> {
    for node in nodes {
        if node.resource_id.as_deref() == Some(resource_id) && !node.label.trim().is_empty() {
            return Some(node.label.clone());
        }
        if let Some(label) = toc_label(&node.children, resource_id) {
            return Some(label);
        }
    }
    None
}

fn new_bookmark_id() -> String {
    let mut bytes = [0_u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        return format!("bm-{}", now_ms().unwrap_or_default());
    }
    let mut value = String::with_capacity(35);
    value.push_str("bm-");
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}
