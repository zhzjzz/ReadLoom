use std::path::PathBuf;

use serde::Deserialize;
use tauri::State;

use crate::{
    application::epub_edit_service::EpubEditService,
    domain::epub_edit::{
        ChapterDraftAccepted, ChapterDraftUpdate, ChapterEditDto, EpubDraftValidation,
        EpubEditDraft, EpubMetadataPatch, ImportedChapterImage, SavedEpubDocument,
    },
    error::AppError,
    infrastructure::storage::local_state::{LocalStateStore, RecentDocumentRecord},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BeginEpubEditRequest {
    document_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EpubEditSessionRequest {
    edit_session_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateEpubMetadataRequest {
    edit_session_id: String,
    expected_revision: u64,
    metadata_patch: EpubMetadataPatch,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReplaceEpubCoverRequest {
    edit_session_id: String,
    expected_revision: u64,
    selected_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RevisionedEpubEditRequest {
    edit_session_id: String,
    expected_revision: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrepareEpubOverwriteRequest {
    edit_session_id: String,
    expected_revision: u64,
    target_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveEpubAsRequest {
    edit_session_id: String,
    expected_revision: u64,
    target_path: String,
    confirmation_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BeginChapterEditRequest {
    edit_session_id: String,
    spine_index: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChapterEditRequest {
    chapter_edit_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FlushChapterEditRequest {
    chapter_edit_id: String,
    revision: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportChapterImageRequest {
    edit_session_id: String,
    chapter_edit_id: String,
    selected_path: String,
}

#[tauri::command]
pub(crate) fn begin_epub_edit(
    state: State<'_, EpubEditService>,
    request: BeginEpubEditRequest,
) -> Result<EpubEditDraft, AppError> {
    validate_identifier(&request.document_id, "epub-")?;
    state.begin(&request.document_id)
}

#[tauri::command]
pub(crate) fn get_epub_edit_draft(
    state: State<'_, EpubEditService>,
    request: EpubEditSessionRequest,
) -> Result<EpubEditDraft, AppError> {
    validate_identifier(&request.edit_session_id, "edit-")?;
    state.get(&request.edit_session_id)
}

#[tauri::command]
pub(crate) fn update_epub_metadata(
    state: State<'_, EpubEditService>,
    request: UpdateEpubMetadataRequest,
) -> Result<EpubEditDraft, AppError> {
    validate_identifier(&request.edit_session_id, "edit-")?;
    state.update_metadata(
        &request.edit_session_id,
        request.expected_revision,
        request.metadata_patch,
    )
}

#[tauri::command]
pub(crate) async fn replace_epub_cover(
    state: State<'_, EpubEditService>,
    request: ReplaceEpubCoverRequest,
) -> Result<EpubEditDraft, AppError> {
    validate_identifier(&request.edit_session_id, "edit-")?;
    if request.selected_path.trim().is_empty() {
        return Err(invalid_request());
    }
    let service = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        service.replace_cover(
            &request.edit_session_id,
            request.expected_revision,
            &PathBuf::from(request.selected_path),
        )
    })
    .await
    .map_err(|_| AppError::internal("INVALID_COVER_IMAGE", "join cover validation worker"))?
}

#[tauri::command]
pub(crate) fn remove_epub_cover_change(
    state: State<'_, EpubEditService>,
    request: RevisionedEpubEditRequest,
) -> Result<EpubEditDraft, AppError> {
    validate_identifier(&request.edit_session_id, "edit-")?;
    state.remove_cover_change(&request.edit_session_id, request.expected_revision)
}

#[tauri::command]
pub(crate) fn analyze_epub_chapter_editability(
    state: State<'_, EpubEditService>,
    request: BeginChapterEditRequest,
) -> Result<ChapterEditDto, AppError> {
    validate_identifier(&request.edit_session_id, "edit-")?;
    state.begin_chapter_edit(&request.edit_session_id, request.spine_index)
}

#[tauri::command]
pub(crate) fn begin_epub_chapter_edit(
    state: State<'_, EpubEditService>,
    request: BeginChapterEditRequest,
) -> Result<ChapterEditDto, AppError> {
    validate_identifier(&request.edit_session_id, "edit-")?;
    state.begin_chapter_edit(&request.edit_session_id, request.spine_index)
}

#[tauri::command]
pub(crate) fn update_epub_chapter_draft(
    state: State<'_, EpubEditService>,
    request: ChapterDraftUpdate,
) -> Result<ChapterDraftAccepted, AppError> {
    validate_identifier(&request.chapter_edit_id, "chapter-edit-")?;
    state.update_chapter_draft(request)
}

#[tauri::command]
pub(crate) fn flush_epub_chapter_draft(
    state: State<'_, EpubEditService>,
    request: FlushChapterEditRequest,
) -> Result<ChapterDraftAccepted, AppError> {
    validate_identifier(&request.chapter_edit_id, "chapter-edit-")?;
    state.flush_chapter_draft(&request.chapter_edit_id, request.revision)
}

#[tauri::command]
pub(crate) fn validate_epub_chapter_draft(
    state: State<'_, EpubEditService>,
    request: ChapterEditRequest,
) -> Result<ChapterEditDto, AppError> {
    validate_identifier(&request.chapter_edit_id, "chapter-edit-")?;
    state.validate_chapter_draft(&request.chapter_edit_id)
}

#[tauri::command]
pub(crate) fn revert_epub_chapter_draft(
    state: State<'_, EpubEditService>,
    request: ChapterEditRequest,
) -> Result<ChapterEditDto, AppError> {
    validate_identifier(&request.chapter_edit_id, "chapter-edit-")?;
    state.revert_chapter_draft(&request.chapter_edit_id)
}

#[tauri::command]
pub(crate) fn close_epub_chapter_edit(
    state: State<'_, EpubEditService>,
    request: ChapterEditRequest,
) -> Result<(), AppError> {
    validate_identifier(&request.chapter_edit_id, "chapter-edit-")?;
    state.validate_chapter_draft(&request.chapter_edit_id)?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn import_epub_chapter_image(
    state: State<'_, EpubEditService>,
    request: ImportChapterImageRequest,
) -> Result<ImportedChapterImage, AppError> {
    validate_identifier(&request.edit_session_id, "edit-")?;
    validate_identifier(&request.chapter_edit_id, "chapter-edit-")?;
    if request.selected_path.trim().is_empty() {
        return Err(invalid_request());
    }
    let service = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        service.import_chapter_image(
            &request.edit_session_id,
            &request.chapter_edit_id,
            &PathBuf::from(request.selected_path),
        )
    })
    .await
    .map_err(|_| AppError::internal("IMAGE_IMPORT_FAILED", "join image validation worker"))?
}

#[tauri::command]
pub(crate) fn validate_epub_draft(
    state: State<'_, EpubEditService>,
    request: EpubEditSessionRequest,
) -> Result<EpubDraftValidation, AppError> {
    validate_identifier(&request.edit_session_id, "edit-")?;
    state.validate(&request.edit_session_id)
}

#[tauri::command]
pub(crate) fn prepare_epub_overwrite_confirmation(
    state: State<'_, EpubEditService>,
    request: PrepareEpubOverwriteRequest,
) -> Result<String, AppError> {
    validate_identifier(&request.edit_session_id, "edit-")?;
    if request.target_path.trim().is_empty() {
        return Err(invalid_request());
    }
    state.prepare_overwrite(
        &request.edit_session_id,
        request.expected_revision,
        &PathBuf::from(request.target_path),
    )
}

#[tauri::command]
pub(crate) async fn save_epub_as(
    state: State<'_, EpubEditService>,
    local_state: State<'_, LocalStateStore>,
    request: SaveEpubAsRequest,
) -> Result<SavedEpubDocument, AppError> {
    validate_identifier(&request.edit_session_id, "edit-")?;
    if request.target_path.trim().is_empty() {
        return Err(invalid_request());
    }
    let service = state.inner().clone();
    let saved = tauri::async_runtime::spawn_blocking(move || {
        service.save_as(
            &request.edit_session_id,
            request.expected_revision,
            &PathBuf::from(&request.target_path),
            request.confirmation_token.as_deref(),
        )
    })
    .await
    .map_err(|_| AppError::internal("REPACK_FAILED", "join EPUB save worker"))??;
    let author = saved.document.metadata.creators.join("、");
    let _ = local_state.record_recent(RecentDocumentRecord {
        path: &PathBuf::from(&saved.target_path),
        document_kind: "epub",
        display_title: &saved.document.metadata.title,
        author: (!author.is_empty()).then_some(author.as_str()),
        fingerprint: Some(&saved.file_fingerprint),
    });
    Ok(saved)
}

#[tauri::command]
pub(crate) fn cancel_epub_save(
    state: State<'_, EpubEditService>,
    request: EpubEditSessionRequest,
) -> Result<(), AppError> {
    validate_identifier(&request.edit_session_id, "edit-")?;
    state.cancel_save(&request.edit_session_id)
}

#[tauri::command]
pub(crate) fn discard_epub_draft(
    state: State<'_, EpubEditService>,
    request: EpubEditSessionRequest,
) -> Result<(), AppError> {
    validate_identifier(&request.edit_session_id, "edit-")?;
    state.discard(&request.edit_session_id)
}

#[tauri::command]
pub(crate) fn close_epub_edit_session(
    state: State<'_, EpubEditService>,
    request: EpubEditSessionRequest,
) -> Result<(), AppError> {
    validate_identifier(&request.edit_session_id, "edit-")?;
    state.discard(&request.edit_session_id)
}

fn validate_identifier(value: &str, prefix: &str) -> Result<(), AppError> {
    if value.starts_with(prefix)
        && value.len() <= 96
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        Ok(())
    } else {
        Err(invalid_request())
    }
}

fn invalid_request() -> AppError {
    AppError::validation(
        "EPUB_EDIT_SESSION_NOT_FOUND",
        "EPUB 编辑请求无效或草稿已失效。",
        "重新打开书籍信息面板后再试。",
    )
}
