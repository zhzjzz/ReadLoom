import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import type { RecentDocumentDto } from '../types/epub';
import type { TextHeading } from '../editors/textHeadings';
import NavigationPane from './NavigationPane.svelte';

const recentDocument: RecentDocumentDto = {
  path: 'C:\\books\\第一本.txt',
  documentKind: 'txt',
  displayTitle: '第一本',
  author: null,
  fingerprint: null,
  lastOpenedAtMs: 1,
  available: true,
};

describe('NavigationPane', () => {
  it('switches between the document workspace and the library', async () => {
    const onSelectWorkspace = vi.fn();
    const onSelectLibrary = vi.fn();
    render(NavigationPane, {
      desktopRuntime: true,
      onOpen: vi.fn(),
      activeView: 'library',
      onSelectWorkspace,
      onSelectLibrary,
    });

    expect(screen.getByRole('button', { name: '书库' }).getAttribute('aria-current')).toBe('page');
    await fireEvent.click(screen.getByRole('button', { name: '阅读与编辑' }));
    await fireEvent.click(screen.getByRole('button', { name: '书库' }));

    expect(onSelectWorkspace).toHaveBeenCalledOnce();
    expect(onSelectLibrary).toHaveBeenCalledOnce();
  });

  it('removes a recent record from its small close button without opening it', async () => {
    const onOpenRecent = vi.fn();
    const onRemoveRecent = vi.fn();
    render(NavigationPane, {
      desktopRuntime: true,
      onOpen: vi.fn(),
      recentDocuments: [recentDocument],
      onOpenRecent,
      onRemoveRecent,
    });

    await fireEvent.click(screen.getByRole('button', { name: '从最近文件中移除 第一本' }));

    expect(onRemoveRecent).toHaveBeenCalledWith(recentDocument);
    expect(onOpenRecent).not.toHaveBeenCalled();
  });

  it('reveals a recognized TXT heading from the outline', async () => {
    const heading: TextHeading = { label: '第十二章 风起', lineNumber: 42, from: 320, to: 327 };
    const onRevealHeading = vi.fn();
    render(NavigationPane, {
      desktopRuntime: true,
      onOpen: vi.fn(),
      textHeadings: [heading],
      onRevealHeading,
    });

    await fireEvent.click(screen.getByRole('button', { name: '第 42 行 第十二章 风起' }));

    expect(onRevealHeading).toHaveBeenCalledWith(heading);
  });

  it('adds TXT bookmarks and reveals full-document search results', async () => {
    const onAddTextBookmark = vi.fn();
    const onRevealTextOffset = vi.fn();
    render(NavigationPane, {
      desktopRuntime: true,
      onOpen: vi.fn(),
      activeTextDocument: true,
      textContent: '第一行\n目标正文\n第三行目标',
      onAddTextBookmark,
      onRevealTextOffset,
    });

    await fireEvent.click(screen.getByRole('button', { name: '添加 TXT 书签' }));
    expect(onAddTextBookmark).toHaveBeenCalledOnce();

    await fireEvent.input(screen.getByLabelText('TXT 全文检索'), { target: { value: '目标' } });
    await fireEvent.click(screen.getByRole('button', { name: '搜索 TXT 全文' }));
    await fireEvent.click(screen.getByRole('button', { name: /第 2 行.*目标正文/ }));

    expect(onRevealTextOffset).toHaveBeenCalledWith(4);
  });
});
