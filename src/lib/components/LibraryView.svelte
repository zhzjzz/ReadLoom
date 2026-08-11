<script lang="ts">
  import { libraryCoverUrl } from '../services/libraryService';
  import type { LibraryDocumentDto, LibraryGroupDto } from '../types/library';
  import type { LibraryColumns } from '../types/settings';

  export let documents: LibraryDocumentDto[] = [];
  export let groups: LibraryGroupDto[] = [];
  export let desktopRuntime = true;
  export let loading = false;
  export let importStatus: string | null = null;
  export let columns: LibraryColumns = 4;
  export let onImportFiles: () => void = () => {};
  export let onImportDirectory: () => void = () => {};
  export let onOpen: (document: LibraryDocumentDto) => void = () => {};
  export let onRefresh: () => void = () => {};
  export let onRemove: (document: LibraryDocumentDto) => void = () => {};
  export let onRemoveUnavailable: () => void = () => {};
  export let onCreateGroup: (name: string) => void | Promise<void> = () => {};
  export let onRenameGroup: (group: LibraryGroupDto, name: string) => void | Promise<void> = () => {};
  export let onDeleteGroup: (group: LibraryGroupDto) => void | Promise<void> = () => {};
  export let onMoveToGroup: (document: LibraryDocumentDto, groupId: string | null) => void | Promise<void> = () => {};

  type LibraryFilter = 'all' | 'epub' | 'txt' | 'unavailable';
  type LibrarySort = 'recent' | 'title';
  interface LibraryShelf {
    id: string | null;
    name: string;
    group: LibraryGroupDto | null;
    documents: LibraryDocumentDto[];
  }

  let query = '';
  let filter: LibraryFilter = 'all';
  let sort: LibrarySort = 'recent';
  let groupFilter = 'all';
  let creatingGroup = false;
  let groupDraft = '';
  let failedCoverKeys = new Set<string>();

  $: unavailableCount = documents.filter((document) => !document.available).length;
  $: visibleDocuments = filteredAndSortedDocuments(documents, query, filter, sort, groupFilter);
  $: shelves = buildShelves(visibleDocuments, groups, query, filter, groupFilter);

  function filteredAndSortedDocuments(
    source: LibraryDocumentDto[],
    searchQuery: string,
    activeFilter: LibraryFilter,
    activeSort: LibrarySort,
    activeGroup: string,
  ): LibraryDocumentDto[] {
    const normalizedQuery = searchQuery.trim().toLocaleLowerCase('zh-CN');
    const filtered = source.filter((document) => {
      if (activeFilter === 'unavailable' && document.available) return false;
      if (activeFilter === 'epub' && document.documentKind !== 'epub') return false;
      if (activeFilter === 'txt' && document.documentKind !== 'txt') return false;
      if (activeGroup === 'ungrouped' && document.groupId !== null) return false;
      if (activeGroup !== 'all' && activeGroup !== 'ungrouped' && document.groupId !== activeGroup) return false;
      if (!normalizedQuery) return true;
      return [document.displayTitle, document.author ?? '', document.path]
        .some((value) => value.toLocaleLowerCase('zh-CN').includes(normalizedQuery));
    });
    return filtered.sort((left, right) => activeSort === 'title'
      ? left.displayTitle.localeCompare(right.displayTitle, 'zh-CN')
      : right.lastOpenedAtMs - left.lastOpenedAtMs);
  }

  function buildShelves(
    source: LibraryDocumentDto[],
    sourceGroups: LibraryGroupDto[],
    searchQuery: string,
    activeFilter: LibraryFilter,
    activeGroup: string,
  ): LibraryShelf[] {
    const showEmptyGroups = !searchQuery.trim() && activeFilter === 'all' && activeGroup === 'all';
    const orderedGroups = [...sourceGroups].sort((left, right) =>
      left.position - right.position || left.name.localeCompare(right.name, 'zh-CN'));
    const result: LibraryShelf[] = orderedGroups
      .filter((group) => activeGroup === 'all' || activeGroup === group.groupId)
      .map((group) => ({
        id: group.groupId,
        name: group.name,
        group,
        documents: source.filter((document) => document.groupId === group.groupId),
      }))
      .filter((shelf) => showEmptyGroups || shelf.documents.length > 0);
    const ungrouped = source.filter((document) => document.groupId === null);
    if ((activeGroup === 'all' || activeGroup === 'ungrouped') && ungrouped.length > 0) {
      result.push({ id: null, name: '未分组', group: null, documents: ungrouped });
    }
    return result;
  }

  async function submitNewGroup(): Promise<void> {
    const name = groupDraft.trim();
    if (!name) return;
    await onCreateGroup(name);
    groupDraft = '';
    creatingGroup = false;
  }

  async function renameGroup(group: LibraryGroupDto): Promise<void> {
    const name = window.prompt('重命名书架分组', group.name)?.trim();
    if (!name || name === group.name) return;
    await onRenameGroup(group, name);
  }

  async function deleteGroup(group: LibraryGroupDto): Promise<void> {
    if (!window.confirm(`删除分组“${group.name}”？其中的书会移到“未分组”。`)) return;
    await onDeleteGroup(group);
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

  function markCoverFailed(coverKey: string): void {
    failedCoverKeys = new Set([...failedCoverKeys, coverKey]);
  }

  function fallbackCoverTitle(title: string): string {
    const normalized = title.replace(/\.txt$/iu, '').trim() || '未命名书籍';
    const maximum = columns === 3 ? 36 : columns === 4 ? 28 : 20;
    const characters = [...normalized];
    return characters.length > maximum
      ? `${characters.slice(0, maximum - 1).join('')}…`
      : normalized;
  }
</script>

<section
  aria-label="书库"
  class="library-view"
  style={`--library-columns:${columns};--cover-max-width:${columns === 3 ? 230 : columns === 4 ? 205 : 175}px;--fallback-title-size:${columns === 3 ? 26 : columns === 4 ? 22 : 18}px;--fallback-title-lines:${columns === 5 ? 4 : 5}`}
>
  <header class="library-header">
    <div>
      <h1>我的书库</h1>
      <p>整理本机 EPUB 与文本书籍，打开记录与书库收藏彼此独立。</p>
    </div>
    <div class="header-actions">
      <button disabled={loading || !desktopRuntime} onclick={onRefresh} type="button">
        {loading ? '刷新中…' : '刷新'}
      </button>
      <button disabled={!desktopRuntime} onclick={() => (creatingGroup = !creatingGroup)} type="button">新建分组</button>
      <button disabled={!desktopRuntime} onclick={onImportDirectory} type="button">导入目录</button>
      <button class="primary" disabled={!desktopRuntime} onclick={onImportFiles} title="支持 Ctrl 或 Shift 多选" type="button">导入图书</button>
      <button class="cleanup" disabled={!unavailableCount} onclick={onRemoveUnavailable} type="button">清理无效书籍</button>
    </div>
  </header>

  {#if importStatus}<p aria-live="polite" class="import-status">{importStatus}</p>{/if}

  {#if creatingGroup}
    <form class="group-create" onsubmit={(event) => { event.preventDefault(); void submitNewGroup(); }}>
      <label><span>分组名称</span><input aria-label="新分组名称" bind:value={groupDraft} maxlength="64" placeholder="例如：待读、小说、技术" /></label>
      <button class="primary" disabled={!groupDraft.trim()} type="submit">创建书架</button>
      <button onclick={() => { creatingGroup = false; groupDraft = ''; }} type="button">取消</button>
    </form>
  {/if}

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
    <label class="group-filter-control">
      <span>分组</span>
      <select aria-label="书库分组筛选" bind:value={groupFilter}>
        <option value="all">全部分组</option>
        {#each groups as group (group.groupId)}<option value={group.groupId}>{group.name}</option>{/each}
        <option value="ungrouped">未分组</option>
      </select>
    </label>
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
    <span>{query.trim() || filter !== 'all' || groupFilter !== 'all' ? '项匹配' : '本地书籍'}</span>
  </div>

  {#if documents.length && visibleDocuments.length}
    <div class="library-shelves">
      {#each shelves as shelf (shelf.id ?? 'ungrouped')}
        <section aria-label={`书架 ${shelf.name}`} class="shelf-group">
          <header class="shelf-heading">
            <div class="shelf-title"><span aria-hidden="true">▥</span><h2>{shelf.name}</h2><small>{shelf.documents.length} 本</small></div>
            {#if shelf.group}
              <div class="shelf-actions">
                <button aria-label={`重命名分组 ${shelf.name}`} onclick={() => void renameGroup(shelf.group!)} type="button">重命名</button>
                <button aria-label={`删除分组 ${shelf.name}`} onclick={() => void deleteGroup(shelf.group!)} type="button">删除分组</button>
              </div>
            {/if}
          </header>
          {#if shelf.documents.length}
            <div class="library-grid">
              {#each shelf.documents as document (document.path)}
                <article class:unavailable={!document.available} class="book-card">
                  <div class:epub={document.documentKind === 'epub'} class:real-cover={Boolean(document.coverKey && !failedCoverKeys.has(document.coverKey))} class="book-cover">
                    {#if document.coverKey && !failedCoverKeys.has(document.coverKey)}
                      <img
                        alt={`${document.displayTitle} 封面`}
                        decoding="async"
                        loading="lazy"
                        onerror={() => markCoverFailed(document.coverKey!)}
                        src={libraryCoverUrl(document.coverKey)}
                      />
                    {:else}
                      <span>{document.documentKind.toUpperCase()}</span>
                      <strong class="fallback-title" title={document.displayTitle}>{fallbackCoverTitle(document.displayTitle)}</strong>
                      <small>READLOOM</small>
                    {/if}
                  </div>
                  <div class="book-details">
                    <div class="book-heading">
                      <div>
                        <span class="kind-label">{document.documentKind === 'epub' ? '电子书' : '文本书籍'}</span>
                        {#if !document.available}<span class="missing-label">文件已移动</span>{/if}
                      </div>
                      <h3 title={document.displayTitle}>{document.displayTitle}</h3>
                      <p>{document.author ?? (document.documentKind === 'epub' ? '未知作者' : '本地文本')}</p>
                    </div>
                    <div class="book-metadata">
                      <span>{openedAtLabel(document.lastOpenedAtMs)}</span>
                      <span title={document.path}>{folderLabel(document.path)}</span>
                    </div>
                    <label class="book-group-select">
                      <span>移动到</span>
                      <select
                        aria-label={`设置 ${document.displayTitle} 的分组`}
                        onchange={(event) => void onMoveToGroup(document, event.currentTarget.value || null)}
                        value={document.groupId ?? ''}
                      >
                        <option value="">未分组</option>
                        {#each groups as group (group.groupId)}<option value={group.groupId}>{group.name}</option>{/each}
                      </select>
                    </label>
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
                        title="只移除书库收藏，不删除文件或打开历史"
                        type="button"
                      >移除</button>
                    </div>
                  </div>
                </article>
              {/each}
            </div>
          {:else}
            <div class="empty-shelf">这个书架还是空的，可从书籍卡片的“移动到”菜单加入。</div>
          {/if}
        </section>
      {/each}
    </div>
  {:else}
    <div class="library-empty">
      <div>R</div>
      {#if documents.length}
        <h2>没有符合条件的书籍</h2>
        <p>尝试清除搜索词或切换类型、分组筛选。</p>
        <button onclick={() => { query = ''; filter = 'all'; groupFilter = 'all'; }} type="button">显示全部</button>
      {:else}
        <h2>书库还是空的</h2>
        <p>导入 EPUB、TXT 或其他文本文件后，它们会保存在本地书库中。</p>
        <div class="empty-import-actions">
          <button class="primary" disabled={!desktopRuntime} onclick={onImportFiles} type="button">选择图书</button>
          <button disabled={!desktopRuntime} onclick={onImportDirectory} type="button">选择目录</button>
        </div>
      {/if}
    </div>
  {/if}
</section>

<style>
  .library-view {
    background:color-mix(in srgb,var(--surface-canvas) 91%,transparent);
    box-sizing: border-box;
    height: 100%;
    min-height: 0;
    overflow: auto;
    padding: clamp(24px, 4vw, 52px);
  }

  .library-header {
    align-items: center;
    display: flex;
    gap: 24px;
    justify-content: space-between;
    margin: 0 auto 22px;
    max-width: 1180px;
  }

  h1 { color: var(--text-primary); font: 720 clamp(26px, 3vw, 34px)/1.1 var(--font-ui); letter-spacing: -0.035em; margin: 0 0 7px; }
  .library-header p { color: var(--text-tertiary); font: 500 13px/1.5 var(--font-ui); margin: 0; }
  .header-actions, .book-actions, .shelf-actions { display: flex; gap: 8px; }
  .import-status { background: var(--accent-soft); border: 1px solid color-mix(in srgb, var(--accent) 28%, var(--border-subtle)); border-radius: var(--radius-sm); color: var(--accent-strong); font: 600 10px/1.4 var(--font-ui); margin: 0 auto 14px; max-width: 1154px; padding: 9px 12px; }

  button, select, input {
    background: var(--surface-control);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    font: 600 11px/1 var(--font-ui);
  }
  button { min-height: 34px; padding: 0 13px; }
  button:hover:not(:disabled) { background: var(--surface-hover); color: var(--text-primary); }
  button.primary, .open-action { background: var(--accent); border-color: var(--accent); color: white; }
  button.primary:hover:not(:disabled), .open-action:hover:not(:disabled) { background: var(--accent-strong); color: white; }
  button.cleanup:not(:disabled) { color:var(--danger); }
  button.cleanup:hover:not(:disabled) { background:var(--danger-soft); border-color:color-mix(in srgb,var(--danger) 35%,var(--border-strong)); }
  button:disabled { color: var(--text-disabled); cursor: default; }

  .group-create {
    align-items: end;
    background: var(--surface-pane);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    display: flex;
    gap: 8px;
    margin: 0 auto 14px;
    max-width: 1180px;
    padding: 12px;
  }
  .group-create label { display: grid; flex: 1; gap: 6px; }
  .group-create label span, .search-control > span, .group-filter-control > span, .sort-control > span {
    color: var(--text-tertiary); font: 600 9px/1 var(--font-ui); letter-spacing: 0.04em;
  }
  .group-create input { box-sizing: border-box; height: 36px; padding: 0 10px; width: 100%; }

  .library-controls {
    align-items: end; background: var(--surface-pane); border: 1px solid var(--border-subtle); border-radius: var(--radius-md); display: grid; gap: 12px;
    box-shadow:var(--shadow-sm); grid-template-columns: minmax(170px, 1fr) auto 150px 130px; margin: 0 auto 15px; max-width: 1180px; padding: 12px;
  }
  .search-control, .group-filter-control, .sort-control { display: grid; gap: 6px; }
  input, select { box-sizing: border-box; height: 36px; min-width: 0; padding: 0 10px; width: 100%; }
  .filter-control { background: var(--surface-subtle); border-radius: var(--radius-sm); display: flex; gap: 2px; padding: 3px; }
  .filter-control button { background: transparent; border-color: transparent; min-height: 30px; padding-inline: 9px; }
  .filter-control button.active { background: var(--surface-control); border-color: var(--border-subtle); color: var(--accent-strong); }
  .result-summary { align-items: baseline; color: var(--text-tertiary); display: flex; font: 500 10px/1 var(--font-ui); gap: 5px; margin: 0 auto 10px; max-width: 1180px; }
  .result-summary strong { color: var(--text-secondary); font-size: 12px; }

  .library-shelves { display: grid; gap: 20px; margin: 0 auto; max-width: 1180px; }
  .shelf-group { min-width: 0; }
  .shelf-heading { align-items: center; display: flex; justify-content: space-between; margin-bottom: 9px; min-height: 34px; }
  .shelf-title { align-items: center; display: flex; gap: 8px; min-width: 0; }
  .shelf-title > span { color: var(--accent-strong); font-size: 20px; transform: rotate(90deg); }
  .shelf-title h2 { color: var(--text-primary); font: 680 16px/1.2 var(--font-ui); margin: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .shelf-title small { background: var(--surface-subtle); border-radius: 99px; color: var(--text-tertiary); font: 600 9px/1 var(--font-ui); padding: 4px 7px; }
  .shelf-actions button { background: transparent; border-color: transparent; color: var(--text-tertiary); min-height: 28px; padding-inline: 7px; }
  .shelf-actions button:last-child:hover { background: var(--danger-soft); color: var(--danger); }
  .empty-shelf { border: 1px dashed var(--border-strong); border-radius: var(--radius-md); color: var(--text-tertiary); font: 500 10px/1.5 var(--font-ui); padding: 18px; }

  .library-grid {
    border-bottom: 7px solid color-mix(in srgb, var(--border-strong) 72%, transparent);
    display: grid; gap: 14px; grid-template-columns: repeat(var(--library-columns), minmax(0, 1fr)); padding-bottom: 13px;
  }
  .book-card {
    background:color-mix(in srgb,var(--surface-pane) 96%,transparent); border:1px solid var(--border-subtle); border-radius:var(--radius-lg); box-shadow:var(--shadow-sm);
    display:grid; gap:13px; grid-template-rows:auto minmax(0,1fr); min-height:440px; min-width:0; padding:13px; transition:border-color 140ms ease,box-shadow 140ms ease,transform 140ms ease;
  }
  .book-card:hover { border-color:color-mix(in srgb,var(--accent) 35%,var(--border-strong)); box-shadow:var(--shadow-md); transform:translateY(-2px); }
  .book-card.unavailable { box-shadow: none; opacity: 0.78; }
  .book-cover {
    aspect-ratio: 2 / 3;
    background: linear-gradient(145deg, #5f5448, #2c2824); border-radius: 5px 10px 10px 5px; box-shadow: inset 3px 0 rgba(255,255,255,.08), 0 7px 16px rgba(0,0,0,.15);
    box-sizing: border-box; color: #fff9ee; display: flex; flex-direction: column; justify-content: space-between; justify-self: center; max-width: var(--cover-max-width); overflow: hidden; padding: 15px 12px; width: 100%;
  }
  .book-cover.epub { background: linear-gradient(145deg, #2f67c7, #18376e); }
  .book-cover.real-cover { background: var(--surface-subtle); box-shadow: 0 7px 16px rgba(0,0,0,.15); padding: 0; }
  .book-cover img { display: block; height: 100%; object-fit: cover; width: 100%; }
  .book-cover span, .book-cover small { font: 700 7px/1 var(--font-mono); letter-spacing: .12em; opacity: .72; }
  .book-cover .fallback-title { display:-webkit-box; font:720 var(--fallback-title-size)/1.24 var(--font-content); letter-spacing:-.03em; line-clamp:var(--fallback-title-lines); overflow:hidden; overflow-wrap:anywhere; text-wrap:balance; -webkit-box-orient:vertical; -webkit-line-clamp:var(--fallback-title-lines); }
  .book-details { display: flex; flex-direction: column; min-width: 0; }
  .book-heading > div { align-items: center; display: flex; gap: 5px; min-height: 16px; }
  .kind-label, .missing-label { color: var(--accent-strong); font: 700 8px/1 var(--font-ui); letter-spacing: .05em; }
  .missing-label { background: var(--danger-soft); border-radius: 99px; color: var(--danger); padding: 3px 5px; }
  h3 { color: var(--text-primary); font: 650 15px/1.3 var(--font-ui); margin: 7px 0 4px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .book-heading p { color: var(--text-tertiary); font: 500 10px/1.3 var(--font-ui); margin: 0; }
  .book-metadata { display: grid; gap: 4px; margin: auto 0 9px; min-width: 0; padding-top: 12px; }
  .book-metadata span { color: var(--text-disabled); font: 500 9px/1.25 var(--font-ui); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .book-group-select { align-items: center; display: grid; gap: 6px; grid-template-columns: auto minmax(0, 1fr); margin-bottom: 8px; }
  .book-group-select > span { color: var(--text-tertiary); font: 600 9px/1 var(--font-ui); }
  .book-group-select select { height: 28px; padding-inline: 7px; }
  .open-action { flex: 1; }
  .remove-action { color: var(--text-tertiary); }

  .library-empty { align-items: center; border: 1px dashed var(--border-strong); border-radius: var(--radius-md); display: flex; flex-direction: column; margin: 34px auto 0; max-width: 520px; padding: 48px 24px; text-align: center; }
  .library-empty > div:first-child { align-items: center; background: var(--surface-subtle); border: 1px solid var(--border-subtle); border-radius: 14px; color: var(--accent-strong); display: flex; font: 700 18px/1 var(--font-ui); height: 54px; justify-content: center; width: 54px; }
  .library-empty h2 { color: var(--text-primary); font: 650 18px/1.3 var(--font-ui); margin: 16px 0 7px; }
  .library-empty p { color: var(--text-tertiary); font: 500 11px/1.5 var(--font-ui); margin: 0 0 16px; }
  .empty-import-actions { display: flex; gap: 8px; }

  @media (max-width: 1000px) {
    .library-controls { grid-template-columns: minmax(180px, 1fr) auto; }
    .library-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  }
  @media (max-width: 760px) {
    .library-header { align-items: flex-start; flex-direction: column; }
    .library-controls { grid-template-columns: 1fr; }
    .filter-control { overflow-x: auto; }
    .group-create { align-items: stretch; flex-wrap: wrap; }
    .group-create label { flex-basis: 100%; }
  }
  @media (max-width: 620px) {
    .library-view { padding: 20px 14px; }
    .library-grid { grid-template-columns: 1fr; }
    .book-card { min-height: 0; }
    .header-actions { flex-wrap: wrap; width: 100%; }
    .header-actions button { flex: 1; }
    .shelf-heading { align-items: flex-start; gap: 8px; }
    .shelf-actions { gap: 2px; }
  }
</style>
