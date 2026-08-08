import { describe, expect, it, vi } from 'vitest';

import { createLazyTextEditorLoader } from './textEditorModules';

describe('CodeMirror lazy loading', () => {
  it('does not import editor code until a document asks for it and reuses the chunk', async () => {
    const importer = vi.fn(async () => ({ editor: 'loaded' }));
    const loader = createLazyTextEditorLoader(importer);

    expect(importer).not.toHaveBeenCalled();
    await expect(loader.load()).resolves.toEqual({ editor: 'loaded' });
    await loader.load();
    expect(importer).toHaveBeenCalledOnce();
  });
});
