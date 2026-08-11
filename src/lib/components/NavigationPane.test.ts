import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import NavigationPane from './NavigationPane.svelte';

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

  it('keeps recent files out of the navigation pane', () => {
    render(NavigationPane, {
      desktopRuntime: true,
      onOpen: vi.fn(),
    });

    expect(screen.queryByRole('heading', { name: '最近文件' })).toBeNull();
  });

});
