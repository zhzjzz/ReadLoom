use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    application::epub_document_service::{EpubDocumentService, EpubResourceResponse},
    config::EpubChapterEditLimits,
    domain::{
        epub_document::ParsedEpubDocument,
        epub_edit::{
            ChapterCompatibilityLevel, ChapterDraftAccepted, ChapterDraftUpdate,
            ChapterEditCapabilities, ChapterEditDto, ChapterEditWarning, ChapterValidationState,
            EpubCoverDraft, EpubCoverState, EpubDraftChanges, EpubDraftValidation, EpubEditDraft,
            EpubMetadataDraft, EpubMetadataPatch, EpubValidationIssue, EpubValidationSeverity,
            ImportedChapterImage, SavedEpubDocument,
        },
    },
    error::AppError,
    formats::epub::{
        chapter_xhtml::{
            AnalyzedChapter, PreservedOuterDocument, analyze_chapter, serialize_editor_document,
        },
        cover_image::{ValidatedCover, load_cover},
        cover_xhtml::patch_cover_reference,
        opf_editor::{
            CoverManifestChange, ManifestAddition, changed_fields, patch_opf_with_resources,
            validate_xml,
        },
        parser::parse_epub_document,
        repack::{repack_epub, verify_unchanged_resources},
    },
    infrastructure::{
        archive::{
            archive_limits::ArchiveLimits,
            safe_zip::{ResourceClass, SafeArchivePath, SafeEpubArchive},
        },
        filesystem::{
            FileFingerprint, commit_prepared_output, create_prepared_output, fingerprint_file,
        },
    },
};

const OVERWRITE_TOKEN_LIFETIME_MS: u64 = 120_000;

#[derive(Clone)]
pub(crate) struct EpubEditService {
    limits: ArchiveLimits,
    chapter_limits: EpubChapterEditLimits,
    documents: EpubDocumentService,
    state: Arc<Mutex<EditState>>,
}

#[derive(Default)]
struct EditState {
    drafts: HashMap<String, PublicationDraft>,
    overwrite_tokens: HashMap<String, OverwriteConfirmation>,
}

#[derive(Clone)]
struct PublicationDraft {
    edit_session_id: String,
    document_id: String,
    reading_session_id: String,
    source_path: PathBuf,
    source_fingerprint: FileFingerprint,
    source_publication: ParsedEpubDocument,
    source_metadata: EpubMetadataDraft,
    saved_metadata: EpubMetadataDraft,
    metadata: EpubMetadataDraft,
    original_opf: Vec<u8>,
    cover_change: Option<DraftCover>,
    saved_cover_hash: Option<String>,
    chapter_drafts: HashMap<usize, ChapterEditDraft>,
    imported_images: HashMap<String, DraftChapterImage>,
    revision: u64,
    saved_revision: u64,
    saving: bool,
    cancelled: Arc<AtomicBool>,
    created_at_ms: u64,
    updated_at_ms: u64,
}

#[derive(Clone)]
struct DraftCover {
    image: ValidatedCover,
    resource_id: String,
    item_id: String,
}

#[derive(Clone)]
struct ChapterEditDraft {
    chapter_edit_id: String,
    spine_index: usize,
    manifest_item_id: String,
    chapter_href: String,
    chapter_title: String,
    original_resource_hash: String,
    original_xhtml: Vec<u8>,
    preserved_outer_document: Option<PreservedOuterDocument>,
    editor_document: serde_json::Value,
    normalized_xhtml: Vec<u8>,
    saved_xhtml: Vec<u8>,
    compatibility_level: ChapterCompatibilityLevel,
    warnings: Vec<ChapterEditWarning>,
    draft_revision: u64,
    accepted_revision: u64,
    preview_revision: u64,
    validation_state: ChapterValidationState,
}

#[derive(Clone)]
struct DraftChapterImage {
    image: ValidatedCover,
    resource_id: String,
    item_id: String,
    referenced: bool,
}

#[derive(Clone)]
struct OverwriteConfirmation {
    edit_session_id: String,
    target_path: PathBuf,
    target_fingerprint: FileFingerprint,
    revision: u64,
    expires_at_ms: u64,
}

struct SaveSnapshot {
    draft: PublicationDraft,
    target_path: PathBuf,
    expected_target: Option<FileFingerprint>,
}

impl EpubEditService {
    pub(crate) fn new(limits: ArchiveLimits, documents: EpubDocumentService) -> Self {
        Self {
            limits,
            chapter_limits: EpubChapterEditLimits::default(),
            documents,
            state: Arc::new(Mutex::new(EditState::default())),
        }
    }

    pub(crate) fn begin(&self, document_id: &str) -> Result<EpubEditDraft, AppError> {
        let context = self.documents.session_context(document_id)?;
        if !context.parsed.capabilities.can_edit_metadata
            || !context.parsed.capabilities.can_replace_cover
            || !context.parsed.capabilities.can_save_as
        {
            return Err(editing_not_supported());
        }
        let mut state = self.lock_state()?;
        if let Some(existing) = state
            .drafts
            .values()
            .find(|draft| draft.document_id == document_id)
        {
            return Ok(draft_dto(existing));
        }
        let source_fingerprint =
            fingerprint_file(&context.path).map_err(|_| source_modified_externally())?;
        if source_fingerprint.blake3 != context.file_fingerprint {
            return Err(source_modified_externally());
        }
        let archive = SafeEpubArchive::open(&context.path, self.limits)?;
        let opf_path = SafeArchivePath::parse(&context.parsed.package_resource_id)?;
        let original_opf = archive.read(&opf_path, ResourceClass::Xml)?;
        validate_xml(&original_opf)?;
        let timestamp = now_ms()?;
        let source_metadata = EpubMetadataDraft::from_publication(
            &context.parsed.metadata,
            &context.parsed.publication_id,
        );
        validate_metadata(&source_metadata)?;
        let edit_session_id = format!("edit-{}", random_token(16)?);
        let draft = PublicationDraft {
            edit_session_id: edit_session_id.clone(),
            document_id: context.document_id,
            reading_session_id: context.session_id,
            source_path: context.path,
            source_fingerprint,
            source_publication: context.parsed,
            source_metadata: source_metadata.clone(),
            saved_metadata: source_metadata.clone(),
            metadata: source_metadata,
            original_opf,
            cover_change: None,
            saved_cover_hash: None,
            chapter_drafts: HashMap::new(),
            imported_images: HashMap::new(),
            revision: 0,
            saved_revision: 0,
            saving: false,
            cancelled: Arc::new(AtomicBool::new(false)),
            created_at_ms: timestamp,
            updated_at_ms: timestamp,
        };
        let dto = draft_dto(&draft);
        state.drafts.insert(edit_session_id, draft);
        Ok(dto)
    }

    pub(crate) fn get(&self, edit_session_id: &str) -> Result<EpubEditDraft, AppError> {
        let state = self.lock_state()?;
        let draft = state
            .drafts
            .get(edit_session_id)
            .ok_or_else(edit_session_not_found)?;
        Ok(draft_dto(draft))
    }

    pub(crate) fn update_metadata(
        &self,
        edit_session_id: &str,
        expected_revision: u64,
        patch: EpubMetadataPatch,
    ) -> Result<EpubEditDraft, AppError> {
        let mut state = self.lock_state()?;
        let draft = state
            .drafts
            .get_mut(edit_session_id)
            .ok_or_else(edit_session_not_found)?;
        ensure_revision(draft, expected_revision)?;
        ensure_not_saving(draft)?;
        let mut updated = draft.metadata.clone();
        apply_metadata_patch(&mut updated, patch);
        normalize_metadata(&mut updated);
        validate_metadata(&updated)?;
        if updated != draft.metadata {
            draft.metadata = updated;
            advance_revision(draft)?;
        }
        Ok(draft_dto(draft))
    }

    pub(crate) fn replace_cover(
        &self,
        edit_session_id: &str,
        expected_revision: u64,
        selected_path: &Path,
    ) -> Result<EpubEditDraft, AppError> {
        let cover = load_cover(selected_path, self.limits)?;
        let mut state = self.lock_state()?;
        let draft = state
            .drafts
            .get_mut(edit_session_id)
            .ok_or_else(edit_session_not_found)?;
        ensure_revision(draft, expected_revision)?;
        ensure_not_saving(draft)?;
        if draft
            .cover_change
            .as_ref()
            .is_some_and(|current| current.image.content_hash == cover.content_hash)
        {
            return Ok(draft_dto(draft));
        }
        let (resource_id, item_id) = unique_cover_names(draft, &cover)?;
        draft.cover_change = Some(DraftCover {
            image: cover,
            resource_id,
            item_id,
        });
        advance_revision(draft)?;
        Ok(draft_dto(draft))
    }

    pub(crate) fn remove_cover_change(
        &self,
        edit_session_id: &str,
        expected_revision: u64,
    ) -> Result<EpubEditDraft, AppError> {
        let mut state = self.lock_state()?;
        let draft = state
            .drafts
            .get_mut(edit_session_id)
            .ok_or_else(edit_session_not_found)?;
        ensure_revision(draft, expected_revision)?;
        ensure_not_saving(draft)?;
        if draft.cover_change.take().is_some() {
            advance_revision(draft)?;
        }
        Ok(draft_dto(draft))
    }

    pub(crate) fn begin_chapter_edit(
        &self,
        edit_session_id: &str,
        spine_index: usize,
    ) -> Result<ChapterEditDto, AppError> {
        let mut state = self.lock_state()?;
        let draft = state
            .drafts
            .get_mut(edit_session_id)
            .ok_or_else(edit_session_not_found)?;
        ensure_not_saving(draft)?;
        if let Some(existing) = draft.chapter_drafts.get(&spine_index) {
            return Ok(chapter_dto(draft, existing));
        }
        let spine = draft
            .source_publication
            .spine
            .get(spine_index)
            .filter(|item| item.index == spine_index)
            .cloned()
            .ok_or_else(chapter_not_found)?;
        if !matches!(
            spine.media_type.as_str(),
            "application/xhtml+xml" | "text/html"
        ) {
            return Err(chapter_not_supported());
        }
        let archive = SafeEpubArchive::open(&draft.source_path, self.limits)?;
        let source = archive.read(
            &SafeArchivePath::parse(&spine.resource_id)?,
            ResourceClass::Xhtml,
        )?;
        let resources = manifest_resource_ids(draft);
        let analysis = analyze_chapter(
            &source,
            &spine.resource_id,
            &draft.reading_session_id,
            &resources,
            draft.source_publication.layout == crate::domain::epub_document::EpubLayout::Fixed,
            self.chapter_limits,
        );
        let chapter = chapter_from_analysis(
            format!("chapter-edit-{}", random_token(16)?),
            spine_index,
            spine.idref,
            spine.resource_id.clone(),
            chapter_title(&draft.source_publication, &spine.resource_id, spine_index),
            analysis,
        );
        let dto = chapter_dto(draft, &chapter);
        draft.chapter_drafts.insert(spine_index, chapter);
        Ok(dto)
    }

