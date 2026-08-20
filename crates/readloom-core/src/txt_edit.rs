use std::{ops::Range, sync::Arc};

use crate::{
    BlockId, ChapterKey, EditError, EditableBlock, JoinDirection, LineEnding, ParagraphKind,
    ReaderDocument,
};

#[derive(Debug, Clone)]
struct TxtReplacementGroup {
    source_range: Range<usize>,
    original_blocks: Vec<(BlockId, String)>,
    current_block_ids: Vec<BlockId>,
}

#[derive(Debug, Clone)]
pub struct TxtDraft {
    original: Arc<str>,
    blocks: Vec<EditableBlock>,
    replacement_groups: Vec<TxtReplacementGroup>,
    inserted_line_ending: &'static str,
    next_block_id: u64,
}

impl TxtDraft {
    pub fn from_document(document: &ReaderDocument) -> Self {
        let mut blocks = Vec::with_capacity(document.paragraphs().len());
        let mut replacement_groups = Vec::with_capacity(document.paragraphs().len());
        for paragraph in document.paragraphs() {
            let text = paragraph.text.clone();
            let id = paragraph.block_id.clone();
            blocks.push(EditableBlock {
                id: id.clone(),
                chapter_key: ChapterKey::new(format!("txt:{}", paragraph.chapter_index)),
                kind: paragraph.kind,
                text: text.clone(),
                editable: true,
            });
            replacement_groups.push(TxtReplacementGroup {
                source_range: paragraph.source_start..paragraph.source_end,
                original_blocks: vec![(id.clone(), text)],
                current_block_ids: vec![id],
            });
        }
        Self {
            original: Arc::from(document.content()),
            next_block_id: blocks.len() as u64,
            blocks,
            replacement_groups,
            inserted_line_ending: match document.primary_line_ending() {
                LineEnding::Crlf => "\r\n",
                LineEnding::Cr => "\r",
                LineEnding::Lf | LineEnding::Mixed | LineEnding::None => "\n",
            },
        }
    }

    pub fn blocks(&self) -> &[EditableBlock] {
        &self.blocks
    }

    pub fn replace_block_text(&mut self, id: &BlockId, text: String) -> Result<String, EditError> {
        let block = self
            .blocks
            .iter_mut()
            .find(|block| block.id == *id)
            .ok_or(EditError::MissingBlock)?;
        if !block.editable {
            return Err(EditError::ReadOnlyBlock);
        }
        Ok(std::mem::replace(&mut block.text, text))
    }

    pub(crate) fn split_block_text(
        &mut self,
        id: &BlockId,
        parts: Vec<String>,
    ) -> Result<Vec<BlockId>, EditError> {
        debug_assert!(parts.len() > 1);
        let block_index = self
            .blocks
            .iter()
            .position(|block| block.id == *id)
            .ok_or(EditError::MissingBlock)?;
        if !self.blocks[block_index].editable {
            return Err(EditError::ReadOnlyBlock);
        }
        let original = self.blocks[block_index].clone();
        let mut replacement_ids = Vec::with_capacity(parts.len());
        replacement_ids.push(original.id.clone());
        self.blocks[block_index].text = parts[0].clone();
        for (offset, text) in parts.into_iter().enumerate().skip(1) {
            let id = BlockId::txt_draft(self.next_block_id);
            self.next_block_id = self.next_block_id.wrapping_add(1).max(1);
            replacement_ids.push(id.clone());
            self.blocks.insert(
                block_index + offset,
                EditableBlock {
                    id,
                    chapter_key: original.chapter_key.clone(),
                    kind: ParagraphKind::Paragraph,
                    text,
                    editable: true,
                },
            );
        }
        let group = self
            .replacement_groups
            .iter_mut()
            .find(|group| group.current_block_ids.contains(&original.id))
            .expect("every TXT draft block belongs to a replacement group");
        let group_index = group
            .current_block_ids
            .iter()
            .position(|block_id| block_id == &original.id)
            .expect("located TXT draft block remains in its group");
        group
            .current_block_ids
            .splice(group_index..=group_index, replacement_ids.iter().cloned());
        Ok(replacement_ids)
    }

