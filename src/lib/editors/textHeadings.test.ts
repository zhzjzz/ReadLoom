import { describe, expect, it } from 'vitest';

import { findTextHeadings } from './textHeadings';

describe('findTextHeadings', () => {
  it('recognizes the supplied Chinese and numeric title forms without matching 正文完', () => {
    const content = [
      '普通前言',
      '序章 起点',
      '正文完',
      '第  十二  章 风起',
      '123　数字标题',
      '普通正文',
    ].join('\n');

    expect(findTextHeadings(content).map(({ label, lineNumber }) => ({ label, lineNumber }))).toEqual([
      { label: '序章 起点', lineNumber: 2 },
      { label: '第  十二  章 风起', lineNumber: 4 },
      { label: '123　数字标题', lineNumber: 5 },
    ]);
  });

  it('covers prefaces, endings, traditional numerals and the excluded suffixes', () => {
    const content = [
      ' 楔子 引子',
      '正文 开始',
      '正文结',
      '终章',
      '后记',
      '尾声',
      '番外一',
      '第壹卷 旧事',
      '第一节 内容',
      '第一节课 课程',
      '第2集 剧情',
      '第2集合',
      '999999 数字标题',
    ].join('\n');

    expect(findTextHeadings(content).map((heading) => heading.label)).toEqual([
      '楔子 引子',
      '正文 开始',
      '终章',
      '后记',
      '尾声',
      '番外一',
      '第壹卷 旧事',
      '第一节 内容',
      '第2集 剧情',
      '999999 数字标题',
    ]);
  });

  it('accepts a custom title recognition expression from settings', () => {
    const headings = findTextHeadings(
      '普通内容\nCHAPTER 01 Start\n第二章 不使用默认规则',
      String.raw`^CHAPTER\s+\d+\s+[^\r\n]+$`,
    );

    expect(headings.map((heading) => heading.label)).toEqual(['CHAPTER 01 Start']);
  });
});
