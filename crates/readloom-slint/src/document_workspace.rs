use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
};

use readloom_core::{
    BlockId, DocumentDraft, EditError, EditSession, EpubDraft, SaveOutcome, SaveTicket, TxtDraft,
    ViewAnchor,
};

use crate::OpenDocument;

#[derive(Clone)]
pub(crate) struct CloseDocumentResult {
    pub(crate) active_changed: bool,
    pub(crate) active: Option<OpenDocument>,
    pub(crate) blocked_by_dirty_draft: bool,
}

pub(crate) struct DocumentSession {
    document: RefCell<OpenDocument>,
    presentation_document: RefCell<OpenDocument>,
    edit: RefCell<Option<Rc<RefCell<EditSession>>>>,
    editing: Cell<bool>,
    anchor: RefCell<Option<ViewAnchor>>,
}

impl DocumentSession {
    fn new(document: OpenDocument) -> Self {
        Self {
            presentation_document: RefCell::new(document.clone()),
            document: RefCell::new(document),
            edit: RefCell::new(None),
            editing: Cell::new(false),
            anchor: RefCell::new(None),
        }
    }

    pub(crate) fn document(&self) -> OpenDocument {
        self.document.borrow().clone()
    }

    pub(crate) fn path(&self) -> PathBuf {
        self.document()
            .path()
            .expect("workspace documents have canonical paths")
            .to_owned()
    }

    pub(crate) fn presentation_document(&self) -> OpenDocument {
        self.presentation_document.borrow().clone()
    }

    pub(crate) fn editing(&self) -> bool {
        self.editing.get()
    }

    pub(crate) fn edit_session(&self) -> Option<Rc<RefCell<EditSession>>> {
        self.edit.borrow().clone()
    }

    pub(crate) fn anchor(&self) -> Option<ViewAnchor> {
        self.edit_session()
            .map(|edit| edit.borrow().anchor().clone())
            .or_else(|| self.anchor.borrow().clone())
    }

    pub(crate) fn set_anchor(&self, anchor: ViewAnchor) {
        if let Some(edit) = self.edit_session() {
            edit.borrow_mut().set_anchor(anchor.clone());
        }
        *self.anchor.borrow_mut() = Some(anchor);
    }

    pub(crate) fn begin_edit(&self, anchor: ViewAnchor) -> Result<(), String> {
        if let Some(edit) = self.edit_session() {
            edit.borrow_mut().set_anchor(anchor.clone());
            *self.anchor.borrow_mut() = Some(anchor);
            self.editing.set(true);
            return Ok(());
        }
        let document = self.document();
        let (base_fingerprint, draft) = match &document {
            OpenDocument::Txt(document) => (
                document
                    .fingerprint()
                    .ok_or_else(|| "TXT 缺少打开时的文件指纹，无法开始安全编辑。".to_owned())?
                    .to_owned(),
                DocumentDraft::Txt(TxtDraft::from_document(document)),
            ),
            OpenDocument::Epub(document) => (
                document.fingerprint().to_owned(),
                DocumentDraft::Epub(
                    EpubDraft::from_document(document).map_err(|error| error.to_string())?,
                ),
            ),
        };
        let edit = EditSession::new(base_fingerprint, draft, anchor.clone());
        *self.edit.borrow_mut() = Some(Rc::new(RefCell::new(edit)));
        *self.anchor.borrow_mut() = Some(anchor);
        self.editing.set(true);
        Ok(())
    }

    pub(crate) fn replace_block_text(
        &self,
        block_id: &BlockId,
        text: String,
        caret_utf16: usize,
    ) -> Result<bool, EditError> {
        self.edit_session()
            .ok_or(EditError::MissingBlock)?
            .borrow_mut()
            .replace_block_text(block_id, text, caret_utf16)
    }

    pub(crate) fn begin_save(&self) -> Option<(OpenDocument, SaveTicket)> {
        let edit = self.edit_session()?;
        let ticket = edit.borrow_mut().begin_save();
        Some((self.document(), ticket))
    }

