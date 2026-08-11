import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { LibraryDocumentDto, LibraryGroupDto } from '../types/library';
import LibraryView from './LibraryView.svelte';

const groups: LibraryGroupDto[] = [
  {
    groupId: 'group-fiction',
    name: '小说',
    position: 0,
    createdAtMs: 1,
    updatedAtMs: 1,
  },
];

const documents: LibraryDocumentDto[] = [
  {
    path: '\\\\?\\C:\\books\\祝福.epub',
    documentKind: 'epub',
    displayTitle: '为美好的世界献上祝福',
    author: '晓なつめ',
    fingerprint: 'epub-fingerprint',
    lastOpenedAtMs: 20,
    available: true,
    groupId: 'group-fiction',
    coverKey: 'a'.repeat(64),
  },
  {
    path: 'C:\\notes\\阶段记录.txt',
    documentKind: 'txt',
    displayTitle: '阶段记录',
    author: null,
    fingerprint: null,
    lastOpenedAtMs: 10,
    available: true,
    groupId: null,
    coverKey: null,
  },
  {
    path: 'C:\\moved\\旧书.epub',
    documentKind: 'epub',
    displayTitle: '已经移动的旧书',
    author: '旧作者',
    fingerprint: 'missing-fingerprint',
    lastOpenedAtMs: 5,
    available: false,
    groupId: null,
    coverKey: null,
  },
];

describe('LibraryView', () => {
  beforeEach(() => window.localStorage.clear());

  it('searches, filters and opens available library entries', async () => {
    const onOpen = vi.fn();
    render(LibraryView, { documents, groups, onOpen });

    expect(screen.getByText('3', { selector: '.result-summary strong' })).toBeTruthy();
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
    render(LibraryView, { documents, groups, onRemove });

    await fireEvent.click(screen.getByRole('button', { name: '已移动' }));
    const card = screen.getByRole('heading', { name: '已经移动的旧书' }).closest('article')!;

    expect((within(card).getByRole('button', { name: '打开 已经移动的旧书' }) as HTMLButtonElement).disabled).toBe(true);
    await fireEvent.click(within(card).getByRole('button', { name: '从书库移除 已经移动的旧书' }));
    expect(onRemove).toHaveBeenCalledWith(documents[2]);
  });

  it('cleans invalid records in one action and truncates long fallback titles by column count', async () => {
    const onRemoveUnavailable = vi.fn();
    const longTitle = '这是一个非常非常非常非常非常非常长的默认封面标题用于测试截断';
    const longDocument: LibraryDocumentDto = {
      ...documents[1],
      path: 'C:\\books\\long-title.txt',
      displayTitle: longTitle,
    };
    render(LibraryView, {
      documents: [longDocument, documents[2]],
      columns: 5,
      onRemoveUnavailable,
    });

    expect(screen.getByText(`${[...longTitle].slice(0, 19).join('')}…`, { selector: '.fallback-title' })).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: '清理无效书籍' }));

    expect(onRemoveUnavailable).toHaveBeenCalledOnce();
  });

  it('offers file and directory import from an empty library', async () => {
    const onImportFiles = vi.fn();
    const onImportDirectory = vi.fn();
    render(LibraryView, { documents: [], onImportFiles, onImportDirectory });

    await fireEvent.click(screen.getByRole('button', { name: '选择图书' }));
    await fireEvent.click(screen.getByRole('button', { name: '选择目录' }));

    expect(onImportFiles).toHaveBeenCalledOnce();
    expect(onImportDirectory).toHaveBeenCalledOnce();
  });

  it('renders extracted covers and receives the 3 to 5 column choice from settings', () => {
    const { container } = render(LibraryView, { documents, groups, columns: 5 });

    const cover = screen.getByRole('img', { name: '为美好的世界献上祝福 封面' });
    expect(cover.getAttribute('src')).toBe(`http://readloom-library.localhost/${'a'.repeat(64)}`);
    expect((container.querySelector('.library-view') as HTMLElement).style.getPropertyValue('--library-columns')).toBe('5');
    expect(screen.queryByRole('button', { name: '5 本' })).toBeNull();
    expect(screen.getByText('阶段记录', { selector: '.fallback-title' })).toBeTruthy();
  });

  it('creates shelf groups and moves books between them', async () => {
    const onCreateGroup = vi.fn();
    const onMoveToGroup = vi.fn();
    render(LibraryView, { documents, groups, onCreateGroup, onMoveToGroup });

    expect(screen.getByRole('heading', { name: '小说' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: '未分组' })).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: '新建分组' }));
    await fireEvent.input(screen.getByLabelText('新分组名称'), { target: { value: '待读' } });
    await fireEvent.click(screen.getByRole('button', { name: '创建书架' }));

    expect(onCreateGroup).toHaveBeenCalledWith('待读');

    await fireEvent.change(screen.getByLabelText('设置 阶段记录 的分组'), {
      target: { value: 'group-fiction' },
    });
    expect(onMoveToGroup).toHaveBeenCalledWith(documents[1], 'group-fiction');
  });

  it('renames and deletes groups while explaining that books become ungrouped', async () => {
    const onRenameGroup = vi.fn();
    const onDeleteGroup = vi.fn();
    vi.spyOn(window, 'prompt').mockReturnValue('长篇小说');
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    render(LibraryView, { documents, groups, onRenameGroup, onDeleteGroup });

    await fireEvent.click(screen.getByRole('button', { name: '重命名分组 小说' }));
    await fireEvent.click(screen.getByRole('button', { name: '删除分组 小说' }));

    expect(onRenameGroup).toHaveBeenCalledWith(groups[0], '长篇小说');
    expect(window.confirm).toHaveBeenCalledWith('删除分组“小说”？其中的书会移到“未分组”。');
    expect(onDeleteGroup).toHaveBeenCalledWith(groups[0]);
  });
});
