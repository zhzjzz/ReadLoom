import { get } from 'svelte/store';
import { describe, expect, it } from 'vitest';

import type { OpenedTextDocumentDto, SavedTextDocumentDto } from '../types/document';
import { isDirty } from '../types/document';
import { createDocumentStore } from './documentStore';

const opened: OpenedTextDocumentDto = {
  documentId: 'doc-0000000000000001',
  fileName: '中文.txt',
  displayPath: 'C:\\books\\中文.txt',
  content: '打开后的正文',
  encoding: 'utf8',
  hasBom: false,
  lineEnding: 'crlf',
  sizeBytes: 24,
  readOnly: false,
  revision: 0,
  bookmarks: [],
};

describe('documentStore', () => {
  it('opens metadata without duplicating the document content in Svelte state', () => {
    const store = createDocumentStore();
    store.open(opened);
    const state = get(store);

    expect(state.active?.fileName).toBe('中文.txt');
    expect(state.active?.encoding).toBe('utf8');
    expect(state.active?.lineEnding).toBe('crlf');
    expect(state.active).not.toHaveProperty('content');
    expect(isDirty(state.active)).toBe(false);
  });

  it('shows dirty after editing and clears it only after a successful save', () => {
    const store = createDocumentStore();
    store.open(opened);
    store.markContentDirty(true);
    expect(isDirty(get(store).active)).toBe(true);

    store.saving();
    const { content: _content, bookmarks: _bookmarks, ...openedMetadata } = opened;
    const saved: SavedTextDocumentDto = { ...openedMetadata, revision: 1 };
    store.saved(saved);

    expect(get(store).saveStatus).toBe('idle');
    expect(get(store).active?.revision).toBe(1);
    expect(isDirty(get(store).active)).toBe(false);
  });

  it('keeps dirty content and exposes the error when save fails', () => {
    const store = createDocumentStore();
    store.open(opened);
    store.markContentDirty(true);
    store.saving();
    store.failed({
      code: 'EXTERNAL_MODIFICATION',
      message: '文件已被其他程序修改。',
      recoverable: true,
      suggestedAction: '重新加载或另存为。',
    });

    expect(get(store).saveStatus).toBe('error');
    expect(get(store).error?.code).toBe('EXTERNAL_MODIFICATION');
    expect(isDirty(get(store).active)).toBe(true);
  });
});
