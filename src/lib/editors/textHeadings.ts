export interface TextHeading {
  label: string;
  lineNumber: number;
  from: number;
  to: number;
}

export const DEFAULT_TEXT_HEADING_PATTERN = String.raw`(?<!\S)(?:序章|楔子|正文(?!完|结)|终章|后记|尾声|番外|第\s{0,4}[\d〇零一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟]+?\s{0,4}(?:章|节(?!课)|卷|集(?![合和]))|\d{1,6}(?=[ \t　]+))[^\r\n]{0,30}$`;
const MAX_HEADINGS = 5_000;

export function findTextHeadings(
  content: string,
  source = DEFAULT_TEXT_HEADING_PATTERN,
): TextHeading[] {
  const headings: TextHeading[] = [];
  let pattern: RegExp;
  try {
    pattern = new RegExp(source, 'gm');
  } catch {
    return headings;
  }
  let previousOffset = 0;
  let lineNumber = 1;

  for (const match of content.matchAll(pattern)) {
    const from = match.index;
    for (let offset = previousOffset; offset < from; offset += 1) {
      if (content.charCodeAt(offset) === 10) lineNumber += 1;
    }
    const label = match[0].trimEnd();
    headings.push({ label, lineNumber, from, to: from + label.length });
    previousOffset = from;
    if (headings.length >= MAX_HEADINGS) break;
  }

  return headings;
}
