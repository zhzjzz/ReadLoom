import { describe, expect, it } from 'vitest';

import { epubResourceUrl } from './epubDocumentService';

describe('Windows EPUB protocol URL', () => {
  it('uses the Tauri WebView2 mapped host and encodes resource segments once', () => {
    const sessionId = '0123456789abcdef0123456789abcdef0123456789abcdef';

    expect(epubResourceUrl(sessionId, 'EPUB/中文 chapter.xhtml', '第一节')).toBe(
      `http://readloom-epub.localhost/${sessionId}/EPUB/%E4%B8%AD%E6%96%87%20chapter.xhtml#%E7%AC%AC%E4%B8%80%E8%8A%82`,
    );
  });
});
