import type { DocumentSession, TextBookmark } from './document';
import type { ChapterEditDto, EpubEditDraft, OpenedEpubDocumentDto } from './epub';

export interface TextWorkspaceTab {
  kind: 'txt';
  documentId: string;
  session: DocumentSession;
  content: string;
  bookmarks: TextBookmark[];
  readingOffset: number;
}

export interface EpubWorkspaceTab {
  kind: 'epub';
  documentId: string;
  document: OpenedEpubDocumentDto;
  spineIndex: number;
  editDraft: EpubEditDraft | null;
  editPanelOpen: boolean;
  chapterEditMode: boolean;
  activeChapterDraft: ChapterEditDto | null;
  saving: boolean;
}

export type WorkspaceTab = TextWorkspaceTab | EpubWorkspaceTab;

export interface WorkspaceTabSummary {
  id: string;
  kind: 'txt' | 'epub';
  title: string;
  path: string;
  detail: string | null;
  dirty: boolean;
}
