import type { DocumentSession } from './document';
import type { OpenedEpubDocumentDto } from './epub';

export interface TextWorkspaceTab {
  kind: 'txt';
  documentId: string;
  session: DocumentSession;
  content: string;
}

export interface EpubWorkspaceTab {
  kind: 'epub';
  documentId: string;
  document: OpenedEpubDocumentDto;
  spineIndex: number;
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