    pub(crate) fn update_chapter_draft(
        &self,
        update: ChapterDraftUpdate,
    ) -> Result<ChapterDraftAccepted, AppError> {
        if update.request_id.is_empty()
            || update.request_id.len() > 128
            || !update.request_id.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
        {
            return Err(invalid_chapter_request());
        }
        let mut state = self.lock_state()?;
        let publication_key = publication_for_chapter(&state, &update.chapter_edit_id)?;
        let publication = state
            .drafts
            .get_mut(&publication_key)
            .ok_or_else(edit_session_not_found)?;
        ensure_not_saving(publication)?;
        let resources = manifest_resource_ids(publication);
        let reading_session_id = publication.reading_session_id.clone();
        let chapter_index = publication
            .chapter_drafts
            .iter()
            .find(|(_, chapter)| chapter.chapter_edit_id == update.chapter_edit_id)
            .map(|(index, _)| *index)
            .ok_or_else(chapter_draft_expired)?;
        let (content_changed, accepted) = {
            let chapter = publication
                .chapter_drafts
                .get_mut(&chapter_index)
                .ok_or_else(chapter_draft_expired)?;
            ensure_chapter_editable(chapter)?;
            if update.base_revision != chapter.accepted_revision
                || update.client_revision <= update.base_revision
            {
                return Err(chapter_revision_conflict());
            }
            let outer = chapter
                .preserved_outer_document
                .as_ref()
                .ok_or_else(chapter_not_supported)?;
            let normalized = serialize_editor_document(
                outer,
                &update.editor_document,
                &chapter.chapter_href,
                &reading_session_id,
                &resources,
                self.chapter_limits,
            )?;
            let content_changed = normalized != chapter.normalized_xhtml;
            chapter.editor_document = update.editor_document;
            chapter.normalized_xhtml = normalized;
            chapter.draft_revision = update.client_revision;
            chapter.accepted_revision = update.client_revision;
            chapter.validation_state = if chapter.warnings.is_empty() {
                ChapterValidationState::Valid
            } else {
                ChapterValidationState::Warning
            };
            if content_changed {
                chapter.preview_revision = chapter
                    .preview_revision
                    .checked_add(1)
                    .ok_or_else(chapter_revision_conflict)?;
            }
            let accepted = ChapterDraftAccepted {
                chapter_edit_id: chapter.chapter_edit_id.clone(),
                request_id: update.request_id,
                client_revision: update.client_revision,
                accepted_revision: chapter.accepted_revision,
                dirty: chapter.normalized_xhtml != chapter.saved_xhtml,
                warnings: chapter.warnings.clone(),
                preview_revision: chapter.preview_revision,
                publication_revision: 0,
            };
            (content_changed, accepted)
        };
        if content_changed {
            advance_revision(publication)?;
        }
        refresh_imported_image_references(publication);
        let mut accepted = accepted;
        accepted.publication_revision = publication.revision;
        Ok(accepted)
    }

    pub(crate) fn flush_chapter_draft(
        &self,
        chapter_edit_id: &str,
        revision: u64,
    ) -> Result<ChapterDraftAccepted, AppError> {
        let state = self.lock_state()?;
        let publication_key = publication_for_chapter(&state, chapter_edit_id)?;
        let publication = state
            .drafts
            .get(&publication_key)
            .ok_or_else(edit_session_not_found)?;
        let chapter = publication
            .chapter_drafts
            .values()
            .find(|chapter| chapter.chapter_edit_id == chapter_edit_id)
            .ok_or_else(chapter_draft_expired)?;
        if revision != chapter.accepted_revision {
            return Err(chapter_revision_conflict());
        }
        Ok(ChapterDraftAccepted {
            chapter_edit_id: chapter.chapter_edit_id.clone(),
            request_id: "flush".to_owned(),
            client_revision: revision,
            accepted_revision: chapter.accepted_revision,
            dirty: chapter.normalized_xhtml != chapter.saved_xhtml,
            warnings: chapter.warnings.clone(),
            preview_revision: chapter.preview_revision,
            publication_revision: publication.revision,
        })
    }

    pub(crate) fn validate_chapter_draft(
        &self,
        chapter_edit_id: &str,
    ) -> Result<ChapterEditDto, AppError> {
        let state = self.lock_state()?;
        let publication_key = publication_for_chapter(&state, chapter_edit_id)?;
        let publication = state
            .drafts
            .get(&publication_key)
            .ok_or_else(edit_session_not_found)?;
        let chapter = publication
            .chapter_drafts
            .values()
            .find(|chapter| chapter.chapter_edit_id == chapter_edit_id)
            .ok_or_else(chapter_draft_expired)?;
        if matches!(
            chapter.compatibility_level,
            ChapterCompatibilityLevel::Full | ChapterCompatibilityLevel::Limited
        ) {
            let resources = manifest_resource_ids(publication);
            serialize_editor_document(
                chapter
                    .preserved_outer_document
                    .as_ref()
                    .ok_or_else(chapter_not_supported)?,
                &chapter.editor_document,
                &chapter.chapter_href,
                &publication.reading_session_id,
                &resources,
                self.chapter_limits,
            )?;
        }
        Ok(chapter_dto(publication, chapter))
    }

    pub(crate) fn revert_chapter_draft(
        &self,
        chapter_edit_id: &str,
    ) -> Result<ChapterEditDto, AppError> {
        let mut state = self.lock_state()?;
        let publication_key = publication_for_chapter(&state, chapter_edit_id)?;
        let publication = state
            .drafts
            .get_mut(&publication_key)
            .ok_or_else(edit_session_not_found)?;
        ensure_not_saving(publication)?;
        let resources = manifest_resource_ids(publication);
        let reading_session_id = publication.reading_session_id.clone();
        let chapter_index = publication
            .chapter_drafts
            .iter()
            .find(|(_, chapter)| chapter.chapter_edit_id == chapter_edit_id)
            .map(|(index, _)| *index)
            .ok_or_else(chapter_draft_expired)?;
        let changed = {
            let chapter = publication
                .chapter_drafts
                .get_mut(&chapter_index)
                .ok_or_else(chapter_draft_expired)?;
            ensure_chapter_editable(chapter)?;
            let analysis = analyze_chapter(
                &chapter.original_xhtml,
                &chapter.chapter_href,
                &reading_session_id,
                &resources,
                false,
                self.chapter_limits,
            );
            let changed = chapter.normalized_xhtml != chapter.original_xhtml;
            chapter.editor_document = analysis.editor_document;
            chapter.normalized_xhtml = chapter.original_xhtml.clone();
            chapter.accepted_revision = chapter
                .accepted_revision
                .checked_add(1)
                .ok_or_else(chapter_revision_conflict)?;
            chapter.draft_revision = chapter.accepted_revision;
            chapter.preview_revision = chapter.preview_revision.saturating_add(1);
            chapter.validation_state = analysis.validation_state;
            changed
        };
        if changed {
            advance_revision(publication)?;
        }
        refresh_imported_image_references(publication);
        let chapter = publication
            .chapter_drafts
            .get(&chapter_index)
            .ok_or_else(chapter_draft_expired)?;
        Ok(chapter_dto(publication, chapter))
    }

