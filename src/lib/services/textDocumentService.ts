import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';

import type {
  OpenedTextDocumentDto,
  SaveLineEnding,
  SavedTextDocumentDto,
  TextEncoding,
} from '../types/document';
import { normalizeAppError } from './backend';

export async function chooseSavePath(defaultPath: string): Promise<string | null> {
  return await save({
    title: '另存为 TXT',
    defaultPath,
    filters: [{ name: '文本文件', extensions: ['txt'] }],
  });
}

export async function openTextDocument(
  path: string,
  encodingOverride: TextEncoding | null = null,
  allowLarge = false,
): Promise<OpenedTextDocumentDto> {
  return invokeChecked('open_text_document', {
    request: { path, encodingOverride, allowLarge },
  });
}

export async function reopenTextDocument(
  documentId: string,
  encoding: TextEncoding,
  allowLarge = false,
): Promise<OpenedTextDocumentDto> {
  return invokeChecked('reopen_text_document', {
    request: { documentId, encoding, allowLarge },
  });
}

export interface SaveDocumentRequest {
  documentId: string;
  content: string;
  encoding: TextEncoding;
  hasBom: boolean;
  lineEnding: SaveLineEnding;
  expectedRevision: number;
}

export async function saveTextDocument(
  request: SaveDocumentRequest,
): Promise<SavedTextDocumentDto> {
  return invokeChecked('save_text_document', { request });
}

export async function saveTextDocumentAs(
  request: SaveDocumentRequest & { targetPath: string; allowOverwrite: boolean },
): Promise<SavedTextDocumentDto> {
  return invokeChecked('save_text_document_as', { request });
}

export async function closeTextDocument(documentId: string): Promise<void> {
  await invokeChecked<void>('close_text_document', { request: { documentId } });
}

async function invokeChecked<T>(command: string, args: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw normalizeAppError(error);
  }
}
