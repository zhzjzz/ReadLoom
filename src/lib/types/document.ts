import type { AppErrorDto } from './ipc';

export type TextEncoding = 'utf8' | 'utf16Le' | 'utf16Be' | 'gbk' | 'gb18030';
export type LineEnding = 'crlf' | 'lf' | 'cr' | 'mixed' | 'none';
export type SaveLineEnding = 'preserve' | 'crlf' | 'lf';
export type SaveStatus = 'idle' | 'saving' | 'error';

export interface OpenedTextDocumentDto {
  documentId: string;
  fileName: string;
  displayPath: string;
  content: string;
  encoding: TextEncoding;
  hasBom: boolean;
  lineEnding: LineEnding;
  sizeBytes: number;
  readOnly: boolean;
  revision: number;
}

export interface SavedTextDocumentDto extends Omit<OpenedTextDocumentDto, 'content'> {}

export interface DocumentSession {
  documentId: string;
  fileName: string;
  displayPath: string;
  encoding: TextEncoding;
  hasBom: boolean;
  savedEncoding: TextEncoding;
  savedHasBom: boolean;
  lineEnding: LineEnding;
  lineEndingChoice: SaveLineEnding;
  sizeBytes: number;
  readOnly: boolean;
  revision: number;
  contentDirty: boolean;
  formatDirty: boolean;
}

export interface DocumentState {
  active: DocumentSession | null;
  saveStatus: SaveStatus;
  error: AppErrorDto | null;
}

export interface SaveOptions {
  encoding: TextEncoding;
  hasBom: boolean;
  lineEnding: SaveLineEnding;
}

export interface TextEditorHandle {
  discardChanges(): void;
  focus(): void;
  getContent(): string;
  markSaved(): void;
  setEditing(editing: boolean): void;
}

export interface EditorStatistics {
  lines: number;
  characters: number;
}

export function isDirty(session: DocumentSession | null): boolean {
  return Boolean(session?.contentDirty || session?.formatDirty);
}
