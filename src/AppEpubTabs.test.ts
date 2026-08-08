import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import type { OpenedTextDocumentDto } from './lib/types/document';
import type { OpenedEpubDocumentDto } from './lib/types/epub';

const fixtures = vi.hoisted(() => ({
  closeHandler: null as ((event: { preventDefault(): void }) => void) | null,
  dragDropHandler: null as ((event: { payload: { type: string; paths?: string[] } }) => void) | null,
  chooseDocumentFile: vi.fn<() => Promise<string | null>>(),
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    destroy: vi.fn(async () => {}),
    onCloseRequested: async (handler: (event: { preventDefault(): void }) => void) => {
      fixtures.closeHandler = handler;
      return vi.fn();
    },
    onDragDropEvent: async (
      handler: (event: { payload: { type: string; paths?: string[] } }) => void,
    ) => {
      fixtures.dragDropHandler = handler;
      return vi.fn();
    },
  }),
}));

vi.mock('./lib/services/backend', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./lib/services/backend')>()),
  hasTauriRuntime: () => true,
  probeBackend: vi.fn().mockResolvedValue({}),
  reportFrontendReady: vi.fn().mockResolvedValue({}),
}));

const openedEpub: OpenedEpubDocumentDto = {
  documentId: 'epub-0000000000000001',
  sessionId: '0123456789abcdef0123456789abcdef0123456789abcdef',
  bridgeToken: 'abcdef0123456789abcdef0123456789abcdef0123456789',
  fileName: '测试.epub',
  displayPath: 'C:\测试.epub',
  fileFingerprint: 'epub-fingerprint',
  initialLocator: null,
  bookmarks: [],
  document: {
    kind: 'epub',
    publicationId: 'urn:test',
    version: '3.0',
    metadata: {
      title: '测试 EPUB',
      creators: ['作者'],
      languages: ['zh-CN'],
      publisher: null,
      description: null,
      identifier: 'urn:test',
      publicationDate: null,
      modifiedDate: null,
      rights: [],
      subjects: [],
    },
    coverResourceId: null,
    manifest: [
      { id: 'chapter', resourceId: 'EPUB/chapter.xhtml', mediaType: 'application/xhtml+xml', properties: [] },
    ],
    spine: [
      { index: 0, idref: 'chapter', resourceId: 'EPUB/chapter.xhtml', mediaType: 'application/xhtml+xml', linear: true, properties: [] },
    ],
    toc: [
      { id: 'chapter', label: '第一章', resourceId: 'EPUB/chapter.xhtml', fragment: null, children: [] },
    ],
    layout: 'reflowable',
    capabilities: {
      canRead: true,
      canEditText: false,
      canEditMetadata: false,
      canSearch: true,
      hasChapters: true,
      hasBookmarks: true,
      canSave: false,
      canSaveAs: false,
    },
  },
};

const openedText: OpenedTextDocumentDto = {
  documentId: 'txt-0000000000000001',
  fileName: '并行.txt',
  displayPath: 'C:\并行.txt',
  content: 'TXT 与 EPUB 并行标签',
  encoding: 'utf8',
  hasBom: false,
  lineEnding: 'none',
  sizeBytes: 28,
  readOnly: false,
  revision: 0,
};

vi.mock('./lib/services/epubDocumentService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./lib/services/epubDocumentService')>()),
  openEpubDocument: vi.fn(async () => openedEpub),
  closeEpubDocument: vi.fn(async () => {}),
  saveEpubProgress: vi.fn(async (locator) => locator),
  saveEpubBookmark: vi.fn(),
  deleteEpubBookmark: vi.fn(),
  searchEpubDocument: vi.fn(async () => []),
  cancelEpubSearch: vi.fn(async () => {}),
}));

vi.mock('./lib/services/textDocumentService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./lib/services/textDocumentService')>()),
  openTextDocument: vi.fn(async () => openedText),
  closeTextDocument: vi.fn(async () => {}),
}));

vi.mock('./lib/services/workspaceFileService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./lib/services/workspaceFileService')>()),
  chooseDocumentFile: fixtures.chooseDocumentFile,
}));

import App from './App.svelte';
import { openEpubDocument } from './lib/services/epubDocumentService';
import { openTextDocument } from './lib/services/textDocumentService';

describe('EPUB and TXT workspace tabs', () => {
  it('lazy-loads only the active reader and preserves both document tabs', async () => {
    fixtures.chooseDocumentFile
      .mockResolvedValueOnce('C:\\测试.epub')
      .mockResolvedValueOnce('C:\\并行.txt');
    const { container } = render(App);

    await fireEvent.click(screen.getByText('打开文件'));
    await screen.findByRole('region', { name: 'EPUB 阅读器' });
    expect(container.querySelector('.cm-editor')).toBeNull();
    expect(screen.queryByText('开始编辑')).toBeNull();

    await fireEvent.click(screen.getByText('打开文件'));
    await waitFor(() => expect(container.querySelector('.cm-editor')).not.toBeNull());
    expect(screen.getByRole('button', { name: /EPUB 测试 EPUB/ })).toBeTruthy();
    expect(screen.getByRole('button', { name: /TXT 并行.txt/ })).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: /EPUB 测试 EPUB/ }));
    await screen.findByRole('region', { name: 'EPUB 阅读器' });
    expect(container.querySelector('.cm-editor')).toBeNull();
  });

  it('opens dropped EPUB files as books and unknown extensions as text', async () => {
    render(App);
    await waitFor(() => expect(fixtures.dragDropHandler).not.toBeNull());

    fixtures.dragDropHandler!({
      payload: { type: 'enter', paths: ['C:\\测试.epub', 'C:\\随笔.markdown'] },
    });
    expect(await screen.findByText('松开以打开文件')).toBeTruthy();

    fixtures.dragDropHandler!({
      payload: { type: 'drop', paths: ['C:\\测试.epub', 'C:\\随笔.markdown'] },
    });

    await waitFor(() => {
      expect(openEpubDocument).toHaveBeenCalledWith('C:\\测试.epub');
      expect(openTextDocument).toHaveBeenCalledWith('C:\\随笔.markdown', null, false);
    });
    expect(screen.queryByText('松开以打开文件')).toBeNull();
  });
});
