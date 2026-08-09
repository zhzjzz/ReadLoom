export const MAXIMUM_EPUB_PASTE_BYTES = 2 * 1024 * 1024;

const allowedTags = new Set([
  'p', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'br', 'strong', 'b', 'em', 'i', 's',
  'strike', 'del', 'u', 'sub', 'sup', 'blockquote', 'ul', 'ol', 'li', 'hr', 'a', 'img',
]);
const removeWithContent = new Set([
  'script', 'style', 'iframe', 'object', 'embed', 'form', 'input', 'button', 'meta', 'link',
  'template', 'canvas', 'audio', 'video', 'svg', 'math',
]);
const commonAttributes = new Set(['id', 'lang', 'dir', 'title']);

export class UnsafePastedContentError extends Error {
  constructor(message = '粘贴内容超过安全限制或包含无法清理的结构。') {
    super(message);
    this.name = 'UnsafePastedContentError';
  }
}

export function sanitizeEpubPastedHtml(source: string): string {
  if (new TextEncoder().encode(source).byteLength > MAXIMUM_EPUB_PASTE_BYTES) {
    throw new UnsafePastedContentError('粘贴内容超过 2 MiB 安全上限。');
  }
  const parsed = new DOMParser().parseFromString(source, 'text/html');
  if (parsed.querySelector('parsererror')) throw new UnsafePastedContentError();
  for (const element of [...parsed.body.querySelectorAll('*')].reverse()) {
    const tag = element.tagName.toLowerCase();
    if (removeWithContent.has(tag)) {
      element.remove();
      continue;
    }
    const originalStyle = element.getAttribute('style')?.toLowerCase() ?? '';
    if (/display\s*:\s*none|visibility\s*:\s*hidden|opacity\s*:\s*0(?:\D|$)|font-size\s*:\s*0/.test(originalStyle)) {
      element.remove();
      continue;
    }
    if (!allowedTags.has(tag)) {
      element.replaceWith(...element.childNodes);
      continue;
    }
    for (const attribute of [...element.attributes]) {
      const name = attribute.name.toLowerCase();
      const allowed = commonAttributes.has(name)
        || (tag === 'a' && ['href', 'title'].includes(name))
        || (tag === 'img' && ['src', 'alt', 'title', 'width', 'height'].includes(name))
        || (tag === 'ol' && name === 'start')
        || (name === 'style' && /^(?:\s*text-align\s*:\s*(?:left|center|right|justify)\s*;?\s*)$/i.test(attribute.value));
      if (!allowed || name.startsWith('on') || name === 'class') element.removeAttribute(attribute.name);
    }
    if (tag === 'a') {
      const href = element.getAttribute('href')?.trim() ?? '';
      if (!isSafeLink(href)) element.removeAttribute('href');
    }
    if (tag === 'img') {
      const src = element.getAttribute('src')?.trim() ?? '';
      const width = Number(element.getAttribute('width'));
      const height = Number(element.getAttribute('height'));
      if (!isSafeInternalImage(src) || (width > 0 && width <= 1) || (height > 0 && height <= 1)) {
        element.remove();
      }
    }
  }
  return parsed.body.innerHTML;
}

function isSafeLink(href: string): boolean {
  if (!href || href.length > 2048 || href.includes('\\') || href.startsWith('//')) return false;
  if (href.startsWith('#') || /^https?:\/\//i.test(href)) return true;
  return !/^[a-z][a-z\d+.-]*:/i.test(href) && !href.includes('\0');
}

function isSafeInternalImage(src: string): boolean {
  if (!src || src.length > 2048 || src.includes('\\') || src.startsWith('//')) return false;
  if (/^https?:\/\//i.test(src)) return /^http:\/\/readloom-epub\.localhost\/[a-f\d]{48}\//i.test(src);
  return !/^[a-z][a-z\d+.-]*:/i.test(src) && !src.includes('\0');
}
