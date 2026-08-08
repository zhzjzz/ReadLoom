#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextDocumentLimits {
    pub confirmation_threshold_bytes: u64,
    pub maximum_editable_bytes: u64,
}

impl TextDocumentLimits {
    pub const fn stage1_default() -> Self {
        Self {
            confirmation_threshold_bytes: 40 * 1024 * 1024,
            maximum_editable_bytes: 160 * 1024 * 1024,
        }
    }
}

impl Default for TextDocumentLimits {
    fn default() -> Self {
        Self::stage1_default()
    }
}
