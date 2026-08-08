use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    application::text_document_service::{
        OpenTextDocument, SaveTextDocument, SaveTextDocumentAs, TextDocumentService,
    },
    domain::text_document::{
        DocumentId, LineEnding, OpenedTextDocument, SaveLineEnding, SavedTextDocument, TextEncoding,
    },
    error::AppError,
    infrastructure::storage::local_state::{LocalStateStore, RecentDocumentRecord},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenTextDocumentRequest {
    path: String,
    encoding_override: Option<TextEncoding>,
    #[serde(default)]
    allow_large: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReopenTextDocumentRequest {
    document_id: String,
    encoding: TextEncoding,
    #[serde(default)]
    allow_large: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveTextDocumentRequest {
    document_id: String,
    content: String,
    encoding: TextEncoding,
    has_bom: bool,
    line_ending: SaveLineEnding,
    expected_revision: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveTextDocumentAsRequest {
    document_id: String,
    target_path: String,
    content: String,
    encoding: TextEncoding,
    has_bom: bool,
    line_ending: SaveLineEnding,
    expected_revision: u64,
    #[serde(default)]
    allow_overwrite: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseTextDocumentRequest {
    document_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedTextDocumentDto {
    document_id: String,
    file_name: String,
    display_path: String,
    content: String,
    encoding: TextEncoding,
    has_bom: bool,
    line_ending: LineEnding,
    size_bytes: u64,
    read_only: bool,
    revision: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedTextDocumentDto {
    document_id: String,
    file_name: String,
    display_path: String,
    encoding: TextEncoding,
    has_bom: bool,
    line_ending: LineEnding,
    size_bytes: u64,
    read_only: bool,
    revision: u64,
}

#[tauri::command]
pub fn open_text_document(
    state: State<'_, TextDocumentService>,
    local_state: State<'_, LocalStateStore>,
    request: OpenTextDocumentRequest,
) -> Result<OpenedTextDocumentDto, AppError> {
    let path = validated_path(request.path)?;
    let opened = state.open(OpenTextDocument {
        path,
        encoding_override: request.encoding_override,
        allow_large: request.allow_large,
    })?;
    let _ = local_state.record_recent(RecentDocumentRecord {
        path: &opened.path,
        document_kind: "txt",
        display_title: &opened.file_name,
        author: None,
        fingerprint: None,
    });
    Ok(OpenedTextDocumentDto::from(opened))
}

#[tauri::command]
pub fn reopen_text_document(
    state: State<'_, TextDocumentService>,
    request: ReopenTextDocumentRequest,
) -> Result<OpenedTextDocumentDto, AppError> {
    let document_id = validated_document_id(request.document_id)?;
    state
        .reopen(&document_id, request.encoding, request.allow_large)
        .map(OpenedTextDocumentDto::from)
}

#[tauri::command]
pub fn save_text_document(
    state: State<'_, TextDocumentService>,
    request: SaveTextDocumentRequest,
) -> Result<SavedTextDocumentDto, AppError> {
    state
        .save(SaveTextDocument {
            document_id: validated_document_id(request.document_id)?,
            content: request.content,
            encoding: request.encoding,
            has_bom: request.has_bom,
            line_ending: request.line_ending,
            expected_revision: request.expected_revision,
        })
        .map(SavedTextDocumentDto::from)
}

#[tauri::command]
pub fn save_text_document_as(
    state: State<'_, TextDocumentService>,
    request: SaveTextDocumentAsRequest,
) -> Result<SavedTextDocumentDto, AppError> {
    state
        .save_as(SaveTextDocumentAs {
            document_id: validated_document_id(request.document_id)?,
            target_path: validated_path(request.target_path)?,
            content: request.content,
            encoding: request.encoding,
            has_bom: request.has_bom,
            line_ending: request.line_ending,
            expected_revision: request.expected_revision,
            allow_overwrite: request.allow_overwrite,
        })
        .map(SavedTextDocumentDto::from)
}

#[tauri::command]
pub fn close_text_document(
    state: State<'_, TextDocumentService>,
    request: CloseTextDocumentRequest,
) -> Result<(), AppError> {
    state.close(&validated_document_id(request.document_id)?)
}

fn validated_path(path: String) -> Result<PathBuf, AppError> {
    if path.trim().is_empty() {
        return Err(AppError::validation(
            "INVALID_PATH",
            "文件路径不能为空。",
            "请通过文件选择器选择文件。",
        ));
    }
    Ok(PathBuf::from(path))
}

fn validated_document_id(document_id: String) -> Result<DocumentId, AppError> {
    if !document_id.starts_with("txt-") || document_id.len() != 20 {
        return Err(AppError::validation(
            "DOCUMENT_NOT_FOUND",
            "文档会话标识无效。",
            "重新打开文件后再试。",
        ));
    }
    Ok(DocumentId(document_id))
}

impl From<OpenedTextDocument> for OpenedTextDocumentDto {
    fn from(document: OpenedTextDocument) -> Self {
        Self {
            document_id: document.document_id.0,
            file_name: document.file_name,
            display_path: document.path.display().to_string(),
            content: document.content,
            encoding: document.encoding,
            has_bom: document.has_bom,
            line_ending: document.line_ending,
            size_bytes: document.size_bytes,
            read_only: document.read_only,
            revision: document.revision,
        }
    }
}

impl From<SavedTextDocument> for SavedTextDocumentDto {
    fn from(document: SavedTextDocument) -> Self {
        Self {
            document_id: document.document_id.0,
            file_name: document.file_name,
            display_path: document.path.display().to_string(),
            encoding: document.encoding,
            has_bom: document.has_bom,
            line_ending: document.line_ending,
            size_bytes: document.size_bytes,
            read_only: document.read_only,
            revision: document.revision,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_free_form_invalid_document_ids() {
        let error = validated_document_id("../../other".to_owned()).expect_err("invalid id");
        assert_eq!(error.to_dto().code, "DOCUMENT_NOT_FOUND");
    }
}
