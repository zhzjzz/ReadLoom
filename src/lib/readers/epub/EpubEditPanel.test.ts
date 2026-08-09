import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import type { EpubEditDraft } from '../../types/epub';
import EpubEditPanel from './EpubEditPanel.svelte';

const draft: EpubEditDraft = {
  editSessionId: 'edit-0123456789abcdef',
  documentId: 'epub-0000000000000001',
  sourcePath: 'C:\书.epub',
  publicationId: 'urn:test',
  opfResourceId: 'EPUB/package.opf',
  metadata: {
    title: '原书名',
    creators: ['作者一', '作者二'],
    contributors: [],
    language: 'zh-CN',
    publisher: null,
    description: null,
    identifier: 'urn:test',
    publicationDate: null,
    subjects: ['主题'],
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

describe('EpubEditPanel', () => {
  it('edits metadata through a bounded DTO and keeps save disabled while clean', async () => {
    const onMetadataChange = vi.fn();
    render(EpubEditPanel, {
      draft,
      previewUrl: null,
      onMetadataChange,
      onReplaceCover: vi.fn(),
      onRestoreCover: vi.fn(),
      onSaveAs: vi.fn(),
      onCancelSave: vi.fn(),
      onDiscard: vi.fn(),
      onClose: vi.fn(),
    });

    expect(screen.getByText('安全另存为，不会覆盖原 EPUB')).toBeTruthy();
    expect(screen.getByRole('button', { name: '另存为 EPUB' }).hasAttribute('disabled')).toBe(true);
    await fireEvent.input(screen.getByLabelText('书名'), { target: { value: '新书名' } });
    await fireEvent.click(screen.getByRole('button', { name: '应用元数据' }));
    expect(onMetadataChange).toHaveBeenCalledWith(expect.objectContaining({
      title: '新书名',
      creators: ['作者一', '作者二'],
      language: 'zh-CN',
    }));
  });

  it('renders a protocol URL preview and allows restoring a replaced cover', async () => {
    const onRestoreCover = vi.fn();
    render(EpubEditPanel, {
      draft: {
        ...draft,
        dirty: true,
        validation: { ...draft.validation, canSave: true },
        cover: {
          state: 'replaced',
          originalResourceId: null,
          currentResourceId: 'EPUB/readloom-assets/cover.png',
          previewResourceId: '__readloom_edit/edit-1/cover',
          mediaType: 'image/png',
          width: 600,
          height: 800,
        },
      },
      previewUrl: 'http://readloom-epub.localhost/session/edit-cover',
      onMetadataChange: vi.fn(),
      onReplaceCover: vi.fn(),
      onRestoreCover,
      onSaveAs: vi.fn(),
      onCancelSave: vi.fn(),
      onDiscard: vi.fn(),
      onClose: vi.fn(),
    });

    expect(screen.getByAltText('当前 EPUB 封面预览').getAttribute('src')).toContain('readloom-epub.localhost');
    await fireEvent.click(screen.getByRole('button', { name: '恢复原封面' }));
    expect(onRestoreCover).toHaveBeenCalledOnce();
    expect(screen.getByRole('button', { name: '另存为 EPUB' }).hasAttribute('disabled')).toBe(false);
  });
});
