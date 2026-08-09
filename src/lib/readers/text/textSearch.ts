export interface TextSearchResult {
  characterOffset: number;
  lineNumber: number;
  matchStart: number;
  matchEnd: number;
  snippet: string;
}

export interface TextSearchOptions {
  caseSensitive?: boolean;
  wholeWord?: boolean;
  maximumResults?: number;
}

export interface TextPositionDescription {
  characterOffset: number;
  lineNumber: number;
  preview: string;
}

export function describeTextPosition(content: string, offset: number): TextPositionDescription {
  const characterOffset = Math.max(0, Math.min(offset, content.length));
  const lineStart = content.lastIndexOf('\n', Math.max(0, characterOffset - 1)) + 1;
  const foundLineEnd = content.indexOf('\n', characterOffset);
  const lineEnd = foundLineEnd < 0 ? content.length : foundLineEnd;
  const lineNumber = content.slice(0, lineStart).split('\n').length;
  const preview = content.slice(lineStart, lineEnd).trim().slice(0, 160) || `第 ${lineNumber} 行`;
  return { characterOffset, lineNumber, preview };
}

export function searchTextDocument(
  content: string,
  query: string,
  options: TextSearchOptions = {},
): TextSearchResult[] {
  const needle = query.trim();
  if (!needle) return [];
  const maximumResults = Math.max(1, Math.min(options.maximumResults ?? 100, 500));
  const expression = new RegExp(escapeRegExp(needle), options.caseSensitive ? 'gu' : 'giu');
  const results: TextSearchResult[] = [];
  let lineNumber = 1;
  let nextLineBreak = content.indexOf('\n');

  for (const match of content.matchAll(expression)) {
    const characterOffset = match.index;
    if (options.wholeWord && !isWholeWord(content, characterOffset, match[0].length)) continue;
    while (nextLineBreak >= 0 && nextLineBreak < characterOffset) {
      lineNumber += 1;
      nextLineBreak = content.indexOf('\n', nextLineBreak + 1);
    }
    const lineStart = content.lastIndexOf('\n', Math.max(0, characterOffset - 1)) + 1;
    const foundLineEnd = content.indexOf('\n', characterOffset + match[0].length);
    const lineEnd = foundLineEnd < 0 ? content.length : foundLineEnd;
    const snippet = content.slice(lineStart, lineEnd).trim();
    const leadingWhitespace = content.slice(lineStart, lineEnd).length - content.slice(lineStart, lineEnd).trimStart().length;
    results.push({
      characterOffset,
      lineNumber,
      matchStart: Math.max(0, characterOffset - lineStart - leadingWhitespace),
      matchEnd: Math.max(0, characterOffset - lineStart - leadingWhitespace) + match[0].length,
      snippet,
    });
    if (results.length >= maximumResults) break;
  }
  return results;
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function isWholeWord(content: string, offset: number, length: number): boolean {
  const before = Array.from(content.slice(0, offset)).at(-1);
  const after = Array.from(content.slice(offset + length))[0];
  return !isWordCharacter(before) && !isWordCharacter(after);
}

function isWordCharacter(value: string | undefined): boolean {
  return value !== undefined && /[\p{L}\p{N}_]/u.test(value);
}
