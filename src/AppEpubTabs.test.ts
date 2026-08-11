import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import type { OpenedTextDocumentDto } from './lib/types/document';
import type { ChapterEditDto, EpubEditDraft, OpenedEpubDocumentDto } from './lib/types/epub';

const fixtures = vi.hoisted(() => ({
  closeHandler: null as ((event: { preventDefault(): void }) => void) | null,
  dragDropHandler: null as ((event: { payload: { type: string; paths?: string[] } }) => void) | null,
  chooseDocumentFile: vi.fn<() => Promise<string | null>>(),
  chooseLibraryDirectory: vi.fn<() => Promise<string | null>>(),
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    destroy: vi.fn(async () => {}),
    hide: vi.fn(async () => {}),
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

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => vi.fn()),
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
      contributors: [],
      languages: ['zh-CN'],
      publisher: null,
      description: null,
      identifier: 'urn:test',
      publicationDate: null,
      modifiedDate: null,
      rights: [],
      subjects: [],
    },
    packageResourceId: 'EPUB/package.opf',
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
      canEditText: true,
      canEditMetadata: true,
      canSearch: true,
      hasChapters: true,
      hasBookmarks: true,
      canSave: false,
      canSaveAs: true,
      canReplaceCover: true,
      canEditStructure: false,
      canOverwriteOriginal: false,
    },
  },
};

const editDraft: EpubEditDraft = {
  editSessionId: 'edit-0123456789abcdef',
  documentId: openedEpub.documentId,
  sourcePath: openedEpub.displayPath,
  publicationId: openedEpub.document.publicationId,
  opfResourceId: openedEpub.document.packageResourceId,
  metadata: {
    title: openedEpub.document.metadata.title,
    creators: openedEpub.document.metadata.creators,
    contributors: [],
    language: 'zh-CN',
    publisher: null,
    description: null,
    identifier: 'urn:test',
    publicationDate: null,
    subjects: [],
    rights: [],
  },
  cover: {
    state: 'unchanged',
    originalResourceId: null,
    currentResourceId: null,
    previewResourceId: null,
    mediaType: null,
    width: null,
    height: null,
  },
  changes: { metadataFields: [], coverChanged: false, modifiedChapters: [], addedResources: 0 },
  dirty: false,
  validation: { errors: [], warnings: [], information: [], canSave: false },
  revision: 0,
  savedRevision: 0,
  saving: false,
  createdAtMs: 1,
  updatedAtMs: 1,
};

const chapterDraft: ChapterEditDto = {
  chapterEditId: 'chapter-edit-0001',
  editSessionId: editDraft.editSessionId,
  documentId: openedEpub.documentId,
  spineIndex: 0,
  manifestItemId: 'chapter',
  chapterHref: 'EPUB/chapter.xhtml',
  chapterTitle: '第一章',
  originalResourceHash: 'chapter-fingerprint',
  editorDocument: {
    type: 'doc',
    content: [{ type: 'paragraph', content: [{ type: 'text', text: '可视化编辑正文' }] }],
  },
  compatibilityLevel: 'full',
  warnings: [],
  revision: 0,
  acceptedRevision: 0,
  dirty: false,
  validationState: 'valid',
  previewRevision: 0,
  capabilities: {
    canEdit: true,
    canFormat: true,
    canEditLinks: true,
    canImportImages: true,
    canPreview: true,
    canRevert: false,
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
  bookmarks: [],
  initialCharacterOffset: 0,
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
  beginEpubEdit: vi.fn(async () => editDraft),
  beginEpubChapterEdit: vi.fn(async () => chapterDraft),
  updateEpubChapterDraft: vi.fn(),
  flushEpubChapterDraft: vi.fn(),
  validateEpubChapterDraft: vi.fn(async () => chapterDraft),
  closeEpubChapterEdit: vi.fn(async () => {}),
  updateEpubMetadata: vi.fn(async () => editDraft),
  replaceEpubCover: vi.fn(async () => editDraft),
  removeEpubCoverChange: vi.fn(async () => editDraft),
  discardEpubDraft: vi.fn(async () => {}),
}));

