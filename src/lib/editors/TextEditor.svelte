<script lang="ts">
  import { onMount, tick } from 'svelte';

  import { buildTextReadingBlocks } from '../readers/text/textParagraphs';
  import {
    adjustTextWindowStartForViewport,
    MAX_RENDERED_TEXT_BLOCKS,
    TEXT_WINDOW_EDGE_BLOCKS,
    estimateTextBlockLayout,
    textBlockIndexForScrollOffset,
    textBlockIndexForSourceOffset,
    textWindowStartForIndex,
  } from '../readers/text/textVirtualization';
  import { defaultReadingTypographySettings } from '../stores/appSettings';
  import { readingFontStack, type ReadingTypographySettings } from '../types/settings';
  import type { EditorStatistics, TextEditorHandle } from '../types/document';
  import type { TextHeading } from './textHeadings';
  import { DEFAULT_TEXT_HEADING_PATTERN, findTextHeadings } from './textHeadings';
  import { loadTextEditorModules } from './textEditorModules';

  export let initialContent: string;
  export let headingPattern = DEFAULT_TEXT_HEADING_PATTERN;
  export let readingSettings: ReadingTypographySettings = defaultReadingTypographySettings;
  export let hasCustomBackground = false;
  export let onReady: (handle: TextEditorHandle) => void = () => {};
  export let onDirtyChange: (dirty: boolean) => void = () => {};
  export let onHeadingsChange: (headings: TextHeading[]) => void = () => {};
  export let onReadingPositionChange: (offset: number) => void = () => {};
  export let onStatisticsChange: (statistics: EditorStatistics) => void = () => {};

  let container: HTMLDivElement;
  let readingContainer: HTMLDivElement;
  let view: import('@codemirror/view').EditorView | null = null;
  let loading = true;
  let loadError = false;
  let headingTimer: ReturnType<typeof setTimeout> | null = null;
  let appliedHeadingPattern = headingPattern;
  let editing = false;
  let readerContent = initialContent;
  let readingOffset = 0;
  let scrollFrame = 0;
  let synchronizingReadingWindow = false;
  let visibleStart = 0;

  $: blocks = buildTextReadingBlocks(readerContent, readingSettings.txt, headingPattern);
  $: pageWidth = readingSettings.columns === 2
    ? readingSettings.contentWidth * 2 + 56
    : readingSettings.contentWidth;
  $: readerStyle = [
    `--reader-font:${readingFontStack(readingSettings.fontFamily)}`,
    `--reader-size:${readingSettings.fontSize}px`,
    `--reader-weight:${readingSettings.fontWeight}`,
    `--reader-tracking:${readingSettings.letterSpacing}em`,
    `--reader-indent:${readingSettings.firstLineIndent}em`,
    `--reader-line-height:${readingSettings.lineHeight}`,
    `--reader-paragraph-gap:${readingSettings.paragraphSpacing}em`,
    `--reader-align:${readingSettings.textAlign}`,
    `--reader-page-width:${pageWidth}px`,
    `--reader-horizontal-margin:${readingSettings.horizontalMargin}px`,
    `--reader-vertical-margin:${readingSettings.verticalMargin}px`,
    `--reader-columns:${readingSettings.columns}`,
  ].join(';');
  $: virtualLayout = estimateTextBlockLayout(
    blocks,
    readingSettings.fontSize,
    readingSettings.lineHeight,
    readingSettings.paragraphSpacing,
    readingSettings.contentWidth,
    readingSettings.columns,
  );
  $: maximumVisibleStart = Math.max(0, blocks.length - MAX_RENDERED_TEXT_BLOCKS);
  $: if (visibleStart > maximumVisibleStart) visibleStart = maximumVisibleStart;
  $: visibleEnd = Math.min(blocks.length, visibleStart + MAX_RENDERED_TEXT_BLOCKS);
  $: visibleBlocks = blocks.slice(visibleStart, visibleEnd).map((block, offset) => ({
    block,
    index: visibleStart + offset,
  }));
  $: virtualized = blocks.length > MAX_RENDERED_TEXT_BLOCKS;
  $: topSpacerHeight = virtualized ? (virtualLayout.offsets[visibleStart] ?? 0) : 0;
  $: bottomSpacerHeight = virtualized
    ? Math.max(0, virtualLayout.totalHeight - (virtualLayout.offsets[visibleEnd] ?? virtualLayout.totalHeight))
    : 0;

  $: if (view && headingPattern !== appliedHeadingPattern) {
    appliedHeadingPattern = headingPattern;
    onHeadingsChange(findTextHeadings(view.state.doc.toString(), headingPattern));
  }

  onMount(() => {
    let cancelled = false;
    void initialize().catch(() => {
      if (!cancelled) {
        loadError = true;
        loading = false;
      }
    });

    async function initialize(): Promise<void> {
      const { state, view: viewModule, commands, search } = await loadTextEditorModules();
      if (cancelled) return;

      let savedDocument: import('@codemirror/state').Text;
      const editMode = new state.Compartment();
      const editModeExtensions = (enabled: boolean) => [
        viewModule.EditorView.editable.of(enabled),
        state.EditorState.readOnly.of(!enabled),
      ];
      const editorTheme = viewModule.EditorView.theme({
        '&': {
          height: '100%',
          backgroundColor: 'transparent',
          color: 'var(--text-primary)',
          fontSize: '15px',
        },
        '.cm-content': {
          caretColor: 'var(--accent)',
          fontFamily: 'var(--font-content)',
          lineHeight: '1.72',
          padding: '32px clamp(28px, 6vw, 88px)',
        },
        '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--accent)' },
        '&.cm-focused': { outline: 'none' },
        '.cm-scroller': { overflow: 'auto' },
        '.cm-gutters': {
          backgroundColor: 'color-mix(in srgb, var(--surface-input) 88%, transparent)',
          border: 'none',
          color: 'var(--text-disabled)',
          paddingLeft: '8px',
        },
        '.cm-activeLine, .cm-activeLineGutter': { backgroundColor: 'var(--surface-subtle)' },
        '.cm-selectionBackground, &.cm-focused .cm-selectionBackground': {
          backgroundColor: 'var(--accent-focus)',
        },
        '.cm-panels': {
          backgroundColor: 'var(--surface-pane)',
          borderColor: 'var(--border-subtle)',
          color: 'var(--text-secondary)',
          fontFamily: 'var(--font-ui)',
        },
        '.cm-searchMatch': { backgroundColor: 'var(--warning)', color: 'var(--surface-canvas)' },
        '.cm-searchMatch.cm-searchMatch-selected': { outline: '2px solid var(--accent)' },
      });

      const editorState = state.EditorState.create({
        doc: initialContent,
        extensions: [
          viewModule.lineNumbers(),
          viewModule.highlightActiveLineGutter(),
          viewModule.highlightSpecialChars(),
          commands.history(),
          viewModule.drawSelection(),
          viewModule.dropCursor(),
          state.EditorState.allowMultipleSelections.of(true),
          viewModule.rectangularSelection(),
          viewModule.crosshairCursor(),
          search.search({ top: true }),
          viewModule.keymap.of([
            ...commands.defaultKeymap,
            ...commands.historyKeymap,
            ...search.searchKeymap,
            { key: 'Mod-h', run: search.openSearchPanel },
          ]),
          viewModule.EditorView.lineWrapping,
          editMode.of(editModeExtensions(false)),
          viewModule.EditorView.contentAttributes.of({
            'aria-label': 'TXT 文本编辑器',
            autocapitalize: 'off',
            autocomplete: 'off',
            spellcheck: 'false',
          }),
          editorTheme,
          viewModule.EditorView.updateListener.of((update) => {
            if (editing && (update.viewportChanged || update.selectionSet)) {
              onReadingPositionChange(update.view.viewport.from);
            }
            if (!update.docChanged) return;
            onDirtyChange(!update.state.doc.eq(savedDocument));
            onStatisticsChange({ lines: update.state.doc.lines, characters: update.state.doc.length });
            if (headingTimer) clearTimeout(headingTimer);
            headingTimer = setTimeout(() => {
              onHeadingsChange(findTextHeadings(update.state.doc.toString(), headingPattern));
              headingTimer = null;
            }, 180);
          }),
        ],
      });
      savedDocument = editorState.doc;
      view = new viewModule.EditorView({ state: editorState, parent: container });
      loading = false;
      onStatisticsChange({ lines: editorState.doc.lines, characters: editorState.doc.length });
      onHeadingsChange(findTextHeadings(editorState.doc.toString(), headingPattern));
      onReady({
        discardChanges: () => {
          if (!view) return;
          if (!view.state.doc.eq(savedDocument)) {
            view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: savedDocument } });
          }
          readerContent = savedDocument.toString();
          onDirtyChange(false);
        },
        focus: () => editing ? view?.focus() : readingContainer?.focus(),
        getContent: () => view?.state.doc.toString() ?? readerContent,
        getCursorOffset: () => editing ? (view?.state.selection.main.head ?? 0) : readingOffset,
        getReadingOffset: () => editing ? (view?.viewport.from ?? 0) : readingOffset,
        markSaved: () => {
          if (!view) return;
          savedDocument = view.state.doc;
          onDirtyChange(false);
        },
        revealOffset: (offset, focus = true) => {
          if (editing) revealEditorOffset(offset, focus, viewModule);
          else {
            revealEditorOffset(offset, false, viewModule);
            void revealReaderOffset(offset, focus);
          }
        },
        setEditing: (enabled) => {
          if (!view) return;
          const preservedOffset = editing ? view.viewport.from : readingOffset;
          editing = enabled;
          view.dispatch({ effects: editMode.reconfigure(editModeExtensions(enabled)) });
          if (enabled) {
            revealEditorOffset(preservedOffset, true, viewModule);
          } else {
            readerContent = view.state.doc.toString();
            void revealReaderOffset(preservedOffset, false);
          }
        },
      });
    }

    return () => {
      cancelled = true;
      if (headingTimer) clearTimeout(headingTimer);
      if (scrollFrame) cancelAnimationFrame(scrollFrame);
      view?.destroy();
      view = null;
    };
  });

  function revealEditorOffset(
    offset: number,
    focus: boolean,
    viewModule: typeof import('@codemirror/view'),
  ): void {
    if (!view) return;
    const position = Math.max(0, Math.min(offset, view.state.doc.length));
    view.dispatch({
      selection: { anchor: position },
      effects: viewModule.EditorView.scrollIntoView(position, { y: 'center' }),
    });
    if (focus) view.focus();
  }

  async function revealReaderOffset(offset: number, focus: boolean): Promise<void> {
    readingOffset = Math.max(0, Math.min(offset, readerContent.length));
    const index = textBlockIndexForSourceOffset(blocks, readingOffset);
    visibleStart = textWindowStartForIndex(blocks.length, index);
    await tick();
    if (readingContainer) {
      readingContainer.scrollTop = virtualLayout.offsets[index] ?? 0;
      const element = readingContainer.querySelector<HTMLElement>(`[data-block-index="${index}"]`);
      if (element) {
        readingContainer.scrollTop = Math.max(0, element.offsetTop - readingContainer.clientHeight / 3);
      }
    }
    if (focus) readingContainer?.focus();
  }

  function trackReadingPosition(): void {
    if (synchronizingReadingWindow) return;
    if (scrollFrame) cancelAnimationFrame(scrollFrame);
    scrollFrame = requestAnimationFrame(() => {
      scrollFrame = 0;
      if (!readingContainer) return;
      const preservedScrollTop = readingContainer.scrollTop;
      void synchronizeReadingWindow(preservedScrollTop);
    });
  }

  async function synchronizeReadingWindow(preservedScrollTop: number): Promise<void> {
    if (!readingContainer || synchronizingReadingWindow) return;
    synchronizingReadingWindow = true;
    try {
      const estimatedIndex = textBlockIndexForScrollOffset(
        virtualLayout.offsets,
        preservedScrollTop + 12,
      );
      if (virtualized
        && (estimatedIndex < visibleStart + TEXT_WINDOW_EDGE_BLOCKS
          || estimatedIndex >= visibleEnd - TEXT_WINDOW_EDGE_BLOCKS)) {
        visibleStart = textWindowStartForIndex(blocks.length, estimatedIndex);
      }

      for (let attempt = 0; attempt < 8; attempt += 1) {
        await tick();
        if (!readingContainer) return;
        readingContainer.scrollTop = preservedScrollTop;
        const rendered = Array.from(
          readingContainer.querySelectorAll<HTMLElement>('[data-block-index]'),
        );
        const first = rendered[0];
        const last = rendered.at(-1);
        if (!virtualized || !first || !last) break;
        const viewport = readingContainer.getBoundingClientRect();
        const estimatedWindowHeight = Math.max(
          1,
          (virtualLayout.offsets[visibleEnd] ?? virtualLayout.totalHeight)
            - (virtualLayout.offsets[visibleStart] ?? 0),
        );
        const nextStart = adjustTextWindowStartForViewport({
          currentStart: visibleStart,
          maximumStart: maximumVisibleStart,
          firstBlockTop: first.getBoundingClientRect().top - viewport.top,
          lastBlockBottom: last.getBoundingClientRect().bottom - viewport.top,
          viewportHeight: readingContainer.clientHeight,
          estimatedBlockHeight: estimatedWindowHeight / rendered.length,
        });
        if (nextStart === visibleStart) break;
        visibleStart = nextStart;
      }

      await tick();
      if (!readingContainer) return;
      readingContainer.scrollTop = preservedScrollTop;
      const viewport = readingContainer.getBoundingClientRect();
      const anchor = Array.from(
        readingContainer.querySelectorAll<HTMLElement>('[data-block-index]'),
      ).find((element) => {
        const rect = element.getBoundingClientRect();
        return rect.bottom >= viewport.top + 12 && rect.top < viewport.bottom;
      });
      const actualIndex = Number(anchor?.dataset.blockIndex);
      const index = Number.isInteger(actualIndex) && actualIndex >= 0
        ? actualIndex
        : estimatedIndex;
      const block = blocks[index];
      if (!block) return;
      readingOffset = block.sourceStart;
      onReadingPositionChange(readingOffset);
    } finally {
      synchronizingReadingWindow = false;
    }
  }
