import { open } from '@tauri-apps/plugin-dialog';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { chooseDocumentFile, classifyDocumentPath } from './workspaceFileService';

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(),
}));

describe('workspace file selection', () => {
  beforeEach(() => vi.mocked(open).mockReset());

  it('shows one unfiltered file picker so every file can be imported', async () => {
    vi.mocked(open).mockResolvedValue('C:\\books\\notes.markdown');

    await expect(chooseDocumentFile()).resolves.toBe('C:\\books\\notes.markdown');
    expect(open).toHaveBeenCalledWith({
      directory: false,
      multiple: false,
      title: '打开文件',
    });
  });

  it('routes EPUB by extension and treats every other extension as text', () => {
    expect(classifyDocumentPath('C:\\books\\novel.EPUB')).toBe('epub');
    expect(classifyDocumentPath('C:\\books\\notes.txt')).toBe('text');
    expect(classifyDocumentPath('C:\\books\\notes.md')).toBe('text');
    expect(classifyDocumentPath('C:\\books\\README')).toBe('text');
  });
});
