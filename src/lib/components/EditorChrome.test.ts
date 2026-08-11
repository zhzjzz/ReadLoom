import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import type { DocumentSession } from '../types/document';
import EditorStatusBar from './EditorStatusBar.svelte';
import EditorToolbar from './EditorToolbar.svelte';
import TopBar from './TopBar.svelte';
import UnsavedChangesDialog from './UnsavedChangesDialog.svelte';

const document: DocumentSession = {
  documentId: 'doc-0000000000000001',
  fileName: '示例.txt',
  displayPath: 'C:\\示例.txt',
  encoding: 'utf8',
  hasBom: true,
  savedEncoding: 'utf8',
  savedHasBom: true,
  lineEnding: 'mixed',
  lineEndingChoice: 'crlf',
  sizeBytes: 2048,
  readOnly: false,
  revision: 0,
  contentDirty: true,
  formatDirty: true,
};

describe('editor chrome', () => {
  it('shows encoding, BOM, mixed line endings and the unsaved marker', () => {
    render(EditorStatusBar, { document, statistics: { lines: 3, characters: 18 } });

    expect(screen.getByText('未保存')).toBeTruthy();
    expect(screen.getByText('UTF-8 BOM')).toBeTruthy();
    expect(screen.getByText('Mixed')).toBeTruthy();
    expect(screen.getByText('3 行')).toBeTruthy();
  });

  it('exposes format controls and save actions', async () => {
    const onOptionsChange = vi.fn();
    const onSave = vi.fn();
    render(EditorToolbar, {
      document,
      editing: true,
      onOpen: vi.fn(),
      onToggleEditing: vi.fn(),
      onSave,
      onSaveAs: vi.fn(),
      onClose: vi.fn(),
      onReopen: vi.fn(),
      onOptionsChange,
    });

    await fireEvent.click(screen.getByTitle('保存'));
    await fireEvent.change(screen.getByLabelText('保存换行符'), { target: { value: 'lf' } });
    expect(onSave).toHaveBeenCalledOnce();
    expect(onOptionsChange).toHaveBeenCalledWith(expect.objectContaining({ lineEnding: 'lf' }));
  });

  it('switches between start and exit editing without adding long action labels', async () => {
    const onToggleEditing = vi.fn();
    const props = {
      document,
      editing: false,
      onOpen: vi.fn(),
      onToggleEditing,
      onSave: vi.fn(),
      onSaveAs: vi.fn(),
      onClose: vi.fn(),
      onReopen: vi.fn(),
      onOptionsChange: vi.fn(),
    };
    const { rerender } = render(EditorToolbar, props);

    await fireEvent.click(screen.getByText('开始编辑'));
    expect(onToggleEditing).toHaveBeenCalledOnce();
    expect((screen.getByTitle('保存') as HTMLButtonElement).disabled).toBe(true);

    await rerender({ ...props, editing: true });
    expect(screen.getByText('退出编辑')).toBeTruthy();
    expect((screen.getByTitle('保存') as HTMLButtonElement).disabled).toBe(false);
  });

  it('offers an x button after the active document title', async () => {
    const onClose = vi.fn();
    render(TopBar, {
      connection: { status: 'browser-preview' },
      document,
      onClose,
    });

    await fireEvent.click(screen.getByRole('button', { name: '关闭 示例.txt' }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('requires an explicit choice before discarding an unsaved document', async () => {
    const onDiscard = vi.fn();
    const onCancel = vi.fn();
    render(UnsavedChangesDialog, {
      fileName: '示例.txt',
      onSave: vi.fn(),
      onDiscard,
      onCancel,
    });

    expect(screen.getByRole('dialog')).toBeTruthy();
    expect(screen.getByText('保存对“示例.txt”的更改？')).toBeTruthy();
    await fireEvent.click(screen.getByText('不保存'));
    expect(onDiscard).toHaveBeenCalledOnce();
  });
});