    pub(crate) fn finish_save(
        &self,
        ticket: &SaveTicket,
        document: OpenDocument,
    ) -> Option<SaveOutcome> {
        let fingerprint = document.fingerprint()?.to_owned();
        let path = document.path()?.to_owned();
        let edit = self.edit_session()?;
        let outcome = edit.borrow_mut().finish_save(ticket, fingerprint, &path);
        *self.document.borrow_mut() = document;
        Some(outcome)
    }

    pub(crate) fn cancel_edit(&self) {
        if let Some(edit) = self.edit_session() {
            edit.borrow_mut().cancel_changes();
        }
        *self.edit.borrow_mut() = None;
        self.editing.set(false);
        *self.presentation_document.borrow_mut() = self.document();
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.edit_session()
            .is_some_and(|edit| edit.borrow().is_dirty())
    }

    pub(crate) fn undo(&self) -> Result<bool, EditError> {
        self.edit_session()
            .ok_or(EditError::MissingBlock)?
            .borrow_mut()
            .undo()
    }

    pub(crate) fn redo(&self) -> Result<bool, EditError> {
        self.edit_session()
            .ok_or(EditError::MissingBlock)?
            .borrow_mut()
            .redo()
    }

    fn replace_document(&self, document: OpenDocument) {
        *self.document.borrow_mut() = document.clone();
        if !self.editing() {
            *self.presentation_document.borrow_mut() = document;
        }
    }
}

/// Owns the lifecycle invariant between open document sessions, per-tab drafts,
/// viewport anchors, and the active tab.
#[derive(Default)]
pub(crate) struct DocumentWorkspace {
    active_path: RefCell<Option<PathBuf>>,
    sessions: RefCell<Vec<Rc<DocumentSession>>>,
}

impl DocumentWorkspace {
    pub(crate) fn active(&self) -> Option<OpenDocument> {
        self.active_session().map(|session| session.document())
    }

    pub(crate) fn active_session(&self) -> Option<Rc<DocumentSession>> {
        let path = self.active_path.borrow().clone()?;
        self.session(&path)
    }

    pub(crate) fn active_path(&self) -> Option<PathBuf> {
        self.active_path.borrow().clone()
    }

    pub(crate) fn contains(&self, path: &Path) -> bool {
        self.session(path).is_some()
    }

    pub(crate) fn session(&self, path: &Path) -> Option<Rc<DocumentSession>> {
        self.sessions
            .borrow()
            .iter()
            .find(|session| session.path() == path)
            .cloned()
    }

    pub(crate) fn select(&self, path: &Path) -> Option<OpenDocument> {
        self.session(path).map(|session| session.document())
    }

    pub(crate) fn first(&self) -> Option<OpenDocument> {
        self.sessions
            .borrow()
            .first()
            .map(|session| session.document())
    }

    pub(crate) fn snapshot(&self) -> Vec<OpenDocument> {
        self.sessions
            .borrow()
            .iter()
            .map(|session| session.document())
            .collect()
    }

    pub(crate) fn upsert(&self, document: OpenDocument) {
        let Some(path) = document.path().map(Path::to_path_buf) else {
            return;
        };
        if let Some(session) = self.session(&path) {
            session.replace_document(document);
        } else {
            self.sessions
                .borrow_mut()
                .push(Rc::new(DocumentSession::new(document)));
        }
    }

    pub(crate) fn activate(&self, document: &OpenDocument) {
        let path = document
            .path()
            .expect("active documents must have canonical paths");
        debug_assert!(self.contains(path));
        *self.active_path.borrow_mut() = Some(path.to_owned());
    }

    pub(crate) fn adopt_session_path(&self, previous_path: &Path, document: &OpenDocument) {
        if self.active_path().as_deref() == Some(previous_path) {
            *self.active_path.borrow_mut() = document.path().map(Path::to_path_buf);
        }
    }

