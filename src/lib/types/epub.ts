export interface DocumentCapabilities {
  canRead: boolean;
  canEditText: boolean;
  canEditMetadata: boolean;
  canSearch: boolean;
  hasChapters: boolean;
  hasBookmarks: boolean;
  canSave: boolean;
  canSaveAs: boolean;
}

export interface EpubMetadata {
  title: string;
  creators: string[];
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
