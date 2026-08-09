import { describe, expect, it, vi } from 'vitest';

import type { ChapterDraftAccepted, ChapterEditDto } from '../../types/epub';
import { ChapterDraftSync } from './chapterDraftSync';

function chapter(): ChapterEditDto {
  return {
    chapterEditId: 'chapter-edit-1',
    editSessionId: 'edit-1',
    documentId: 'epub-1',
    spineIndex: 0,
    manifestItemId: 'chapter',
    chapterHref: 'EPUB/chapter.xhtml',
    chapterTitle: '第一章',
    originalResourceHash: 'hash',
    editorDocument: { type: 'doc' },
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
}

function accepted(update: Parameters<ChapterDraftSync['update']>[0], revision: number): ChapterDraftAccepted {
  return {
    chapterEditId: 'chapter-edit-1',
    requestId: '',
    clientRevision: revision,
    acceptedRevision: revision,
    dirty: true,
    warnings: [],
    previewRevision: revision,
    publicationRevision: revision,
  };
}

describe('ChapterDraftSync', () => {
  it('debounces updates and never submits during composition', async () => {
    vi.useFakeTimers();
    const submit = vi.fn(async (update) => ({
      ...accepted(update.editorDocument, update.clientRevision),
      requestId: update.requestId,
    }));
    const sync = new ChapterDraftSync({
      debounceMs: 550,
      submit,
      onStatus: vi.fn(),
      onAccepted: vi.fn(),
      onError: vi.fn(),
    });
    sync.open(chapter());
    sync.compositionStart();
    sync.update({ type: 'doc', text: '中' });
    await vi.advanceTimersByTimeAsync(1000);
    expect(submit).not.toHaveBeenCalled();
    sync.compositionEnd();
    await vi.advanceTimersByTimeAsync(550);
    expect(submit).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });

  it('coalesces waiting snapshots and serializes the submission pipeline', async () => {
    const resolvers: Array<(value: ChapterDraftAccepted) => void> = [];
    const submit = vi.fn((update) => new Promise<ChapterDraftAccepted>((resolve) => {
      resolvers.push((value) => resolve({ ...value, requestId: update.requestId }));
    }));
    const acceptedEvents = vi.fn();
    const sync = new ChapterDraftSync({
      debounceMs: 1,
      submit,
      onStatus: vi.fn(),
      onAccepted: acceptedEvents,
      onError: vi.fn(),
    });
    sync.open(chapter());
    sync.update({ type: 'doc', text: 'a' });
    const flushing = sync.flush();
    await Promise.resolve();
    sync.update({ type: 'doc', text: 'latest' });
    expect(submit).toHaveBeenCalledTimes(1);
    resolvers[0]!(accepted({}, 1));
    await Promise.resolve();
    await Promise.resolve();
    expect(submit).toHaveBeenCalledTimes(2);
    resolvers[1]!(accepted({}, 2));
    await flushing;
    expect(acceptedEvents).toHaveBeenCalledTimes(2);
  });

  it('ignores mismatched old responses', async () => {
    const onAccepted = vi.fn();
    const submit = vi.fn(async (update) => ({
      ...accepted(update.editorDocument, update.clientRevision),
      chapterEditId: 'chapter-edit-old',
      requestId: update.requestId,
    }));
    const sync = new ChapterDraftSync({
      submit,
      onStatus: vi.fn(),
      onAccepted,
      onError: vi.fn(),
    });
    sync.open(chapter());
    sync.update({ type: 'doc' });
    await sync.flush();
    expect(onAccepted).not.toHaveBeenCalled();
  });
});