vi.mock('./lib/services/appearanceService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./lib/services/appearanceService')>()),
  getBackgroundImage: vi.fn(async () => null),
  applyWindowBehavior: vi.fn(async () => {}),
  setBackgroundImage: vi.fn(),
  clearBackgroundImage: vi.fn(),
}));

vi.mock('./lib/services/libraryService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./lib/services/libraryService')>()),
  listLibrary: vi.fn(async () => ({ documents: [], groups: [] })),
  removeLibraryDocument: vi.fn(async () => {}),
  createLibraryGroup: vi.fn(),
  renameLibraryGroup: vi.fn(async () => {}),
  deleteLibraryGroup: vi.fn(async () => {}),
  assignLibraryGroup: vi.fn(async () => {}),
  removeUnavailableLibraryDocuments: vi.fn(async () => 0),
  previewLibraryDirectory: vi.fn(async () => ({
    rootPath: 'D:\\Books',
    totalSizeBytes: 2048,
    importable: 1,
    alreadyImported: 1,
    candidates: [
      { path: 'D:\\Books\\新书.txt', fileName: '新书.txt', documentKind: 'txt', sizeBytes: 1024, alreadyImported: false },
      { path: 'D:\\Books\\旧书.epub', fileName: '旧书.epub', documentKind: 'epub', sizeBytes: 1024, alreadyImported: true },
    ],
  })),
  importLibraryDocuments: vi.fn(async () => ({ imported: 1, skipped: 0, failed: [] })),
}));

vi.mock('./lib/services/textDocumentService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./lib/services/textDocumentService')>()),
  openTextDocument: vi.fn(async () => openedText),
  closeTextDocument: vi.fn(async () => {}),
  saveTextBookmark: vi.fn(async () => ({
    bookmarkId: 'tbm-test',
    characterOffset: 0,
    lineNumber: 1,
    title: null,
    preview: openedText.content,
    createdAtMs: 1,
    updatedAtMs: 1,
  })),
  deleteTextBookmark: vi.fn(async () => {}),
  saveTextProgress: vi.fn(async () => {}),
}));

vi.mock('./lib/services/workspaceFileService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./lib/services/workspaceFileService')>()),
  chooseDocumentFile: fixtures.chooseDocumentFile,
  chooseLibraryDirectory: fixtures.chooseLibraryDirectory,
}));

import App from './App.svelte';
import { openEpubDocument } from './lib/services/epubDocumentService';
import {
  listLibrary,
  importLibraryDocuments,
  previewLibraryDirectory,
  removeLibraryDocument,
  removeUnavailableLibraryDocuments,
} from './lib/services/libraryService';
import { openTextDocument, saveTextBookmark } from './lib/services/textDocumentService';

