import { EditorView } from '@codemirror/view';
import { render, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import type { TextEditorHandle } from '../types/document';
import TextEditor from './TextEditor.svelte';

describe('TextEditor', () => {
  it('displays opened Chinese, English and emoji content after the lazy chunk loads', async () => {
    let handle: TextEditorHandle | null = null;
    const { container } = render(TextEditor, {
      initialContent: '第一行中文\nEnglish 😀',
      onReady: (value) => (handle = value),
    });

    await waitFor(() => expect(handle).not.toBeNull());
    expect(handle!.getContent()).toBe('第一行中文\nEnglish 😀');
    expect(container.querySelector('.cm-content')?.textContent).toContain('第一行中文');
  });

  it('starts in protected mode and toggles editability through its handle', async () => {
    let handle: TextEditorHandle | null = null;
    const { container } = render(TextEditor, {
      initialContent: '防误触内容',
      onReady: (value) => (handle = value),
    });

    await waitFor(() => expect(handle).not.toBeNull());
    expect(container.querySelector('.cm-content')?.getAttribute('contenteditable')).toBe('false');

    handle!.setEditing(true);
    await waitFor(() =>
      expect(container.querySelector('.cm-content')?.getAttribute('contenteditable')).toBe('true'),
    );

    handle!.setEditing(false);
    await waitFor(() =>
      expect(container.querySelector('.cm-content')?.getAttribute('contenteditable')).toBe('false'),
    );
  });

  it('restores the last saved content when editing is discarded', async () => {
    let handle: TextEditorHandle | null = null;
    const onDirtyChange = vi.fn();
    const { container } = render(TextEditor, {
      initialContent: '上次保存的内容',
      onDirtyChange,
      onReady: (value) => (handle = value),
    });

    await waitFor(() => expect(handle).not.toBeNull());
    const editorView = EditorView.findFromDOM(container.querySelector('.cm-editor') as HTMLElement);
    expect(editorView).not.toBeNull();
    editorView!.dispatch({ changes: { from: editorView!.state.doc.length, insert: '（已修改）' } });

    expect(handle!.getContent()).toBe('上次保存的内容（已修改）');
    expect(onDirtyChange).toHaveBeenLastCalledWith(true);

    handle!.discardChanges();
    expect(handle!.getContent()).toBe('上次保存的内容');
    expect(onDirtyChange).toHaveBeenLastCalledWith(false);
  });
});
