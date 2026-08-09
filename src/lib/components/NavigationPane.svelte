<script lang="ts">
  import Icon, { type IconName } from './Icon.svelte';
  import type { TextHeading } from '../editors/textHeadings';
  import { searchTextDocument, type TextSearchResult } from '../readers/text/textSearch';
  import type { TextBookmark } from '../types/document';
  import type { RecentDocumentDto } from '../types/epub';

  export let desktopRuntime = true;
  export let onOpen: () => void;
  export let activeView: 'workspace' | 'library' = 'workspace';
  export let onSelectWorkspace: () => void = () => {};
  export let onSelectLibrary: () => void = () => {};
  export let recentDocuments: RecentDocumentDto[] = [];
  export let onOpenRecent: (document: RecentDocumentDto) => void = () => {};
  export let onRemoveRecent: (document: RecentDocumentDto) => void = () => {};
  export let textHeadings: TextHeading[] = [];
  export let onRevealHeading: (heading: TextHeading) => void = () => {};
  export let activeTextDocument = false;
  export let textContent = '';
  export let getTextContent: () => string = () => textContent;
  export let textBookmarks: TextBookmark[] = [];
  export let onAddTextBookmark: () => void = () => {};
  export let onRevealTextOffset: (offset: number) => void = () => {};
  export let onRenameTextBookmark: (bookmark: TextBookmark) => void = () => {};
  export let onDeleteTextBookmark: (bookmark: TextBookmark) => void = () => {};

  let textSearchQuery = '';
  let textSearchResults: TextSearchResult[] = [];
  let textSearchPerformed = false;
  let textSearchCaseSensitive = false;
  let textSearchWholeWord = false;

  function performTextSearch(): void {
    textSearchResults = searchTextDocument(getTextContent(), textSearchQuery, {
      caseSensitive: textSearchCaseSensitive,
      wholeWord: textSearchWholeWord,
    });
    textSearchPerformed = true;
  }

  const sections: Array<{
    label: string;
    items: Array<{ id: 'workspace' | 'library'; label: string; icon: IconName }>;
  }> = [
    {
      label: '工作区',
      items: [
        { id: 'workspace', label: '阅读与编辑', icon: 'document' },
        { id: 'library', label: '书库', icon: 'library' },
      ],
    },
  ];
</script>

