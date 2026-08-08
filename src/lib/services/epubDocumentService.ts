import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

import type {
  EpubBookmark,
  EpubLocator,
  EpubSearchRequest,
  EpubSearchResult,
  OpenedEpubDocumentDto,
  RecentDocumentDto,
} from '../types/epub';
import { normalizeAppError } from './backend';

export async function chooseEpubFile(): Promise<string | null> {
  const selected = await open({
    directory: false,
    multiple: false,
    title: '打开 EPUB 电子书',
    filters: [{ name: 'EPUB 电子书', extensions: ['epub'] }],
  });
  return typeof selected === 'string' ? selected : null;
}

export async function openEpubDocument(path: string): Promise<OpenedEpubDocumentDto> {
  try {
    return await invoke<OpenedEpubDocumentDto>('open_epub_document', { request: { path } });
  } catch (error) {
    throw normalizeAppError(error);
  }
}

export async function closeEpubDocument(documentId: string): Promise<void> {
  try {
    await invoke<void>('close_epub_document', { request: { documentId } });
  } catch (error) {
    throw normalizeAppError(error);
  }
}

export async function saveEpubProgress(locator: EpubLocator): Promise<EpubLocator> {
  try {
    return await invoke<EpubLocator>('save_epub_progress', { request: { locator } });
  } catch (error) {
    throw normalizeAppError(error);
  }
}

export async function saveEpubBookmark(
  locator: EpubLocator,
  title: string | null,
  bookmarkId: string | null = null,
): Promise<EpubBookmark> {
  try {
    return await invoke<EpubBookmark>('save_epub_bookmark', {
      request: { locator, title, bookmarkId },
    });
  } catch (error) {
    throw normalizeAppError(error);
  }
}

export async function deleteEpubBookmark(documentId: string, bookmarkId: string): Promise<void> {
  try {
    await invoke<void>('delete_epub_bookmark', { request: { documentId, bookmarkId } });
  } catch (error) {
    throw normalizeAppError(error);
  }
}

export async function searchEpubDocument(
  request: EpubSearchRequest,
): Promise<EpubSearchResult[]> {
  try {
    return await invoke<EpubSearchResult[]>('search_epub_document', { request });
  } catch (error) {
    throw normalizeAppError(error);
  }
}

export async function cancelEpubSearch(documentId: string, requestId: string): Promise<void> {
  await invoke<void>('cancel_epub_search', { request: { documentId, requestId } }).catch(() => {});
}

export async function listRecentDocuments(maximum = 20): Promise<RecentDocumentDto[]> {
  try {
    return await invoke<RecentDocumentDto[]>('list_recent_documents', { request: { maximum } });
  } catch (error) {
    throw normalizeAppError(error);
  }
}

export function epubResourceUrl(
  sessionId: string,
  resourceId: string,
  fragment: string | null = null,
): string {
  if (!/^[a-f\d]{48}$/i.test(sessionId)) throw new Error('Invalid EPUB session');
  const path = resourceId
    .split('/')
    .map((segment) => encodeURIComponent(segment))
    .join('/');
  const hash = fragment ? `#${encodeURIComponent(fragment)}` : '';
  return `http://readloom-epub.localhost/${sessionId}/${path}${hash}`;
}