</script>

<div class:has-background={hasCustomBackground} class:editing class="editor-host" style={readerStyle}>
  <div
    aria-label="TXT 阅读正文"
    bind:this={readingContainer}
    class:hidden={editing}
    class="text-reading-surface"
    onscroll={trackReadingPosition}
    role="region"
    tabindex="-1"
  >
    <article class:double={readingSettings.columns === 2} class:virtualized class={`chapter-style-${readingSettings.txt.chapterTitleStyle}`}>
      {#if topSpacerHeight > 0}<div aria-hidden="true" class="virtual-spacer" style={`height:${topSpacerHeight}px`}></div>{/if}
      {#each visibleBlocks as item}
        {#if item.block.kind === 'heading'}
          <h2 aria-current={item.block.sourceStart <= readingOffset && item.block.sourceEnd >= readingOffset ? 'location' : undefined} data-block-index={item.index} data-source-start={item.block.sourceStart}>{item.block.text}</h2>
        {:else if item.block.kind === 'blank'}
          <div aria-hidden="true" class="source-blank" data-block-index={item.index} data-source-start={item.block.sourceStart}></div>
        {:else}
          <p aria-current={item.block.sourceStart <= readingOffset && item.block.sourceEnd >= readingOffset ? 'location' : undefined} data-block-index={item.index} data-source-start={item.block.sourceStart}>{item.block.text}</p>
        {/if}
      {/each}
      {#if bottomSpacerHeight > 0}<div aria-hidden="true" class="virtual-spacer" style={`height:${bottomSpacerHeight}px`}></div>{/if}
    </article>
  </div>
  <div bind:this={container} class:hidden={!editing} class="codemirror-host">
    {#if loading}<div class="editor-message">正在按需加载文本编辑器…</div>{:else if loadError}<div class="editor-message error" role="alert">文本编辑器加载失败，请重新打开文档。</div>{/if}
  </div>
</div>

<style>
  .editor-host { background:var(--surface-input); height:100%; min-height:0; min-width:0; position:relative; }
  .editor-host.has-background { background:color-mix(in srgb,var(--surface-input) 80%,transparent); }
  .text-reading-surface, .codemirror-host { height:100%; min-height:0; overflow:auto; position:relative; }
  .hidden { display:none; }
  .text-reading-surface { background:transparent; color:var(--text-primary); outline:none; }
  article { box-sizing:border-box; column-count:var(--reader-columns); column-gap:56px; font-family:var(--reader-font); font-size:var(--reader-size); font-weight:var(--reader-weight); letter-spacing:var(--reader-tracking); line-height:var(--reader-line-height); margin:0 auto; max-width:calc(var(--reader-page-width) + var(--reader-horizontal-margin) * 2); min-height:100%; padding:var(--reader-vertical-margin) var(--reader-horizontal-margin); text-align:var(--reader-align); width:100%; }
  article h2 { break-after:avoid; break-inside:avoid; column-span:all; font-family:var(--reader-font); letter-spacing:0; line-height:1.35; text-align:center; text-indent:0; }
  article.chapter-style-prominent h2 { font-size:1.55em; font-weight:700; margin:1.1em 0 1.8em; }
  article.chapter-style-compact h2 { font-size:1.2em; font-weight:650; margin:.7em 0 1em; text-align:left; }
  article.chapter-style-plain h2 { font-size:1em; font-weight:var(--reader-weight); margin:0 0 var(--reader-paragraph-gap); text-align:var(--reader-align); }
  article p { break-inside:avoid; content-visibility:auto; margin:0 0 var(--reader-paragraph-gap); overflow-wrap:anywhere; text-indent:var(--reader-indent); }
  .virtual-spacer { break-inside:avoid; width:100%; }
  article.virtualized.double { column-count:1; display:grid; gap:var(--reader-paragraph-gap) 56px; grid-template-columns:repeat(2,minmax(0,1fr)); }
  article.virtualized.double .virtual-spacer, article.virtualized.double h2 { grid-column:1 / -1; }
  article.virtualized.double p { margin-bottom:0; }
  .source-blank { break-inside:avoid; height:calc(1em * var(--reader-line-height)); }
  .codemirror-host { background:color-mix(in srgb,var(--surface-input) 88%,transparent); }
  .codemirror-host :global(.cm-editor) { height:100%; }
  .editor-message { color:var(--text-tertiary); font:500 12px/1.5 var(--font-ui); left:50%; position:absolute; top:50%; transform:translate(-50%,-50%); }
  .editor-message.error { color:var(--danger); }
  @media (max-width:900px) { article.double { column-count:1; max-width:calc(min(780px,var(--reader-page-width)) + var(--reader-horizontal-margin) * 2); } }
</style>