    pub(crate) fn import_chapter_image(
        &self,
        edit_session_id: &str,
        chapter_edit_id: &str,
        selected_path: &Path,
    ) -> Result<ImportedChapterImage, AppError> {
        let image = load_cover(selected_path, self.limits).map_err(|_| image_import_failed())?;
        let mut state = self.lock_state()?;
        let publication = state
            .drafts
            .get_mut(edit_session_id)
            .ok_or_else(edit_session_not_found)?;
        ensure_not_saving(publication)?;
        let chapter = publication
            .chapter_drafts
            .values()
            .find(|chapter| chapter.chapter_edit_id == chapter_edit_id)
            .ok_or_else(chapter_draft_expired)?;
        ensure_chapter_editable(chapter)?;
        let (resource_id, item_id) = unique_chapter_image_names(publication, &image)?;
        let encoded = resource_id
            .split('/')
            .map(|segment| {
                percent_encoding::utf8_percent_encode(segment, percent_encoding::NON_ALPHANUMERIC)
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("/");
        let preview_url = format!(
            "http://readloom-epub.localhost/{}/{encoded}",
            publication.reading_session_id
        );
        publication.imported_images.insert(
            resource_id.clone(),
            DraftChapterImage {
                image: image.clone(),
                resource_id: resource_id.clone(),
                item_id,
                referenced: false,
            },
        );
        Ok(ImportedChapterImage {
            chapter_edit_id: chapter_edit_id.to_owned(),
            resource_id,
            editor_src: preview_url.clone(),
            preview_url,
            media_type: image.media_type,
            width: image.width,
            height: image.height,
        })
    }

    pub(crate) fn chapter_override(
        &self,
        reading_session_id: &str,
        resource_id: &str,
    ) -> Result<Option<Vec<u8>>, AppError> {
        let state = self.lock_state()?;
        let Some(publication) = state
            .drafts
            .values()
            .find(|draft| draft.reading_session_id == reading_session_id)
        else {
            return Ok(None);
        };
        Ok(publication
            .chapter_drafts
            .values()
            .find(|chapter| chapter.chapter_href == resource_id)
            .map(|chapter| chapter.normalized_xhtml.clone()))
    }

    pub(crate) fn chapter_image_resource(
        &self,
        reading_session_id: &str,
        resource_id: &str,
    ) -> Result<Option<EpubResourceResponse>, AppError> {
        let state = self.lock_state()?;
        let Some(publication) = state
            .drafts
            .values()
            .find(|draft| draft.reading_session_id == reading_session_id)
        else {
            return Ok(None);
        };
        Ok(publication
            .imported_images
            .get(resource_id)
            .map(|image| EpubResourceResponse {
                body: image.image.bytes.as_ref().clone(),
                content_type: image.image.media_type.clone(),
                content_security_policy: None,
            }))
    }

    pub(crate) fn validate(&self, edit_session_id: &str) -> Result<EpubDraftValidation, AppError> {
        let state = self.lock_state()?;
        let draft = state
            .drafts
            .get(edit_session_id)
            .ok_or_else(edit_session_not_found)?;
        validate_draft(draft)
    }

    pub(crate) fn prepare_overwrite(
        &self,
        edit_session_id: &str,
        expected_revision: u64,
        target_path: &Path,
    ) -> Result<String, AppError> {
        let target_path = normalize_target(target_path)?;
        let mut state = self.lock_state()?;
        let draft = state
            .drafts
            .get(edit_session_id)
            .ok_or_else(edit_session_not_found)?;
        ensure_revision(draft, expected_revision)?;
        ensure_target_is_not_source(&draft.source_path, &target_path)?;
        if !target_path.exists() {
            return Err(AppError::validation(
                "OVERWRITE_CONFIRMATION_REQUIRED",
                "目标文件尚不存在，不需要覆盖确认。",
                "直接执行另存为即可。",
            ));
        }
        let target_fingerprint = fingerprint_file(&target_path).map_err(|_| {
            AppError::validation(
                "TARGET_ALREADY_EXISTS",
                "无法读取现有目标文件以确认覆盖。",
                "关闭占用该文件的程序后重试。",
            )
        })?;
        let token = format!("overwrite-{}", random_token(24)?);
        state
            .overwrite_tokens
            .retain(|_, value| value.expires_at_ms >= now_ms().unwrap_or(0));
        state.overwrite_tokens.insert(
            token.clone(),
            OverwriteConfirmation {
                edit_session_id: edit_session_id.to_owned(),
                target_path,
                target_fingerprint,
                revision: expected_revision,
                expires_at_ms: now_ms()?.saturating_add(OVERWRITE_TOKEN_LIFETIME_MS),
            },
        );
        Ok(token)
    }

    pub(crate) fn save_as(
        &self,
        edit_session_id: &str,
        expected_revision: u64,
        target_path: &Path,
        confirmation_token: Option<&str>,
    ) -> Result<SavedEpubDocument, AppError> {
        let target_path = normalize_target(target_path)?;
        let snapshot = {
            let mut state = self.lock_state()?;
            {
                let draft = state
                    .drafts
                    .get(edit_session_id)
                    .ok_or_else(edit_session_not_found)?;
                ensure_revision(draft, expected_revision)?;
                ensure_target_is_not_source(&draft.source_path, &target_path)?;
            }
            let expected_target = if target_path.exists() {
                let token = confirmation_token.ok_or_else(target_already_exists)?;
                let confirmation = state
                    .overwrite_tokens
                    .remove(token)
                    .ok_or_else(overwrite_confirmation_expired)?;
                if confirmation.expires_at_ms < now_ms()?
                    || confirmation.edit_session_id != edit_session_id
                    || confirmation.target_path != target_path
                    || confirmation.revision != expected_revision
                {
                    return Err(overwrite_confirmation_expired());
                }
                Some(confirmation.target_fingerprint)
            } else {
                if confirmation_token.is_some() {
                    return Err(overwrite_confirmation_expired());
                }
                None
            };
            let draft = state
                .drafts
                .get_mut(edit_session_id)
                .ok_or_else(edit_session_not_found)?;
            ensure_revision(draft, expected_revision)?;
            ensure_not_saving(draft)?;
            ensure_target_is_not_source(&draft.source_path, &target_path)?;
            let validation = validate_draft(draft)?;
            if !validation.can_save {
                return Err(draft_validation_failed());
            }
            draft.saving = true;
            draft.cancelled.store(false, Ordering::Release);
            SaveSnapshot {
                draft: draft.clone(),
                target_path,
                expected_target,
            }
        };

        let saved = self.save_snapshot(&snapshot);
        let mut state = self.lock_state()?;
        let draft = state
            .drafts
            .get_mut(edit_session_id)
            .ok_or_else(edit_session_not_found)?;
        draft.saving = false;
        match saved {
            Ok((fingerprint, parsed)) => {
                if draft.revision != snapshot.draft.revision {
                    return Err(draft_conflict());
                }
                draft.saved_metadata = draft.metadata.clone();
                draft.saved_cover_hash = draft
                    .cover_change
                    .as_ref()
                    .map(|cover| cover.image.content_hash.clone());
                for chapter in draft.chapter_drafts.values_mut() {
                    chapter.saved_xhtml = chapter.normalized_xhtml.clone();
                }
                draft.saved_revision = draft.revision;
                draft.updated_at_ms = now_ms()?;
                Ok(SavedEpubDocument {
                    edit_session_id: edit_session_id.to_owned(),
                    target_path: snapshot.target_path.display().to_string(),
                    file_fingerprint: fingerprint.blake3,
                    document: parsed,
                    draft: draft_dto(draft),
                })
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn cancel_save(&self, edit_session_id: &str) -> Result<(), AppError> {
        let state = self.lock_state()?;
        let draft = state
            .drafts
            .get(edit_session_id)
            .ok_or_else(edit_session_not_found)?;
        if draft.saving {
            draft.cancelled.store(true, Ordering::Release);
        }
        Ok(())
    }

    pub(crate) fn discard(&self, edit_session_id: &str) -> Result<(), AppError> {
        let mut state = self.lock_state()?;
        let draft = state
            .drafts
            .get(edit_session_id)
            .ok_or_else(edit_session_not_found)?;
        ensure_not_saving(draft)?;
        state.drafts.remove(edit_session_id);
        state
            .overwrite_tokens
            .retain(|_, value| value.edit_session_id != edit_session_id);
        Ok(())
    }

    pub(crate) fn close_document(&self, document_id: &str) {
        if let Ok(mut state) = self.state.lock() {
            let removed = state
                .drafts
                .iter()
                .filter(|(_, draft)| draft.document_id == document_id)
                .map(|(id, _)| id.clone())
                .collect::<HashSet<_>>();
            for id in &removed {
                if let Some(draft) = state.drafts.remove(id) {
                    draft.cancelled.store(true, Ordering::Release);
                }
            }
            state
                .overwrite_tokens
                .retain(|_, value| !removed.contains(&value.edit_session_id));
        }
    }

    pub(crate) fn cover_resource(
        &self,
        reading_session_id: &str,
        resource_id: &str,
    ) -> Result<Option<EpubResourceResponse>, AppError> {
        let Some(edit_session_id) = resource_id
            .strip_prefix("__readloom_edit/")
            .and_then(|value| value.strip_suffix("/cover"))
        else {
            return Ok(None);
        };
        if !edit_session_id.starts_with("edit-") || edit_session_id.len() > 80 {
            return Err(edit_session_not_found());
        }
        let state = self.lock_state()?;
        let draft = state
            .drafts
            .get(edit_session_id)
            .ok_or_else(edit_session_not_found)?;
        if draft.reading_session_id != reading_session_id {
            return Err(edit_session_not_found());
        }
        let cover = draft
            .cover_change
            .as_ref()
            .ok_or_else(edit_session_not_found)?;
        Ok(Some(EpubResourceResponse {
            body: cover.image.bytes.as_ref().clone(),
            content_type: cover.image.media_type.clone(),
            content_security_policy: None,
        }))
    }

    fn save_snapshot(
        &self,
        snapshot: &SaveSnapshot,
    ) -> Result<(FileFingerprint, ParsedEpubDocument), AppError> {
        verify_source_unchanged(&snapshot.draft)?;
        let modified_at = rfc3339_now()?;
        let cover_manifest =
            snapshot
                .draft
                .cover_change
                .as_ref()
                .map(|cover| CoverManifestChange {
                    item_id: cover.item_id.clone(),
                    resource_id: cover.resource_id.clone(),
                    media_type: cover.image.media_type.clone(),
                });
        let manifest_additions = snapshot
            .draft
            .imported_images
            .values()
            .filter(|image| image.referenced)
            .map(|image| ManifestAddition {
                item_id: image.item_id.clone(),
                resource_id: image.resource_id.clone(),
                media_type: image.image.media_type.clone(),
            })
            .collect::<Vec<_>>();
        let patched_opf = patch_opf_with_resources(
            &snapshot.draft.original_opf,
            &snapshot.draft.source_metadata,
            &snapshot.draft.metadata,
            &snapshot.draft.source_publication.package_resource_id,
            &snapshot.draft.source_publication.version,
            cover_manifest.as_ref(),
            &manifest_additions,
            &modified_at,
        )?;
        let mut overlays = HashMap::new();
        let opf_changed = patched_opf != snapshot.draft.original_opf;
        if opf_changed {
            overlays.insert(
                snapshot
                    .draft
                    .source_publication
                    .package_resource_id
                    .clone(),
                patched_opf,
            );
        }
        if let Some(cover) = &snapshot.draft.cover_change {
            overlays.insert(
                cover.resource_id.clone(),
                cover.image.bytes.as_ref().clone(),
            );
            append_cover_document_overlays(&snapshot.draft, cover, &mut overlays, self.limits)?;
        }
        for chapter in snapshot.draft.chapter_drafts.values() {
            if chapter.normalized_xhtml != chapter.original_xhtml {
                overlays.insert(
                    chapter.chapter_href.clone(),
                    chapter.normalized_xhtml.clone(),
                );
            }
        }
        for image in snapshot
            .draft
            .imported_images
            .values()
            .filter(|image| image.referenced)
        {
            overlays.insert(
                image.resource_id.clone(),
                image.image.bytes.as_ref().clone(),
            );
        }
        let modified_paths = overlays.keys().cloned().collect::<HashSet<_>>();
        let (temporary_file, temporary_path) = create_prepared_output(&snapshot.target_path)?;
        let result = (|| {
            let (temporary_file, _) = repack_epub(
                &snapshot.draft.source_path,
                temporary_file,
                &overlays,
                self.limits,
                &snapshot.draft.cancelled,
            )?;
            drop(temporary_file);
            SafeEpubArchive::open(&temporary_path, self.limits)
                .map_err(|_| generated_epub_invalid())?;
            let parsed = parse_epub_document(&temporary_path, self.limits)
                .map_err(|_| generated_epub_invalid())?;
            validate_generated_publication(&snapshot.draft, &parsed)?;
            verify_saved_chapters(&snapshot.draft, &temporary_path, self.limits)?;
            verify_unchanged_resources(
                &snapshot.draft.source_path,
                &temporary_path,
                &modified_paths,
            )?;
            verify_source_unchanged(&snapshot.draft)?;
            let fingerprint = commit_prepared_output(
                &snapshot.target_path,
                &temporary_path,
                snapshot.expected_target.as_ref(),
            )?;
            Ok((fingerprint, parsed))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        result
    }

    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, EditState>, AppError> {
        self.state
            .lock()
            .map_err(|_| AppError::internal("INTERNAL", "lock EPUB edit sessions"))
    }
}

fn draft_dto(draft: &PublicationDraft) -> EpubEditDraft {
    let metadata_fields = changed_fields(&draft.saved_metadata, &draft.metadata)
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let current_cover_hash = draft
        .cover_change
        .as_ref()
        .map(|cover| cover.image.content_hash.as_str());
    let cover_changed = current_cover_hash != draft.saved_cover_hash.as_deref();
    let mut modified_chapters = draft
        .chapter_drafts
        .values()
        .filter(|chapter| chapter.normalized_xhtml != chapter.saved_xhtml)
        .map(|chapter| chapter.spine_index)
        .collect::<Vec<_>>();
    modified_chapters.sort_unstable();
    let added_resources = draft
        .imported_images
        .values()
        .filter(|image| image.referenced)
        .count();
    let dirty = !metadata_fields.is_empty() || cover_changed || !modified_chapters.is_empty();
    let cover = if let Some(replacement) = &draft.cover_change {
        EpubCoverDraft {
            state: EpubCoverState::Replaced,
            original_resource_id: draft.source_publication.cover_resource_id.clone(),
            current_resource_id: Some(replacement.resource_id.clone()),
            preview_resource_id: Some(format!("__readloom_edit/{}/cover", draft.edit_session_id)),
            media_type: Some(replacement.image.media_type.clone()),
            width: Some(replacement.image.width),
            height: Some(replacement.image.height),
        }
    } else {
        EpubCoverDraft {
            state: EpubCoverState::Unchanged,
            original_resource_id: draft.source_publication.cover_resource_id.clone(),
            current_resource_id: draft.source_publication.cover_resource_id.clone(),
            preview_resource_id: draft.source_publication.cover_resource_id.clone(),
            media_type: draft
                .source_publication
                .cover_resource_id
                .as_ref()
                .and_then(|id| {
                    draft
                        .source_publication
                        .manifest
                        .iter()
                        .find(|item| &item.resource_id == id)
                })
                .map(|item| item.media_type.clone()),
            width: None,
            height: None,
        }
    };
    let mut validation = validate_draft(draft).unwrap_or_else(|error| EpubDraftValidation {
        errors: vec![validation_issue(
            error.to_dto().code,
            error.to_dto().message,
            EpubValidationSeverity::Error,
        )],
        warnings: Vec::new(),
        information: Vec::new(),
        can_save: false,
    });
    validation.can_save = validation.errors.is_empty() && dirty && !draft.saving;
    EpubEditDraft {
        edit_session_id: draft.edit_session_id.clone(),
        document_id: draft.document_id.clone(),
        source_path: draft.source_path.display().to_string(),
        publication_id: draft.source_publication.publication_id.clone(),
        opf_resource_id: draft.source_publication.package_resource_id.clone(),
        metadata: draft.metadata.clone(),
        cover,
        changes: EpubDraftChanges {
            metadata_fields,
            cover_changed,
            modified_chapters,
            added_resources,
        },
        dirty,
        validation,
        revision: draft.revision,
        saved_revision: draft.saved_revision,
        saving: draft.saving,
        created_at_ms: draft.created_at_ms,
        updated_at_ms: draft.updated_at_ms,
    }
}

fn validate_draft(draft: &PublicationDraft) -> Result<EpubDraftValidation, AppError> {
    validate_metadata(&draft.metadata)?;
    let cover = draft
        .cover_change
        .as_ref()
        .map(|cover| CoverManifestChange {
            item_id: cover.item_id.clone(),
            resource_id: cover.resource_id.clone(),
            media_type: cover.image.media_type.clone(),
        });
    let additions = draft
        .imported_images
        .values()
        .filter(|image| image.referenced)
        .map(|image| ManifestAddition {
            item_id: image.item_id.clone(),
            resource_id: image.resource_id.clone(),
            media_type: image.image.media_type.clone(),
        })
        .collect::<Vec<_>>();
    patch_opf_with_resources(
        &draft.original_opf,
        &draft.source_metadata,
        &draft.metadata,
        &draft.source_publication.package_resource_id,
        &draft.source_publication.version,
        cover.as_ref(),
        &additions,
        "2000-01-01T00:00:00Z",
    )?;
    let resources = manifest_resource_ids(draft);
    for chapter in draft.chapter_drafts.values() {
        if matches!(
            chapter.compatibility_level,
            ChapterCompatibilityLevel::Full | ChapterCompatibilityLevel::Limited
        ) {
            serialize_editor_document(
                chapter
                    .preserved_outer_document
                    .as_ref()
                    .ok_or_else(chapter_not_supported)?,
                &chapter.editor_document,
                &chapter.chapter_href,
                &draft.reading_session_id,
                &resources,
                EpubChapterEditLimits::default(),
            )?;
        }
    }
    let dirty = draft.metadata != draft.saved_metadata
        || draft
            .cover_change
            .as_ref()
            .map(|cover| cover.image.content_hash.as_str())
            != draft.saved_cover_hash.as_deref()
        || draft
            .chapter_drafts
            .values()
            .any(|chapter| chapter.normalized_xhtml != chapter.saved_xhtml);
    Ok(EpubDraftValidation {
        errors: Vec::new(),
        warnings: Vec::new(),
        information: vec![validation_issue(
            "DRAFT_STRUCTURE_VALID",
            "元数据、章节 XHTML、OPF 和资源覆盖层已通过保存前检查。",
            EpubValidationSeverity::Information,
        )],
        can_save: dirty && !draft.saving,
    })
}

fn apply_metadata_patch(metadata: &mut EpubMetadataDraft, patch: EpubMetadataPatch) {
    if let Some(value) = patch.title {
        metadata.title = value;
    }
    if let Some(value) = patch.creators {
        metadata.creators = value;
    }
    if let Some(value) = patch.contributors {
        metadata.contributors = value;
    }
    if let Some(value) = patch.language {
        metadata.language = value;
    }
    if let Some(value) = patch.publisher {
        metadata.publisher = value;
    }
    if let Some(value) = patch.description {
        metadata.description = value;
    }
    if let Some(value) = patch.identifier {
        metadata.identifier = value;
    }
    if let Some(value) = patch.publication_date {
        metadata.publication_date = value;
    }
    if let Some(value) = patch.subjects {
        metadata.subjects = value;
    }
    if let Some(value) = patch.rights {
        metadata.rights = value;
    }
}

fn normalize_metadata(metadata: &mut EpubMetadataDraft) {
    metadata.title = metadata.title.trim().to_owned();
    metadata.identifier = metadata.identifier.trim().to_owned();
    metadata.language = metadata.language.trim().to_owned();
    normalize_list(&mut metadata.creators);
    normalize_list(&mut metadata.contributors);
    normalize_list(&mut metadata.subjects);
    normalize_list(&mut metadata.rights);
    normalize_optional(&mut metadata.publisher);
    normalize_optional(&mut metadata.description);
    normalize_optional(&mut metadata.publication_date);
}

fn normalize_list(values: &mut Vec<String>) {
    *values = values
        .drain(..)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect();
}

fn normalize_optional(value: &mut Option<String>) {
    *value = value
        .take()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
}

fn validate_metadata(metadata: &EpubMetadataDraft) -> Result<(), AppError> {
    validate_text("title", &metadata.title, 512, false)?;
    if metadata.title.is_empty() {
        return Err(AppError::validation(
            "INVALID_METADATA",
            "书名不能为空。",
            "请输入书名后重试。",
        ));
    }
    validate_text("identifier", &metadata.identifier, 1024, false)?;
    if metadata.identifier.is_empty() {
        return Err(AppError::validation(
            "INVALID_IDENTIFIER",
            "EPUB identifier 不能为空。",
            "保留原 identifier，或输入新的非空标识符。",
        ));
    }
    if !is_basic_bcp47(&metadata.language) {
        return Err(AppError::validation(
            "INVALID_LANGUAGE_TAG",
            "语言标签不是有效的基础 BCP 47 格式。",
            "请输入例如 zh-CN、en 或 ja-JP。",
        ));
    }
    for value in metadata
        .creators
        .iter()
        .chain(&metadata.contributors)
        .chain(&metadata.subjects)
        .chain(&metadata.rights)
    {
        validate_text("list", value, 512, false)?;
    }
    if metadata.creators.len() > 64
        || metadata.contributors.len() > 64
        || metadata.subjects.len() > 256
        || metadata.rights.len() > 64
    {
        return Err(invalid_metadata());
    }
    for value in [
        metadata.publisher.as_deref(),
        metadata.publication_date.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        validate_text("optional", value, 512, false)?;
    }
    if let Some(description) = &metadata.description {
        validate_text("description", description, 16_384, true)?;
    }
    Ok(())
}

fn validate_text(_: &str, value: &str, maximum: usize, multiline: bool) -> Result<(), AppError> {
    if value.chars().count() > maximum
        || value.chars().any(|character| {
            character.is_control() && !(multiline && matches!(character, '\n' | '\r' | '\t'))
        })
    {
        return Err(invalid_metadata());
    }
    Ok(())
}

fn is_basic_bcp47(value: &str) -> bool {
    if value.len() > 63 || value.is_empty() {
        return false;
    }
    let mut parts = value.split('-');
    let Some(primary) = parts.next() else {
        return false;
    };
    (2..=8).contains(&primary.len())
        && primary.bytes().all(|byte| byte.is_ascii_alphabetic())
        && parts.all(|part| {
            (1..=8).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
}

fn unique_cover_names(
    draft: &PublicationDraft,
    cover: &ValidatedCover,
) -> Result<(String, String), AppError> {
    let opf_parent = draft
        .source_publication
        .package_resource_id
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or_default();
    let hash = &cover.content_hash[..12];
    let mut suffix = 0_u32;
    loop {
        let discriminator = if suffix == 0 {
            String::new()
        } else {
            format!("-{suffix}")
        };
        let relative = format!(
            "readloom-assets/cover-{hash}{discriminator}.{}",
            cover.extension
        );
        let resource_id = if opf_parent.is_empty() {
            relative
        } else {
            format!("{opf_parent}/{relative}")
        };
        SafeArchivePath::parse(&resource_id)?;
        let item_id = format!("readloom-cover-{hash}{discriminator}");
        let resource_conflict = draft
            .source_publication
            .manifest
            .iter()
            .any(|item| item.resource_id.eq_ignore_ascii_case(&resource_id));
        let id_conflict = draft
            .source_publication
            .manifest
            .iter()
            .any(|item| item.id == item_id);
        if !resource_conflict && !id_conflict {
            return Ok((resource_id, item_id));
        }
        suffix = suffix.checked_add(1).ok_or_else(|| {
            AppError::validation(
                "UNSAFE_COVER_PATH",
                "无法为新封面生成安全且唯一的内部路径。",
                "请重新打开 EPUB 后重试。",
            )
        })?;
    }
}

fn append_cover_document_overlays(
    draft: &PublicationDraft,
    cover: &DraftCover,
    overlays: &mut HashMap<String, Vec<u8>>,
    limits: ArchiveLimits,
) -> Result<(), AppError> {
    let Some(old_cover) = draft.source_publication.cover_resource_id.as_deref() else {
        return Ok(());
    };
    let archive = SafeEpubArchive::open(&draft.source_path, limits)?;
    for item in draft.source_publication.manifest.iter().filter(|item| {
        matches!(
            item.media_type.as_str(),
            "application/xhtml+xml" | "text/html"
        ) && (item.id.to_ascii_lowercase().contains("cover")
            || item.resource_id.to_ascii_lowercase().contains("cover")
            || item.properties.iter().any(|property| property == "cover"))
    }) {
        let source = archive.read(
            &SafeArchivePath::parse(&item.resource_id)?,
            ResourceClass::Xhtml,
        )?;
        if let Some(updated) =
            patch_cover_reference(&source, &item.resource_id, old_cover, &cover.resource_id)?
        {
            overlays.insert(item.resource_id.clone(), updated);
        }
    }
    Ok(())
}

fn chapter_from_analysis(
    chapter_edit_id: String,
    spine_index: usize,
    manifest_item_id: String,
    chapter_href: String,
    chapter_title: String,
    analysis: AnalyzedChapter,
) -> ChapterEditDraft {
    ChapterEditDraft {
        chapter_edit_id,
        spine_index,
        manifest_item_id,
        chapter_href,
        chapter_title,
        original_resource_hash: analysis.original_resource_hash,
        original_xhtml: analysis.original_xhtml.clone(),
        preserved_outer_document: analysis.preserved_outer_document,
        editor_document: analysis.editor_document,
        normalized_xhtml: analysis.original_xhtml.clone(),
        saved_xhtml: analysis.original_xhtml,
        compatibility_level: analysis.compatibility_level,
        warnings: analysis.warnings,
        draft_revision: 0,
        accepted_revision: 0,
        preview_revision: 0,
        validation_state: analysis.validation_state,
    }
}

fn chapter_dto(publication: &PublicationDraft, chapter: &ChapterEditDraft) -> ChapterEditDto {
    let can_edit = matches!(
        chapter.compatibility_level,
        ChapterCompatibilityLevel::Full | ChapterCompatibilityLevel::Limited
    );
    ChapterEditDto {
        chapter_edit_id: chapter.chapter_edit_id.clone(),
        edit_session_id: publication.edit_session_id.clone(),
        document_id: publication.document_id.clone(),
        spine_index: chapter.spine_index,
        manifest_item_id: chapter.manifest_item_id.clone(),
        chapter_href: chapter.chapter_href.clone(),
        chapter_title: chapter.chapter_title.clone(),
        original_resource_hash: chapter.original_resource_hash.clone(),
        editor_document: chapter.editor_document.clone(),
        compatibility_level: chapter.compatibility_level,
        warnings: chapter.warnings.clone(),
        revision: chapter.draft_revision,
        accepted_revision: chapter.accepted_revision,
        dirty: chapter.normalized_xhtml != chapter.saved_xhtml,
        validation_state: chapter.validation_state,
        preview_revision: chapter.preview_revision,
        capabilities: ChapterEditCapabilities {
            can_edit,
            can_format: can_edit,
            can_edit_links: can_edit,
            can_import_images: can_edit,
            can_preview: true,
            can_revert: can_edit && chapter.normalized_xhtml != chapter.original_xhtml,
        },
    }
}

fn publication_for_chapter(state: &EditState, chapter_edit_id: &str) -> Result<String, AppError> {
    state
        .drafts
        .iter()
        .find(|(_, publication)| {
            publication
                .chapter_drafts
                .values()
                .any(|chapter| chapter.chapter_edit_id == chapter_edit_id)
        })
        .map(|(key, _)| key.clone())
        .ok_or_else(chapter_draft_expired)
}

fn manifest_resource_ids(draft: &PublicationDraft) -> HashSet<String> {
    draft
        .source_publication
        .manifest
        .iter()
        .map(|item| item.resource_id.clone())
        .chain(draft.imported_images.keys().cloned())
        .collect()
}

fn chapter_title(
    publication: &ParsedEpubDocument,
    resource_id: &str,
    spine_index: usize,
) -> String {
    fn find(nodes: &[crate::domain::epub_document::TocNode], resource_id: &str) -> Option<String> {
        for node in nodes {
            if node.resource_id.as_deref() == Some(resource_id) && !node.label.trim().is_empty() {
                return Some(node.label.clone());
            }
            if let Some(label) = find(&node.children, resource_id) {
                return Some(label);
            }
        }
        None
    }
    find(&publication.toc, resource_id).unwrap_or_else(|| format!("第 {} 章", spine_index + 1))
}

fn ensure_chapter_editable(chapter: &ChapterEditDraft) -> Result<(), AppError> {
    match chapter.compatibility_level {
        ChapterCompatibilityLevel::Full | ChapterCompatibilityLevel::Limited => Ok(()),
        ChapterCompatibilityLevel::ReadOnly => Err(AppError::validation(
            "CHAPTER_READ_ONLY",
            format!("章节“{}”为只读兼容模式。", chapter.chapter_title),
            "此章节包含无法保证无损往返的结构，请继续使用安全阅读模式。",
        )),
        ChapterCompatibilityLevel::Unsupported => Err(chapter_not_supported()),
    }
}

fn refresh_imported_image_references(draft: &mut PublicationDraft) {
    for image in draft.imported_images.values_mut() {
        let needle = image
            .resource_id
            .rsplit('/')
            .next()
            .unwrap_or(image.resource_id.as_str())
            .as_bytes();
        image.referenced = draft.chapter_drafts.values().any(|chapter| {
            chapter
                .normalized_xhtml
                .windows(needle.len().max(1))
                .any(|window| window == needle)
        });
    }
}

fn unique_chapter_image_names(
    draft: &PublicationDraft,
    image: &ValidatedCover,
) -> Result<(String, String), AppError> {
    let extension = match image.media_type.as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => return Err(image_import_failed()),
    };
    let opf_parent = draft
        .source_publication
        .package_resource_id
        .rsplit_once('/')
        .map_or("", |(parent, _)| parent);
    let digest = &image.content_hash[..image.content_hash.len().min(16)];
    for suffix in 0_u32..10_000 {
        let stem = if suffix == 0 {
            format!("readloom-{digest}")
        } else {
            format!("readloom-{digest}-{suffix}")
        };
        let resource_id = if opf_parent.is_empty() {
            format!("images/{stem}.{extension}")
        } else {
            format!("{opf_parent}/images/{stem}.{extension}")
        };
        SafeArchivePath::parse(&resource_id)?;
        let item_id = format!("readloom-image-{stem}");
        let resource_conflict = draft
            .source_publication
            .manifest
            .iter()
            .any(|item| item.resource_id.eq_ignore_ascii_case(&resource_id))
            || draft.imported_images.contains_key(&resource_id);
        let id_conflict = draft
            .source_publication
            .manifest
            .iter()
            .any(|item| item.id == item_id)
            || draft
                .imported_images
                .values()
                .any(|image| image.item_id == item_id);
        if !resource_conflict && !id_conflict {
            return Ok((resource_id, item_id));
        }
    }
    Err(image_manifest_update_failed())
}

fn verify_saved_chapters(
    draft: &PublicationDraft,
    generated_path: &Path,
    limits: ArchiveLimits,
) -> Result<(), AppError> {
    let archive = SafeEpubArchive::open(generated_path, limits)?;
    for chapter in draft
        .chapter_drafts
        .values()
        .filter(|chapter| chapter.normalized_xhtml != chapter.original_xhtml)
    {
        let body = archive.read(
            &SafeArchivePath::parse(&chapter.chapter_href)?,
            ResourceClass::Xhtml,
        )?;
        if body != chapter.normalized_xhtml {
            return Err(modified_chapter_validation_failed(&chapter.chapter_title));
        }
    }
    Ok(())
}

fn normalize_target(path: &Path) -> Result<PathBuf, AppError> {
    if !path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("epub"))
    {
        return Err(AppError::validation(
            "INVALID_EPUB",
            "另存为目标必须使用 .epub 扩展名。",
            "请选择以 .epub 结尾的文件名。",
        ));
    }
    let parent = path.parent().ok_or_else(invalid_target)?;
    let parent = fs::canonicalize(parent).map_err(|_| invalid_target())?;
    if !parent.is_dir() {
        return Err(invalid_target());
    }
    let file_name = path.file_name().ok_or_else(invalid_target)?;
    Ok(parent.join(file_name))
}

fn ensure_target_is_not_source(source: &Path, target: &Path) -> Result<(), AppError> {
    let equal = if target.exists() {
        fs::canonicalize(target)
            .ok()
            .is_some_and(|canonical| canonical == source)
    } else {
        source
            .to_string_lossy()
            .eq_ignore_ascii_case(&target.to_string_lossy())
    };
    if equal {
        return Err(AppError::validation(
            "TARGET_EQUALS_SOURCE",
            "安全另存为不能覆盖原 EPUB。",
            "请选择与原书不同的目标路径。",
        ));
    }
    Ok(())
}

fn verify_source_unchanged(draft: &PublicationDraft) -> Result<(), AppError> {
    let current = fingerprint_file(&draft.source_path).map_err(|_| source_modified_externally())?;
    if current != draft.source_fingerprint {
        return Err(source_modified_externally());
    }
    Ok(())
}

fn validate_generated_publication(
    draft: &PublicationDraft,
    generated: &ParsedEpubDocument,
) -> Result<(), AppError> {
    if generated.package_resource_id != draft.source_publication.package_resource_id
        || generated.spine != draft.source_publication.spine
        || generated.toc != draft.source_publication.toc
    {
        return Err(round_trip_mismatch());
    }
    let expected_manifest_count = draft.source_publication.manifest.len()
        + usize::from(draft.cover_change.is_some())
        + draft
            .imported_images
            .values()
            .filter(|image| image.referenced)
            .count();
    if generated.manifest.len() != expected_manifest_count {
        return Err(round_trip_mismatch());
    }
    let expected = &draft.metadata;
    let actual = &generated.metadata;
    if actual.title != expected.title
        || actual.creators != expected.creators
        || actual.contributors != expected.contributors
        || actual.languages.first() != Some(&expected.language)
        || actual.publisher != expected.publisher
        || actual.description != expected.description
        || actual.identifier.as_deref() != Some(expected.identifier.as_str())
        || actual.publication_date != expected.publication_date
        || actual.subjects != expected.subjects
        || actual.rights != expected.rights
    {
        return Err(round_trip_mismatch());
    }
    if let Some(cover) = &draft.cover_change
        && generated.cover_resource_id.as_deref() != Some(cover.resource_id.as_str())
    {
        return Err(round_trip_mismatch());
    }
    Ok(())
}

fn advance_revision(draft: &mut PublicationDraft) -> Result<(), AppError> {
    draft.revision = draft.revision.checked_add(1).ok_or_else(draft_conflict)?;
    draft.updated_at_ms = now_ms()?;
    Ok(())
}

fn ensure_revision(draft: &PublicationDraft, expected: u64) -> Result<(), AppError> {
    if draft.revision == expected {
        Ok(())
    } else {
        Err(draft_conflict())
    }
}

fn ensure_not_saving(draft: &PublicationDraft) -> Result<(), AppError> {
    if draft.saving {
        Err(AppError::validation(
            "EPUB_DRAFT_CONFLICT",
            "此 EPUB 草稿正在另存为。",
            "等待保存完成或取消保存后再试。",
        ))
    } else {
        Ok(())
    }
}

fn validation_issue(
    code: impl Into<String>,
    message: impl Into<String>,
    severity: EpubValidationSeverity,
) -> EpubValidationIssue {
    EpubValidationIssue {
        code: code.into(),
        message: message.into(),
        severity,
    }
}

fn random_token(length: usize) -> Result<String, AppError> {
    let mut bytes = vec![0_u8; length];
    getrandom::fill(&mut bytes)
        .map_err(|_| AppError::internal("INTERNAL", "generate EPUB edit token"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn now_ms() -> Result<u64, AppError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AppError::internal("INTERNAL", "read system time"))?
        .as_millis()
        .try_into()
        .map_err(|_| AppError::internal("INTERNAL", "convert system time"))
}

fn rfc3339_now() -> Result<String, AppError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AppError::internal("INTERNAL", "read system time"))?
        .as_secs();
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;
    let second = seconds_of_day % 60;
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
    ))
}

fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month as u32, day as u32)
}

fn editing_not_supported() -> AppError {
    AppError::validation(
        "EPUB_EDITING_NOT_SUPPORTED",
        "此 EPUB 无法安全编辑。",
        "加密、DRM、固定布局或结构异常的 EPUB 保持只读。",
    )
}

fn edit_session_not_found() -> AppError {
    AppError::validation(
        "EPUB_EDIT_SESSION_NOT_FOUND",
        "EPUB 编辑草稿已关闭或失效。",
        "重新打开书籍信息面板后再试。",
    )
}

fn draft_conflict() -> AppError {
    AppError::validation(
        "EPUB_DRAFT_CONFLICT",
        "EPUB 草稿版本已变化。",
        "刷新书籍信息后重试。",
    )
}

fn invalid_metadata() -> AppError {
    AppError::validation(
        "INVALID_METADATA",
        "EPUB 元数据包含无效或过长的文本。",
        "缩短内容并移除控制字符后重试。",
    )
}

fn source_modified_externally() -> AppError {
    AppError::validation(
        "SOURCE_MODIFIED_EXTERNALLY",
        "原 EPUB 已被其他程序修改，Readloom 已停止另存为。",
        "请重新加载原书、放弃草稿或取消操作。",
    )
}

fn target_already_exists() -> AppError {
    AppError::validation(
        "TARGET_ALREADY_EXISTS",
        "另存为目标已经存在。",
        "确认覆盖后重新提交；原文件不会被覆盖。",
    )
}

fn overwrite_confirmation_expired() -> AppError {
    AppError::validation(
        "OVERWRITE_CONFIRMATION_EXPIRED",
        "覆盖确认已失效或与当前目标不匹配。",
        "请重新确认覆盖。",
    )
}

fn draft_validation_failed() -> AppError {
    AppError::validation(
        "DRAFT_VALIDATION_FAILED",
        "EPUB 草稿未通过保存前检查，或当前没有修改。",
        "修正元数据/封面问题后重试。",
    )
}

fn generated_epub_invalid() -> AppError {
    AppError::validation(
        "GENERATED_EPUB_INVALID",
        "生成的 EPUB 未通过内部重新打开检查。",
        "目标没有被替换；请保留草稿并重试。",
    )
}

fn round_trip_mismatch() -> AppError {
    AppError::validation(
        "ROUND_TRIP_MISMATCH",
        "生成 EPUB 的结构或修改结果与草稿不一致。",
        "目标没有被替换；请保留草稿并重试。",
    )
}

fn invalid_target() -> AppError {
    AppError::validation(
        "TEMPORARY_OUTPUT_FAILED",
        "另存为目标目录无效或不可访问。",
        "请选择现有且可写的目录。",
    )
}

fn chapter_not_found() -> AppError {
    AppError::validation(
        "INVALID_INTERNAL_RESOURCE",
        "所选章节不在当前 EPUB 的 spine 中。",
        "返回目录并重新选择章节。",
    )
}

fn chapter_not_supported() -> AppError {
    AppError::validation(
        "CHAPTER_EDITING_NOT_SUPPORTED",
        "当前章节无法安全进入可视化编辑。",
        "继续使用阅读模式；Readloom 不会静默删除不支持结构。",
    )
}

fn invalid_chapter_request() -> AppError {
    AppError::validation(
        "INVALID_CHAPTER_DRAFT",
        "章节草稿同步请求无效。",
        "当前编辑内容仍保留在编辑器中，请重试。",
    )
}

fn chapter_revision_conflict() -> AppError {
    AppError::validation(
        "CHAPTER_REVISION_CONFLICT",
        "章节草稿版本已经变化，旧同步请求已被拒绝。",
        "Readloom 将保留较新的编辑内容并重新同步。",
    )
}

fn chapter_draft_expired() -> AppError {
    AppError::validation(
        "CHAPTER_DRAFT_EXPIRED",
        "章节编辑草稿已经关闭或失效。",
        "重新进入此章节的编辑模式。",
    )
}

fn image_import_failed() -> AppError {
    AppError::validation(
        "IMAGE_IMPORT_FAILED",
        "无法导入章节图片；内容或尺寸校验未通过。",
        "请选择有效且大小适中的 PNG、JPEG 或 WebP 图片。",
    )
}

fn image_manifest_update_failed() -> AppError {
    AppError::validation(
        "IMAGE_MANIFEST_UPDATE_FAILED",
        "无法为章节图片生成唯一的 EPUB manifest 项。",
        "重新打开 EPUB 后再试。",
    )
}

fn modified_chapter_validation_failed(chapter_title: &str) -> AppError {
    AppError::validation(
        "MODIFIED_CHAPTER_VALIDATION_FAILED",
        format!("章节“{chapter_title}”在重新打包后未通过内容校验。"),
        "生成文件未提交到目标；全部章节草稿仍然保留。",
    )
}

#[cfg(test)]
mod tests {
    use std::{io::Write, time::Instant};

    use tempfile::tempdir;

    use super::*;
    use crate::epub_test_fixtures::{
        epub3_with_padding, minimal_epub2, minimal_epub3, minimal_epub3_two_chapters,
    };

    fn service() -> (
        crate::epub_test_fixtures::EpubFixture,
        EpubDocumentService,
        EpubEditService,
    ) {
        let fixture = minimal_epub3();
        let documents = EpubDocumentService::new(ArchiveLimits::default());
        let edits = EpubEditService::new(ArchiveLimits::default(), documents.clone());
        (fixture, documents, edits)
    }

    #[test]
    fn edits_multiple_chapters_rejects_stale_updates_and_saves_every_overlay() {
        let fixture = minimal_epub3_two_chapters();
        let source_fingerprint = fingerprint_file(fixture.path()).unwrap();
        let documents = EpubDocumentService::new(ArchiveLimits::default());
        let edits = EpubEditService::new(ArchiveLimits::default(), documents.clone());
        let opened = documents.open(fixture.path()).unwrap();
        let publication = edits.begin(&opened.document_id).unwrap();
        let first = edits
            .begin_chapter_edit(&publication.edit_session_id, 0)
            .unwrap();
        let second = edits
            .begin_chapter_edit(&publication.edit_session_id, 1)
            .unwrap();

        let first_accepted = edits
            .update_chapter_draft(ChapterDraftUpdate {
                chapter_edit_id: first.chapter_edit_id.clone(),
                base_revision: 0,
                client_revision: 1,
                editor_document: serde_json::json!({
                    "type":"doc",
                    "content":[
                        {"type":"heading","attrs":{"level":1},"content":[{"type":"text","text":"第一章（已编辑）"}]},
                        {"type":"paragraph","content":[{"type":"text","text":"中文 😀 甲","marks":[{"type":"bold"}]}]}
                    ]
                }),
                request_id: "first-1".to_owned(),
            })
            .unwrap();
        assert!(first_accepted.dirty);
        let stale = edits
            .update_chapter_draft(ChapterDraftUpdate {
                chapter_edit_id: first.chapter_edit_id.clone(),
                base_revision: 0,
                client_revision: 2,
                editor_document: first.editor_document.clone(),
                request_id: "stale-2".to_owned(),
            })
            .unwrap_err();
        assert_eq!(stale.to_dto().code, "CHAPTER_REVISION_CONFLICT");

        let second_accepted = edits
            .update_chapter_draft(ChapterDraftUpdate {
                chapter_edit_id: second.chapter_edit_id.clone(),
                base_revision: 0,
                client_revision: 1,
                editor_document: serde_json::json!({
                    "type":"doc",
                    "content":[
                        {"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":"第二章（已编辑）"}]},
                        {"type":"orderedList","attrs":{"start":1},"content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"乙"}]}]}]}
                    ]
                }),
                request_id: "second-1".to_owned(),
            })
            .unwrap();
        assert_eq!(second_accepted.publication_revision, 2);
        let draft = edits.get(&publication.edit_session_id).unwrap();
        assert_eq!(draft.changes.modified_chapters, [0, 1]);

        let directory = tempdir().unwrap();
        let target = directory.path().join("two-edited.epub");
        let saved = edits
            .save_as(&publication.edit_session_id, draft.revision, &target, None)
            .unwrap();
        assert!(!saved.draft.dirty);
        assert_eq!(
            fingerprint_file(fixture.path()).unwrap(),
            source_fingerprint
        );
        let archive = SafeEpubArchive::open(&target, ArchiveLimits::default()).unwrap();
        let first_saved = String::from_utf8(
            archive
                .read(
                    &SafeArchivePath::parse("EPUB/one.xhtml").unwrap(),
                    ResourceClass::Xhtml,
                )
                .unwrap(),
        )
        .unwrap();
        let second_saved = String::from_utf8(
            archive
                .read(
                    &SafeArchivePath::parse("EPUB/two.xhtml").unwrap(),
                    ResourceClass::Xhtml,
                )
                .unwrap(),
        )
        .unwrap();
        assert!(first_saved.contains("第一章（已编辑）"));
        assert!(first_saved.contains("<strong>中文 😀 甲</strong>"));
        assert!(second_saved.contains("第二章（已编辑）"));
        assert!(second_saved.contains("<ol start=\"1\">"));

        assert!(
            !edits
                .validate_chapter_draft(&first.chapter_edit_id)
                .unwrap()
                .dirty
        );
        let reverted = edits.revert_chapter_draft(&first.chapter_edit_id).unwrap();
        assert!(reverted.dirty);
        assert_eq!(
            edits
                .get(&publication.edit_session_id)
                .unwrap()
                .changes
                .modified_chapters,
            [0]
        );
    }

    #[test]
    fn imports_referenced_chapter_images_into_manifest_and_saved_archive() {
        let fixture = minimal_epub3_two_chapters();
        let documents = EpubDocumentService::new(ArchiveLimits::default());
        let edits = EpubEditService::new(ArchiveLimits::default(), documents.clone());
        let opened = documents.open(fixture.path()).unwrap();
        let publication = edits.begin(&opened.document_id).unwrap();
        let chapter = edits
            .begin_chapter_edit(&publication.edit_session_id, 0)
            .unwrap();
        let directory = tempdir().unwrap();
        let image_path = directory.path().join("章节插图.png");
        let mut png = vec![0_u8; 33];
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        png[12..16].copy_from_slice(b"IHDR");
        png[16..20].copy_from_slice(&320_u32.to_be_bytes());
        png[20..24].copy_from_slice(&240_u32.to_be_bytes());
        fs::write(&image_path, &png).unwrap();

        let imported = edits
            .import_chapter_image(
                &publication.edit_session_id,
                &chapter.chapter_edit_id,
                &image_path,
            )
            .unwrap();
        assert_eq!(imported.media_type, "image/png");
        assert!(imported.resource_id.starts_with("EPUB/images/readloom-"));
        assert_eq!(
            edits
                .get(&publication.edit_session_id)
                .unwrap()
                .changes
                .added_resources,
            0,
            "unreferenced imports must not be written"
        );

        edits
            .update_chapter_draft(ChapterDraftUpdate {
                chapter_edit_id: chapter.chapter_edit_id,
                base_revision: 0,
                client_revision: 1,
                editor_document: serde_json::json!({
                    "type":"doc",
                    "content":[
                        {"type":"heading","attrs":{"level":1},"content":[{"type":"text","text":"第一章"}]},
                        {"type":"image","attrs":{"src":imported.editor_src,"alt":"章节插图","width":"320","height":"240"}}
                    ]
                }),
                request_id: "image-1".to_owned(),
            })
            .unwrap();
        let draft = edits.get(&publication.edit_session_id).unwrap();
        assert_eq!(draft.changes.added_resources, 1);
        let target = directory.path().join("image-edited.epub");
        edits
            .save_as(&publication.edit_session_id, draft.revision, &target, None)
            .unwrap();

        let archive = SafeEpubArchive::open(&target, ArchiveLimits::default()).unwrap();
        assert_eq!(
            archive
                .read(
                    &SafeArchivePath::parse(&imported.resource_id).unwrap(),
                    ResourceClass::Image,
                )
                .unwrap(),
            png
        );
        let opf = String::from_utf8(
            archive
                .read(
                    &SafeArchivePath::parse("EPUB/package.opf").unwrap(),
                    ResourceClass::Xml,
                )
                .unwrap(),
        )
        .unwrap();
        assert!(opf.contains("href=\"images/readloom-"));
        assert!(opf.contains("media-type=\"image/png\""));
        let xhtml = String::from_utf8(
            archive
                .read(
                    &SafeArchivePath::parse("EPUB/one.xhtml").unwrap(),
                    ResourceClass::Xhtml,
                )
                .unwrap(),
        )
        .unwrap();
        assert!(xhtml.contains("src=\"images/readloom-"));
        assert!(xhtml.contains("alt=\"章节插图\""));
    }

    #[test]
    #[ignore = "run explicitly in release mode for the EPUB chapter editing performance record"]
    fn epub_chapter_edit_release_performance_probe() {
        const SYNCHRONIZATIONS: u64 = 40;
        const PARAGRAPHS: usize = 250;

        fn performance_document(revision: u64) -> serde_json::Value {
            let content = (0..PARAGRAPHS)
                .map(|index| {
                    serde_json::json!({
                        "type":"paragraph",
                        "content":[{
                            "type":"text",
                            "text":format!("第 {index} 段 · 修订 {revision} · {}", "中文😀é".repeat(32))
                        }]
                    })
                })
                .collect::<Vec<_>>();
            serde_json::json!({"type":"doc","content":content})
        }

        let fixture = minimal_epub3_two_chapters();
        let source_bytes = fs::metadata(fixture.path()).unwrap().len();
        let documents = EpubDocumentService::new(ArchiveLimits::default());
        let edits = EpubEditService::new(ArchiveLimits::default(), documents.clone());
        let opened = documents.open(fixture.path()).unwrap();
        let publication_started = Instant::now();
        let publication = edits.begin(&opened.document_id).unwrap();
        let publication_begin_us = publication_started.elapsed().as_micros();
        let chapter_started = Instant::now();
        let chapter = edits
            .begin_chapter_edit(&publication.edit_session_id, 0)
            .unwrap();
        let chapter_begin_us = chapter_started.elapsed().as_micros();

        let mut sync_total_us = 0_u128;
        let mut editor_json_bytes = 0_usize;
        for revision in 1..=SYNCHRONIZATIONS {
            let document = performance_document(revision);
            editor_json_bytes = serde_json::to_vec(&document).unwrap().len();
            let sync_started = Instant::now();
            edits
                .update_chapter_draft(ChapterDraftUpdate {
                    chapter_edit_id: chapter.chapter_edit_id.clone(),
                    base_revision: revision - 1,
                    client_revision: revision,
                    editor_document: document,
                    request_id: format!("perf-{revision}"),
                })
                .unwrap();
            sync_total_us += sync_started.elapsed().as_micros();
        }

        let draft = edits.get(&publication.edit_session_id).unwrap();
        let directory = tempdir().unwrap();
        let target = directory.path().join("chapter-performance.epub");
        let save_started = Instant::now();
        edits
            .save_as(&publication.edit_session_id, draft.revision, &target, None)
            .unwrap();
        let save_us = save_started.elapsed().as_micros();
        let output_bytes = fs::metadata(target).unwrap().len();
        println!(
            "EPUB_CHAPTER_EDIT_PERF_JSON:{}",
            serde_json::json!({
                "source_bytes": source_bytes,
                "editor_json_bytes": editor_json_bytes,
                "publication_begin_us": publication_begin_us,
                "chapter_begin_us": chapter_begin_us,
                "sync_runs": SYNCHRONIZATIONS,
                "sync_average_us": sync_total_us / u128::from(SYNCHRONIZATIONS),
                "save_as_us": save_us,
                "output_bytes": output_bytes,
                "debounce_budget_ms": 550,
                "maximum_sync_payload_bytes": EpubChapterEditLimits::default().maximum_sync_bytes,
            })
        );
    }

    #[test]
    fn creates_a_draft_lazily_and_structured_dirty_can_return_to_clean() {
        let (fixture, documents, edits) = service();
        let opened = documents.open(fixture.path()).unwrap();
        let draft = edits.begin(&opened.document_id).unwrap();
        assert!(!draft.dirty);
        assert_eq!(draft.metadata.title, "阅织 EPUB 3 测试");

        let changed = edits
            .update_metadata(
                &draft.edit_session_id,
                draft.revision,
                EpubMetadataPatch {
                    title: Some("新书名".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(changed.dirty);
        assert_eq!(changed.changes.metadata_fields, ["title"]);

        let reverted = edits
            .update_metadata(
                &draft.edit_session_id,
                changed.revision,
                EpubMetadataPatch {
                    title: Some("阅织 EPUB 3 测试".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(!reverted.dirty);
    }

    #[test]
    fn rejects_invalid_language_and_stale_revisions_without_losing_the_draft() {
        let (fixture, documents, edits) = service();
        let opened = documents.open(fixture.path()).unwrap();
        let draft = edits.begin(&opened.document_id).unwrap();
        let error = edits
            .update_metadata(
                &draft.edit_session_id,
                draft.revision,
                EpubMetadataPatch {
                    language: Some("not_a_tag".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert_eq!(error.to_dto().code, "INVALID_LANGUAGE_TAG");
        assert!(!edits.get(&draft.edit_session_id).unwrap().dirty);
    }

    #[test]
    fn save_as_reopens_the_generated_epub_and_never_changes_the_source() {
        let (fixture, documents, edits) = service();
        let source_before = fingerprint_file(fixture.path()).unwrap();
        let opened = documents.open(fixture.path()).unwrap();
        let draft = edits.begin(&opened.document_id).unwrap();
        let changed = edits
            .update_metadata(
                &draft.edit_session_id,
                draft.revision,
                EpubMetadataPatch {
                    title: Some("另存的新书名 & <安全>".to_owned()),
                    creators: Some(vec!["作者甲".to_owned(), "作者乙".to_owned()]),
                    ..Default::default()
                },
            )
            .unwrap();
        let directory = tempdir().unwrap();
        let target = directory.path().join("新书.epub");
        let saved = edits
            .save_as(&draft.edit_session_id, changed.revision, &target, None)
            .unwrap();

        assert_eq!(saved.document.metadata.title, "另存的新书名 & <安全>");
        assert_eq!(saved.document.metadata.creators, ["作者甲", "作者乙"]);
        assert!(!saved.draft.dirty);
        assert_eq!(fingerprint_file(fixture.path()).unwrap(), source_before);
        assert!(target.exists());
    }

    #[test]
    fn blocks_source_overwrite_and_external_source_changes() {
        let (fixture, documents, edits) = service();
        let opened = documents.open(fixture.path()).unwrap();
        let draft = edits.begin(&opened.document_id).unwrap();
        let changed = edits
            .update_metadata(
                &draft.edit_session_id,
                draft.revision,
                EpubMetadataPatch {
                    title: Some("改变".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        let error = edits
            .save_as(
                &draft.edit_session_id,
                changed.revision,
                fixture.path(),
                None,
            )
            .unwrap_err();
        assert_eq!(error.to_dto().code, "TARGET_EQUALS_SOURCE");

        let directory = tempdir().unwrap();
        let existing_target = directory.path().join("existing.epub");
        fs::write(&existing_target, b"existing target must survive").unwrap();
        let token = edits
            .prepare_overwrite(&draft.edit_session_id, changed.revision, &existing_target)
            .unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(fixture.path())
            .unwrap()
            .write_all(b"external-change")
            .unwrap();
        let error = edits
            .save_as(
                &draft.edit_session_id,
                changed.revision,
                &existing_target,
                Some(&token),
            )
            .unwrap_err();
        assert_eq!(error.to_dto().code, "SOURCE_MODIFIED_EXTERNALLY");
        assert_eq!(
            fs::read(&existing_target).unwrap(),
            b"existing target must survive"
        );
        assert!(edits.get(&draft.edit_session_id).unwrap().dirty);
    }

    #[test]
    fn replaces_a_validated_cover_and_reopens_it_from_the_generated_manifest() {
        let (fixture, documents, edits) = service();
        let opened = documents.open(fixture.path()).unwrap();
        let draft = edits.begin(&opened.document_id).unwrap();
        let directory = tempdir().unwrap();
        let cover_path = directory.path().join("新封面.png");
        let mut png = vec![0_u8; 33];
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        png[12..16].copy_from_slice(b"IHDR");
        png[16..20].copy_from_slice(&600_u32.to_be_bytes());
        png[20..24].copy_from_slice(&800_u32.to_be_bytes());
        fs::write(&cover_path, &png).unwrap();

        let changed = edits
            .replace_cover(&draft.edit_session_id, draft.revision, &cover_path)
            .unwrap();
        assert!(changed.dirty);
        assert_eq!(changed.cover.width, Some(600));
        assert_eq!(changed.cover.height, Some(800));
        assert!(
            changed
                .cover
                .preview_resource_id
                .as_deref()
                .unwrap()
                .starts_with("__readloom_edit/")
        );

        let target = directory.path().join("带封面.epub");
        let saved = edits
            .save_as(&draft.edit_session_id, changed.revision, &target, None)
            .unwrap();
        let resource_id = saved.document.cover_resource_id.unwrap();
        assert!(resource_id.contains("readloom-assets/cover-"));
        let archive = SafeEpubArchive::open(&target, ArchiveLimits::default()).unwrap();
        assert_eq!(
            archive
                .read(
                    &SafeArchivePath::parse(&resource_id).unwrap(),
                    ResourceClass::Image
                )
                .unwrap(),
            png
        );
    }

    #[test]
    fn existing_targets_require_a_revision_bound_single_use_confirmation() {
        let (fixture, documents, edits) = service();
        let opened = documents.open(fixture.path()).unwrap();
        let draft = edits.begin(&opened.document_id).unwrap();
        let changed = edits
            .update_metadata(
                &draft.edit_session_id,
                draft.revision,
                EpubMetadataPatch {
                    title: Some("准备覆盖".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        let directory = tempdir().unwrap();
        let target = directory.path().join("已有.epub");
        fs::write(&target, b"existing target").unwrap();
        let error = edits
            .save_as(&draft.edit_session_id, changed.revision, &target, None)
            .unwrap_err();
        assert_eq!(error.to_dto().code, "TARGET_ALREADY_EXISTS");
        assert_eq!(fs::read(&target).unwrap(), b"existing target");

        let token = edits
            .prepare_overwrite(&draft.edit_session_id, changed.revision, &target)
            .unwrap();
        let saved = edits
            .save_as(
                &draft.edit_session_id,
                changed.revision,
                &target,
                Some(&token),
            )
            .unwrap();
        assert_eq!(saved.document.metadata.title, "准备覆盖");
        let reused = edits
            .save_as(
                &draft.edit_session_id,
                saved.draft.revision,
                &target,
                Some(&token),
            )
            .unwrap_err();
        assert_eq!(reused.to_dto().code, "OVERWRITE_CONFIRMATION_EXPIRED");
    }

    #[test]
    fn edits_and_round_trips_epub2_metadata_without_changing_ncx_or_spine() {
        let fixture = minimal_epub2();
        let documents = EpubDocumentService::new(ArchiveLimits::default());
        let edits = EpubEditService::new(ArchiveLimits::default(), documents.clone());
        let opened = documents.open(fixture.path()).unwrap();
        let original_spine = opened.document.spine.clone();
        let original_toc = opened.document.toc.clone();
        let draft = edits.begin(&opened.document_id).unwrap();
        let changed = edits
            .update_metadata(
                &draft.edit_session_id,
                draft.revision,
                EpubMetadataPatch {
                    title: Some("新的 EPUB 2 书名".to_owned()),
                    creators: Some(vec!["第一作者".to_owned(), "第二作者".to_owned()]),
                    subjects: Some(vec!["历史".to_owned(), "测试".to_owned()]),
                    ..Default::default()
                },
            )
            .unwrap();
        let directory = tempdir().unwrap();
        let cover_path = directory.path().join("replacement.png");
        let mut replacement_cover = vec![0_u8; 33];
        replacement_cover[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        replacement_cover[12..16].copy_from_slice(b"IHDR");
        replacement_cover[16..20].copy_from_slice(&900_u32.to_be_bytes());
        replacement_cover[20..24].copy_from_slice(&1200_u32.to_be_bytes());
        fs::write(&cover_path, &replacement_cover).unwrap();
        let changed = edits
            .replace_cover(&draft.edit_session_id, changed.revision, &cover_path)
            .unwrap();
        let target = directory.path().join("epub2-edited.epub");
        let saved = edits
            .save_as(&draft.edit_session_id, changed.revision, &target, None)
            .unwrap();
        assert_eq!(saved.document.version, "2.0");
        assert_eq!(saved.document.metadata.title, "新的 EPUB 2 书名");
        assert_eq!(saved.document.metadata.creators, ["第一作者", "第二作者"]);
        assert_eq!(saved.document.spine, original_spine);
        assert_eq!(saved.document.toc, original_toc);
        let new_cover = saved.document.cover_resource_id.unwrap();
        assert_ne!(new_cover, "OEBPS/images/cover.png");
        let archive = SafeEpubArchive::open(&target, ArchiveLimits::default()).unwrap();
        assert!(archive.contains(&SafeArchivePath::parse("OEBPS/images/cover.png").unwrap()));
        assert_eq!(
            archive
                .read(
                    &SafeArchivePath::parse(&new_cover).unwrap(),
                    ResourceClass::Image
                )
                .unwrap(),
            replacement_cover
        );
    }

    #[test]
    #[ignore = "run explicitly in release mode for the EPUB repack performance record"]
    fn epub_repack_release_performance_probe() {
        const VALIDATION_RUNS: u32 = 1_000;
        const LARGE_PADDING_BYTES: u64 = 64 * 1024 * 1024;
        const COVER_BYTES: usize = 16 * 1024 * 1024;

        let ordinary = minimal_epub3();
        let ordinary_source_bytes = fs::metadata(ordinary.path()).unwrap().len();
        let ordinary_documents = EpubDocumentService::new(ArchiveLimits::default());
        let ordinary_edits =
            EpubEditService::new(ArchiveLimits::default(), ordinary_documents.clone());
        let ordinary_opened = ordinary_documents.open(ordinary.path()).unwrap();
        let begin_started = Instant::now();
        let ordinary_draft = ordinary_edits.begin(&ordinary_opened.document_id).unwrap();
        let ordinary_begin_us = begin_started.elapsed().as_micros();

        let validation_started = Instant::now();
        for _ in 0..VALIDATION_RUNS {
            ordinary_edits
                .validate(&ordinary_draft.edit_session_id)
                .unwrap();
        }
        let validation_average_us =
            validation_started.elapsed().as_micros() / u128::from(VALIDATION_RUNS);
        let ordinary_changed = ordinary_edits
            .update_metadata(
                &ordinary_draft.edit_session_id,
                ordinary_draft.revision,
                EpubMetadataPatch {
                    title: Some("EPUB repack ordinary performance".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        let ordinary_directory = tempdir().unwrap();
        let ordinary_target = ordinary_directory.path().join("ordinary-output.epub");
        let ordinary_save_started = Instant::now();
        let ordinary_saved = ordinary_edits
            .save_as(
                &ordinary_draft.edit_session_id,
                ordinary_changed.revision,
                &ordinary_target,
                None,
            )
            .unwrap();
        let ordinary_save_us = ordinary_save_started.elapsed().as_micros();

        let cover_fixture = minimal_epub3();
        let cover_documents = EpubDocumentService::new(ArchiveLimits::default());
        let cover_edits = EpubEditService::new(ArchiveLimits::default(), cover_documents.clone());
        let cover_opened = cover_documents.open(cover_fixture.path()).unwrap();
        let cover_draft = cover_edits.begin(&cover_opened.document_id).unwrap();
        let cover_directory = tempdir().unwrap();
        let cover_path = cover_directory.path().join("large-cover.png");
        let mut cover_bytes = vec![0_u8; COVER_BYTES];
        cover_bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        cover_bytes[12..16].copy_from_slice(b"IHDR");
        cover_bytes[16..20].copy_from_slice(&4_000_u32.to_be_bytes());
        cover_bytes[20..24].copy_from_slice(&6_000_u32.to_be_bytes());
        fs::write(&cover_path, &cover_bytes).unwrap();
        let cover_started = Instant::now();
        let cover_changed = cover_edits
            .replace_cover(
                &cover_draft.edit_session_id,
                cover_draft.revision,
                &cover_path,
            )
            .unwrap();
        let cover_processing_us = cover_started.elapsed().as_micros();

        let large = epub3_with_padding(LARGE_PADDING_BYTES);
        let large_source_bytes = fs::metadata(large.path()).unwrap().len();
        let large_documents = EpubDocumentService::new(ArchiveLimits::default());
        let large_edits = EpubEditService::new(ArchiveLimits::default(), large_documents.clone());
        let large_opened = large_documents.open(large.path()).unwrap();
        let large_begin_started = Instant::now();
        let large_draft = large_edits.begin(&large_opened.document_id).unwrap();
        let large_begin_us = large_begin_started.elapsed().as_micros();
        let large_changed = large_edits
            .update_metadata(
                &large_draft.edit_session_id,
                large_draft.revision,
                EpubMetadataPatch {
                    publisher: Some(Some("Readloom performance probe".to_owned())),
                    ..Default::default()
                },
            )
            .unwrap();
        let large_directory = tempdir().unwrap();
        let large_target = large_directory.path().join("large-output.epub");
        let large_save_started = Instant::now();
        let large_saved = large_edits
            .save_as(
                &large_draft.edit_session_id,
                large_changed.revision,
                &large_target,
                None,
            )
            .unwrap();
        let large_save_us = large_save_started.elapsed().as_micros();

        let no_temporary_outputs = [ordinary_directory.path(), large_directory.path()]
            .into_iter()
            .flat_map(|directory| fs::read_dir(directory).unwrap())
            .filter_map(Result::ok)
            .all(|entry| !entry.file_name().to_string_lossy().contains("readloom"));
        large_edits.close_document(&large_opened.document_id);
        let session_cleaned = large_edits.get(&large_draft.edit_session_id).is_err();

        println!(
            "EPUB_REPACK_PERF_JSON:{}",
            serde_json::json!({
                "profile": "release",
                "ordinarySourceBytes": ordinary_source_bytes,
                "ordinaryBeginSessionUs": ordinary_begin_us,
                "metadataValidationAverageUs": validation_average_us,
                "metadataValidationRuns": VALIDATION_RUNS,
                "ordinaryRepackAndVerifyUs": ordinary_save_us,
                "ordinaryOutputBytes": fs::metadata(&ordinary_target).unwrap().len(),
                "largeSourceBytes": large_source_bytes,
                "largePaddingBytes": LARGE_PADDING_BYTES,
                "largeBeginSessionUs": large_begin_us,
                "largeRepackAndVerifyUs": large_save_us,
                "largeOutputBytes": fs::metadata(&large_target).unwrap().len(),
                "coverInputBytes": COVER_BYTES,
                "coverProcessingUs": cover_processing_us,
                "coverDraftDirty": cover_changed.dirty,
                "ordinaryDraftCleanAfterSave": !ordinary_saved.draft.dirty,
                "largeDraftCleanAfterSave": !large_saved.draft.dirty,
                "temporaryOutputsCleaned": no_temporary_outputs,
                "sessionCleanedAfterClose": session_cleaned,
                "processId": std::process::id(),
            })
        );
    }
}
