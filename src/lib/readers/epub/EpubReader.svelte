<script lang="ts">
  import { onDestroy, onMount } from 'svelte';

  import {
    cancelEpubSearch,
    deleteEpubBookmark,
    epubResourceUrl,
    saveEpubBookmark,
    saveEpubProgress,
    searchEpubDocument,
  } from '../../services/epubDocumentService';
  import type {
    EpubBookmark,
    EpubLocator,
    EpubReadingSettings,
    EpubSearchResult,
    OpenedEpubDocumentDto,
    TocNode,
  } from '../../types/epub';
  import type { AppErrorDto } from '../../types/ipc';
  import { normalizeAppError } from '../../services/backend';
  import {
    parseEpubBridgeMessage,
    parseExternalEpubHref,
    parseInternalEpubHref,
    type ExternalEpubTarget,
  } from './epubBridge';
  import {
    loadEpubReadingSettings,
    normalizeEpubReadingSettings,
    persistEpubReadingSettings,
  } from './epubSettings';

  interface VisibleTocNode extends TocNode {
    depth: number;
    hasChildren: boolean;
  }

  export let document: OpenedEpubDocumentDto;
  export let spineIndex = 0;
  export let onSpineChange: (index: number) => void;
  export let onError: (error: AppErrorDto) => void = () => {};
  export let onLocatorChange: (locator: EpubLocator) => void = () => {};
  export let onBookmarksChange: (bookmarks: EpubBookmark[]) => void = () => {};

  let iframe: HTMLIFrameElement | null = null;
  let showMetadata = false;
  let showSettings = false;
  let showSearch = false;
  let showBookmarks = false;
  let settings = loadEpubReadingSettings();
  let bookmarks: EpubBookmark[] = [...document.bookmarks];
  let expandedTocIds = new Set(document.document.toc.map((node) => node.id));
  let currentFragment =
    document.initialLocator?.spineIndex === spineIndex ? document.initialLocator.fragment : null;
  let currentProgression =
    document.initialLocator?.spineIndex === spineIndex
      ? document.initialLocator.progressionInChapter
      : 0;
  let progressTimer: ReturnType<typeof setTimeout> | null = null;
  let progressStatus: 'idle' | 'saving' | 'saved' | 'failed' = 'idle';
  let query = '';
  let caseSensitive = false;
  let wholeWord = false;
  let searchResults: EpubSearchResult[] = [];
  let searchStatus: 'idle' | 'searching' | 'failed' = 'idle';
  let activeSearchId: string | null = null;
  let searchSequence = 0;
  let externalLink: ExternalEpubTarget | null = null;

  $: current = document.document.spine[spineIndex] ?? document.document.spine[0];
  $: chapterUrl = current
    ? epubResourceUrl(document.sessionId, current.resourceId, currentFragment)
    : 'about:blank';
  $: visibleToc = flattenToc(document.document.toc, expandedTocIds);
  $: currentTitle =
    findTocLabel(document.document.toc, current?.resourceId) ?? `第 ${spineIndex + 1} 章`;

  export async function flushProgress(): Promise<void> {
    if (progressTimer) {
      clearTimeout(progressTimer);
      progressTimer = null;
    }
    await persistProgress();
  }

  onMount(() => {
    const listener = (event: MessageEvent<unknown>) => handleBridgeMessage(event);
    window.addEventListener('message', listener);
    return () => window.removeEventListener('message', listener);
  });

  onDestroy(() => {
    if (progressTimer) clearTimeout(progressTimer);
    if (activeSearchId) void cancelEpubSearch(document.documentId, activeSearchId);
  });

  function flattenToc(
    nodes: TocNode[],
    expanded: Set<string>,
    depth = 0,
  ): VisibleTocNode[] {
    return nodes.flatMap((node) => {
      const visible: VisibleTocNode = {
        ...node,
        depth,
        hasChildren: node.children.length > 0,
      };
      return [
        visible,
        ...(node.children.length && expanded.has(node.id)
          ? flattenToc(node.children, expanded, depth + 1)
          : []),
      ];
    });
  }

  function findTocLabel(nodes: TocNode[], resourceId: string | undefined): string | null {
    if (!resourceId) return null;
    for (const node of nodes) {
      if (node.resourceId === resourceId && node.label.trim()) return node.label;
      const child = findTocLabel(node.children, resourceId);
      if (child) return child;
    }
    return null;
  }

  function toggleToc(node: VisibleTocNode): void {
    const next = new Set(expandedTocIds);
    if (next.has(node.id)) next.delete(node.id);
    else next.add(node.id);
    expandedTocIds = next;
  }

  function navigateTo(
    resourceId: string | null,
    fragment: string | null = null,
    progression = 0,
  ): void {
    if (!resourceId) return;
    const index = document.document.spine.findIndex((item) => item.resourceId === resourceId);
    if (index < 0) return;
    if (index !== spineIndex) void persistProgress();
    currentFragment = fragment;
    currentProgression = progression;
    if (index === spineIndex) {
      scheduleProgressSave();
      return;
    }
    onSpineChange(index);
    scheduleProgressSave();
  }

  function changeChapter(index: number): void {
    if (index < 0 || index >= document.document.spine.length) return;
    navigateTo(document.document.spine[index]?.resourceId ?? null);
  }

  function handleBridgeMessage(event: MessageEvent<unknown>): void {
    const message = parseEpubBridgeMessage(event, {
      source: iframe?.contentWindow ?? null,
      document,
    });
    if (!message) return;
    if (message.type === 'progress') {
      currentProgression = message.payload.progression;
      currentFragment = message.payload.fragment;
      onLocatorChange(currentLocator());
      scheduleProgressSave();
      return;
    }
    const target = parseInternalEpubHref(message.payload.href, document.sessionId);
    if (target) {
      navigateTo(target.resourceId, target.fragment);
      return;
    }
    externalLink = parseExternalEpubHref(message.payload.href);
  }

  function currentLocator(): EpubLocator {
    return {
      documentId: document.documentId,
      documentFingerprint: document.fileFingerprint,
      spineIndex,
      spineHref: current?.resourceId ?? document.document.spine[0]?.resourceId ?? '',
      fragment: currentFragment,
      progressionInChapter: Number.isFinite(currentProgression) ? currentProgression : 0,
      characterOffset: null,
    };
  }

  function scheduleProgressSave(): void {
    progressStatus = 'saving';
    if (progressTimer) clearTimeout(progressTimer);
    progressTimer = setTimeout(() => {
      progressTimer = null;
      void persistProgress();
    }, 900);
  }

  async function persistProgress(): Promise<void> {
    if (!current) return;
    try {
      await saveEpubProgress(currentLocator());
      progressStatus = 'saved';
    } catch {
      progressStatus = 'failed';
    }
  }

  function postReaderState(): void {
    const target = iframe?.contentWindow;
    if (!target) return;
    const identity = {
      source: 'readloom-host',
      version: 1,
      documentId: document.documentId,
      sessionId: document.sessionId,
      token: document.bridgeToken,
    } as const;
    target.postMessage({ ...identity, type: 'settings', payload: settings }, '*');
    const locator = currentLocator();
    target.postMessage(
      {
        ...identity,
        type: 'restore',
        payload: { progression: locator.progressionInChapter },
      },
      '*',
    );
  }

  function updateSettings(patch: Partial<EpubReadingSettings>): void {
    settings = normalizeEpubReadingSettings({ ...settings, ...patch });
    persistEpubReadingSettings(settings);
    postReaderState();
  }

  async function addBookmark(): Promise<void> {
    try {
      const bookmark = await saveEpubBookmark(currentLocator(), null);
      bookmarks = [...bookmarks, bookmark];
      onBookmarksChange(bookmarks);
      showBookmarks = true;
    } catch (error) {
      onError(normalizeAppError(error));
    }
  }

  async function renameBookmark(bookmark: EpubBookmark): Promise<void> {
    const title = window.prompt('书签标题', bookmark.title ?? bookmark.chapterTitle);
    if (title === null) return;
    try {
      const updated = await saveEpubBookmark(bookmark.locator, title, bookmark.bookmarkId);
      bookmarks = bookmarks.map((item) =>
        item.bookmarkId === bookmark.bookmarkId ? updated : item,
      );
      onBookmarksChange(bookmarks);
    } catch (error) {
      onError(normalizeAppError(error));
    }
  }

  async function removeBookmark(bookmark: EpubBookmark): Promise<void> {
    if (!window.confirm(`删除书签“${bookmark.title ?? bookmark.chapterTitle}”？`)) return;
    try {
      await deleteEpubBookmark(document.documentId, bookmark.bookmarkId);
      bookmarks = bookmarks.filter((item) => item.bookmarkId !== bookmark.bookmarkId);
      onBookmarksChange(bookmarks);
    } catch (error) {
      onError(normalizeAppError(error));
    }
  }

  function jumpToBookmark(bookmark: EpubBookmark): void {
    if (!bookmark.valid) return;
    navigateTo(
      bookmark.locator.spineHref,
      bookmark.locator.fragment,
      bookmark.locator.progressionInChapter,
    );
  }

  async function performSearch(): Promise<void> {
    const trimmed = query.trim();
    if (!trimmed) {
      searchResults = [];
      return;
    }
    if (activeSearchId) await cancelEpubSearch(document.documentId, activeSearchId);
    const requestId = `search-${Date.now()}-${++searchSequence}`;
    activeSearchId = requestId;
    searchStatus = 'searching';
    try {
      const results = await searchEpubDocument({
        documentId: document.documentId,
        requestId,
        query: trimmed,
        caseSensitive,
        wholeWord,
        maximumResults: 200,
      });
      if (activeSearchId !== requestId) return;
      searchResults = results;
      searchStatus = 'idle';
    } catch (error) {
      const normalized = normalizeAppError(error);
      if (normalized.code === 'SEARCH_CANCELLED') return;
      searchStatus = 'failed';
      onError(normalized);
    }
  }

  function jumpToSearchResult(result: EpubSearchResult): void {
    navigateTo(result.spineHref);
  }

  async function copyExternalLink(): Promise<void> {
    if (!externalLink) return;
    const href = externalLink.href;
    try {
      await navigator.clipboard.writeText(href);
      externalLink = null;
    } catch {
      window.prompt('复制外部链接', href);
    }
  }

  function snippetPart(result: EpubSearchResult, part: 'before' | 'match' | 'after'): string {
    const characters = [...result.temporarySnippet];
    if (part === 'before') return characters.slice(0, result.matchStart).join('');
    if (part === 'match') return characters.slice(result.matchStart, result.matchEnd).join('');
    return characters.slice(result.matchEnd).join('');
  }
