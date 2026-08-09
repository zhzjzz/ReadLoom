#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextDocumentLimits {
    pub confirmation_threshold_bytes: u64,
    pub maximum_editable_bytes: u64,
}

impl Default for TextDocumentLimits {
    fn default() -> Self {
        Self {
            confirmation_threshold_bytes: 40 * 1024 * 1024,
            maximum_editable_bytes: 160 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EpubChapterEditLimits {
    pub maximum_xhtml_bytes: usize,
    pub maximum_text_characters: usize,
    pub maximum_nodes: usize,
    pub maximum_depth: usize,
    pub maximum_images: usize,
    pub maximum_paste_bytes: usize,
    pub maximum_sync_bytes: usize,
}

impl Default for EpubChapterEditLimits {
    fn default() -> Self {
        Self {
            maximum_xhtml_bytes: 2 * 1024 * 1024,
            maximum_text_characters: 1_000_000,
            maximum_nodes: 60_000,
            maximum_depth: 64,
            maximum_images: 200,
            maximum_paste_bytes: 2 * 1024 * 1024,
            maximum_sync_bytes: 3 * 1024 * 1024,
        }
    }
}