describe('EPUB and TXT workspace tabs', () => {
  it('keeps secondary appearance and heading controls behind the settings button', async () => {
    render(App);

    expect(screen.queryByText(/阶段\s*[0-9一二三四五六七八九十]/)).toBeNull();
    expect(screen.queryByText('后端版本')).toBeNull();
    expect(screen.queryByText('协议版本')).toBeNull();
    expect(screen.queryByText('文件安全')).toBeNull();
    expect(screen.queryByRole('region', { name: '设置' })).toBeNull();
    expect(screen.queryByRole('button', { name: '展开设置面板' })).toBeNull();

    await fireEvent.click(screen.getByRole('button', { name: '打开设置' }));

    expect(await screen.findByRole('region', { name: '设置' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: '收起设置面板' })).toBeNull();
    expect(screen.getByText('外观')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: '页面布局' }));
    expect(screen.getByRole('radiogroup', { name: '书库每行显示' })).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: '章节识别' }));
    const headingPattern = screen.getByLabelText('TXT 标题识别正则');
    expect(headingPattern).toBeTruthy();

    await fireEvent.input(headingPattern, { target: { value: '[' } });
    expect(screen.getByRole('alert').textContent).toContain('仍继续使用上一次有效规则');
    await fireEvent.click(screen.getByText('恢复默认规则'));
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('removes a library entry through the independent library backend', async () => {
    vi.mocked(listLibrary).mockResolvedValueOnce({
      documents: [{
        path: 'C:\\书库.txt',
        documentKind: 'txt',
        displayTitle: '书库.txt',
        author: null,
        fingerprint: null,
        lastOpenedAtMs: 1,
        available: true,
        groupId: null,
        coverKey: null,
      }],
      groups: [],
    });
    render(App);
    const removeButton = await screen.findByRole('button', { name: '从书库移除 书库.txt' });

    await fireEvent.click(removeButton);

    await waitFor(() => expect(removeLibraryDocument).toHaveBeenCalledWith('C:\\书库.txt'));
    expect(screen.queryByText('书库.txt')).toBeNull();
  });

  it('reviews a selected directory before importing only new checked books', async () => {
    fixtures.chooseLibraryDirectory.mockResolvedValueOnce('D:\\Books');
    vi.mocked(previewLibraryDirectory).mockClear();
    vi.mocked(importLibraryDocuments).mockClear();
    render(App);

    await fireEvent.click(await screen.findByRole('button', { name: '导入目录' }));

    expect(await screen.findByRole('dialog', { name: '导入前确认' })).toBeTruthy();
    expect(previewLibraryDirectory).toHaveBeenCalledWith('D:\\Books');
    expect((screen.getByLabelText('选择 旧书.epub') as HTMLInputElement).disabled).toBe(true);
    await fireEvent.click(screen.getByRole('button', { name: '导入所选图书' }));

    await waitFor(() => expect(importLibraryDocuments).toHaveBeenCalledWith(['D:\\Books\\新书.txt']));
    await waitFor(() => expect(screen.queryByRole('dialog', { name: '导入前确认' })).toBeNull());
  });

  it('confirms and removes all unavailable library records without deleting files', async () => {
    vi.mocked(listLibrary).mockResolvedValueOnce({
      documents: [{
        path: 'C:\\moved\\旧书.epub',
        documentKind: 'epub',
        displayTitle: '旧书',
        author: null,
        fingerprint: 'missing',
        lastOpenedAtMs: 1,
        available: false,
        groupId: null,
        coverKey: null,
      }],
      groups: [],
    });
    vi.mocked(removeUnavailableLibraryDocuments).mockResolvedValueOnce(1);
    const confirm = vi.spyOn(window, 'confirm').mockImplementation(() => true);
    render(App);

    const cleanupButton = await screen.findByRole('button', { name: '清理无效书籍' });
    await waitFor(() => expect((cleanupButton as HTMLButtonElement).disabled).toBe(false));
    await fireEvent.click(cleanupButton);

    expect(confirm).toHaveBeenCalledOnce();
    await waitFor(() => expect(removeUnavailableLibraryDocuments).toHaveBeenCalledOnce());
    expect(confirm).toHaveBeenCalledWith('从书库移除 1 本已移动或删除的无效书籍？原文件不会被删除。');
    confirm.mockRestore();
  });

  it('opens the persisted library, filters books and returns to the workspace from a card', async () => {
    vi.mocked(listLibrary).mockResolvedValueOnce({
      documents: [{
        path: 'C:\\测试.epub',
        documentKind: 'epub',
        displayTitle: '测试 EPUB',
        author: '作者',
        fingerprint: 'epub-fingerprint',
        lastOpenedAtMs: 2,
        available: true,
        groupId: null,
        coverKey: null,
      },
      {
        path: 'C:\\并行.txt',
        documentKind: 'txt',
        displayTitle: '并行.txt',
        author: null,
        fingerprint: null,
        lastOpenedAtMs: 1,
        available: true,
        groupId: null,
        coverKey: null,
      },
      ],
      groups: [],
    });
    render(App);

    expect(await screen.findByRole('heading', { name: '我的书库' })).toBeTruthy();
    expect(await screen.findByRole('heading', { name: '测试 EPUB' })).toBeTruthy();
    await fireEvent.input(screen.getByLabelText('搜索书库'), { target: { value: '作者' } });
    expect(screen.getByRole('heading', { name: '测试 EPUB' })).toBeTruthy();
    expect(screen.queryByRole('heading', { name: '并行.txt' })).toBeNull();

    await fireEvent.click(screen.getByRole('button', { name: '打开 测试 EPUB' }));

    await waitFor(() => expect(openEpubDocument).toHaveBeenCalledWith('C:\\测试.epub'));
    expect(await screen.findByRole('region', { name: 'EPUB 阅读器' })).toBeTruthy();
    expect(screen.getByRole('button', { name: '阅读与编辑' }).getAttribute('aria-current')).toBe('page');
  });

  it('keeps the active TXT content when visiting the library and returning', async () => {
    fixtures.chooseDocumentFile.mockResolvedValueOnce('C:\\并行.txt');
    const { container } = render(App);

    await fireEvent.click(screen.getByText('打开文件'));
    await waitFor(() => expect(container.querySelector('.cm-content')?.textContent).toBe(openedText.content));
    await fireEvent.click(screen.getByRole('button', { name: '书库' }));
    expect(await screen.findByRole('heading', { name: '我的书库' })).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: '阅读与编辑' }));

    await waitFor(() => expect(container.querySelector('.cm-content')?.textContent).toBe(openedText.content));
  });

  it('shows recognized headings in the TXT outline after opening a text document', async () => {
    vi.mocked(openTextDocument).mockResolvedValueOnce({
      ...openedText,
      content: '序章 起点\n普通内容\n第十二章 风起',
    });
    fixtures.chooseDocumentFile.mockResolvedValueOnce('C:\\并行.txt');
    render(App);

    await fireEvent.click(screen.getByText('打开文件'));

    expect(await screen.findByRole('button', { name: '第 1 行 序章 起点' })).toBeTruthy();
    expect(screen.getByRole('button', { name: '第 3 行 第十二章 风起' })).toBeTruthy();
  });

  it('restores the persisted TXT reading position without stealing focus', async () => {
    vi.mocked(openTextDocument).mockResolvedValueOnce({
      ...openedText,
      content: '开头\n恢复位置\n末尾',
      initialCharacterOffset: 3,
    });
    fixtures.chooseDocumentFile.mockResolvedValueOnce('C:\\并行.txt');
    const { container } = render(App);

    await fireEvent.click(screen.getByText('打开文件'));

    await waitFor(() => expect(container.querySelector('[aria-current="location"]')?.textContent).toBe('恢复位置'));
    expect(document.activeElement?.getAttribute('aria-label')).not.toBe('TXT 文本编辑器');
  });

  it('searches the active TXT document and persists a bookmark at the current cursor', async () => {
    fixtures.chooseDocumentFile.mockResolvedValueOnce('C:\\并行.txt');
    render(App);

    await fireEvent.click(screen.getByText('打开文件'));
    await screen.findByLabelText('TXT 文本编辑器');

    await fireEvent.input(screen.getByLabelText('TXT 全文检索'), {
      target: { value: 'EPUB' },
    });
    await fireEvent.click(screen.getByRole('button', { name: '搜索 TXT 全文' }));

    const result = screen.getByRole('button', {
      name: `第 1 行 · ${openedText.content}`,
    });
    await fireEvent.click(result);
    await fireEvent.click(screen.getByRole('button', { name: '添加 TXT 书签' }));

    await waitFor(() => {
      expect(saveTextBookmark).toHaveBeenCalledWith(
        openedText.documentId,
        6,
        1,
        null,
        openedText.content,
      );
    });
    expect(within(screen.getByLabelText('TXT 书签')).getByRole('button', {
      name: `第 1 行 · ${openedText.content}`,
    })).toBeTruthy();
  });

  it('collapses and expands the navigation while settings uses the document region', async () => {
    const { container } = render(App);

    expect(screen.getByRole('complementary', { name: '主导航' })).toBeTruthy();
    expect(screen.queryByRole('region', { name: '设置' })).toBeNull();

    await fireEvent.click(screen.getByRole('button', { name: '收起左侧栏' }));
    expect(screen.queryByRole('complementary', { name: '主导航' })).toBeNull();
    expect(screen.getByRole('main')).toBeTruthy();
    expect(container.querySelector('.brand')?.classList.contains('compact')).toBe(true);
    expect((container.querySelector('.left-divider') as HTMLElement).style.gridColumn).toBe('2');
    expect(screen.getByRole('main').style.gridColumn).toBe('3');
    expect(container.querySelector('.right-divider')).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: '展开左侧栏' }));
    expect(screen.getByRole('complementary', { name: '主导航' })).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: '打开设置' }));
    expect(await screen.findByRole('region', { name: '设置' })).toBeTruthy();
    expect(container.querySelector('.right-divider')).toBeNull();
    expect(container.querySelector('.inspector-slot')).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: '关闭设置' }));
    await waitFor(() => expect(screen.queryByRole('region', { name: '设置' })).toBeNull());
  });

  it('resizes the document region from accessible pane separators', async () => {
    render(App);
    const leftSeparator = screen.getByRole('separator', { name: '调整左侧栏宽度' });
    expect(leftSeparator.getAttribute('aria-valuenow')).toBe('220');

    await fireEvent.pointerDown(leftSeparator, { button: 0, clientX: 220 });
    await fireEvent.pointerMove(window, { clientX: 300 });
    await fireEvent.pointerUp(window);
    expect(leftSeparator.getAttribute('aria-valuenow')).toBe('300');

    await fireEvent.keyDown(leftSeparator, { key: 'ArrowRight' });
    expect(leftSeparator.getAttribute('aria-valuenow')).toBe('312');
  });

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

  it('shows metadata editing only from capabilities and creates the draft on demand', async () => {
    fixtures.chooseDocumentFile.mockResolvedValueOnce('C:\\测试.epub');
    render(App);

    await fireEvent.click(screen.getByText('打开文件'));
    const editButton = await screen.findByRole('button', { name: '编辑书籍信息' });
    expect(editButton).toBeTruthy();
    await fireEvent.click(editButton);
    expect(await screen.findByRole('complementary', { name: '编辑书籍信息' })).toBeTruthy();
    const { beginEpubEdit } = await import('./lib/services/epubDocumentService');
    expect(beginEpubEdit).toHaveBeenCalledWith(openedEpub.documentId);
  });

  it('loads the Tiptap chapter editor only after the explicit editing action', async () => {
    fixtures.chooseDocumentFile.mockResolvedValueOnce('C:\\测试.epub');
    const { container } = render(App);

    await fireEvent.click(screen.getByText('打开文件'));
    await screen.findByRole('region', { name: 'EPUB 阅读器' });
    expect(container.querySelector('.ProseMirror')).toBeNull();

    await fireEvent.click(screen.getByRole('button', { name: '编辑当前章节' }));
    expect(await screen.findByRole('region', { name: 'EPUB 章节编辑器' })).toBeTruthy();
    await waitFor(() => expect(container.querySelector('.ProseMirror')).not.toBeNull());
    expect(screen.getByRole('button', { name: '退出章节编辑' })).toBeTruthy();
    const { beginEpubChapterEdit } = await import('./lib/services/epubDocumentService');
    expect(beginEpubChapterEdit).toHaveBeenCalledWith(editDraft.editSessionId, 0);
  });

  it('does not expose editing for a capability-read-only EPUB', async () => {
    vi.mocked(openEpubDocument).mockResolvedValueOnce({
      ...openedEpub,
      document: {
        ...openedEpub.document,
        capabilities: {
          ...openedEpub.document.capabilities,
          canEditText: false,
          canEditMetadata: false,
          canReplaceCover: false,
          canSaveAs: false,
        },
      },
    });
    fixtures.chooseDocumentFile.mockResolvedValueOnce('C:\\只读.epub');
    render(App);

    await fireEvent.click(screen.getByText('打开文件'));
    await screen.findByRole('region', { name: 'EPUB 阅读器' });
    expect(screen.queryByRole('button', { name: '编辑书籍信息' })).toBeNull();
  });
});
