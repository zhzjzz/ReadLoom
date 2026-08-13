use std::collections::HashMap;

use readloom_core::{BlockId, ChapterKey, EditableBlock, ViewAnchor};

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
    pub(crate) fn set_blocks<'a>(&mut self, blocks: impl IntoIterator<Item = &'a EditableBlock>) {
        self.block_order = blocks.into_iter().map(|block| block.id.clone()).collect();
        self.geometry
            .retain(|block_id, _| self.block_order.contains(block_id));
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
        let top_visible = self.block_order.iter().find_map(|block_id| {
            let geometry = self.geometry.get(block_id)?;
            let pixel = geometry.y + viewport_y;
            (pixel + geometry.height >= 0.0).then_some((block_id, pixel))
        });
        let (block_id, pixel_offset_from_viewport_top) = preferred.or(top_visible)?;
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

    #[cfg(test)]
    pub(crate) fn resolve_surviving_anchor(
        &self,
        anchor: &ViewAnchor,
        previous_order: &[BlockId],
    ) -> Option<ViewAnchor> {
        if self.geometry.contains_key(&anchor.block_id) {
            return Some(anchor.clone());
        }
        let old_index = previous_order
            .iter()
            .position(|block_id| block_id == &anchor.block_id)
            .unwrap_or(0);
        let fallback = previous_order
            .iter()
            .skip(old_index.saturating_add(1))
            .find(|block_id| self.geometry.contains_key(*block_id))
            .or_else(|| {
                previous_order
                    .iter()
                    .take(old_index)
                    .rev()
                    .find(|block_id| self.geometry.contains_key(*block_id))
            })
            .or_else(|| self.block_order.first())?;
        Some(ViewAnchor {
            block_id: fallback.clone(),
            character_offset_utf16: 0,
            ..anchor.clone()
        })
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
    fn a_deleted_anchor_falls_forward_then_backward_deterministically() {
        let previous = vec![BlockId::new("a"), BlockId::new("b"), BlockId::new("c")];
        let mut controller = ViewportAnchorController::default();
        controller.set_blocks(&[block("a"), block("c")]);
        controller.report_geometry(BlockId::new("a"), 0.0, 30.0);
        controller.report_geometry(BlockId::new("c"), 30.0, 30.0);
        let deleted = ViewAnchor {
            chapter_key: ChapterKey::new("chapter"),
            block_id: BlockId::new("b"),
            character_offset_utf16: 9,
            pixel_offset_from_viewport_top: -2.0,
        };

        let resolved = controller
            .resolve_surviving_anchor(&deleted, &previous)
            .expect("fallback anchor");

        assert_eq!(resolved.block_id, BlockId::new("c"));
        assert_eq!(resolved.character_offset_utf16, 0);
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
