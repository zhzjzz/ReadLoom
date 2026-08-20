use std::collections::HashMap;

use readloom_core::{BlockId, ChapterKey, ViewAnchor};

#[cfg(test)]
use readloom_core::EditableBlock;

#[derive(Debug, Clone, Copy, PartialEq)]
struct BlockGeometry {
    y: f32,
    height: f32,
}

#[derive(Debug, Default)]
pub(crate) struct ViewportAnchorController {
    geometry: HashMap<BlockId, BlockGeometry>,
    block_order: Vec<BlockId>,
}

impl ViewportAnchorController {
    pub(crate) fn clear(&mut self) {
        self.geometry.clear();
        self.block_order.clear();
    }
    #[cfg(test)]
    pub(crate) fn set_blocks<'a>(&mut self, blocks: impl IntoIterator<Item = &'a EditableBlock>) {
        self.set_block_ids(blocks.into_iter().map(|block| &block.id));
    }

    #[cfg(test)]
    pub(crate) fn set_block_ids<'a>(&mut self, block_ids: impl IntoIterator<Item = &'a BlockId>) {
        self.block_order = block_ids.into_iter().cloned().collect();
        self.geometry.clear();
    }

    pub(crate) fn report_geometry(&mut self, block_id: BlockId, y: f32, height: f32) {
        if height.is_finite() && height >= 0.0 && y.is_finite() {
            if !self.block_order.contains(&block_id) {
                self.block_order.push(block_id.clone());
            }
            self.geometry.insert(block_id, BlockGeometry { y, height });
        }
    }

    pub(crate) fn capture(
        &self,
        chapter_key: ChapterKey,
        preferred_block: Option<&BlockId>,
        character_offset_utf16: usize,
        viewport_y: f32,
    ) -> Option<ViewAnchor> {
        let preferred = preferred_block.and_then(|block_id| {
            let geometry = self.geometry.get(block_id)?;
            Some((block_id, geometry.y + viewport_y))
        });
        let first_with_visible_top = self.block_order.iter().find_map(|block_id| {
            let geometry = self.geometry.get(block_id)?;
            let pixel = geometry.y + viewport_y;
            (pixel >= 0.0).then_some((block_id, pixel))
        });
        let intersecting = self.block_order.iter().find_map(|block_id| {
            let geometry = self.geometry.get(block_id)?;
            let pixel = geometry.y + viewport_y;
            (pixel + geometry.height >= 0.0).then_some((block_id, pixel))
        });
        let (block_id, pixel_offset_from_viewport_top) =
            preferred.or(first_with_visible_top).or(intersecting)?;
        Some(ViewAnchor {
            chapter_key,
            block_id: block_id.clone(),
            character_offset_utf16,
            pixel_offset_from_viewport_top,
        })
    }

    pub(crate) fn viewport_y_for(&self, anchor: &ViewAnchor) -> Option<f32> {
        self.geometry
            .get(&anchor.block_id)
            .map(|geometry| anchor.pixel_offset_from_viewport_top - geometry.y)
    }
}

pub(crate) fn utf16_offset_for_byte(text: &str, byte_offset: usize) -> usize {
    let mut byte_offset = byte_offset.min(text.len());
    while !text.is_char_boundary(byte_offset) {
        byte_offset = byte_offset.saturating_sub(1);
    }
    text[..byte_offset].encode_utf16().count()
}

pub(crate) fn byte_offset_for_utf16(text: &str, utf16_offset: usize) -> usize {
    let mut units = 0;
    for (byte_offset, character) in text.char_indices() {
        let next_units = units + character.len_utf16();
        if next_units > utf16_offset {
            return byte_offset;
        }
        units = next_units;
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use readloom_core::ParagraphKind;

    use super::*;

    fn block(id: &str) -> EditableBlock {
        EditableBlock {
            id: BlockId::new(id),
            chapter_key: ChapterKey::new("chapter"),
            kind: ParagraphKind::Paragraph,
            text: id.to_owned(),
            editable: true,
        }
    }

    #[test]
    fn restoring_an_anchor_uses_measured_geometry_without_line_height_estimation() {
        let blocks = [block("a"), block("b"), block("c")];
        let mut controller = ViewportAnchorController::default();
        controller.set_blocks(&blocks);
        controller.report_geometry(BlockId::new("a"), 20.0, 71.25);
        controller.report_geometry(BlockId::new("b"), 91.25, 183.75);
        controller.report_geometry(BlockId::new("c"), 275.0, 44.5);
        let anchor = ViewAnchor {
            chapter_key: ChapterKey::new("chapter"),
            block_id: BlockId::new("b"),
            character_offset_utf16: 7,
            pixel_offset_from_viewport_top: -23.625,
        };

        let viewport_y = controller.viewport_y_for(&anchor).expect("resolve anchor");

        assert_eq!(viewport_y, -114.875);
        assert_eq!(91.25 + viewport_y, anchor.pixel_offset_from_viewport_top);
    }

    #[test]
    fn replacing_the_presentation_model_invalidates_stale_geometry() {
        let blocks = [block("a"), block("b")];
        let mut controller = ViewportAnchorController::default();
        controller.set_blocks(&blocks);
        controller.report_geometry(BlockId::new("b"), 91.25, 183.75);
        let anchor = ViewAnchor {
            chapter_key: ChapterKey::new("chapter"),
            block_id: BlockId::new("b"),
            character_offset_utf16: 0,
            pixel_offset_from_viewport_top: -20.0,
        };
        assert!(controller.viewport_y_for(&anchor).is_some());

        controller.set_blocks(&blocks);

        assert!(
            controller.viewport_y_for(&anchor).is_none(),
            "a new model must wait for its own geometry instead of restoring from stale rows"
        );
    }

    #[test]
    fn viewport_capture_prefers_the_first_block_whose_top_is_visible() {
        let blocks = [block("estimated-tall"), block("actually-visible")];
        let mut controller = ViewportAnchorController::default();
        controller.set_blocks(&blocks);
        controller.report_geometry(BlockId::new("estimated-tall"), 0.0, 240.0);
        controller.report_geometry(BlockId::new("actually-visible"), 200.0, 40.0);

        let anchor = controller
            .capture(ChapterKey::new("chapter"), None, 0, -150.0)
            .expect("visible anchor");

        assert_eq!(anchor.block_id, BlockId::new("actually-visible"));
        assert_eq!(anchor.pixel_offset_from_viewport_top, 50.0);
    }

    #[test]
    fn chinese_caret_bytes_are_converted_to_utf16_units() {
        assert_eq!(utf16_offset_for_byte("a中文😀z", 7), 3);
        assert_eq!(utf16_offset_for_byte("a中文😀z", 11), 5);
    }

    #[test]
    fn utf16_caret_is_converted_back_to_a_valid_utf8_boundary() {
        assert_eq!(byte_offset_for_utf16("a中文😀z", 3), 7);
        assert_eq!(byte_offset_for_utf16("a中文😀z", 5), 11);
        assert_eq!(byte_offset_for_utf16("a中文😀z", 4), 7);
        assert_eq!(byte_offset_for_utf16("a中文😀z", 99), 12);
    }
}
