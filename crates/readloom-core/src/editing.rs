use std::{fmt, path::Path, sync::Arc};

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

    pub(crate) fn epub_draft(resource_path: &str, draft_id: u64) -> Self {
        Self(format!("epub:{resource_path}:draft:{draft_id}"))
    }

    pub(crate) fn txt_draft(draft_id: u64) -> Self {
        Self(format!("txt:draft:{draft_id}"))
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

    fn split_block_text(
        &mut self,
        id: &BlockId,
        parts: Vec<String>,
    ) -> Result<Vec<BlockId>, EditError> {
        match self {
            Self::Txt(draft) => draft.split_block_text(id, parts),
            Self::Epub(draft) => draft.split_block_text(id, parts),
        }
    }

    fn join_adjacent_text(
        &mut self,
        id: &BlockId,
        direction: JoinDirection,
    ) -> Result<(BlockId, usize), EditError> {
        match self {
            Self::Txt(draft) => draft.join_adjacent_text(id, direction),
            Self::Epub(draft) => draft.join_adjacent_text(id, direction),
        }
    }

    fn insert_epub_image(
        &mut self,
        anchor_block_id: &BlockId,
        side: InsertSide,
        asset: ValidatedImageAsset,
        alt_text: String,
    ) -> Result<(usize, crate::epub_edit::EpubNodeSnapshot), EditError> {
        match self {
            Self::Epub(draft) => draft.insert_image(anchor_block_id, side, asset, alt_text),
            Self::Txt(_) => Err(EditError::EpubCommandForTxt),
        }
    }

    fn remove_epub_image(
        &mut self,
        block_id: &BlockId,
    ) -> Result<(usize, crate::epub_edit::EpubNodeSnapshot), EditError> {
        match self {
            Self::Epub(draft) => draft.remove_image(block_id),
            Self::Txt(_) => Err(EditError::EpubCommandForTxt),
        }
    }

    fn restore_epub_node(
        &mut self,
        index: usize,
        snapshot: crate::epub_edit::EpubNodeSnapshot,
    ) -> Result<(), EditError> {
        match self {
            Self::Epub(draft) => draft.restore_node(index, snapshot),
            Self::Txt(_) => Err(EditError::EpubCommandForTxt),
        }
    }

    fn set_epub_image_alt(
        &mut self,
        block_id: &BlockId,
        alt_text: String,
    ) -> Result<String, EditError> {
        match self {
            Self::Epub(draft) => draft.set_image_alt(block_id, alt_text),
            Self::Txt(_) => Err(EditError::EpubCommandForTxt),
        }
    }

    fn rebase(&mut self, path: &Path, fingerprint: &str) {
        if let Self::Epub(draft) = self {
            draft.rebase(path.to_owned(), fingerprint.to_owned());
        }
    }
}

