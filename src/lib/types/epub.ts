export interface DocumentCapabilities {
  canRead: boolean;
  canEditText: boolean;
  canEditMetadata: boolean;
  canSearch: boolean;
  hasChapters: boolean;
  hasBookmarks: boolean;
  canSave: boolean;
  canSaveAs: boolean;
  canReplaceCover: boolean;
  canEditStructure: boolean;
  canOverwriteOriginal: boolean;
}

export interface EpubMetadata {
  title: string;
  creators: string[];
  contributors: string[];
  languages: string[];
  publisher: string | null;
  description: string | null;
  identifier: string | null;
  publicationDate: string | null;
  modifiedDate: string | null;
  rights: string[];
  subjects: string[];
}

export interface ManifestItem {
  id: string;
  resourceId: string;
  mediaType: string;
  properties: string[];
}

export interface SpineItem {
  index: number;
  idref: string;
  resourceId: string;
  mediaType: string;
  linear: boolean;
  properties: string[];
}

export interface TocNode {
  id: string;
  label: string;
  resourceId: string | null;
  fragment: string | null;
  children: TocNode[];
}

export interface ParsedEpubDocument {
  kind: 'epub';
  publicationId: string;
  packageResourceId: string;
  version: string;
  metadata: EpubMetadata;
  coverResourceId: string | null;
  manifest: ManifestItem[];
  spine: SpineItem[];
  toc: TocNode[];
  layout: 'reflowable' | 'fixed';
  capabilities: DocumentCapabilities;
}

export interface OpenedEpubDocumentDto {
  documentId: string;
  sessionId: string;
  bridgeToken: string;
  fileName: string;
  displayPath: string;
  fileFingerprint: string;
  document: ParsedEpubDocument;
  initialLocator: EpubLocator | null;
  bookmarks: EpubBookmark[];
}

export interface FlatTocNode extends TocNode {
  depth: number;
}

export interface EpubLocator {
  documentId: string;
  documentFingerprint: string;
  spineIndex: number;
  spineHref: string;
  fragment: string | null;
  progressionInChapter: number;
  characterOffset: number | null;
}

export interface EpubBookmark {
  bookmarkId: string;
  locator: EpubLocator;
  title: string | null;
  chapterTitle: string;
  createdAtMs: number;
  updatedAtMs: number;
  valid: boolean;
}

export interface EpubSearchRequest {
  documentId: string;
  requestId: string;
  query: string;
  caseSensitive: boolean;
  wholeWord: boolean;
  maximumResults: number;
}

export interface EpubSearchResult {
  requestId: string;
  spineIndex: number;
  spineHref: string;
  chapterTitle: string;
  characterOffset: number;
  temporarySnippet: string;
  matchStart: number;
  matchEnd: number;
}

export interface EpubReadingSettings {
  fontFamily: 'system' | 'serif' | 'sans';
  fontSize: number;
  lineHeight: number;
  contentWidth: number;
  pageMargin: number;
  textAlign: 'start' | 'justify';
  publisherStyles: 'use' | 'partial' | 'ignore';
  ignorePublisherFonts: boolean;
  ignorePublisherColors: boolean;
  allowInternalFonts: boolean;
  imageMaximumWidth: number;
}

export interface RecentDocumentDto {
  path: string;
  documentKind: 'txt' | 'epub';
  displayTitle: string;
  author: string | null;
  fingerprint: string | null;
  lastOpenedAtMs: number;
  available: boolean;
}

export interface EpubMetadataDraft {
  title: string;
  creators: string[];
  contributors: string[];
  language: string;
  publisher: string | null;
  description: string | null;
  identifier: string;
  publicationDate: string | null;
  subjects: string[];
  rights: string[];
}

export type EpubMetadataPatch = Partial<EpubMetadataDraft>;

export interface EpubCoverDraft {
  state: 'unchanged' | 'replaced';
  originalResourceId: string | null;
  currentResourceId: string | null;
  previewResourceId: string | null;
  mediaType: string | null;
  width: number | null;
  height: number | null;
}

export interface EpubDraftChanges {
  metadataFields: string[];
  coverChanged: boolean;
  modifiedChapters: number[];
  addedResources: number;
}

export type ChapterCompatibilityLevel = 'full' | 'limited' | 'readOnly' | 'unsupported';
export type ChapterValidationState = 'valid' | 'warning' | 'invalid';

export interface ChapterEditWarning {
  code: string;
  message: string;
}

export interface ChapterEditCapabilities {
  canEdit: boolean;
  canFormat: boolean;
  canEditLinks: boolean;
  canImportImages: boolean;
  canPreview: boolean;
  canRevert: boolean;
}

export interface ChapterEditDto {
  chapterEditId: string;
  editSessionId: string;
  documentId: string;
  spineIndex: number;
  manifestItemId: string;
  chapterHref: string;
  chapterTitle: string;
  originalResourceHash: string;
  editorDocument: Record<string, unknown>;
  compatibilityLevel: ChapterCompatibilityLevel;
  warnings: ChapterEditWarning[];
  revision: number;
  acceptedRevision: number;
  dirty: boolean;
  validationState: ChapterValidationState;
  previewRevision: number;
  capabilities: ChapterEditCapabilities;
}

export interface ChapterDraftUpdate {
  chapterEditId: string;
  baseRevision: number;
  clientRevision: number;
  editorDocument: Record<string, unknown>;
  requestId: string;
}

export interface ChapterDraftAccepted {
  chapterEditId: string;
  requestId: string;
  clientRevision: number;
  acceptedRevision: number;
  dirty: boolean;
  warnings: ChapterEditWarning[];
  previewRevision: number;
  publicationRevision: number;
}

export interface ImportedChapterImage {
  chapterEditId: string;
  resourceId: string;
  editorSrc: string;
  previewUrl: string;
  mediaType: string;
  width: number;
  height: number;
}

export interface EpubValidationIssue {
  code: string;
  message: string;
  severity: 'error' | 'warning' | 'information';
}

export interface EpubDraftValidation {
  errors: EpubValidationIssue[];
  warnings: EpubValidationIssue[];
  information: EpubValidationIssue[];
  canSave: boolean;
}

export interface EpubEditDraft {
  editSessionId: string;
  documentId: string;
  sourcePath: string;
  publicationId: string;
  opfResourceId: string;
  metadata: EpubMetadataDraft;
  cover: EpubCoverDraft;
  changes: EpubDraftChanges;
  dirty: boolean;
  validation: EpubDraftValidation;
  revision: number;
  savedRevision: number;
  saving: boolean;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface SavedEpubDocument {
  editSessionId: string;
  targetPath: string;
  fileFingerprint: string;
  document: ParsedEpubDocument;
  draft: EpubEditDraft;
}

export type EpubBridgeMessage =
  | {
      source: 'readloom-epub';
      version: 1;
      type: 'progress';
      documentId: string;
      sessionId: string;
      token: string;
      payload: { progression: number; fragment: string | null };
    }
  | {
      source: 'readloom-epub';
      version: 1;
      type: 'link';
      documentId: string;
      sessionId: string;
      token: string;
      payload: { href: string };
    };