    pub(crate) fn join_adjacent_text(
        &mut self,
        id: &BlockId,
        direction: JoinDirection,
    ) -> Result<(BlockId, usize), EditError> {
        let current_index = self
            .blocks
            .iter()
            .position(|block| block.id == *id)
            .ok_or(EditError::MissingBlock)?;
        let (left_index, right_index) = match direction {
            JoinDirection::Previous => (
                current_index
                    .checked_sub(1)
                    .ok_or(EditError::NoAdjacentTextBlock)?,
                current_index,
            ),
            JoinDirection::Next => (
                current_index,
                current_index
                    .checked_add(1)
                    .filter(|index| *index < self.blocks.len())
                    .ok_or(EditError::NoAdjacentTextBlock)?,
            ),
        };
        let left = self.blocks[left_index].clone();
        let right = self.blocks[right_index].clone();
        if !left.editable || !right.editable {
            return Err(EditError::ReadOnlyBlock);
        }
        if left.chapter_key != right.chapter_key
            || !matches!(left.kind, ParagraphKind::Paragraph | ParagraphKind::Blank)
            || !matches!(right.kind, ParagraphKind::Paragraph | ParagraphKind::Blank)
        {
            return Err(EditError::IncompatibleAdjacentTextBlock);
        }

        let caret_utf16 = left.text.encode_utf16().count();
        let kept_id = left.id.clone();
        self.blocks[left_index].text.push_str(&right.text);
        self.blocks[left_index].kind = ParagraphKind::Paragraph;
        self.blocks.remove(right_index);
        self.merge_group_boundary(&left.id, &right.id);
        Ok((kept_id, caret_utf16))
    }

    fn merge_group_boundary(&mut self, left_id: &BlockId, right_id: &BlockId) {
        let left_group = self
            .replacement_groups
            .iter()
            .position(|group| group.current_block_ids.contains(left_id))
            .expect("left TXT block belongs to a replacement group");
        let right_group = self
            .replacement_groups
            .iter()
            .position(|group| group.current_block_ids.contains(right_id))
            .expect("right TXT block belongs to a replacement group");
        if left_group == right_group {
            let group = &mut self.replacement_groups[left_group];
            let left_position = group
                .current_block_ids
                .iter()
                .position(|block_id| block_id == left_id)
                .expect("left block remains in its group");
            debug_assert_eq!(
                group.current_block_ids.get(left_position + 1),
                Some(right_id)
            );
            group.current_block_ids.remove(left_position + 1);
            return;
        }

        debug_assert_eq!(right_group, left_group + 1);
        let right = self.replacement_groups.remove(right_group);
        let left = &mut self.replacement_groups[left_group];
        left.source_range.end = right.source_range.end;
        left.original_blocks.extend(right.original_blocks);
        left.current_block_ids.extend(right.current_block_ids);
        let left_position = left
            .current_block_ids
            .iter()
            .position(|block_id| block_id == left_id)
            .expect("left block remains in merged group");
        debug_assert_eq!(
            left.current_block_ids.get(left_position + 1),
            Some(right_id)
        );
        left.current_block_ids.remove(left_position + 1);
    }

    pub fn materialize(&self) -> String {
        let mut changed = self
            .replacement_groups
            .iter()
            .filter(|group| !self.group_is_unchanged(group))
            .collect::<Vec<_>>();
        if changed.is_empty() {
            return self.original.to_string();
        }
        changed.sort_by_key(|group| group.source_range.start);
        let replacement_bytes = changed
            .iter()
            .map(|group| self.render_group(group).len())
            .sum::<usize>();
        let removed_bytes = changed
            .iter()
            .map(|group| group.source_range.len())
            .sum::<usize>();
        let mut output = String::with_capacity(
            self.original
                .len()
                .saturating_sub(removed_bytes)
                .saturating_add(replacement_bytes),
        );
        let mut cursor = 0usize;
        for group in changed {
            debug_assert!(group.source_range.start >= cursor);
            output.push_str(&self.original[cursor..group.source_range.start]);
            output.push_str(&self.render_group(group));
            cursor = group.source_range.end;
        }
        output.push_str(&self.original[cursor..]);
        output
    }

