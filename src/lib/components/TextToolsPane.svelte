<script lang="ts">
  import { onDestroy } from 'svelte';

  import type { TextHeading } from '../editors/textHeadings';
  import { resizedPaneWidth, resizedPaneWidthFromKeyboard } from '../layout/workspaceLayout';
  import { searchTextDocument, type TextSearchResult } from '../readers/text/textSearch';
  import type { TextBookmark } from '../types/document';

  export let textHeadings: TextHeading[] = [];
  export let getTextContent: () => string = () => '';
  export let textBookmarks: TextBookmark[] = [];
  export let onAddTextBookmark: () => void = () => {};
  export let onRevealTextOffset: (offset: number) => void = () => {};
  export let onRenameTextBookmark: (bookmark: TextBookmark) => void = () => {};
  export let onDeleteTextBookmark: (bookmark: TextBookmark) => void = () => {};

  let paneElement: HTMLElement;
  let paneWidth = 240;
  let resizeStartX = 0;
  let resizeStartWidth = 240;
  let resizing = false;
  let textSearchQuery = '';
  let textSearchResults: TextSearchResult[] = [];
  let textSearchPerformed = false;
  let textSearchCaseSensitive = false;
  let textSearchWholeWord = false;

  onDestroy(endResize);

  function maximumPaneWidth(): number {
    const workspaceWidth = paneElement?.parentElement?.clientWidth || 900;
    return Math.max(180, Math.min(480, workspaceWidth - 360));
  }

  function beginResize(event: PointerEvent): void {
    if (event.button !== 0) return;
    resizeStartX = event.clientX;
    resizeStartWidth = paneWidth;
    resizing = true;
    window.addEventListener('pointermove', continueResize);
    window.addEventListener('pointerup', endResize);
    window.addEventListener('pointercancel', endResize);
    event.preventDefault();
  }

  function continueResize(event: PointerEvent): void {
    if (!resizing) return;
    paneWidth = resizedPaneWidth(
      'left',
      resizeStartWidth,
      resizeStartX,
      event.clientX,
      180,
      maximumPaneWidth(),
    );
  }

  function endResize(): void {
    resizing = false;
    window.removeEventListener('pointermove', continueResize);
    window.removeEventListener('pointerup', endResize);
    window.removeEventListener('pointercancel', endResize);
  }

  function resizeFromKeyboard(event: KeyboardEvent): void {
    const nextWidth = resizedPaneWidthFromKeyboard(
      'left',
      paneWidth,
      event.key,
      180,
      maximumPaneWidth(),
    );
    if (nextWidth === null) return;
    paneWidth = Math.round(Math.max(180, Math.min(maximumPaneWidth(), nextWidth)));
    event.preventDefault();
  }

  function performTextSearch(): void {
    textSearchResults = searchTextDocument(getTextContent(), textSearchQuery, {
      caseSensitive: textSearchCaseSensitive,
      wholeWord: textSearchWholeWord,
    });
    textSearchPerformed = true;
  }

  function snippetPart(result: TextSearchResult, part: 'before' | 'match' | 'after'): string {
    if (part === 'before') return result.snippet.slice(0, result.matchStart);
    if (part === 'match') return result.snippet.slice(result.matchStart, result.matchEnd);
    return result.snippet.slice(result.matchEnd);
  }
</script>

<aside
  aria-label="TXT 目录与工具"
  bind:this={paneElement}
  class:resizing
  class="text-tools-pane"
  style={`width: ${paneWidth}px`}
