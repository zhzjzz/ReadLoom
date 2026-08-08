import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { OpenedEpubDocumentDto } from '../../types/epub';
import EpubReader from './EpubReader.svelte';

const serviceMocks = vi.hoisted(() => ({
  saveProgress: vi.fn(async (locator) => locator),
  saveBookmark: vi.fn(),
  deleteBookmark: vi.fn(),
  search: vi.fn(async () => []),
  cancel: vi.fn(async () => {}),
}));

vi.mock('../../services/epubDocumentService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../services/epubDocumentService')>()),
  saveEpubProgress: serviceMocks.saveProgress,
  saveEpubBookmark: serviceMocks.saveBookmark,
  deleteEpubBookmark: serviceMocks.deleteBookmark,
  searchEpubDocument: serviceMocks.search,
  cancelEpubSearch: serviceMocks.cancel,
}));

const document: OpenedEpubDocumentDto = {
  documentId: 'doc-0000000000000001',
  sessionId: '0123456789abcdef0123456789abcdef0123456789abcdef',
  bridgeToken: 'abcdef0123456789abcdef0123456789abcdef0123456789',
  fileName: '测试.epub',
  displayPath: 'C:\书籍\测试.epub',
  fileFingerprint: 'fingerprint',
  initialLocator: null,
  bookmarks: [],
  document: {
    kind: 'epub',
    publicationId: 'urn:test',
    version: '3.0',
    metadata: {
      title: '阅织测试书',
      creators: ['测试作者'],
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
      { id: 'one', resourceId: 'EPUB/one.xhtml', mediaType: 'application/xhtml+xml', properties: [] },
      { id: 'two', resourceId: 'EPUB/two.xhtml', mediaType: 'application/xhtml+xml', properties: [] },
    ],
    spine: [
      { index: 0, idref: 'one', resourceId: 'EPUB/one.xhtml', mediaType: 'application/xhtml+xml', linear: true, properties: [] },
      { index: 1, idref: 'two', resourceId: 'EPUB/two.xhtml', mediaType: 'application/xhtml+xml', linear: true, properties: [] },
    ],
    toc: [
      {
        id: 'part',
        label: '第一部',
        resourceId: 'EPUB/one.xhtml',
        fragment: null,
        children: [
          { id: 'chapter-two', label: '第二章', resourceId: 'EPUB/two.xhtml', fragment: 'start', children: [] },
        ],
      },
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

describe('EpubReader', () => {
  beforeEach(() => vi.clearAllMocks());

  it('renders nested navigation, switches chapters, and exposes no editing action', async () => {
    const onSpineChange = vi.fn();
    render(EpubReader, { document, spineIndex: 0, onSpineChange });

    expect(screen.getAllByText('第一部')).toHaveLength(2);
    expect(screen.getByText('第二章')).toBeTruthy();
    await fireEvent.click(screen.getByLabelText('下一章'));
    expect(onSpineChange).toHaveBeenCalledWith(1);
    expect(screen.queryByText('开始编辑')).toBeNull();
    expect(screen.queryByText('保存')).toBeNull();

    await fireEvent.click(screen.getByLabelText('折叠 第一部'));
    expect(screen.queryByText('第二章')).toBeNull();
  });

  it('keeps the iframe opaque and grants only the trusted bridge script permission', () => {
    const { container } = render(EpubReader, {
      document,
      spineIndex: 0,
      onSpineChange: vi.fn(),
    });
    const iframe = container.querySelector('iframe');

    expect(iframe?.getAttribute('sandbox')).toBe('allow-scripts');
    expect(iframe?.getAttribute('sandbox')).not.toContain('allow-same-origin');
    expect(iframe?.getAttribute('referrerpolicy')).toBe('no-referrer');
  });

  it('shows an explicit compatibility warning for fixed-layout publications', () => {
    render(EpubReader, {
      document: { ...document, document: { ...document.document, layout: 'fixed' } },
      spineIndex: 0,
      onSpineChange: vi.fn(),
    });

    expect(screen.getByText(/固定布局 EPUB/)).toBeTruthy();
  });

  it('reports external links in the host and only offers copy or cancel', async () => {
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });
    const { container } = render(EpubReader, {
      document,
      spineIndex: 0,
      onSpineChange: vi.fn(),
    });
    const iframe = container.querySelector('iframe');
    expect(iframe?.contentWindow).toBeTruthy();

    window.dispatchEvent(
      new MessageEvent('message', {
        source: iframe!.contentWindow,
        data: {
          source: 'readloom-epub',
          version: 1,
          type: 'link',
          documentId: document.documentId,
          sessionId: document.sessionId,
          token: document.bridgeToken,
          payload: {
            href: 'readloom-external:https%3A%2F%2Fexample%2Ecom%2Fread%3Fq%3D1',
          },
        },
      }),
    );

    expect(await screen.findByRole('dialog', { name: '外部链接' })).toBeTruthy();
    expect(screen.getByText('example.com')).toBeTruthy();
    expect(screen.queryByText('打开浏览器')).toBeNull();
    await fireEvent.click(screen.getByText('复制链接'));
    expect(writeText).toHaveBeenCalledWith('https://example.com/read?q=1');
  });
});
