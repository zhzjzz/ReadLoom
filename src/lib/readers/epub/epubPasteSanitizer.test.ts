import { describe, expect, it } from 'vitest';

import {
  MAXIMUM_EPUB_PASTE_BYTES,
  sanitizeEpubPastedHtml,
  UnsafePastedContentError,
} from './epubPasteSanitizer';

describe('sanitizeEpubPastedHtml', () => {
  it('keeps basic semantics while removing scripts, Word noise, events, styles, and trackers', () => {
    const cleaned = sanitizeEpubPastedHtml(`
      <p class="MsoNormal" style="font-family:Calibri" onclick="alert(1)">
        正文 <strong>粗体</strong><script>alert(1)</script>
      </p>
      <img src="https://tracker.invalid/pixel.gif" width="1" height="1" onerror="x" />
      <a href="javascript:alert(1)">危险链接文本</a>
      <div><em>保留语义</em></div>
    `);

    expect(cleaned).toContain('<p>');
    expect(cleaned).toContain('<strong>粗体</strong>');
    expect(cleaned).toContain('<em>保留语义</em>');
    expect(cleaned).toContain('危险链接文本');
    expect(cleaned).not.toContain('script');
    expect(cleaned).not.toContain('javascript:');
    expect(cleaned).not.toContain('tracker.invalid');
    expect(cleaned).not.toContain('Mso');
    expect(cleaned).not.toContain('onclick');
    expect(cleaned).not.toContain('font-family');
  });

  it('retains only validated links, internal images, and basic text alignment', () => {
    const cleaned = sanitizeEpubPastedHtml(`
      <p style="text-align:center"><a href="https://example.com/read">外链</a></p>
      <p><img src="../images/local.png" alt="本地图" /></p>
      <p><img src="data:text/html;base64,PHNjcmlwdD4=" /></p>
    `);
    expect(cleaned).toContain('text-align:center');
    expect(cleaned).toContain('https://example.com/read');
    expect(cleaned).toContain('../images/local.png');
    expect(cleaned).not.toContain('data:text/html');
  });

  it('rejects oversized paste before parsing it', () => {
    expect(() => sanitizeEpubPastedHtml('x'.repeat(MAXIMUM_EPUB_PASTE_BYTES + 1)))
      .toThrow(UnsafePastedContentError);
  });
});
