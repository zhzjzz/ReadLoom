import { invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';

import type {
  ChapterDraftAccepted,
  ChapterDraftUpdate,
  ChapterEditDto,
  EpubBookmark,
  EpubDraftValidation,
  EpubEditDraft,
  EpubLocator,
  EpubMetadataPatch,
  EpubSearchRequest,
  EpubSearchResult,
  ImportedChapterImage,
  OpenedEpubDocumentDto,
  RecentDocumentDto,
  SavedEpubDocument,
} from '../types/epub';
import { normalizeAppError } from './backend';

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

export async function deleteRecentDocument(path: string): Promise<void> {
  try {
    await invoke<void>('delete_recent_document', { request: { path } });
  } catch (error) {
    throw normalizeAppError(error);
  }
}

export async function chooseEpubCoverPath(): Promise<string | null> {
  const selected = await open({
    title: '选择 EPUB 封面',
    multiple: false,
    directory: false,
    filters: [{ name: '封面图片', extensions: ['png', 'jpg', 'jpeg', 'webp'] }],
  });
  return typeof selected === 'string' ? selected : null;
}

export async function chooseEpubSavePath(defaultPath: string): Promise<string | null> {
  return await save({
    title: 'EPUB 另存为',
    defaultPath,
    filters: [{ name: 'EPUB 电子书', extensions: ['epub'] }],
  });
}

export async function beginEpubEdit(documentId: string): Promise<EpubEditDraft> {
  return invokeChecked('begin_epub_edit', { request: { documentId } });
}

export async function getEpubEditDraft(editSessionId: string): Promise<EpubEditDraft> {
  return invokeChecked('get_epub_edit_draft', { request: { editSessionId } });
}

export async function updateEpubMetadata(
  editSessionId: string,
  expectedRevision: number,
  metadataPatch: EpubMetadataPatch,
): Promise<EpubEditDraft> {
  return invokeChecked('update_epub_metadata', {
    request: { editSessionId, expectedRevision, metadataPatch },
  });
}

export async function replaceEpubCover(
  editSessionId: string,
  expectedRevision: number,
  selectedPath: string,
): Promise<EpubEditDraft> {
  return invokeChecked('replace_epub_cover', {
    request: { editSessionId, expectedRevision, selectedPath },
  });
}

export async function removeEpubCoverChange(
  editSessionId: string,
  expectedRevision: number,
): Promise<EpubEditDraft> {
  return invokeChecked('remove_epub_cover_change', {
    request: { editSessionId, expectedRevision },
  });
}

export async function validateEpubDraft(
  editSessionId: string,
): Promise<EpubDraftValidation> {
  return invokeChecked('validate_epub_draft', { request: { editSessionId } });
}

export async function prepareEpubOverwriteConfirmation(
  editSessionId: string,
  expectedRevision: number,
  targetPath: string,
): Promise<string> {
  return invokeChecked('prepare_epub_overwrite_confirmation', {
    request: { editSessionId, expectedRevision, targetPath },
  });
}

export async function saveEpubAs(
  editSessionId: string,
  expectedRevision: number,
  targetPath: string,
  confirmationToken: string | null = null,
): Promise<SavedEpubDocument> {
  return invokeChecked('save_epub_as', {
    request: { editSessionId, expectedRevision, targetPath, confirmationToken },
  });
}

export async function cancelEpubSave(editSessionId: string): Promise<void> {
  await invokeChecked<void>('cancel_epub_save', { request: { editSessionId } });
}

export async function discardEpubDraft(editSessionId: string): Promise<void> {
  await invokeChecked<void>('discard_epub_draft', { request: { editSessionId } });
}

export async function closeEpubEditSession(editSessionId: string): Promise<void> {
  await invokeChecked<void>('close_epub_edit_session', { request: { editSessionId } });
}

export async function analyzeEpubChapterEditability(
  editSessionId: string,
  spineIndex: number,
): Promise<ChapterEditDto> {
  return invokeChecked('analyze_epub_chapter_editability', {
    request: { editSessionId, spineIndex },
  });
}

export async function beginEpubChapterEdit(
  editSessionId: string,
  spineIndex: number,
): Promise<ChapterEditDto> {
  return invokeChecked('begin_epub_chapter_edit', {
    request: { editSessionId, spineIndex },
  });
}

export async function updateEpubChapterDraft(
  update: ChapterDraftUpdate,
): Promise<ChapterDraftAccepted> {
  return invokeChecked('update_epub_chapter_draft', { request: update });
}

export async function flushEpubChapterDraft(
  chapterEditId: string,
  revision: number,
): Promise<ChapterDraftAccepted> {
  return invokeChecked('flush_epub_chapter_draft', {
    request: { chapterEditId, revision },
  });
}

export async function validateEpubChapterDraft(
  chapterEditId: string,
): Promise<ChapterEditDto> {
  return invokeChecked('validate_epub_chapter_draft', { request: { chapterEditId } });
}

export async function revertEpubChapterDraft(chapterEditId: string): Promise<ChapterEditDto> {
  return invokeChecked('revert_epub_chapter_draft', { request: { chapterEditId } });
}

export async function closeEpubChapterEdit(chapterEditId: string): Promise<void> {
  await invokeChecked<void>('close_epub_chapter_edit', { request: { chapterEditId } });
}

export async function chooseEpubChapterImagePath(): Promise<string | null> {
  const selected = await open({
    title: '导入章节图片',
    multiple: false,
    directory: false,
    filters: [{ name: '章节图片', extensions: ['png', 'jpg', 'jpeg', 'webp'] }],
  });
  return typeof selected === 'string' ? selected : null;
}

export async function importEpubChapterImage(
  editSessionId: string,
  chapterEditId: string,
  selectedPath: string,
): Promise<ImportedChapterImage> {
  return invokeChecked('import_epub_chapter_image', {
    request: { editSessionId, chapterEditId, selectedPath },
  });
}

async function invokeChecked<T>(command: string, args: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
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
