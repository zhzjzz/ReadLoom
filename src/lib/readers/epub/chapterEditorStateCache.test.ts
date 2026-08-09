import { describe, expect, it } from 'vitest';

import {
  pruneChapterEditorStates,
  readChapterEditorState,
  rememberChapterEditorState,
} from './chapterEditorStateCache';

describe('chapter editor state cache', () => {
  it('keeps three recent chapter states while traversing four chapters', () => {
    const cache = new Map();
    rememberChapterEditorState(cache, 'chapter-1', { selection: 1 }, 1);
    rememberChapterEditorState(cache, 'chapter-2', { selection: 2 }, 2);
    rememberChapterEditorState(cache, 'chapter-3', { selection: 3 }, 3);

    expect(readChapterEditorState(cache, 'chapter-1', 4)).toEqual({ selection: 1 });
    rememberChapterEditorState(cache, 'chapter-4', { selection: 4 }, 5);

    expect(pruneChapterEditorStates(cache, 'chapter-4')).toBe('chapter-2');
    expect([...cache.keys()]).toEqual(['chapter-1', 'chapter-3', 'chapter-4']);
    expect(readChapterEditorState(cache, 'chapter-1', 6)).toEqual({ selection: 1 });
  });

  it('never evicts the active chapter even when it has the oldest timestamp', () => {
    const cache = new Map();
    rememberChapterEditorState(cache, 'active', 'active-state', 1);
    rememberChapterEditorState(cache, 'chapter-2', 'second-state', 2);
    rememberChapterEditorState(cache, 'chapter-3', 'third-state', 3);
    rememberChapterEditorState(cache, 'chapter-4', 'fourth-state', 4);

    expect(pruneChapterEditorStates(cache, 'active')).toBe('chapter-2');
    expect(cache.get('active')?.state).toBe('active-state');
  });
});