    fn group_is_unchanged(&self, group: &TxtReplacementGroup) -> bool {
        if group.current_block_ids.len() != group.original_blocks.len() {
            return false;
        }
        group
            .current_block_ids
            .iter()
            .zip(&group.original_blocks)
            .all(|(current_id, (original_id, original_text))| {
                current_id == original_id
                    && self
                        .blocks
                        .iter()
                        .find(|block| &block.id == current_id)
                        .is_some_and(|block| block.text == *original_text)
            })
    }

    fn render_group(&self, group: &TxtReplacementGroup) -> String {
        group
            .current_block_ids
            .iter()
            .map(|block_id| {
                self.blocks
                    .iter()
                    .find(|block| &block.id == block_id)
                    .expect("TXT replacement group references an existing block")
                    .text
                    .as_str()
            })
            .collect::<Vec<_>>()
            .join(self.inserted_line_ending)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        LineEnding, ReaderDocument, ReadloomCore, TextEncoding, TxtSettings,
        text_codec::{decode_text, encode_text},
    };

    use super::*;

    #[test]
    fn only_the_edited_source_range_is_replaced() {
        let settings = TxtSettings {
            merge_wrapped_lines: true,
            ..TxtSettings::default()
        };
        let source = "第一章\n  未编辑缩进。\n错误软换行\n继续这一段。\n\n尾声\n";
        let document = ReaderDocument::from_text_with_settings(
            "source.txt",
            source.to_owned(),
            &settings,
            crate::DEFAULT_TXT_CHAPTER_PATTERN,
        );
        let mut draft = TxtDraft::from_document(&document);
        let target = draft
            .blocks()
            .iter()
            .find(|block| block.text.contains("错误软换行"))
            .expect("merged source block")
            .id
            .clone();

        draft
            .replace_block_text(&target, "改成一行中文。".to_owned())
            .expect("replace target block");

        assert_eq!(
            draft.materialize(),
            "第一章\n  未编辑缩进。\n改成一行中文。\n\n尾声\n"
        );
    }

    #[test]
    fn draft_saves_preserve_supported_encodings_boms_and_line_endings() {
        let cases = [
            (TextEncoding::Utf8, false, LineEnding::Lf, "正文"),
            (TextEncoding::Utf8, true, LineEnding::Crlf, "正文"),
            (TextEncoding::Utf16Le, true, LineEnding::Crlf, "正文"),
            (TextEncoding::Utf16Be, true, LineEnding::Lf, "正文"),
            (TextEncoding::Gbk, false, LineEnding::Crlf, "中文"),
            (TextEncoding::Gb18030, false, LineEnding::Lf, "𠀀"),
        ];
        for (index, (encoding, has_bom, line_ending, distinct_text)) in
            cases.into_iter().enumerate()
        {
            let directory = tempfile::tempdir().expect("temporary directory");
            let core =
                ReadloomCore::open(&directory.path().join("state.sqlite3")).expect("open core");
            let path = directory.path().join(format!("encoding-{index}.txt"));
            let content = format!("第一章\n{distinct_text}\n尾声\n");
            let bytes =
                encode_text(&content, encoding, has_bom, line_ending).expect("encode TXT fixture");
            std::fs::write(&path, bytes).expect("write TXT fixture");
            let document = core.open_txt(&path).expect("open encoded TXT");
            let mut draft = TxtDraft::from_document(&document);
            let target = draft.blocks()[1].id.clone();
            draft
                .replace_block_text(&target, "已编辑中文。".to_owned())
                .expect("edit encoded TXT block");

            let saved = core
                .save_txt_draft(&document, &draft)
                .expect("save encoded TXT draft");

            let saved_bytes = std::fs::read(&path).expect("read saved TXT");
            let decoded = decode_text(&saved_bytes).expect("decode saved TXT");
            assert_eq!(saved.encoding(), encoding, "case {index}");
            if encoding != TextEncoding::Gb18030 {
                assert_eq!(decoded.encoding, encoding, "case {index}");
            }
            assert_eq!(decoded.has_bom, has_bom, "case {index}");
            assert_eq!(decoded.primary_line_ending, line_ending, "case {index}");
            assert!(decoded.content.contains("已编辑中文。"), "case {index}");
        }
    }

