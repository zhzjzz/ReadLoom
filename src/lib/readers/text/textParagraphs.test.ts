import { describe, expect, it } from 'vitest';

import { DEFAULT_TEXT_HEADING_PATTERN } from '../../editors/textHeadings';
import type { TxtTypographySettings } from '../../types/settings';
import { buildTextReadingBlocks } from './textParagraphs';

const defaults: TxtTypographySettings = {
  leadingIndent: 'clean',
  blankLines: 'single',
  mergeWrappedLines: false,
  chapterTitleStyle: 'prominent',
};

describe('TXT paragraph recognition', () => {
  it('separates source blank lines from visual paragraph spacing and keeps offsets', () => {
    const blocks = buildTextReadingBlocks(
      '　　第一段\r\n\r\n\r\n第二段',
      defaults,
      DEFAULT_TEXT_HEADING_PATTERN,
    );

    expect(blocks.map((block) => [block.kind, block.text])).toEqual([
      ['paragraph', '第一段'],
      ['blank', ''],
      ['paragraph', '第二段'],
    ]);
    expect(blocks[2].sourceStart).toBe(11);
  });

  it('keeps chapter headings independent from正文 indentation', () => {
    const blocks = buildTextReadingBlocks(
      '第十二章 风起\n　　正文开始。',
      defaults,
      DEFAULT_TEXT_HEADING_PATTERN,
    );

    expect(blocks[0]).toMatchObject({ kind: 'heading', text: '第十二章 风起', sourceStart: 0 });
    expect(blocks[1]).toMatchObject({ kind: 'paragraph', text: '正文开始。' });
  });

  it('conservatively merges only likely fixed-width hard wraps', () => {
    const fixed = '这是固定宽度导出的长行，长度足够接近并且行末没有中文句号用于判断硬换行';
    const continuation = '下一行同样保持相近长度，因此开启选项后可以安全地与上一行自动合并起来';
    const blocks = buildTextReadingBlocks(
      `${fixed}\n${continuation}\n“这是新的对话段落，不能合并。”`,
      { ...defaults, mergeWrappedLines: true },
      DEFAULT_TEXT_HEADING_PATTERN,
    );

    expect(blocks).toHaveLength(2);
    expect(blocks[0].text).toBe(`${fixed}${continuation}`);
    expect(blocks[1].text).toContain('新的对话段落');
  });
});
