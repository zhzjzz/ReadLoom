use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DocumentKind {
    Txt,
    Epub,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocumentCapabilities {
    pub can_read: bool,
    pub can_edit_text: bool,
    pub can_edit_metadata: bool,
    pub can_search: bool,
    pub has_chapters: bool,
    pub has_bookmarks: bool,
    pub can_save: bool,
    pub can_save_as: bool,
}

impl DocumentCapabilities {
    pub(crate) const fn epub() -> Self {
        Self {
            can_read: true,
            can_edit_text: false,
            can_edit_metadata: false,
            can_search: true,
            has_chapters: true,
            has_bookmarks: true,
            can_save: false,
            can_save_as: false,
        }
    }
}
