#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ArchiveLimits {
    pub maximum_archive_bytes: u64,
    pub maximum_entries: usize,
    pub maximum_entry_bytes: u64,
    pub maximum_total_uncompressed_bytes: u64,
    pub maximum_compression_ratio: u64,
    pub maximum_xhtml_bytes: u64,
    pub maximum_css_bytes: u64,
    pub maximum_image_bytes: u64,
    pub maximum_font_bytes: u64,
    pub maximum_xml_bytes: u64,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            maximum_archive_bytes: 512 * 1024 * 1024,
            maximum_entries: 10_000,
            maximum_entry_bytes: 64 * 1024 * 1024,
            maximum_total_uncompressed_bytes: 1024 * 1024 * 1024,
            maximum_compression_ratio: 200,
            maximum_xhtml_bytes: 8 * 1024 * 1024,
            maximum_css_bytes: 4 * 1024 * 1024,
            maximum_image_bytes: 32 * 1024 * 1024,
            maximum_font_bytes: 16 * 1024 * 1024,
            maximum_xml_bytes: 4 * 1024 * 1024,
        }
    }
}
