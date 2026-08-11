import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import LibraryImportReviewDialog from './LibraryImportReviewDialog.svelte';

const preview = {
  rootPath: 'D:\\Books',
  totalSizeBytes: 3072,
  importable: 2,
  alreadyImported: 1,
  candidates: [
    { path: 'D:\\Books\\活着.epub', fileName: '活着.epub', documentKind: 'epub' as const, sizeBytes: 1024, alreadyImported: false },
    { path: 'D:\\Books\\三体.txt', fileName: '三体.txt', documentKind: 'txt' as const, sizeBytes: 1024, alreadyImported: false },
    { path: 'D:\\Books\\旧书.epub', fileName: '旧书.epub', documentKind: 'epub' as const, sizeBytes: 1024, alreadyImported: true },
  ],
};

describe('LibraryImportReviewDialog', () => {
  it('separates existing books and imports only checked new books', async () => {
    const onConfirm = vi.fn();
    render(LibraryImportReviewDialog, { preview, onConfirm });

    expect(screen.getByText('扫描到 3 本 · 可导入 2 本 · 已在书库 1 本')).toBeTruthy();
    expect((screen.getByLabelText('选择 旧书.epub') as HTMLInputElement).disabled).toBe(true);
    await fireEvent.click(screen.getByLabelText('选择 三体.txt'));
    await fireEvent.click(screen.getByRole('button', { name: '导入所选图书' }));

    expect(onConfirm).toHaveBeenCalledWith(['D:\\Books\\活着.epub']);
  });

  it('filters the review without losing the selection state', async () => {
    render(LibraryImportReviewDialog, { preview });

    await fireEvent.click(screen.getByRole('radio', { name: '已在书库' }));
    expect(screen.getByText('旧书.epub')).toBeTruthy();
    expect(screen.queryByText('活着.epub')).toBeNull();
    await fireEvent.click(screen.getByRole('radio', { name: '全部' }));
    expect((screen.getByLabelText('选择 活着.epub') as HTMLInputElement).checked).toBe(true);
  });
});
