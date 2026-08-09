use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::epub_document::{EpubMetadata, ParsedEpubDocument};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EpubMetadataDraft {
    pub title: String,
    pub creators: Vec<String>,
    pub contributors: Vec<String>,
    pub language: String,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub identifier: String,
    pub publication_date: Option<String>,
    pub subjects: Vec<String>,
    pub rights: Vec<String>,
}

impl EpubMetadataDraft {
    pub(crate) fn from_publication(metadata: &EpubMetadata, publication_id: &str) -> Self {
        Self {
            title: metadata.title.clone(),
            creators: metadata.creators.clone(),
            contributors: metadata.contributors.clone(),
            language: metadata
                .languages
                .first()
                .cloned()
                .unwrap_or_else(|| "und".to_owned()),
            publisher: metadata.publisher.clone(),
            description: metadata.description.clone(),
            identifier: metadata
                .identifier
                .clone()
                .unwrap_or_else(|| publication_id.to_owned()),
            publication_date: metadata.publication_date.clone(),
            subjects: metadata.subjects.clone(),
            rights: metadata.rights.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EpubMetadataPatch {
    pub title: Option<String>,
    pub creators: Option<Vec<String>>,
    pub contributors: Option<Vec<String>>,
    pub language: Option<String>,
    pub publisher: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub identifier: Option<String>,
    pub publication_date: Option<Option<String>>,
    pub subjects: Option<Vec<String>>,
    pub rights: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum EpubCoverState {
    Unchanged,
    Replaced,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EpubCoverDraft {
    pub state: EpubCoverState,
    pub original_resource_id: Option<String>,
    pub current_resource_id: Option<String>,
    pub preview_resource_id: Option<String>,
    pub media_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EpubDraftChanges {
    pub metadata_fields: Vec<String>,
    pub cover_changed: bool,
    pub modified_chapters: Vec<usize>,
    pub added_resources: usize,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ChapterCompatibilityLevel {
    Full,
    Limited,
    ReadOnly,
    Unsupported,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ChapterValidationState {
    Valid,
    Warning,
    Invalid,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChapterEditWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChapterEditCapabilities {
    pub can_edit: bool,
    pub can_format: bool,
    pub can_edit_links: bool,
    pub can_import_images: bool,
    pub can_preview: bool,
    pub can_revert: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChapterEditDto {
    pub chapter_edit_id: String,
    pub edit_session_id: String,
    pub document_id: String,
    pub spine_index: usize,
    pub manifest_item_id: String,
    pub chapter_href: String,
    pub chapter_title: String,
    pub original_resource_hash: String,
    pub editor_document: Value,
    pub compatibility_level: ChapterCompatibilityLevel,
    pub warnings: Vec<ChapterEditWarning>,
    pub revision: u64,
    pub accepted_revision: u64,
    pub dirty: bool,
    pub validation_state: ChapterValidationState,
    pub preview_revision: u64,
    pub capabilities: ChapterEditCapabilities,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChapterDraftUpdate {
    pub chapter_edit_id: String,
    pub base_revision: u64,
    pub client_revision: u64,
    pub editor_document: Value,
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChapterDraftAccepted {
    pub chapter_edit_id: String,
    pub request_id: String,
    pub client_revision: u64,
    pub accepted_revision: u64,
    pub dirty: bool,
    pub warnings: Vec<ChapterEditWarning>,
    pub preview_revision: u64,
    pub publication_revision: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportedChapterImage {
    pub chapter_edit_id: String,
    pub resource_id: String,
    pub editor_src: String,
    pub preview_url: String,
    pub media_type: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum EpubValidationSeverity {
    Error,
    Warning,
    Information,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EpubValidationIssue {
    pub code: String,
    pub message: String,
    pub severity: EpubValidationSeverity,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EpubDraftValidation {
    pub errors: Vec<EpubValidationIssue>,
    pub warnings: Vec<EpubValidationIssue>,
    pub information: Vec<EpubValidationIssue>,
    pub can_save: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EpubEditDraft {
    pub edit_session_id: String,
    pub document_id: String,
    pub source_path: String,
    pub publication_id: String,
    pub opf_resource_id: String,
    pub metadata: EpubMetadataDraft,
    pub cover: EpubCoverDraft,
    pub changes: EpubDraftChanges,
    pub dirty: bool,
    pub validation: EpubDraftValidation,
    pub revision: u64,
    pub saved_revision: u64,
    pub saving: bool,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SavedEpubDocument {
    pub edit_session_id: String,
    pub target_path: String,
    pub file_fingerprint: String,
    pub document: ParsedEpubDocument,
    pub draft: EpubEditDraft,
}