    pub(crate) fn close(&self, path: &Path) -> CloseDocumentResult {
        if self.session(path).is_some_and(|session| session.is_dirty()) {
            return CloseDocumentResult {
                active_changed: false,
                active: self.active(),
                blocked_by_dirty_draft: true,
            };
        }
        let active_changed = self.active_path().as_deref() == Some(path);
        self.sessions
            .borrow_mut()
            .retain(|session| session.path() != path);
        if active_changed {
            *self.active_path.borrow_mut() =
                self.sessions.borrow().first().map(|session| session.path());
        }
        CloseDocumentResult {
            active_changed,
            active: self.active(),
            blocked_by_dirty_draft: false,
        }
    }

    pub(crate) fn clear_active(&self) {
        *self.active_path.borrow_mut() = None;
    }

    pub(crate) fn has_dirty_drafts(&self) -> bool {
        self.sessions
            .borrow()
            .iter()
            .any(|session| session.is_dirty())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use readloom_core::{ChapterKey, ReadloomCore};

    use super::*;

    fn document(core: &ReadloomCore, path: &Path) -> OpenDocument {
        std::fs::write(path, "第一段。\n第二段。\n").expect("write TXT fixture");
        OpenDocument::Txt(Arc::new(core.open_txt(path).expect("open TXT fixture")))
    }

    #[test]
    fn upsert_replaces_the_snapshot_without_discarding_the_session() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3")).expect("open core");
        let path = directory.path().join("one.txt");
        let workspace = DocumentWorkspace::default();
        let opened = document(&core, &path);
        let canonical_path = opened.path().expect("canonical fixture path").to_owned();
        workspace.upsert(opened);
        let original_session = workspace
            .session(&canonical_path)
            .expect("original session");
        workspace.upsert(document(&core, &path));

        assert_eq!(workspace.snapshot().len(), 1);
        assert!(Rc::ptr_eq(
            &original_session,
            &workspace.session(&canonical_path).expect("same session")
        ));
    }

    #[test]
    fn switching_tabs_keeps_independent_drafts_and_anchors() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3")).expect("open core");
        let workspace = DocumentWorkspace::default();
        let first = document(&core, &directory.path().join("one.txt"));
        let second = document(&core, &directory.path().join("two.txt"));
        workspace.upsert(first.clone());
        workspace.upsert(second.clone());
        let first_session = workspace.session(first.path().unwrap()).unwrap();
        let first_block = first.paragraphs()[1].block_id.clone();
        let first_anchor = ViewAnchor {
            chapter_key: ChapterKey::new("TXT:0"),
            block_id: first_block.clone(),
            character_offset_utf16: 2,
            pixel_offset_from_viewport_top: -7.25,
        };
        first_session.begin_edit(first_anchor.clone()).unwrap();
        first_session
            .replace_block_text(&first_block, "第一本的中文草稿".to_owned(), 8)
            .unwrap();

        workspace.activate(&second);
        workspace.activate(&first);

        assert!(first_session.is_dirty());
        assert_eq!(
            first_session
                .anchor()
                .unwrap()
                .pixel_offset_from_viewport_top,
            first_anchor.pixel_offset_from_viewport_top
        );
        assert_eq!(
            first_session
                .edit_session()
                .unwrap()
                .borrow()
                .draft()
                .block(&first_block)
                .unwrap()
                .text,
            "第一本的中文草稿"
        );
    }

    #[test]
    fn dirty_tabs_cannot_be_closed_without_a_decision() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3")).expect("open core");
        let workspace = DocumentWorkspace::default();
        let open = document(&core, &directory.path().join("dirty.txt"));
        workspace.upsert(open.clone());
        workspace.activate(&open);
        let session = workspace.active_session().unwrap();
        let block = open.paragraphs()[0].block_id.clone();
        session
            .begin_edit(ViewAnchor {
                chapter_key: ChapterKey::new("TXT:0"),
                block_id: block.clone(),
                character_offset_utf16: 0,
                pixel_offset_from_viewport_top: 0.0,
            })
            .unwrap();
        session
            .replace_block_text(&block, "未保存".to_owned(), 3)
            .unwrap();

        let result = workspace.close(open.path().unwrap());

        assert!(result.blocked_by_dirty_draft);
        assert!(workspace.contains(open.path().unwrap()));
    }
}