>
  <div class="pane-scroll">
    <section class="outline-section">
      <header><div><span>CONTENTS</span><h2>TXT 目录</h2></div><small>{textHeadings.length} 项</small></header>
      {#if textHeadings.length}
        <div class="outline-items">
          {#each textHeadings as heading}
            <button
              aria-label={`第 ${heading.lineNumber} 行 ${heading.label}`}
              class="outline-button"
              onclick={() => onRevealTextOffset(heading.from)}
              title={`第 ${heading.lineNumber} 行 · ${heading.label}`}
              type="button"
            ><span class="outline-line">{heading.lineNumber}</span><span class="item-copy">{heading.label}</span></button>
          {/each}
        </div>
      {:else}
        <p class="empty-tool-state">尚未识别到章节标题，可在右侧设置中调整目录规则。</p>
      {/if}
    </section>

    <section class="search-section">
      <div class="section-heading"><h2>全文搜索</h2>{#if textSearchPerformed}<span class="result-count">共 {textSearchResults.length} 条结果</span>{/if}</div>
      <form class="text-search-form" onsubmit={(event) => { event.preventDefault(); performTextSearch(); }}>
        <input aria-label="TXT 全文检索" bind:value={textSearchQuery} placeholder="搜索全文" type="search" />
        <button aria-label="搜索 TXT 全文" class="search-submit" type="submit">搜索</button>
        <label><input bind:checked={textSearchCaseSensitive} type="checkbox" />区分大小写</label>
        <label><input bind:checked={textSearchWholeWord} type="checkbox" />全词</label>
      </form>
      {#if textSearchResults.length}
        <div aria-label="TXT 搜索结果" class="result-items">
          {#each textSearchResults as result, index}
            <button
              aria-label={`第 ${result.lineNumber} 行 · ${result.snippet}`}
              onclick={() => onRevealTextOffset(result.characterOffset)}
              title={result.snippet}
              type="button"
            ><span class="result-index">{String(index + 1).padStart(2, '0')}</span><span class="result-copy"><small>第 {result.lineNumber} 行</small><span>{snippetPart(result, 'before')}<mark>{snippetPart(result, 'match')}</mark>{snippetPart(result, 'after')}</span></span></button>
          {/each}
        </div>
      {:else if textSearchPerformed}
        <p class="empty-tool-state">没有搜索结果。</p>
      {/if}
    </section>

    <section class="bookmarks-section">
      <div class="section-heading"><h2>书签</h2><button aria-label="添加 TXT 书签" onclick={onAddTextBookmark} type="button">＋ 添加</button></div>
      <div aria-label="TXT 书签" class="bookmark-items">
        {#each textBookmarks as bookmark}
          <article>
            <button
              aria-label={`${bookmark.title ?? `第 ${bookmark.lineNumber} 行`} · ${bookmark.preview}`}
              onclick={() => onRevealTextOffset(bookmark.characterOffset)}
              title={bookmark.preview}
              type="button"
            ><span class="outline-line">{bookmark.lineNumber}</span><span class="item-copy">{bookmark.title ?? bookmark.preview}</span></button>
            <div><button onclick={() => onRenameTextBookmark(bookmark)} type="button">改名</button><button onclick={() => onDeleteTextBookmark(bookmark)} type="button">删除</button></div>
          </article>
        {:else}
          <p class="empty-tool-state">还没有书签。</p>
        {/each}
      </div>
    </section>
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_tabindex a11y_no_noninteractive_element_interactions (ARIA Window Splitter pattern) -->
  <div
    aria-label="调整 TXT 目录宽度"
    aria-orientation="vertical"
    aria-valuemax={maximumPaneWidth()}
    aria-valuemin="180"
    aria-valuenow={paneWidth}
    class="resize-handle"
    onkeydown={resizeFromKeyboard}
    onpointerdown={beginResize}
    role="separator"
    tabindex="0"
  ></div>
</aside>

<style>
  .text-tools-pane { background:var(--surface-pane); border-right:1px solid var(--border-subtle); box-sizing:border-box; flex:0 0 auto; height:100%; min-height:0; min-width:180px; position:relative; }
  .pane-scroll { height:100%; overflow:auto; padding:14px 12px 20px; box-sizing:border-box; }
  section + section { border-top:1px solid var(--border-subtle); margin-top:16px; padding-top:16px; }
  header, .section-heading { align-items:center; display:flex; justify-content:space-between; margin-bottom:9px; }
  header > div { display:grid; gap:4px; }
  header span { color:var(--accent-strong); font:700 8px/1 var(--font-mono); letter-spacing:.12em; }
  h2 { color:var(--text-secondary); font:650 11px/1.2 var(--font-ui); margin:0; }
  header small { background:var(--surface-subtle); border-radius:99px; color:var(--text-tertiary); font:600 8px/1 var(--font-ui); padding:4px 6px; }
  .outline-items, .result-items, .bookmark-items { display:grid; gap:2px; min-width:0; }
  button { align-items:center; background:transparent; border:0; border-radius:var(--radius-sm); color:var(--text-secondary); display:flex; font:500 11px/1.3 var(--font-ui); gap:7px; min-height:31px; min-width:0; padding:5px 7px; text-align:left; width:100%; }
  button:hover { background:var(--surface-hover); color:var(--text-primary); }
  .outline-line { color:var(--text-disabled); flex:0 0 28px; font:500 8px/1 var(--font-mono); text-align:right; }
  .item-copy { min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .empty-tool-state { color:var(--text-tertiary); font:500 9px/1.5 var(--font-ui); margin:8px 5px 0; }
  .text-search-form { display:grid; gap:6px; grid-template-columns:minmax(0,1fr) auto; margin-top:9px; }
  .text-search-form input[type='search'] { background:var(--surface-control); border:1px solid var(--border-strong); border-radius:var(--radius-sm); color:var(--text-primary); font:500 10px/1 var(--font-ui); min-width:0; padding:7px; }
  .text-search-form .search-submit { background:var(--surface-control); border:1px solid var(--border-strong); font-size:9px; min-height:30px; padding:0 8px; width:auto; }
  .text-search-form label { align-items:center; color:var(--text-tertiary); display:flex; font:500 8px/1.2 var(--font-ui); gap:4px; }
  .text-search-form label input { margin:0; }
  .result-items { margin-top:8px; }
  .result-items button { align-items:flex-start; border-bottom:1px solid var(--border-subtle); border-radius:0; min-height:58px; padding-block:9px; }
  .result-index { color:var(--text-disabled); flex:0 0 24px; font:650 9px/1.3 var(--font-mono); padding-top:2px; }
  .result-copy { display:grid; gap:4px; min-width:0; }
  .result-copy small { color:var(--text-tertiary); font:550 8px/1.2 var(--font-ui); }
  .result-copy > span { color:var(--text-secondary); display:-webkit-box; font:500 10px/1.45 var(--font-ui); line-clamp:2; overflow:hidden; overflow-wrap:anywhere; -webkit-box-orient:vertical; -webkit-line-clamp:2; }
  .result-count { color:var(--text-tertiary); font:600 8px/1 var(--font-ui); }
  mark { background:var(--accent-soft); color:var(--accent-strong); }
  .section-heading button { border:1px solid var(--border-subtle); font-size:9px; min-height:26px; padding:0 7px; width:auto; }
  .bookmark-items article { border-top:1px solid var(--border-subtle); padding-top:3px; }
  .bookmark-items article > div { display:flex; gap:3px; justify-content:flex-end; }
  .bookmark-items article > div button { font-size:8px; min-height:21px; padding:0 5px; width:auto; }
  .resize-handle { bottom:0; cursor:col-resize; outline:none; position:absolute; right:-4px; top:0; width:8px; z-index:3; }
  .resize-handle::after { background:transparent; bottom:0; content:''; left:3px; position:absolute; top:0; transition:background var(--motion-fast); width:2px; }
  .resize-handle:hover::after, .resize-handle:focus-visible::after, .resizing .resize-handle::after { background:var(--accent); }
  .text-tools-pane.resizing { user-select:none; }
</style>