#[derive(Debug, Clone)]
enum EditOperation {
    ReplaceBlockText {
        block_id: BlockId,
        before: String,
        after: String,
        before_caret_utf16: usize,
        after_caret_utf16: usize,
        before_state_id: u64,
        after_state_id: u64,
    },
    ReplaceBlockRange {
        before: Box<DocumentDraft>,
        after: Box<DocumentDraft>,
        before_anchor: ViewAnchor,
        after_anchor: ViewAnchor,
        before_state_id: u64,
        after_state_id: u64,
    },
    InsertEpubImage {
        index: usize,
        snapshot: crate::epub_edit::EpubNodeSnapshot,
        before_anchor: ViewAnchor,
        after_anchor: ViewAnchor,
        before_state_id: u64,
        after_state_id: u64,
    },
    RemoveEpubImage {
        index: usize,
        snapshot: crate::epub_edit::EpubNodeSnapshot,
        before_anchor: ViewAnchor,
        after_anchor: ViewAnchor,
        before_state_id: u64,
        after_state_id: u64,
    },
    SetEpubImageAlt {
        block_id: BlockId,
        before: String,
        after: String,
        before_anchor: ViewAnchor,
        after_anchor: ViewAnchor,
        before_state_id: u64,
        after_state_id: u64,
    },
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum EditError {
    #[error("编辑块已经不存在。")]
    MissingBlock,
    #[error("该内容块是图片或不受支持的结构，不能直接编辑。")]
    ReadOnlyBlock,
    #[error("TXT 文档不支持 EPUB 图片命令。")]
    EpubCommandForTxt,
    #[error("只能对图片块执行此操作。")]
    NotImageBlock,
    #[error("该位置无法无损映射回 EPUB 源码，不能插入图片。")]
    UnsafeImagePosition,
    #[error("该段落结构无法安全拆分或合并，草稿保持不变。")]
    UnsafeStructureEdit,
    #[error("当前段落在该方向没有可合并的相邻文本块。")]
    NoAdjacentTextBlock,
    #[error("相邻内容属于图片、标题或章节边界，不能合并。")]
    IncompatibleAdjacentTextBlock,
}

#[derive(Debug, Clone)]
pub struct SaveTicket {
    revision: u64,
    state_id: u64,
    draft: DocumentDraft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertSide {
    Before,
    After,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinDirection {
    Previous,
    Next,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageMediaType {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ImageMediaType {
    pub fn media_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedImageAsset {
    pub bytes: Arc<[u8]>,
    pub media_type: ImageMediaType,
    pub width: u32,
    pub height: u32,
    pub digest: String,
}

impl ValidatedImageAsset {
    pub(crate) fn safe_digest_prefix(&self) -> String {
        let supplied = self
            .digest
            .chars()
            .filter(|character| character.is_ascii_hexdigit())
            .take(12)
            .collect::<String>()
            .to_ascii_lowercase();
        if supplied.len() == 12 {
            supplied
        } else {
            blake3::hash(&self.bytes).to_hex()[..12].to_owned()
        }
    }
}

#[derive(Debug, Clone)]
pub enum EditCommand {
    ReplaceText {
        block_id: BlockId,
        text: String,
        caret_utf16: usize,
    },
    JoinAdjacentText {
        block_id: BlockId,
        direction: JoinDirection,
    },
    InsertEpubImage {
        anchor_block_id: BlockId,
        side: InsertSide,
        asset: ValidatedImageAsset,
        alt_text: String,
    },
    RemoveEpubImage {
        block_id: BlockId,
    },
    SetEpubImageAlt {
        block_id: BlockId,
        alt_text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditChange {
    revision: u64,
    affected_block_id: BlockId,
    structure_changed: bool,
    changed: bool,
}

impl EditChange {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn affected_block_id(&self) -> &BlockId {
        &self.affected_block_id
    }

    pub fn structure_changed(&self) -> bool {
        self.structure_changed
    }

    pub fn changed(&self) -> bool {
        self.changed
    }
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
    next_state_id: u64,
    current_state_id: u64,
    saved_state_id: u64,
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
            next_state_id: 1,
            current_state_id: 0,
            saved_state_id: 0,
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
        self.current_state_id != self.saved_state_id
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn apply(&mut self, command: EditCommand) -> Result<EditChange, EditError> {
        match command {
            EditCommand::ReplaceText {
                block_id,
                text,
                caret_utf16,
            } => self.apply_replace_text(block_id, text, caret_utf16),
            EditCommand::JoinAdjacentText {
                block_id,
                direction,
            } => self.apply_join_adjacent_text(block_id, direction),
            EditCommand::InsertEpubImage {
                anchor_block_id,
                side,
                asset,
                alt_text,
            } => {
                let before_anchor = self.anchor.clone();
                let before_state_id = self.current_state_id;
                let after_state_id = self.allocate_state_id();
                let (index, snapshot) =
                    self.draft
                        .insert_epub_image(&anchor_block_id, side, asset, alt_text)?;
                let affected_block_id = snapshot.block.id.clone();
                let after_anchor = ViewAnchor {
                    chapter_key: snapshot.block.chapter_key.clone(),
                    block_id: affected_block_id.clone(),
                    character_offset_utf16: 0,
                    pixel_offset_from_viewport_top: before_anchor.pixel_offset_from_viewport_top,
                };
                self.anchor = after_anchor.clone();
                self.finish_apply(
                    affected_block_id.clone(),
                    true,
                    after_state_id,
                    EditOperation::InsertEpubImage {
                        index,
                        snapshot,
                        before_anchor,
                        after_anchor,
                        before_state_id,
                        after_state_id,
                    },
                );
                Ok(EditChange {
                    revision: self.revision,
                    affected_block_id,
                    structure_changed: true,
                    changed: true,
                })
            }
            EditCommand::RemoveEpubImage { block_id } => {
                let before_anchor = self.anchor.clone();
                let before_state_id = self.current_state_id;
                let after_state_id = self.allocate_state_id();
                let (index, snapshot) = self.draft.remove_epub_image(&block_id)?;
                let fallback = self
                    .draft
                    .blocks()
                    .get(index)
                    .or_else(|| {
                        index
                            .checked_sub(1)
                            .and_then(|index| self.draft.blocks().get(index))
                    })
                    .ok_or(EditError::MissingBlock)?;
                let after_anchor = ViewAnchor {
                    chapter_key: fallback.chapter_key.clone(),
                    block_id: fallback.id.clone(),
                    character_offset_utf16: 0,
                    pixel_offset_from_viewport_top: before_anchor.pixel_offset_from_viewport_top,
                };
                self.anchor = after_anchor.clone();
                self.finish_apply(
                    block_id.clone(),
                    true,
                    after_state_id,
                    EditOperation::RemoveEpubImage {
                        index,
                        snapshot,
                        before_anchor,
                        after_anchor,
                        before_state_id,
                        after_state_id,
                    },
                );
                Ok(EditChange {
                    revision: self.revision,
                    affected_block_id: block_id,
                    structure_changed: true,
                    changed: true,
                })
            }
            EditCommand::SetEpubImageAlt { block_id, alt_text } => {
                let before = self
                    .draft
                    .block(&block_id)
                    .ok_or(EditError::MissingBlock)?
                    .text
                    .clone();
                if before == alt_text {
                    return Ok(EditChange {
                        revision: self.revision,
                        affected_block_id: block_id,
                        structure_changed: false,
                        changed: false,
                    });
                }
                let before_anchor = self.anchor.clone();
                let before_state_id = self.current_state_id;
                let after_state_id = self.allocate_state_id();
                self.draft.set_epub_image_alt(&block_id, alt_text.clone())?;
                let after_anchor = ViewAnchor {
                    chapter_key: self
                        .draft
                        .block(&block_id)
                        .expect("image block remains after alt edit")
                        .chapter_key
                        .clone(),
                    block_id: block_id.clone(),
                    character_offset_utf16: alt_text.encode_utf16().count(),
                    pixel_offset_from_viewport_top: before_anchor.pixel_offset_from_viewport_top,
                };
                self.anchor = after_anchor.clone();
                self.finish_apply(
                    block_id.clone(),
                    false,
                    after_state_id,
                    EditOperation::SetEpubImageAlt {
                        block_id: block_id.clone(),
                        before,
                        after: alt_text,
                        before_anchor,
                        after_anchor,
                        before_state_id,
                        after_state_id,
                    },
                );
                Ok(EditChange {
                    revision: self.revision,
                    affected_block_id: block_id,
                    structure_changed: false,
                    changed: true,
                })
            }
        }
    }

    pub fn replace_block_text(
        &mut self,
        block_id: &BlockId,
        text: String,
        caret_utf16: usize,
    ) -> Result<bool, EditError> {
        self.apply(EditCommand::ReplaceText {
            block_id: block_id.clone(),
            text,
            caret_utf16,
        })
        .map(|change| change.changed())
    }

    fn apply_replace_text(
        &mut self,
        block_id: BlockId,
        text: String,
        caret_utf16: usize,
    ) -> Result<EditChange, EditError> {
        let (normalized_text, normalized_caret_utf16) = normalize_text_input(&text, caret_utf16);
        let parts = normalized_text
            .split('\n')
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if parts.len() > 1 {
            let before = self.draft.clone();
            let before_anchor = self.anchor.clone();
            let before_state_id = self.current_state_id;
            let after_state_id = self.allocate_state_id();
            let (focus_part, focus_caret_utf16) =
                split_caret_position(&parts, normalized_caret_utf16);
            let block_ids = self.draft.split_block_text(&block_id, parts)?;
            let focus_block_id = block_ids[focus_part].clone();
            let focus_block = self
                .draft
                .block(&focus_block_id)
                .expect("split focus block exists");
            let after_anchor = ViewAnchor {
                chapter_key: focus_block.chapter_key.clone(),
                block_id: focus_block_id.clone(),
                character_offset_utf16: focus_caret_utf16,
                pixel_offset_from_viewport_top: before_anchor.pixel_offset_from_viewport_top,
            };
            self.anchor = after_anchor.clone();
            let after = self.draft.clone();
            self.finish_apply(
                focus_block_id.clone(),
                true,
                after_state_id,
                EditOperation::ReplaceBlockRange {
                    before: Box::new(before),
                    after: Box::new(after),
                    before_anchor,
                    after_anchor,
                    before_state_id,
                    after_state_id,
                },
            );
            return Ok(EditChange {
                revision: self.revision,
                affected_block_id: focus_block_id,
                structure_changed: true,
                changed: true,
            });
        }
        let before = self
            .draft
            .block(&block_id)
            .ok_or(EditError::MissingBlock)?
            .text
            .clone();
        if before == normalized_text {
            self.anchor.character_offset_utf16 = normalized_caret_utf16;
            return Ok(EditChange {
                revision: self.revision,
                affected_block_id: block_id,
                structure_changed: false,
                changed: false,
            });
        }
        let before_caret_utf16 = self.anchor.character_offset_utf16;
        let before_state_id = self.current_state_id;
        let after_state_id = self.allocate_state_id();
        self.draft
            .replace_block_text(&block_id, normalized_text.clone())?;
        self.anchor.block_id = block_id.clone();
        self.anchor.character_offset_utf16 = normalized_caret_utf16;
        self.finish_apply(
            block_id.clone(),
            false,
            after_state_id,
            EditOperation::ReplaceBlockText {
                block_id: block_id.clone(),
                before,
                after: normalized_text,
                before_caret_utf16,
                after_caret_utf16: normalized_caret_utf16,
                before_state_id,
                after_state_id,
            },
        );
        Ok(EditChange {
            revision: self.revision,
            affected_block_id: block_id,
            structure_changed: false,
            changed: true,
        })
    }

    fn apply_join_adjacent_text(
        &mut self,
        block_id: BlockId,
        direction: JoinDirection,
    ) -> Result<EditChange, EditError> {
        let before = self.draft.clone();
        let before_anchor = self.anchor.clone();
        let before_state_id = self.current_state_id;
        let after_state_id = self.allocate_state_id();
        let (focus_block_id, focus_caret_utf16) =
            self.draft.join_adjacent_text(&block_id, direction)?;
        let focus_block = self
            .draft
            .block(&focus_block_id)
            .expect("joined focus block exists");
        let after_anchor = ViewAnchor {
            chapter_key: focus_block.chapter_key.clone(),
            block_id: focus_block_id.clone(),
            character_offset_utf16: focus_caret_utf16,
            pixel_offset_from_viewport_top: before_anchor.pixel_offset_from_viewport_top,
        };
        self.anchor = after_anchor.clone();
        let after = self.draft.clone();
        self.finish_apply(
            focus_block_id.clone(),
            true,
            after_state_id,
            EditOperation::ReplaceBlockRange {
                before: Box::new(before),
                after: Box::new(after),
                before_anchor,
                after_anchor,
                before_state_id,
                after_state_id,
            },
        );
        Ok(EditChange {
            revision: self.revision,
            affected_block_id: focus_block_id,
            structure_changed: true,
            changed: true,
        })
    }

    fn allocate_state_id(&mut self) -> u64 {
        let state_id = self.next_state_id;
        self.next_state_id = self.next_state_id.wrapping_add(1).max(1);
        state_id
    }

    fn finish_apply(
        &mut self,
        _affected_block_id: BlockId,
        _structure_changed: bool,
        after_state_id: u64,
        operation: EditOperation,
    ) {
        self.revision = self.revision.wrapping_add(1).max(1);
        self.current_state_id = after_state_id;
        self.undo.push(operation);
        self.redo.clear();
        self.save_state = SaveState::Dirty;
    }

    pub fn begin_save(&mut self) -> SaveTicket {
        let revision = self.revision;
        self.save_state = SaveState::Saving { revision };
        SaveTicket {
            revision,
            state_id: self.current_state_id,
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
                before_state_id,
                ..
            } => {
                self.draft.replace_block_text(block_id, before.clone())?;
                self.anchor.block_id = block_id.clone();
                self.anchor.character_offset_utf16 = *before_caret_utf16;
                self.current_state_id = *before_state_id;
            }
            EditOperation::ReplaceBlockRange {
                before,
                before_anchor,
                before_state_id,
                ..
            } => {
                self.draft = before.as_ref().clone();
                self.anchor = before_anchor.clone();
                self.current_state_id = *before_state_id;
            }
            EditOperation::InsertEpubImage {
                snapshot,
                before_anchor,
                before_state_id,
                ..
            } => {
                self.draft.remove_epub_image(&snapshot.block.id)?;
                self.anchor = before_anchor.clone();
                self.current_state_id = *before_state_id;
            }
            EditOperation::RemoveEpubImage {
                index,
                snapshot,
                before_anchor,
                before_state_id,
                ..
            } => {
                self.draft.restore_epub_node(*index, snapshot.clone())?;
                self.anchor = before_anchor.clone();
                self.current_state_id = *before_state_id;
            }
            EditOperation::SetEpubImageAlt {
                block_id,
                before,
                before_anchor,
                before_state_id,
                ..
            } => {
                self.draft.set_epub_image_alt(block_id, before.clone())?;
                self.anchor = before_anchor.clone();
                self.current_state_id = *before_state_id;
            }
        }
        self.redo.push(operation);
        self.revision = self.revision.wrapping_add(1).max(1);
        self.save_state = if self.is_dirty() {
            SaveState::Dirty
        } else {
            SaveState::Clean
        };
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
                after_state_id,
                ..
            } => {
                self.draft.replace_block_text(block_id, after.clone())?;
                self.anchor.block_id = block_id.clone();
                self.anchor.character_offset_utf16 = *after_caret_utf16;
                self.current_state_id = *after_state_id;
            }
            EditOperation::ReplaceBlockRange {
                after,
                after_anchor,
                after_state_id,
                ..
            } => {
                self.draft = after.as_ref().clone();
                self.anchor = after_anchor.clone();
                self.current_state_id = *after_state_id;
            }
            EditOperation::InsertEpubImage {
                index,
                snapshot,
                after_anchor,
                after_state_id,
                ..
            } => {
                self.draft.restore_epub_node(*index, snapshot.clone())?;
                self.anchor = after_anchor.clone();
                self.current_state_id = *after_state_id;
            }
            EditOperation::RemoveEpubImage {
                snapshot,
                after_anchor,
                after_state_id,
                ..
            } => {
                self.draft.remove_epub_image(&snapshot.block.id)?;
                self.anchor = after_anchor.clone();
                self.current_state_id = *after_state_id;
            }
            EditOperation::SetEpubImageAlt {
                block_id,
                after,
                after_anchor,
                after_state_id,
                ..
            } => {
                self.draft.set_epub_image_alt(block_id, after.clone())?;
                self.anchor = after_anchor.clone();
                self.current_state_id = *after_state_id;
            }
        }
        self.undo.push(operation);
        self.revision = self.revision.wrapping_add(1).max(1);
        self.save_state = if self.is_dirty() {
            SaveState::Dirty
        } else {
            SaveState::Clean
        };
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
            self.saved_state_id = ticket.state_id;
            self.saved_draft = ticket.draft.clone();
            self.saved_draft.rebase(new_path, &self.base_fingerprint);
        }
        let still_dirty = self.is_dirty();
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
        self.current_state_id = self.saved_state_id;
        self.save_state = SaveState::Clean;
        self.undo.clear();
        self.redo.clear();
    }
}

fn normalize_text_input(text: &str, caret_utf16: usize) -> (String, usize) {
    let caret_byte = byte_offset_for_utf16(text, caret_utf16);
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let normalized_caret = text[..caret_byte]
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .encode_utf16()
        .count();
    (normalized, normalized_caret)
}

fn byte_offset_for_utf16(text: &str, caret_utf16: usize) -> usize {
    let mut consumed = 0usize;
    for (byte_offset, character) in text.char_indices() {
        let next = consumed + character.len_utf16();
        if next > caret_utf16 {
            return byte_offset;
        }
        consumed = next;
    }
    text.len()
}

fn split_caret_position(parts: &[String], caret_utf16: usize) -> (usize, usize) {
    let mut consumed = 0usize;
    for (index, part) in parts.iter().enumerate() {
        let part_len = part.encode_utf16().count();
        if caret_utf16 <= consumed + part_len {
            return (index, caret_utf16.saturating_sub(consumed));
        }
        consumed = consumed.saturating_add(part_len);
        if index + 1 < parts.len() {
            consumed = consumed.saturating_add(1);
        }
    }
    let last = parts.len().saturating_sub(1);
    (last, parts[last].encode_utf16().count())
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

    #[test]
    fn undoing_to_the_saved_state_clears_dirty() {
        let mut session = session();
        let block = session.draft().blocks()[0].id.clone();
        session
            .replace_block_text(&block, "临时修改".to_owned(), 4)
            .expect("edit block");
        assert!(session.is_dirty());

        assert!(session.undo().expect("undo to saved state"));

        assert!(!session.is_dirty());
        assert_eq!(session.save_state(), &SaveState::Clean);
    }

    #[test]
    fn structured_text_edit_splits_chinese_emoji_and_restores_atomically() {
        let mut session = session();
        let original_block_id = session.draft().blocks()[0].id.clone();
        let text = "中😀\n文";
        let caret_utf16 = text.encode_utf16().count();

        let change = session
            .apply(EditCommand::ReplaceText {
                block_id: original_block_id.clone(),
                text: text.to_owned(),
                caret_utf16,
            })
            .expect("split text block");

        assert!(change.structure_changed());
        assert_eq!(
            session
                .draft()
                .blocks()
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            ["中😀", "文", "第二段"]
        );
        let split_block_id = session.draft().blocks()[1].id.clone();
        assert_eq!(session.draft().blocks()[0].id, original_block_id);
        assert_eq!(session.anchor().block_id, split_block_id);
        assert_eq!(session.anchor().character_offset_utf16, 1);
        assert!(
            session
                .draft()
                .blocks()
                .iter()
                .all(|block| !block.text.contains(['\r', '\n']))
        );

        assert!(session.undo().expect("undo split"));
        assert_eq!(
            session
                .draft()
                .blocks()
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            ["第一段", "第二段"]
        );

        assert!(session.redo().expect("redo split"));
        assert_eq!(
            session
                .draft()
                .blocks()
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            ["中😀", "文", "第二段"]
        );
        assert_eq!(session.anchor().block_id, split_block_id);
        assert_eq!(session.anchor().character_offset_utf16, 1);
    }

    #[test]
    fn structured_text_edit_backspace_at_start_joins_the_previous_block() {
        let mut session = session();
        let previous_block_id = session.draft().blocks()[0].id.clone();
        let current_block_id = session.draft().blocks()[1].id.clone();

        let change = session
            .apply(EditCommand::JoinAdjacentText {
                block_id: current_block_id,
                direction: JoinDirection::Previous,
            })
            .expect("join with previous block");

        assert!(change.structure_changed());
        assert_eq!(
            session
                .draft()
                .blocks()
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            ["第一段第二段"]
        );
        assert_eq!(session.anchor().block_id, previous_block_id);
        assert_eq!(session.anchor().character_offset_utf16, 3);

        assert!(session.undo().expect("undo join"));
        assert_eq!(
            session
                .draft()
                .blocks()
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            ["第一段", "第二段"]
        );
    }

    #[test]
    fn structured_text_edit_normalizes_multiline_paste_and_keeps_empty_paragraphs() {
        let mut session = session();
        let block_id = session.draft().blocks()[0].id.clone();
        let pasted = "甲\r\n乙\n\n丙";

        session
            .apply(EditCommand::ReplaceText {
                block_id,
                text: pasted.to_owned(),
                caret_utf16: pasted.encode_utf16().count(),
            })
            .expect("paste structured paragraphs");

        assert_eq!(
            session
                .draft()
                .blocks()
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            ["甲", "乙", "", "丙", "第二段"]
        );
        assert!(
            session
                .draft()
                .blocks()
                .iter()
                .all(|block| !block.text.contains(['\r', '\n']))
        );
        assert_eq!(session.anchor().character_offset_utf16, 1);
    }

    #[test]
    fn structured_text_edit_delete_at_end_and_empty_boundary_join_once() {
        let mut session = session();
        let first_block_id = session.draft().blocks()[0].id.clone();
        session
            .apply(EditCommand::ReplaceText {
                block_id: first_block_id.clone(),
                text: "第一段\n".to_owned(),
                caret_utf16: 4,
            })
            .expect("create empty paragraph");
        let empty_block_id = session.draft().blocks()[1].id.clone();

        session
            .apply(EditCommand::JoinAdjacentText {
                block_id: first_block_id.clone(),
                direction: JoinDirection::Next,
            })
            .expect("delete only the empty boundary");
        assert_eq!(
            session
                .draft()
                .blocks()
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            ["第一段", "第二段"]
        );
        assert!(session.draft().block(&empty_block_id).is_none());

        session
            .apply(EditCommand::JoinAdjacentText {
                block_id: first_block_id.clone(),
                direction: JoinDirection::Next,
            })
            .expect("delete the next paragraph boundary");
        assert_eq!(session.draft().blocks()[0].text, "第一段第二段");
        assert_eq!(session.anchor().block_id, first_block_id);
        assert_eq!(session.anchor().character_offset_utf16, 3);
    }

    #[test]
    fn structured_text_edit_rejects_heading_and_chapter_boundaries_without_mutation() {
        let document = ReaderDocument::from_text(
            "chapters.txt",
            "第一章\n第一章正文\n第二章\n第二章正文\n".to_owned(),
        );
        let draft = TxtDraft::from_document(&document);
        let body_id = draft.blocks()[1].id.clone();
        let before = draft
            .blocks()
            .iter()
            .map(|block| (block.id.clone(), block.text.clone()))
            .collect::<Vec<_>>();
        let mut session = EditSession::new(
            "base".to_owned(),
            DocumentDraft::Txt(draft),
            ViewAnchor {
                chapter_key: ChapterKey::new("txt:0"),
                block_id: body_id.clone(),
                character_offset_utf16: 5,
                pixel_offset_from_viewport_top: 0.0,
            },
        );

        let error = session
            .apply(EditCommand::JoinAdjacentText {
                block_id: body_id,
                direction: JoinDirection::Next,
            })
            .expect_err("chapter heading must reject join");

        assert_eq!(error, EditError::IncompatibleAdjacentTextBlock);
        assert_eq!(
            session
                .draft()
                .blocks()
                .iter()
                .map(|block| (block.id.clone(), block.text.clone()))
                .collect::<Vec<_>>(),
            before
        );
        assert!(!session.is_dirty());
    }
}
