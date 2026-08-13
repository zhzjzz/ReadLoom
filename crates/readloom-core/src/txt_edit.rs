use std::{ops::Range, sync::Arc};

use crate::{BlockId, ChapterKey, EditError, EditableBlock, ReaderDocument};

#[derive(Debug, Clone)]
struct TxtSourceBlock {
    original_text: String,
    source_range: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct TxtDraft {
    original: Arc<str>,
    blocks: Vec<EditableBlock>,
    source_blocks: Vec<TxtSourceBlock>,
}

impl TxtDraft {
    pub fn from_document(document: &ReaderDocument) -> Self {
        let mut blocks = Vec::with_capacity(document.paragraphs().len());
        let mut source_blocks = Vec::with_capacity(document.paragraphs().len());
        for paragraph in document.paragraphs() {
            let text = paragraph.text.clone();
            blocks.push(EditableBlock {
                id: paragraph.block_id.clone(),
                chapter_key: ChapterKey::new(format!("txt:{}", paragraph.chapter_index)),
                kind: paragraph.kind,
                text: text.clone(),
                editable: true,
            });
            source_blocks.push(TxtSourceBlock {
                original_text: text,
                source_range: paragraph.source_start..paragraph.source_end,
            });
        }
        Self {
            original: Arc::from(document.content()),
            blocks,
            source_blocks,
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

    pub fn materialize(&self) -> String {
        let mut changed = self
            .blocks
            .iter()
            .zip(&self.source_blocks)
            .filter(|(block, source)| block.text != source.original_text)
            .collect::<Vec<_>>();
        if changed.is_empty() {
            return self.original.to_string();
        }
        changed.sort_by_key(|(_, source)| source.source_range.start);
        let replacement_bytes = changed
            .iter()
            .map(|(block, _)| block.text.len())
            .sum::<usize>();
        let removed_bytes = changed
            .iter()
            .map(|(_, source)| source.source_range.len())
            .sum::<usize>();
        let mut output = String::with_capacity(
            self.original
                .len()
                .saturating_sub(removed_bytes)
                .saturating_add(replacement_bytes),
        );
        let mut cursor = 0usize;
        for (block, source) in changed {
            debug_assert!(source.source_range.start >= cursor);
            output.push_str(&self.original[cursor..source.source_range.start]);
            output.push_str(&block.text);
            cursor = source.source_range.end;
        }
        output.push_str(&self.original[cursor..]);
        output
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
