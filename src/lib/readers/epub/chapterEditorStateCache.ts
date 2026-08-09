export const MAX_CACHED_CHAPTER_STATES = 3;

export type CachedChapterEditorState<T> = {
  state: T;
  touchedAt: number;
};

export function rememberChapterEditorState<T>(
  cache: Map<string, CachedChapterEditorState<T>>,
  chapterEditId: string,
  state: T,
  touchedAt = Date.now(),
): void {
  cache.set(chapterEditId, { state, touchedAt });
}

export function readChapterEditorState<T>(
  cache: Map<string, CachedChapterEditorState<T>>,
  chapterEditId: string,
  touchedAt = Date.now(),
): T | undefined {
  const cached = cache.get(chapterEditId);
  if (!cached) return undefined;
  cached.touchedAt = touchedAt;
  return cached.state;
}

export function pruneChapterEditorStates<T>(
  cache: Map<string, CachedChapterEditorState<T>>,
  activeChapterEditId: string,
  maximum = MAX_CACHED_CHAPTER_STATES,
): string | undefined {
  if (cache.size <= maximum) return undefined;
  const oldest = [...cache.entries()]
    .filter(([chapterEditId]) => chapterEditId !== activeChapterEditId)
    .sort((left, right) => left[1].touchedAt - right[1].touchedAt)[0]?.[0];
  if (oldest) cache.delete(oldest);
  return oldest;
}
