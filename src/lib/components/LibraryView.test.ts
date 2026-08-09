import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import type { RecentDocumentDto } from '../types/epub';
import LibraryView from './LibraryView.svelte';

const documents: RecentDocumentDto[] = [
  {
    path: '\\\\?\\C:\\books\\祝福.epub',
    documentKind: 'epub',
    displayTitle: '为美好的世界献上祝福',
    author: '晓なつめ',
    fingerprint: 'epub-fingerprint',
    lastOpenedAtMs: 20,
    available: true,
  },
  {
    path: 'C:\\notes\\阶段记录.txt',
    documentKind: 'txt',
    displayTitle: '阶段记录',
    author: null,
    fingerprint: null,
    lastOpenedAtMs: 10,
    available: true,
  },
  {
    path: 'C:\\moved\\旧书.epub',
    documentKind: 'epub',
    displayTitle: '已经移动的旧书',
    author: '旧作者',
    fingerprint: 'missing-fingerprint',
    lastOpenedAtMs: 5,
    available: false,
  },
];

describe('LibraryView', () => {
  it('searches, filters and opens available library entries', async () => {
    const onOpen = vi.fn();
    render(LibraryView, { documents, onOpen });

    expect(screen.getByText('3', { selector: '.library-statistics article:first-child strong' })).toBeTruthy();
    await fireEvent.input(screen.getByLabelText('搜索书库'), { target: { value: '晓なつめ' } });

    expect(screen.getByRole('heading', { name: '为美好的世界献上祝福' })).toBeTruthy();
    expect(screen.getByText('C:/books')).toBeTruthy();
    expect(screen.queryByRole('heading', { name: '阶段记录' })).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: '打开 为美好的世界献上祝福' }));

    expect(onOpen).toHaveBeenCalledWith(documents[0]);

    await fireEvent.input(screen.getByLabelText('搜索书库'), { target: { value: '' } });
    await fireEvent.click(screen.getByRole('button', { name: 'TXT' }));
    expect(screen.getByRole('heading', { name: '阶段记录' })).toBeTruthy();
    expect(screen.queryByRole('heading', { name: '为美好的世界献上祝福' })).toBeNull();
  });

  it('marks moved files unavailable but still allows removing their record', async () => {
    const onRemove = vi.fn();
    render(LibraryView, { documents, onRemove });

    await fireEvent.click(screen.getByRole('button', { name: '已移动' }));
    const card = screen.getByRole('heading', { name: '已经移动的旧书' }).closest('article')!;

    expect((within(card).getByRole('button', { name: '打开 已经移动的旧书' }) as HTMLButtonElement).disabled).toBe(true);
    await fireEvent.click(within(card).getByRole('button', { name: '从书库移除 已经移动的旧书' }));
    expect(onRemove).toHaveBeenCalledWith(documents[2]);
  });

  it('offers import from an empty library', async () => {
    const onImport = vi.fn();
    render(LibraryView, { documents: [], onImport });

    await fireEvent.click(screen.getByRole('button', { name: '导入第一本书' }));

    expect(onImport).toHaveBeenCalledOnce();
  });
});