<aside aria-label="主导航" class="navigation-pane">
  <div class="open-area">
    <button class="open-button" disabled={!desktopRuntime} onclick={onOpen} type="button">
      <span>打开文件</span>
    </button>
  </div>
  <div class="nav-scroll">
    {#if recentDocuments.length}
      <section class="recent-section">
        <h2>最近文件</h2>
        <div class="recent-items">
          {#each recentDocuments as document}
            <div class="recent-item">
              <button
                class="recent-open"
                disabled={!document.available}
                onclick={() => onOpenRecent(document)}
                title={document.available ? document.path : `文件已移动 · ${document.path}`}
                type="button"
              >
                <span class="recent-kind">{document.documentKind.toUpperCase()}</span>
                <span class="recent-copy">
                  <strong>{document.displayTitle}</strong>
                  {#if !document.available}<small class="missing-copy">文件已移动</small>{:else if document.author}<small>{document.author}</small>{/if}
                </span>
              </button>
              <button
                aria-label={`从最近文件中移除 ${document.displayTitle}`}
                class="recent-remove"
                onclick={() => onRemoveRecent(document)}
                title="从最近文件中移除"
                type="button"
              >×</button>
            </div>
          {/each}
        </div>
      </section>
    {/if}
    {#if textHeadings.length}
      <section class="outline-section">
        <h2>TXT 大纲</h2>
        <div class="outline-items">
          {#each textHeadings as heading}
            <button
              aria-label={`第 ${heading.lineNumber} 行 ${heading.label}`}
              class="outline-button"
              onclick={() => onRevealHeading(heading)}
              title={`第 ${heading.lineNumber} 行 · ${heading.label}`}
              type="button"
            >
              <span class="outline-line">{heading.lineNumber}</span>
              <span class="outline-label">{heading.label}</span>
            </button>
          {/each}
        </div>
      </section>
    {/if}
    {#if activeTextDocument}
      <section class="text-tools-section">
        <div class="text-tools-heading">
          <h2>TXT 工具</h2>
          <button aria-label="添加 TXT 书签" class="compact-action" onclick={onAddTextBookmark} title="记录当前光标位置" type="button">＋书签</button>
        </div>
        <form class="text-search-form" onsubmit={(event) => { event.preventDefault(); performTextSearch(); }}>
          <input aria-label="TXT 全文检索" bind:value={textSearchQuery} placeholder="搜索全文" type="search" />
          <button aria-label="搜索 TXT 全文" class="search-submit" type="submit">搜索</button>
          <label><input bind:checked={textSearchCaseSensitive} type="checkbox" />区分大小写</label>
          <label><input bind:checked={textSearchWholeWord} type="checkbox" />全词</label>
        </form>
        {#if textSearchResults.length}
          <div aria-label="TXT 搜索结果" class="text-result-items">
            {#each textSearchResults as result}
              <button
                aria-label={`第 ${result.lineNumber} 行 · ${result.snippet}`}
                onclick={() => onRevealTextOffset(result.characterOffset)}
                title={result.snippet}
                type="button"
              ><span class="outline-line">{result.lineNumber}</span><span class="result-copy">{result.snippet}</span></button>
            {/each}
          </div>
        {:else if textSearchPerformed}
          <p class="empty-tool-state">没有搜索结果。</p>
        {/if}
        <div class="text-bookmark-items" aria-label="TXT 书签">
          {#each textBookmarks as bookmark}
            <article>
              <button
                aria-label={`${bookmark.title ?? `第 ${bookmark.lineNumber} 行`} · ${bookmark.preview}`}
                onclick={() => onRevealTextOffset(bookmark.characterOffset)}
                title={bookmark.preview}
                type="button"
              ><span class="outline-line">{bookmark.lineNumber}</span><span class="result-copy">{bookmark.title ?? bookmark.preview}</span></button>
              <div><button onclick={() => onRenameTextBookmark(bookmark)} type="button">改名</button><button onclick={() => onDeleteTextBookmark(bookmark)} type="button">删除</button></div>
            </article>
          {/each}
        </div>
      </section>
    {/if}
    {#each sections as section}
      <section>
        <h2>{section.label}</h2>
        <div class="nav-items">
          {#each section.items as item}
            <button
              aria-current={activeView === item.id ? 'page' : undefined}
              class:active={activeView === item.id}
              onclick={item.id === 'workspace' ? onSelectWorkspace : onSelectLibrary}
              title={item.label}
              type="button"
            >
              <Icon name={item.icon} size={18} />
              <span>{item.label}</span>
            </button>
          {/each}
        </div>
      </section>
    {/each}
  </div>

  <div class="local-status">
    <span class="status-dot"></span>
    <span>本地优先</span>
  </div>
</aside>

<style>
  .navigation-pane {
    background: var(--surface-pane);
    border-right: 1px solid var(--border-subtle);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .nav-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 19px 10px;
  }

  .open-area {
    border-bottom: 1px solid var(--border-subtle);
    padding: 11px 10px;
    display: grid;
  }

  .open-button {
    background: var(--accent);
    color: white;
    justify-content: center;
  }

  .open-button:hover:not(:disabled) {
    background: var(--accent-strong);
    color: white;
  }

  section + section {
    border-top: 1px solid var(--border-subtle);
    margin-top: 17px;
    padding-top: 17px;
  }

  h2 {
    color: var(--text-tertiary);
    font: 600 11px/1.2 var(--font-ui);
    letter-spacing: 0.04em;
    margin: 0 9px 8px;
    text-transform: uppercase;
  }

  .nav-items {
    display: grid;
    gap: 3px;
  }

  .recent-items {
    display: grid;
    gap: 3px;
  }

  .recent-item {
    align-items: center;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 26px;
  }

  .recent-open {
    gap: 7px;
    min-width: 0;
  }

  .recent-remove {
    border-radius: 999px;
    color: var(--text-tertiary);
    font: 500 16px/1 var(--font-ui);
    min-height: 26px;
    padding: 0;
    width: 26px;
  }

  .recent-remove:hover:not(:disabled) {
    background: var(--danger-soft);
    color: var(--danger);
  }

  .recent-kind {
    color: var(--text-disabled);
    flex: 0 0 28px;
    font: 700 8px/1 var(--font-mono);
  }

  .recent-copy {
    display: grid;
    min-width: 0;
  }

  .recent-copy strong,
  .recent-copy small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .missing-copy {
    color: var(--danger);
  }

  .outline-items {
    display: grid;
    gap: 2px;
  }

  .outline-button {
    gap: 8px;
    min-width: 0;
  }

  .outline-line {
    color: var(--text-disabled);
    flex: 0 0 28px;
    font: 500 8px/1 var(--font-mono);
    text-align: right;
  }

  .outline-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .text-tools-heading { align-items:center; display:flex; justify-content:space-between; }
  .text-tools-heading h2 { margin-bottom:0; }
  .compact-action { border:1px solid var(--border-subtle); font-size:10px; min-height:26px; padding:0 7px; width:auto; }
  .text-search-form { display:grid; gap:6px; grid-template-columns:minmax(0,1fr) auto; margin-top:10px; }
  .text-search-form input[type='search'] { background:var(--surface-control); border:1px solid var(--border-strong); border-radius:var(--radius-sm); color:var(--text-primary); font:500 11px/1 var(--font-ui); min-width:0; padding:7px; }
  .text-search-form .search-submit { background:var(--surface-control); border:1px solid var(--border-strong); font-size:10px; min-height:30px; padding:0 8px; width:auto; }
  .text-search-form label { align-items:center; color:var(--text-tertiary); display:flex; font:500 9px/1.2 var(--font-ui); gap:4px; }
  .text-search-form label input { margin:0; }
  .text-result-items, .text-bookmark-items { display:grid; gap:2px; margin-top:9px; min-width:0; overflow:hidden; }
  .text-result-items button, .text-bookmark-items article > button { max-width:100%; min-height:30px; min-width:0; padding:5px 7px; width:100%; }
  .result-copy { min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .empty-tool-state { color:var(--text-tertiary); font:500 10px/1.4 var(--font-ui); margin:9px 6px 0; }
  .text-bookmark-items article { border-top:1px solid var(--border-subtle); padding-top:3px; }
  .text-bookmark-items article > div { display:flex; gap:3px; justify-content:flex-end; }
  .text-bookmark-items article > div button { font-size:9px; min-height:22px; padding:0 5px; width:auto; }

  .recent-copy strong {
    font: 550 11px/1.3 var(--font-ui);
  }

  .recent-copy small {
    color: var(--text-tertiary);
    font: 400 9px/1.25 var(--font-ui);
  }

  button {
    align-items: center;
    background: transparent;
    border: 0;
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    display: flex;
    font: 500 13px/1 var(--font-ui);
    gap: 10px;
    min-height: 36px;
    padding: 0 10px;
    text-align: left;
    width: 100%;
  }

  button:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--text-primary);
  }

  button.active {
    background: var(--accent-soft);
    color: var(--accent-strong);
    position: relative;
  }

  button.active::before {
    background: var(--accent);
    border-radius: 999px;
    bottom: 8px;
    content: '';
    left: 0;
    position: absolute;
    top: 8px;
    width: 2px;
  }

  button:disabled {
    color: var(--text-disabled);
    cursor: default;
  }

  .local-status {
    align-items: center;
    border-top: 1px solid var(--border-subtle);
    color: var(--text-tertiary);
    display: flex;
    font: 500 11px/1 var(--font-ui);
    gap: 8px;
    min-height: 44px;
    padding: 0 18px;
  }

  .status-dot {
    background: var(--success);
    border-radius: 999px;
    height: 7px;
    width: 7px;
  }

</style>
