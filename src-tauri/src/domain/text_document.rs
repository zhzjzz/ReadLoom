use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TextEncoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    Gbk,
    Gb18030,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LineEnding {
    Crlf,
    Lf,
    Cr,
    Mixed,
    None,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SaveLineEnding {
    Preserve,
    Crlf,
    Lf,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DocumentId(pub String);

#[derive(Debug, Clone)]
pub struct OpenedTextDocument {
    pub document_id: DocumentId,
    pub path: PathBuf,
    pub file_name: String,
    pub content: String,
    pub encoding: TextEncoding,
    pub has_bom: bool,
    pub line_ending: LineEnding,
    pub size_bytes: u64,
    pub read_only: bool,
    pub revision: u64,
}

#[derive(Debug, Clone)]
pub struct SavedTextDocument {
    pub document_id: DocumentId,
    pub path: PathBuf,
    pub file_name: String,
    pub encoding: TextEncoding,
    pub has_bom: bool,
    pub line_ending: LineEnding,
    pub size_bytes: u64,
    pub read_only: bool,
    pub revision: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TextBookmark {
    pub bookmark_id: String,
    pub character_offset: usize,
    pub line_number: usize,
    pub title: Option<String>,
    pub preview: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}
