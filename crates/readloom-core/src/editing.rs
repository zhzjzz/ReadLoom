use std::fmt;
use std::path::Path;

use crate::{EpubDraft, ParagraphKind, TxtDraft};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(String);

impl BlockId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn epub(resource_path: &str, source_offset: usize) -> Self {
        Self(format!("epub:{resource_path}:{source_offset}"))
    }
}

impl fmt::Display for BlockId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChapterKey(String);

impl ChapterKey {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViewAnchor {
    pub chapter_key: ChapterKey,
    pub block_id: BlockId,
    pub character_offset_utf16: usize,
    pub pixel_offset_from_viewport_top: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveState {
    Clean,
    Dirty,
    Saving { revision: u64 },
    Conflict { message: String },
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditableBlock {
    pub id: BlockId,
    pub chapter_key: ChapterKey,
    pub kind: ParagraphKind,
    pub text: String,
    pub editable: bool,
}

#[derive(Debug, Clone)]
pub enum DocumentDraft {
    Txt(TxtDraft),
    Epub(EpubDraft),
}

impl DocumentDraft {
    pub fn blocks(&self) -> &[EditableBlock] {
        match self {
            Self::Txt(draft) => draft.blocks(),
            Self::Epub(draft) => draft.blocks(),
        }
    }

    pub fn block(&self, id: &BlockId) -> Option<&EditableBlock> {
        self.blocks().iter().find(|block| block.id == *id)
    }

    fn replace_block_text(&mut self, id: &BlockId, text: String) -> Result<String, EditError> {
        match self {
            Self::Txt(draft) => draft.replace_block_text(id, text),
            Self::Epub(draft) => draft.replace_block_text(id, text),
        }
    }

    fn rebase(&mut self, path: &Path, fingerprint: &str) {
        if let Self::Epub(draft) = self {
            draft.rebase(path.to_owned(), fingerprint.to_owned());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOperation {
    ReplaceBlockText {
        block_id: BlockId,
        before: String,
        after: String,
        before_caret_utf16: usize,
        after_caret_utf16: usize,
    },
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum EditError {
    #[error("编辑块已经不存在。")]
    MissingBlock,
    #[error("该内容块是图片或不受支持的结构，不能直接编辑。")]
    ReadOnlyBlock,
}

#[derive(Debug, Clone)]
pub struct SaveTicket {
    revision: u64,
    draft: DocumentDraft,
}

impl SaveTicket {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn draft(&self) -> &DocumentDraft {
        &self.draft
    }

    pub fn into_draft(self) -> DocumentDraft {
        self.draft
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOutcome {
    pub saved_revision: u64,
    pub current_revision: u64,
    pub still_dirty: bool,
}

#[derive(Debug, Clone)]
pub struct EditSession {
    base_fingerprint: String,
    revision: u64,
    saved_revision: u64,
    draft: DocumentDraft,
    saved_draft: DocumentDraft,
    anchor: ViewAnchor,
    save_state: SaveState,
    undo: Vec<EditOperation>,
    redo: Vec<EditOperation>,
}

impl EditSession {
    pub fn new(base_fingerprint: String, draft: DocumentDraft, anchor: ViewAnchor) -> Self {
        Self {
            base_fingerprint,
            revision: 0,
            saved_revision: 0,
            saved_draft: draft.clone(),
            draft,
            anchor,
            save_state: SaveState::Clean,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn base_fingerprint(&self) -> &str {
        &self.base_fingerprint
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn saved_revision(&self) -> u64 {
        self.saved_revision
    }

    pub fn draft(&self) -> &DocumentDraft {
        &self.draft
    }

    pub fn anchor(&self) -> &ViewAnchor {
        &self.anchor
    }

    pub fn set_anchor(&mut self, anchor: ViewAnchor) {
        self.anchor = anchor;
    }

    pub fn save_state(&self) -> &SaveState {
        &self.save_state
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn replace_block_text(
        &mut self,
        block_id: &BlockId,
        text: String,
        caret_utf16: usize,
    ) -> Result<bool, EditError> {
        let before = self
            .draft
            .block(block_id)
            .ok_or(EditError::MissingBlock)?
            .text
            .clone();
        if before == text {
            self.anchor.character_offset_utf16 = caret_utf16;
            return Ok(false);
        }
        let before_caret_utf16 = self.anchor.character_offset_utf16;
        self.draft.replace_block_text(block_id, text.clone())?;
        self.revision = self.revision.wrapping_add(1).max(1);
        self.anchor.block_id = block_id.clone();
        self.anchor.character_offset_utf16 = caret_utf16;
        self.undo.push(EditOperation::ReplaceBlockText {
            block_id: block_id.clone(),
            before,
            after: text,
            before_caret_utf16,
            after_caret_utf16: caret_utf16,
        });
        self.redo.clear();
        self.save_state = SaveState::Dirty;
        Ok(true)
    }

    pub fn begin_save(&mut self) -> SaveTicket {
        let revision = self.revision;
        self.save_state = SaveState::Saving { revision };
        SaveTicket {
            revision,
            draft: self.draft.clone(),
        }
    }

    pub fn undo(&mut self) -> Result<bool, EditError> {
        let Some(operation) = self.undo.pop() else {
            return Ok(false);
        };
        match &operation {
            EditOperation::ReplaceBlockText {
                block_id,
                before,
                before_caret_utf16,
                ..
            } => {
                self.draft.replace_block_text(block_id, before.clone())?;
                self.anchor.block_id = block_id.clone();
                self.anchor.character_offset_utf16 = *before_caret_utf16;
            }
        }
        self.redo.push(operation);
        self.revision = self.revision.wrapping_add(1).max(1);
        self.save_state = SaveState::Dirty;
        Ok(true)
    }

    pub fn redo(&mut self) -> Result<bool, EditError> {
        let Some(operation) = self.redo.pop() else {
            return Ok(false);
        };
        match &operation {
            EditOperation::ReplaceBlockText {
                block_id,
                after,
                after_caret_utf16,
                ..
            } => {
                self.draft.replace_block_text(block_id, after.clone())?;
                self.anchor.block_id = block_id.clone();
                self.anchor.character_offset_utf16 = *after_caret_utf16;
            }
        }
        self.undo.push(operation);
        self.revision = self.revision.wrapping_add(1).max(1);
        self.save_state = SaveState::Dirty;
        Ok(true)
    }

    pub fn finish_save(
        &mut self,
        ticket: &SaveTicket,
        new_fingerprint: String,
        new_path: &Path,
    ) -> SaveOutcome {
        self.base_fingerprint = new_fingerprint;
        self.draft.rebase(new_path, &self.base_fingerprint);
        if ticket.revision >= self.saved_revision {
            self.saved_revision = ticket.revision;
            self.saved_draft = ticket.draft.clone();
            self.saved_draft.rebase(new_path, &self.base_fingerprint);
        }
        let still_dirty = self.revision != self.saved_revision;
        self.save_state = if still_dirty {
            SaveState::Dirty
        } else {
            SaveState::Clean
        };
        SaveOutcome {
            saved_revision: self.saved_revision,
            current_revision: self.revision,
            still_dirty,
        }
    }

    pub fn mark_conflict(&mut self, message: impl Into<String>) {
        self.save_state = SaveState::Conflict {
            message: message.into(),
        };
    }

    pub fn mark_error(&mut self, message: impl Into<String>) {
        self.save_state = SaveState::Error {
            message: message.into(),
        };
    }

    pub fn cancel_changes(&mut self) {
        self.draft = self.saved_draft.clone();
        self.revision = self.saved_revision;
        self.save_state = SaveState::Clean;
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use crate::{ReaderDocument, TxtDraft};

    use super::*;

    fn session() -> EditSession {
        let document = ReaderDocument::from_text("revision.txt", "第一段\n第二段\n".to_owned());
        let draft = TxtDraft::from_document(&document);
        let block_id = draft.blocks()[0].id.clone();
        EditSession::new(
            "base".to_owned(),
            DocumentDraft::Txt(draft),
            ViewAnchor {
                chapter_key: ChapterKey::new("txt"),
                block_id,
                character_offset_utf16: 0,
                pixel_offset_from_viewport_top: -13.5,
            },
        )
    }

    #[test]
    fn completing_save_n_does_not_clean_revision_n_plus_one() {
        let mut session = session();
        let block = session.draft().blocks()[0].id.clone();
        session
            .replace_block_text(&block, "第一次输入".to_owned(), 5)
            .expect("edit revision one");
        let ticket = session.begin_save();
        session
            .replace_block_text(&block, "保存期间继续输入中文".to_owned(), 10)
            .expect("edit revision two");

        let outcome = session.finish_save(&ticket, "saved-n".to_owned(), Path::new("revision.txt"));

        assert!(outcome.still_dirty);
        assert_eq!(outcome.saved_revision, 1);
        assert_eq!(outcome.current_revision, 2);
        assert_eq!(
            session
                .draft()
                .block(&block)
                .map(|block| block.text.as_str()),
            Some("保存期间继续输入中文")
        );
        assert_eq!(session.anchor().pixel_offset_from_viewport_top, -13.5);
    }

    #[test]
    fn undo_and_redo_survive_outside_the_native_text_input() {
        let mut session = session();
        let block = session.draft().blocks()[0].id.clone();
        session
            .replace_block_text(&block, "中文修改".to_owned(), 4)
            .expect("edit block");

        assert!(session.undo().expect("undo edit"));
        assert_eq!(session.draft().block(&block).unwrap().text, "第一段");
        assert!(session.can_redo());

        assert!(session.redo().expect("redo edit"));
        assert_eq!(session.draft().block(&block).unwrap().text, "中文修改");
        assert!(session.can_undo());
    }
}
