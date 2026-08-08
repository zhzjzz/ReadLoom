use serde::{Deserialize, Serialize};

use crate::domain::document::{DocumentCapabilities, DocumentKind};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EpubMetadata {
    pub title: String,
    pub creators: Vec<String>,
    pub languages: Vec<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub identifier: Option<String>,
    pub publication_date: Option<String>,
    pub modified_date: Option<String>,
    pub rights: Vec<String>,
    pub subjects: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManifestItem {
    pub id: String,
    pub resource_id: String,
    pub media_type: String,
    pub properties: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpineItem {
    pub index: usize,
    pub idref: String,
    pub resource_id: String,
    pub media_type: String,
    pub linear: bool,
    pub properties: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TocNode {
    pub id: String,
    pub label: String,
    pub resource_id: Option<String>,
    pub fragment: Option<String>,
    pub children: Vec<TocNode>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum EpubLayout {
    Reflowable,
    Fixed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParsedEpubDocument {
    pub kind: DocumentKind,
    pub publication_id: String,
    pub version: String,
    pub metadata: EpubMetadata,
    pub cover_resource_id: Option<String>,
    pub manifest: Vec<ManifestItem>,
    pub spine: Vec<SpineItem>,
    pub toc: Vec<TocNode>,
    pub layout: EpubLayout,
    pub capabilities: DocumentCapabilities,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenedEpubDocument {
    pub document_id: String,
    pub session_id: String,
    pub bridge_token: String,
    pub file_name: String,
    pub display_path: String,
    pub file_fingerprint: String,
    pub document: ParsedEpubDocument,
    pub initial_locator: Option<EpubLocator>,
    pub bookmarks: Vec<EpubBookmark>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EpubLocator {
    pub document_id: String,
    pub document_fingerprint: String,
    pub spine_index: usize,
    pub spine_href: String,
    pub fragment: Option<String>,
    pub progression_in_chapter: f32,
    pub character_offset: Option<usize>,
}

impl EpubLocator {
    pub(crate) fn normalized(mut self) -> Self {
        self.progression_in_chapter = if self.progression_in_chapter.is_finite() {
            self.progression_in_chapter.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EpubBookmark {
    pub bookmark_id: String,
    pub locator: EpubLocator,
    pub title: Option<String>,
    pub chapter_title: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub valid: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EpubSearchRequest {
    pub document_id: String,
    pub request_id: String,
    pub query: String,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub whole_word: bool,
    pub maximum_results: Option<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EpubSearchResult {
    pub request_id: String,
    pub spine_index: usize,
    pub spine_href: String,
    pub chapter_title: String,
    pub character_offset: usize,
    pub temporary_snippet: String,
    pub match_start: usize,
    pub match_end: usize,
}
