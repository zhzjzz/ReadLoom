import { open } from '@tauri-apps/plugin-dialog';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
  chooseDocumentFile,
  chooseLibraryDirectory,
  chooseLibraryFiles,
  classifyDocumentPath,
} from './workspaceFileService';

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

  it('allows Ctrl or Shift multi-selection when importing library books', async () => {
    vi.mocked(open).mockResolvedValue(['C:\\books\\one.epub', 'C:\\books\\two.txt']);

    await expect(chooseLibraryFiles()).resolves.toEqual([
      'C:\\books\\one.epub',
      'C:\\books\\two.txt',
    ]);
    expect(open).toHaveBeenCalledWith({
      directory: false,
      filters: [{ name: '图书', extensions: ['epub', 'txt'] }],
      multiple: true,
      title: '批量导入图书',
    });
  });

  it('offers a directory picker for recursive library import', async () => {
    vi.mocked(open).mockResolvedValue('C:\\books');

    await expect(chooseLibraryDirectory()).resolves.toBe('C:\\books');
    expect(open).toHaveBeenCalledWith({
      directory: true,
      multiple: false,
      title: '选择图书目录',
    });
  });
});
