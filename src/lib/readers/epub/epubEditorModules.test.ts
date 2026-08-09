import { describe, expect, it } from 'vitest';

import { editorModulesLoadedForTest, loadEpubEditorModules } from './epubEditorModules';

describe('EPUB editor dynamic modules', () => {
  it('does not load Tiptap until the explicit chapter-edit boundary', async () => {
    expect(editorModulesLoadedForTest()).toBe(false);
    const modules = await loadEpubEditorModules();
    expect(modules.Editor).toBeTypeOf('function');
    expect(modules.Mark).toBeTypeOf('function');
    expect(modules.Document.name).toBe('doc');
    expect(editorModulesLoadedForTest()).toBe(true);
  });
});
