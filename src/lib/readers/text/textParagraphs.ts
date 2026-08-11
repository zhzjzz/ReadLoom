import type { TxtTypographySettings } from '../../types/settings';
import { findTextHeadings } from '../../editors/textHeadings';

export type TextReadingBlockKind = 'heading' | 'paragraph' | 'blank';

export interface TextReadingBlock {
  kind: TextReadingBlockKind;
  text: string;
  sourceStart: number;
  sourceEnd: number;
}

interface SourceLine {
  text: string;
  start: number;
  end: number;
}

export function buildTextReadingBlocks(
  content: string,
  settings: TxtTypographySettings,
  headingPattern: string,
): TextReadingBlock[] {
  if (!content) return [];
  const headingStarts = new Set(findTextHeadings(content, headingPattern).map((heading) => heading.from));
  const lines = sourceLines(content);
  const blocks: TextReadingBlock[] = [];
  for (let index = 0; index < lines.length;) {
    const line = lines[index];
    const trimmed = line.text.trim();
    if (!trimmed) {
      if (settings.blankLines === 'preserve'
        || (settings.blankLines === 'single' && blocks.at(-1)?.kind !== 'blank')) {
        blocks.push({ kind: 'blank', text: '', sourceStart: line.start, sourceEnd: line.end });
      }
      index += 1;
      continue;
    }
    if (headingStarts.has(line.start)) {
      blocks.push({
        kind: 'heading',
        text: trimmed,
        sourceStart: line.start,
        sourceEnd: line.end,
      });
      index += 1;
      continue;
    }

    const parts = [normalizedLineText(line.text, settings.leadingIndent)];
    let sourceEnd = line.end;
    let cursor = index;
    while (settings.mergeWrappedLines && cursor + 1 < lines.length) {
      const next = lines[cursor + 1];
      if (!shouldMerge(lines[cursor], next, headingStarts)) break;
      parts.push(normalizedLineText(next.text, 'clean'));
      sourceEnd = next.end;
      cursor += 1;
    }
    blocks.push({
      kind: 'paragraph',
      text: joinWrappedParts(parts),
      sourceStart: line.start,
      sourceEnd,
    });
    index = cursor + 1;
  }
  return blocks;
}

function sourceLines(content: string): SourceLine[] {
  const lines: SourceLine[] = [];
  let start = 0;
  while (start < content.length) {
    let end = start;
    while (end < content.length && content[end] !== '\r' && content[end] !== '\n') end += 1;
    lines.push({ text: content.slice(start, end), start, end });
    if (content[end] === '\r' && content[end + 1] === '\n') start = end + 2;
    else start = end + 1;
  }
  return lines;
}

function normalizedLineText(text: string, leadingIndent: TxtTypographySettings['leadingIndent']): string {
  return leadingIndent === 'clean' ? text.replace(/^[\t \u3000]+/u, '').trimEnd() : text.trimEnd();
}

function shouldMerge(
  current: SourceLine,
  next: SourceLine,
  headingStarts: ReadonlySet<number>,
): boolean {
  const left = current.text.trim();
  const right = next.text.trim();
  if (!left || !right || headingStarts.has(next.start)) return false;
  if (/^[\t \u3000]/u.test(next.text)) return false;
  if (/^[“‘"'「『（(《〈【\[]/u.test(right)) return false;
  if (/[。！？!?；;：:…」』）)】\]]$/u.test(left)) return false;
  const leftLength = [...left].length;
  const rightLength = [...right].length;
  if (leftLength < 24 || rightLength < 18) return false;
  return Math.abs(leftLength - rightLength) <= Math.max(leftLength, rightLength) * 0.28;
}

function joinWrappedParts(parts: string[]): string {
  return parts.reduce((text, part) => {
    if (!text) return part;
    const previous = text.at(-1) ?? '';
    const next = part[0] ?? '';
    const separator = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}]/u.test(previous)
      && /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}]/u.test(next)
      ? ''
      : ' ';
    return `${text}${separator}${part}`;
  }, '');
}
