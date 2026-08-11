import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import type { TextHeading } from '../editors/textHeadings';
import TextToolsPane from './TextToolsPane.svelte';

describe('TextToolsPane', () => {
  it('places the TXT outline beside the editor and reveals headings', async () => {
    const heading: TextHeading = { label: '第十二章 风起', lineNumber: 42, from: 320, to: 327 };
    const onRevealTextOffset = vi.fn();
    render(TextToolsPane, { textHeadings: [heading], onRevealTextOffset });

    await fireEvent.click(screen.getByRole('button', { name: '第 42 行 第十二章 风起' }));

    expect(screen.getByRole('complementary', { name: 'TXT 目录与工具' })).toBeTruthy();
    expect(onRevealTextOffset).toHaveBeenCalledWith(320);
  });

  it('adds TXT bookmarks and reveals full-document search results', async () => {
    const onAddTextBookmark = vi.fn();
    const onRevealTextOffset = vi.fn();
    render(TextToolsPane, {
      getTextContent: () => '第一行\n目标正文\n第三行目标',
      onAddTextBookmark,
      onRevealTextOffset,
    });

    await fireEvent.click(screen.getByRole('button', { name: /添加/ }));
    expect(onAddTextBookmark).toHaveBeenCalledOnce();

    await fireEvent.input(screen.getByLabelText('TXT 全文检索'), { target: { value: '目标' } });
    await fireEvent.click(screen.getByRole('button', { name: '搜索 TXT 全文' }));
    expect(screen.getByText('共 2 条结果')).toBeTruthy();
    expect(screen.getByText('01', { selector: '.result-index' })).toBeTruthy();
    expect(screen.getByText('02', { selector: '.result-index' })).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: /第 2 行.*目标正文/ }));

    expect(onRevealTextOffset).toHaveBeenCalledWith(4);
  });

  it('supports keyboard resizing for the TXT directory pane', async () => {
    render(TextToolsPane);
    const separator = screen.getByRole('separator', { name: '调整 TXT 目录宽度' });

    expect(separator.getAttribute('aria-valuenow')).toBe('240');
    await fireEvent.keyDown(separator, { key: 'ArrowRight' });

    expect(separator.getAttribute('aria-valuenow')).toBe('252');
  });
});
