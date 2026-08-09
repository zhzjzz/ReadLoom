<script lang="ts">
  import type { RecentDocumentDto } from '../types/epub';

  export let documents: RecentDocumentDto[] = [];
  export let desktopRuntime = true;
  export let loading = false;
  export let onImport: () => void = () => {};
  export let onOpen: (document: RecentDocumentDto) => void = () => {};
  export let onRefresh: () => void = () => {};
  export let onRemove: (document: RecentDocumentDto) => void = () => {};

  type LibraryFilter = 'all' | 'epub' | 'txt' | 'unavailable';
  type LibrarySort = 'recent' | 'title';

  let query = '';
  let filter: LibraryFilter = 'all';
  let sort: LibrarySort = 'recent';

  $: epubCount = documents.filter((document) => document.documentKind === 'epub').length;
  $: textCount = documents.filter((document) => document.documentKind === 'txt').length;
  $: unavailableCount = documents.filter((document) => !document.available).length;
  $: visibleDocuments = filteredAndSortedDocuments(documents, query, filter, sort);

  function filteredAndSortedDocuments(
    source: RecentDocumentDto[],
    searchQuery: string,
    activeFilter: LibraryFilter,
    activeSort: LibrarySort,
  ): RecentDocumentDto[] {
    const normalizedQuery = searchQuery.trim().toLocaleLowerCase('zh-CN');
    const filtered = source.filter((document) => {
      if (activeFilter === 'unavailable' && document.available) return false;
      if (activeFilter === 'epub' && document.documentKind !== 'epub') return false;
      if (activeFilter === 'txt' && document.documentKind !== 'txt') return false;
      if (!normalizedQuery) return true;
      return [document.displayTitle, document.author ?? '', document.path]
        .some((value) => value.toLocaleLowerCase('zh-CN').includes(normalizedQuery));
    });
    return filtered.sort((left, right) => activeSort === 'title'
      ? left.displayTitle.localeCompare(right.displayTitle, 'zh-CN')
      : right.lastOpenedAtMs - left.lastOpenedAtMs);
  }

  function openedAtLabel(timestamp: number): string {
    if (!Number.isFinite(timestamp) || timestamp <= 0) return '打开时间未知';
    return new Intl.DateTimeFormat('zh-CN', {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    }).format(new Date(timestamp));
  }

  function folderLabel(path: string): string {
    const normalized = path.replaceAll('\\', '/').replace(/^\/\/\?\//, '');
    const separator = normalized.lastIndexOf('/');
    return separator <= 0 ? path : normalized.slice(0, separator);
  }
</script>

<section aria-label="书库" class="library-view">
  <header class="library-header">
    <div>
      <span class="eyebrow">LOCAL LIBRARY</span>
      <h1>书库</h1>
      <p>集中管理在 Readloom 中打开过的 EPUB 与文本书籍。</p>
    </div>
    <div class="header-actions">
      <button disabled={loading || !desktopRuntime} onclick={onRefresh} type="button">
        {loading ? '刷新中…' : '刷新'}
      </button>
      <button class="primary" disabled={!desktopRuntime} onclick={onImport} type="button">导入书籍</button>
    </div>
  </header>

  <div class="library-statistics" aria-label="书库统计">
    <article><strong>{documents.length}</strong><span>全部书籍</span></article>
    <article><strong>{epubCount}</strong><span>EPUB</span></article>
    <article><strong>{textCount}</strong><span>TXT / 文本</span></article>
    <article class:warning={unavailableCount > 0}><strong>{unavailableCount}</strong><span>文件已移动</span></article>
  </div>

  <div class="library-controls">
    <label class="search-control">
      <span>搜索书库</span>
      <input aria-label="搜索书库" bind:value={query} placeholder="书名、作者或路径" type="search" />
    </label>
    <div aria-label="书库类型筛选" class="filter-control" role="group">
      {#each [
        ['all', '全部'],
        ['epub', 'EPUB'],
        ['txt', 'TXT'],
        ['unavailable', '已移动'],
      ] as option}
        <button
          aria-pressed={filter === option[0]}
          class:active={filter === option[0]}
          onclick={() => (filter = option[0] as LibraryFilter)}
          type="button"
        >{option[1]}</button>
      {/each}
    </div>
    <label class="sort-control">
      <span>排序</span>
      <select aria-label="书库排序" bind:value={sort}>
        <option value="recent">最近打开</option>
        <option value="title">书名</option>
      </select>
    </label>
  </div>

  <div class="result-summary">
    <strong>{visibleDocuments.length}</strong>
    <span>{query.trim() || filter !== 'all' ? '项匹配' : '本本地书籍'}</span>
  </div>

  {#if visibleDocuments.length}
    <div class="library-grid">
      {#each visibleDocuments as document (document.path)}
        <article class:unavailable={!document.available} class="book-card">
          <div class:epub={document.documentKind === 'epub'} class="book-cover">
            <span>{document.documentKind.toUpperCase()}</span>
            <strong>{document.displayTitle.slice(0, 1).toLocaleUpperCase('zh-CN')}</strong>
            <small>READLOOM</small>
          </div>
          <div class="book-details">
            <div class="book-heading">
              <div>
                <span class="kind-label">{document.documentKind === 'epub' ? '电子书' : '文本书籍'}</span>
                {#if !document.available}<span class="missing-label">文件已移动</span>{/if}
              </div>
              <h2 title={document.displayTitle}>{document.displayTitle}</h2>
              <p>{document.author ?? (document.documentKind === 'epub' ? '未知作者' : '本地文本')}</p>
            </div>
            <div class="book-metadata">
              <span>{openedAtLabel(document.lastOpenedAtMs)}</span>
              <span title={document.path}>{folderLabel(document.path)}</span>
            </div>
            <div class="book-actions">
              <button
                aria-label={`打开 ${document.displayTitle}`}
                class="open-action"
                disabled={!document.available || !desktopRuntime}
                onclick={() => onOpen(document)}
                type="button"
              >{document.available ? '打开' : '不可用'}</button>
              <button
                aria-label={`从书库移除 ${document.displayTitle}`}
                class="remove-action"
                onclick={() => onRemove(document)}
                title="只移除书库记录，不删除文件"
                type="button"
              >移除</button>
            </div>
          </div>
        </article>
      {/each}
    </div>
  {:else}
    <div class="library-empty">
      <div>R</div>
      {#if documents.length}
        <h2>没有符合条件的书籍</h2>
        <p>尝试清除搜索词或切换类型筛选。</p>
        <button onclick={() => { query = ''; filter = 'all'; }} type="button">显示全部</button>
      {:else}
        <h2>书库还是空的</h2>
        <p>导入 EPUB、TXT 或其他文本文件后，它们会保存在本地书库中。</p>
        <button class="primary" disabled={!desktopRuntime} onclick={onImport} type="button">导入第一本书</button>
      {/if}
    </div>
  {/if}
</section>

<style>
  .library-view {
    background:
      radial-gradient(circle at 88% 0%, color-mix(in srgb, var(--accent) 8%, transparent), transparent 34%),
      var(--surface-canvas);
    box-sizing: border-box;
    height: 100%;
    min-height: 0;
    overflow: auto;
    padding: clamp(24px, 4vw, 52px);
  }

  .library-header {
    align-items: flex-end;
    display: flex;
    gap: 24px;
    justify-content: space-between;
    margin: 0 auto 28px;
    max-width: 1180px;
  }

  .eyebrow {
    color: var(--accent-strong);
    font: 700 9px/1 var(--font-mono);
    letter-spacing: 0.16em;
  }

  h1 {
    color: var(--text-primary);
    font: 700 clamp(28px, 4vw, 42px)/1.1 var(--font-ui);
    letter-spacing: -0.035em;
    margin: 8px 0 8px;
  }

  .library-header p {
    color: var(--text-tertiary);
    font: 500 13px/1.5 var(--font-ui);
    margin: 0;
  }

  .header-actions,
  .book-actions {
    display: flex;
    gap: 8px;
  }

  button,
  select,
  input {
    background: var(--surface-control);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    font: 600 11px/1 var(--font-ui);
  }

  button {
    min-height: 34px;
    padding: 0 13px;
  }

  button:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--text-primary);
  }

  button.primary,
  .open-action {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
  }

  button.primary:hover:not(:disabled),
  .open-action:hover:not(:disabled) {
    background: var(--accent-strong);
    color: white;
  }

  button:disabled {
    color: var(--text-disabled);
    cursor: default;
  }

  .library-statistics {
    display: grid;
    gap: 10px;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    margin: 0 auto 16px;
    max-width: 1180px;
  }

  .library-statistics article {
    background: color-mix(in srgb, var(--surface-pane) 92%, transparent);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    display: grid;
    gap: 3px;
    padding: 14px 16px;
  }

  .library-statistics strong {
    color: var(--text-primary);
    font: 700 22px/1 var(--font-ui);
  }

  .library-statistics span {
    color: var(--text-tertiary);
    font: 500 10px/1.2 var(--font-ui);
  }

  .library-statistics .warning strong {
    color: var(--warning);
  }

  .library-controls {
    align-items: end;
    background: var(--surface-pane);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    display: grid;
    gap: 14px;
    grid-template-columns: minmax(220px, 1fr) auto 150px;
    margin: 0 auto 15px;
    max-width: 1180px;
    padding: 12px;
  }

  .search-control,
  .sort-control {
    display: grid;
    gap: 6px;
  }

  .search-control > span,
  .sort-control > span {
    color: var(--text-tertiary);
    font: 600 9px/1 var(--font-ui);
    letter-spacing: 0.04em;
  }

  input,
  select {
    box-sizing: border-box;
    height: 36px;
    min-width: 0;
    padding: 0 10px;
    width: 100%;
  }

  .filter-control {
    background: var(--surface-subtle);
    border-radius: var(--radius-sm);
    display: flex;
    gap: 2px;
    padding: 3px;
  }

  .filter-control button {
    background: transparent;
    border-color: transparent;
    min-height: 30px;
    padding-inline: 10px;
  }

  .filter-control button.active {
    background: var(--surface-control);
    border-color: var(--border-subtle);
    color: var(--accent-strong);
  }

  .result-summary {
    align-items: baseline;
    color: var(--text-tertiary);
    display: flex;
    font: 500 10px/1 var(--font-ui);
    gap: 5px;
    margin: 0 auto 10px;
    max-width: 1180px;
  }

  .result-summary strong {
    color: var(--text-secondary);
    font-size: 12px;
  }

  .library-grid {
    display: grid;
    gap: 14px;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    margin: 0 auto;
    max-width: 1180px;
  }

  .book-card {
    background: var(--surface-pane);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    box-shadow: 0 8px 28px color-mix(in srgb, black 5%, transparent);
    display: grid;
    gap: 16px;
    grid-template-columns: 92px minmax(0, 1fr);
    min-height: 190px;
    padding: 14px;
    transition: border-color 140ms ease, transform 140ms ease;
  }

  .book-card:hover {
    border-color: var(--border-strong);
    transform: translateY(-1px);
  }

  .book-card.unavailable {
    box-shadow: none;
    opacity: 0.78;
  }

  .book-cover {
    background: linear-gradient(145deg, #5f5448, #2c2824);
    border-radius: 5px 10px 10px 5px;
    box-shadow: inset 3px 0 rgba(255, 255, 255, 0.08), 0 7px 16px rgba(0, 0, 0, 0.15);
    color: #fff9ee;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    overflow: hidden;
    padding: 12px 10px;
  }

  .book-cover.epub {
    background: linear-gradient(145deg, #2f67c7, #18376e);
  }

  .book-cover span,
  .book-cover small {
    font: 700 7px/1 var(--font-mono);
    letter-spacing: 0.12em;
    opacity: 0.72;
  }

  .book-cover strong {
    font: 700 32px/1 var(--font-content);
  }

  .book-details {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .book-heading > div {
    align-items: center;
    display: flex;
    gap: 5px;
    min-height: 16px;
  }

  .kind-label,
  .missing-label {
    color: var(--accent-strong);
    font: 700 8px/1 var(--font-ui);
    letter-spacing: 0.05em;
  }

  .missing-label {
    background: var(--danger-soft);
    border-radius: 99px;
    color: var(--danger);
    padding: 3px 5px;
  }

  h2 {
    color: var(--text-primary);
    font: 650 15px/1.3 var(--font-ui);
    margin: 7px 0 4px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .book-heading p {
    color: var(--text-tertiary);
    font: 500 10px/1.3 var(--font-ui);
    margin: 0;
  }

  .book-metadata {
    display: grid;
    gap: 4px;
    margin: auto 0 12px;
    min-width: 0;
    padding-top: 12px;
  }

  .book-metadata span {
    color: var(--text-disabled);
    font: 500 9px/1.25 var(--font-ui);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .open-action {
    flex: 1;
  }

  .remove-action {
    color: var(--text-tertiary);
  }

  .library-empty {
    align-items: center;
    border: 1px dashed var(--border-strong);
    border-radius: var(--radius-md);
    display: flex;
    flex-direction: column;
    margin: 34px auto 0;
    max-width: 520px;
    padding: 48px 24px;
    text-align: center;
  }

  .library-empty > div {
    align-items: center;
    background: var(--surface-subtle);
    border: 1px solid var(--border-subtle);
    border-radius: 14px;
    color: var(--accent-strong);
    display: flex;
    font: 700 18px/1 var(--font-ui);
    height: 54px;
    justify-content: center;
    width: 54px;
  }

  .library-empty h2 {
    font-size: 18px;
    margin-top: 16px;
  }

  .library-empty p {
    color: var(--text-tertiary);
    font: 500 11px/1.5 var(--font-ui);
    margin: 0 0 16px;
  }

  @media (max-width: 880px) {
    .library-header {
      align-items: flex-start;
      flex-direction: column;
    }

    .library-controls {
      grid-template-columns: 1fr;
    }

    .filter-control {
      overflow-x: auto;
    }
  }

  @media (max-width: 620px) {
    .library-view {
      padding: 20px 14px;
    }

    .library-statistics {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .library-grid {
      grid-template-columns: 1fr;
    }

    .header-actions {
      width: 100%;
    }

    .header-actions button {
      flex: 1;
    }
  }
</style>