    #[test]
    fn structured_text_edit_split_merge_reopens_with_exact_encoded_bytes() {
        let cases = [
            (TextEncoding::Utf8, false, LineEnding::Lf),
            (TextEncoding::Utf8, true, LineEnding::Crlf),
            (TextEncoding::Utf16Le, true, LineEnding::Crlf),
            (TextEncoding::Utf16Be, true, LineEnding::Lf),
            (TextEncoding::Gbk, false, LineEnding::Crlf),
            (TextEncoding::Gb18030, false, LineEnding::Lf),
        ];
        for (index, (encoding, has_bom, line_ending)) in cases.into_iter().enumerate() {
            let directory = tempfile::tempdir().expect("temporary directory");
            let core =
                ReadloomCore::open(&directory.path().join("state.sqlite3")).expect("open core");
            let path = directory.path().join(format!("structured-{index}.txt"));
            let source = "序言正文\n甲段正文\n乙段正文\n丙段正文\n";
            let source_bytes =
                encode_text(source, encoding, has_bom, line_ending).expect("encode TXT source");
            std::fs::write(&path, source_bytes).expect("write TXT source");
            let document = core.open_txt(&path).expect("open TXT source");
            let mut draft = TxtDraft::from_document(&document);
            let first_id = draft.blocks()[0].id.clone();
            let split_ids = draft
                .split_block_text(&first_id, vec!["序言".to_owned(), "新段".to_owned()])
                .expect("split first TXT block");
            draft
                .join_adjacent_text(&split_ids[1], JoinDirection::Next)
                .expect("merge inserted block with original next block");

            core.save_txt_draft(&document, &draft)
                .expect("save structured TXT draft");

            let expected = encode_text(
                "序言\n新段甲段正文\n乙段正文\n丙段正文\n",
                encoding,
                has_bom,
                line_ending,
            )
            .expect("encode expected TXT");
            assert_eq!(std::fs::read(&path).expect("read saved TXT"), expected);
            let reopened = core.open_txt(&path).expect("reopen saved TXT");
            assert_eq!(
                reopened
                    .paragraphs()
                    .iter()
                    .map(|paragraph| paragraph.text.as_str())
                    .collect::<Vec<_>>(),
                ["序言", "新段甲段正文", "乙段正文", "丙段正文"]
            );
        }
    }

    #[test]
    fn draft_save_conflict_keeps_the_external_txt_bytes() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3")).expect("open core");
        let path = directory.path().join("conflict.txt");
        std::fs::write(&path, "第一章\n原文\n").expect("write TXT");
        let document = core.open_txt(&path).expect("open TXT");
        let mut draft = TxtDraft::from_document(&document);
        let target = draft.blocks()[1].id.clone();
        draft
            .replace_block_text(&target, "草稿".to_owned())
            .expect("edit TXT draft");
        std::fs::write(&path, "外部程序写入的版本").expect("write external version");

        let error = core
            .save_txt_draft(&document, &draft)
            .expect_err("external edit must conflict");

        assert!(error.to_string().contains("其他程序修改"));
        assert_eq!(
            std::fs::read_to_string(&path).expect("read external TXT"),
            "外部程序写入的版本"
        );
    }

    #[test]
    fn readonly_txt_save_failure_keeps_the_original_bytes() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3")).expect("open core");
        let path = directory.path().join("readonly.txt");
        let original = b"first\r\nsecond\r\n".to_vec();
        std::fs::write(&path, &original).expect("write TXT");
        let document = core.open_txt(&path).expect("open TXT");
        let mut draft = TxtDraft::from_document(&document);
        let target = draft.blocks()[1].id.clone();
        draft
            .replace_block_text(&target, "changed".to_owned())
            .expect("edit TXT draft");
        let original_permissions = std::fs::metadata(&path)
            .expect("TXT metadata")
            .permissions();
        let mut readonly_permissions = original_permissions.clone();
        readonly_permissions.set_readonly(true);
        std::fs::set_permissions(&path, readonly_permissions).expect("make TXT read-only");

        let result = core.save_txt_draft(&document, &draft);

        std::fs::set_permissions(&path, original_permissions).expect("restore TXT permissions");
        let error = result.expect_err("read-only TXT must not be replaced");
        assert!(error.to_string().contains("只读"));
        assert_eq!(std::fs::read(&path).expect("read preserved TXT"), original);
    }
}