</script>

<section class="epub-reader" aria-label="EPUB 阅读器">
  <header class="reader-toolbar">
    <button aria-label="上一章" disabled={spineIndex <= 0} onclick={() => changeChapter(spineIndex - 1)} type="button">←</button>
    <div class="chapter-heading">
      <strong>{currentTitle}</strong>
      <span>{spineIndex + 1} / {document.document.spine.length} · {Math.round(currentProgression * 100)}%</span>
    </div>
    <button aria-label="下一章" disabled={spineIndex >= document.document.spine.length - 1} onclick={() => changeChapter(spineIndex + 1)} type="button">→</button>
    <div class="reader-actions">
      <button onclick={addBookmark} type="button">添加书签</button>
      <button class:active={showBookmarks} onclick={() => (showBookmarks = !showBookmarks)} type="button">书签 {bookmarks.length}</button>
      <button class:active={showSearch} onclick={() => (showSearch = !showSearch)} type="button">搜索</button>
      <button class:active={showSettings} onclick={() => (showSettings = !showSettings)} type="button">排版</button>
      <button class:active={showMetadata} onclick={() => (showMetadata = !showMetadata)} type="button">书籍信息</button>
    </div>
  </header>

  {#if document.document.layout === 'fixed'}
    <div class="layout-warning" role="status">这是固定布局 EPUB，当前仅提供有限的流式阅读兼容。</div>
  {/if}

  {#if showSearch}
    <form class="search-panel" onsubmit={(event) => { event.preventDefault(); void performSearch(); }}>
      <input aria-label="书内搜索" bind:value={query} maxlength="256" placeholder="搜索全部线性章节" />
      <label><input bind:checked={caseSensitive} type="checkbox" /> 区分大小写</label>
      <label><input bind:checked={wholeWord} type="checkbox" /> 全字匹配</label>
      <button disabled={searchStatus === 'searching'} type="submit">{searchStatus === 'searching' ? '搜索中…' : '搜索'}</button>
      <span>{searchResults.length} 个结果</span>
    </form>
  {/if}

  {#if showSettings}
    <section aria-label="EPUB 阅读设置" class="settings-panel">
      <label>字体
        <select value={settings.fontFamily} onchange={(event) => updateSettings({ fontFamily: event.currentTarget.value as EpubReadingSettings['fontFamily'] })}>
          <option value="system">系统</option><option value="serif">衬线</option><option value="sans">无衬线</option>
        </select>
      </label>
      <label>字号 <input min="12" max="32" type="range" value={settings.fontSize} oninput={(event) => updateSettings({ fontSize: Number(event.currentTarget.value) })} /><span>{settings.fontSize}px</span></label>
      <label>行高 <input min="1.2" max="2.4" step="0.1" type="range" value={settings.lineHeight} oninput={(event) => updateSettings({ lineHeight: Number(event.currentTarget.value) })} /><span>{settings.lineHeight}</span></label>
      <label>内容宽度 <input min="480" max="1200" step="16" type="range" value={settings.contentWidth} oninput={(event) => updateSettings({ contentWidth: Number(event.currentTarget.value) })} /><span>{settings.contentWidth}px</span></label>
      <label>出版者样式
        <select value={settings.publisherStyles} onchange={(event) => updateSettings({ publisherStyles: event.currentTarget.value as EpubReadingSettings['publisherStyles'] })}>
          <option value="use">使用</option><option value="partial">部分覆盖</option><option value="ignore">忽略</option>
        </select>
      </label>
      <label><input checked={settings.ignorePublisherFonts} onchange={(event) => updateSettings({ ignorePublisherFonts: event.currentTarget.checked })} type="checkbox" /> 忽略出版者字体</label>
      <label><input checked={settings.ignorePublisherColors} onchange={(event) => updateSettings({ ignorePublisherColors: event.currentTarget.checked })} type="checkbox" /> 忽略出版者颜色</label>
      <label><input checked={settings.allowInternalFonts} onchange={(event) => updateSettings({ allowInternalFonts: event.currentTarget.checked })} type="checkbox" /> 允许内部字体</label>
    </section>
  {/if}

  <div class="reader-body">
    <aside aria-label="EPUB 目录" class="toc-pane">
      <h2>目录</h2>
      {#if visibleToc.length}
        <nav>
          {#each visibleToc as node}
            <div class="toc-row" style={`--toc-depth:${Math.min(node.depth, 8)}`}>
              {#if node.hasChildren}
                <button aria-label={`${expandedTocIds.has(node.id) ? '折叠' : '展开'} ${node.label}`} class="toc-toggle" onclick={() => toggleToc(node)} type="button">{expandedTocIds.has(node.id) ? '▾' : '▸'}</button>
              {:else}<span class="toc-spacer"></span>{/if}
              <button aria-current={node.resourceId === current?.resourceId ? 'page' : undefined} class:active={node.resourceId === current?.resourceId} class="toc-link" disabled={!node.resourceId} onclick={() => navigateTo(node.resourceId, node.fragment)} title={node.label} type="button">{node.label}</button>
            </div>
          {/each}
        </nav>
      {:else}<p>此 EPUB 没有目录，仍可按 spine 顺序阅读。</p>{/if}
    </aside>

    <div class="viewport-shell">
      {#if current}
        {#key chapterUrl}
          <iframe
            allow="camera 'none'; microphone 'none'; geolocation 'none'; clipboard-read 'none'; clipboard-write 'none'"
            bind:this={iframe}
            onload={postReaderState}
            referrerpolicy="no-referrer"
            sandbox="allow-scripts"
            src={chapterUrl}
            title={`${document.document.metadata.title} · ${currentTitle}`}
          ></iframe>
        {/key}
      {:else}<div class="resource-error" role="alert">EPUB 没有可阅读章节。</div>{/if}
      <span class="progress-save" data-status={progressStatus}>{progressStatus === 'failed' ? '进度暂未保存' : progressStatus === 'saving' ? '保存进度…' : ''}</span>
    </div>

    {#if showSearch}
      <aside aria-label="EPUB 搜索结果" class="side-panel results-panel">
        <h2>搜索结果</h2>
        {#each searchResults as result}
          <button onclick={() => jumpToSearchResult(result)} type="button">
            <strong>{result.chapterTitle}</strong>
            <span>{snippetPart(result, 'before')}<mark>{snippetPart(result, 'match')}</mark>{snippetPart(result, 'after')}</span>
          </button>
        {:else}<p>{searchStatus === 'searching' ? '正在安全提取可见正文…' : '没有搜索结果。'}</p>{/each}
      </aside>
    {:else if showBookmarks}
      <aside aria-label="EPUB 书签" class="side-panel bookmarks-panel">
        <h2>书签</h2>
        {#each bookmarks as bookmark}
          <article class:invalid={!bookmark.valid}>
            <button disabled={!bookmark.valid} onclick={() => jumpToBookmark(bookmark)} type="button"><strong>{bookmark.title ?? bookmark.chapterTitle}</strong><span>{bookmark.chapterTitle} · {Math.round(bookmark.locator.progressionInChapter * 100)}%</span></button>
            <div><button onclick={() => void renameBookmark(bookmark)} type="button">改名</button><button onclick={() => void removeBookmark(bookmark)} type="button">删除</button></div>
          </article>
        {:else}<p>还没有书签。</p>{/each}
      </aside>
    {:else if showMetadata}
      <aside aria-label="EPUB 元数据" class="side-panel metadata-pane">
        {#if document.document.coverResourceId}<img alt={`${document.document.metadata.title} 封面`} src={epubResourceUrl(document.sessionId, document.document.coverResourceId)} />{/if}
        <h2>{document.document.metadata.title}</h2>
        <dl>
          <div><dt>作者</dt><dd>{document.document.metadata.creators.join('、') || '未知'}</dd></div>
          <div><dt>语言</dt><dd>{document.document.metadata.languages.join('、') || '未知'}</dd></div>
          <div><dt>版本</dt><dd>EPUB {document.document.version}</dd></div>
          <div><dt>标识</dt><dd>{document.document.metadata.identifier ?? '未提供'}</dd></div>
          {#if document.document.metadata.publisher}<div><dt>出版者</dt><dd>{document.document.metadata.publisher}</dd></div>{/if}
        </dl>
      </aside>
    {/if}
  </div>
</section>

{#if externalLink}
  <div class="external-link-backdrop">
    <dialog aria-labelledby="external-link-title" aria-modal="true" class="external-link-dialog" open>
      <h2 id="external-link-title">外部链接</h2>
      <p>EPUB 想访问以下域名。Readloom 不会自动联网或打开浏览器。</p>
      <strong>{externalLink.domain}</strong>
      <code>{externalLink.href}</code>
      <div>
        <button onclick={() => (externalLink = null)} type="button">取消</button>
        <button onclick={() => void copyExternalLink()} type="button">复制链接</button>
      </div>
    </dialog>
  </div>
{/if}

<style>
  .epub-reader { display:grid; grid-template-rows:auto auto auto minmax(0,1fr); height:100%; min-height:0; }
  .reader-toolbar { align-items:center; background:var(--surface-chrome); border-bottom:1px solid var(--border-subtle); display:flex; gap:8px; min-height:45px; padding:6px 10px; }
  button, select, input { font:500 11px/1 var(--font-ui); }
  button, select { background:var(--surface-control); border:1px solid var(--border-strong); border-radius:var(--radius-sm); color:var(--text-secondary); min-height:30px; padding:0 10px; }
  button:hover:not(:disabled), button.active { background:var(--surface-hover); color:var(--text-primary); }
  button:disabled { color:var(--text-disabled); }
  .chapter-heading { display:grid; min-width:100px; }
  .chapter-heading strong { color:var(--text-primary); font:650 12px/1.25 var(--font-ui); overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .chapter-heading span, .progress-save { color:var(--text-tertiary); font:500 9px/1.2 var(--font-ui); }
  .reader-actions { display:flex; gap:5px; margin-left:auto; }
  .layout-warning { background:var(--warning-soft,#fff5d6); border-bottom:1px solid var(--warning); color:var(--text-secondary); font:500 11px/1.4 var(--font-ui); padding:8px 12px; }
  .search-panel, .settings-panel { align-items:center; background:var(--surface-pane); border-bottom:1px solid var(--border-subtle); display:flex; flex-wrap:wrap; gap:12px; padding:8px 12px; }
  .search-panel > input { background:var(--surface-control); border:1px solid var(--border-strong); border-radius:var(--radius-sm); color:var(--text-primary); min-height:30px; padding:0 9px; width:min(330px,35vw); }
  .search-panel label, .settings-panel label, .search-panel span { align-items:center; color:var(--text-secondary); display:flex; font:500 10px/1 var(--font-ui); gap:5px; }
  .settings-panel input[type='range'] { width:90px; }
  .reader-body { display:flex; min-height:0; }
  .toc-pane, .side-panel { background:var(--surface-pane); flex:0 0 220px; overflow:auto; padding:16px 10px; }
  .toc-pane { border-right:1px solid var(--border-subtle); }
  .side-panel { border-left:1px solid var(--border-subtle); flex-basis:260px; padding-inline:12px; }
  h2 { color:var(--text-primary); font:650 12px/1.3 var(--font-ui); margin:0 8px 12px; }
  nav { display:grid; gap:2px; }
  .toc-row { align-items:center; display:flex; padding-left:calc(var(--toc-depth) * 12px); }
  .toc-toggle { background:transparent; border:0; flex:0 0 22px; min-height:28px; padding:0; }
  .toc-spacer { flex:0 0 22px; }
  .toc-link { background:transparent; border-color:transparent; flex:1; min-width:0; overflow:hidden; text-align:left; text-overflow:ellipsis; white-space:nowrap; }
  .toc-link.active { background:var(--accent-soft); color:var(--accent-strong); }
  .toc-pane p, .side-panel p, dd, dt { color:var(--text-tertiary); font:400 11px/1.5 var(--font-ui); }
  .viewport-shell { background:var(--surface-canvas); flex:1; min-width:0; padding:12px; position:relative; }
  iframe { background:white; border:1px solid var(--border-subtle); border-radius:var(--radius-sm); height:100%; width:100%; }
  .progress-save { bottom:16px; position:absolute; right:20px; }
  .results-panel > button, .bookmarks-panel article > button { background:transparent; border:0; display:grid; gap:5px; height:auto; padding:8px; text-align:left; width:100%; }
  .results-panel strong, .bookmarks-panel strong { color:var(--text-primary); font:650 10px/1.3 var(--font-ui); }
  .results-panel span, .bookmarks-panel span { color:var(--text-tertiary); font:400 10px/1.45 var(--font-ui); white-space:normal; }
  mark { background:var(--warning-soft,#fff5d6); color:var(--text-primary); }
  .bookmarks-panel article { border-bottom:1px solid var(--border-subtle); padding-bottom:5px; }
  .bookmarks-panel article.invalid { opacity:.55; }
  .bookmarks-panel article > div { display:flex; gap:4px; justify-content:flex-end; }
  .bookmarks-panel article > div button { min-height:24px; padding-inline:7px; }
  .metadata-pane img { border-radius:var(--radius-sm); display:block; margin:0 auto 14px; max-height:210px; max-width:100%; }
  .external-link-backdrop { align-items:center; background:rgb(14 20 32 / .48); display:flex; inset:0; justify-content:center; padding:24px; position:fixed; z-index:100; }
  .external-link-dialog { background:var(--surface-pane); border:1px solid var(--border-strong); border-radius:var(--radius-md); box-shadow:0 18px 60px rgb(0 0 0 / .25); display:grid; gap:12px; max-width:560px; padding:20px; width:100%; }
  .external-link-dialog h2, .external-link-dialog p { margin:0; }
  .external-link-dialog p { color:var(--text-secondary); font:400 11px/1.5 var(--font-ui); }
  .external-link-dialog strong { color:var(--text-primary); font:650 13px/1.3 var(--font-ui); }
  .external-link-dialog code { background:var(--surface-control); border-radius:var(--radius-sm); color:var(--text-secondary); font:400 10px/1.5 ui-monospace,monospace; overflow-wrap:anywhere; padding:8px; }
  .external-link-dialog > div { display:flex; gap:8px; justify-content:flex-end; }
  dl { display:grid; gap:11px; margin:0; }
  dl div { display:grid; gap:3px; }
  dd { color:var(--text-secondary); margin:0; overflow-wrap:anywhere; }
  @media (max-width:950px) { .reader-actions button { padding-inline:7px; } .toc-pane { flex-basis:170px; } .side-panel { flex-basis:220px; } }
  @media (max-width:760px) { .toc-pane { display:none; } .reader-actions button:nth-child(n+4) { display:none; } }
</style>
