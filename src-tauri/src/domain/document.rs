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
    pub can_replace_cover: bool,
    pub can_edit_structure: bool,
    pub can_overwrite_original: bool,
}

impl DocumentCapabilities {
    pub(crate) const fn epub(editable: bool) -> Self {
        Self {
            can_read: true,
            can_edit_text: editable,
            can_edit_metadata: editable,
            can_search: true,
            has_chapters: true,
            has_bookmarks: true,
            can_save: false,
            can_save_as: editable,
            can_replace_cover: editable,
            can_edit_structure: false,
            can_overwrite_original: false,
        }
    }
}
