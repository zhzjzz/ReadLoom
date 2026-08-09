import { describe, expect, it } from 'vitest';

import { describeTextPosition, searchTextDocument } from './textSearch';

describe('TXT full-document search', () => {
  it('returns every Unicode match with CodeMirror offsets, line numbers, and snippets', () => {
    const content = '序章\n😀阅织出发\n第三行阅织结束';

    expect(searchTextDocument(content, '阅织')).toEqual([
      {
        characterOffset: 5,
        lineNumber: 2,
        matchStart: 2,
        matchEnd: 4,
        snippet: '😀阅织出发',
      },
      {
        characterOffset: 13,
        lineNumber: 3,
        matchStart: 3,
        matchEnd: 5,
        snippet: '第三行阅织结束',
      },
    ]);
  });

  it('supports case-sensitive and whole-word matching', () => {
    const content = 'Readloom reader\nreadloom ReadloomX';

    expect(searchTextDocument(content, 'Readloom', { caseSensitive: true, wholeWord: true }))
      .toHaveLength(1);
    expect(searchTextDocument(content, 'readloom', { wholeWord: true })).toHaveLength(2);
  });

  it('describes a bookmark position using CodeMirror-compatible offsets', () => {
    expect(describeTextPosition('第一行\n😀第二行\n', 7)).toEqual({
      characterOffset: 7,
      lineNumber: 2,
      preview: '😀第二行',
    });
  });
});
