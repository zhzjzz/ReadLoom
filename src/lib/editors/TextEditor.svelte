<script lang="ts">
  import { onMount } from 'svelte';

  import type { EditorStatistics, TextEditorHandle } from '../types/document';
  import { loadTextEditorModules } from './textEditorModules';

  export let initialContent: string;
  export let onReady: (handle: TextEditorHandle) => void = () => {};
  export let onDirtyChange: (dirty: boolean) => void = () => {};
  export let onStatisticsChange: (statistics: EditorStatistics) => void = () => {};

  let container: HTMLDivElement;
  let view: import('@codemirror/view').EditorView | null = null;
  let loading = true;
  let loadError = false;

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
      const editModeExtensions = (editing: boolean) => [
        viewModule.EditorView.editable.of(editing),
        state.EditorState.readOnly.of(!editing),
      ];
      const editorTheme = viewModule.EditorView.theme({
        '&': {
          height: '100%',
          backgroundColor: 'var(--surface-input)',
          color: 'var(--text-primary)',
          fontSize: '15px',
        },
        '.cm-content': {
          caretColor: 'var(--accent)',
          fontFamily: 'var(--font-content)',
          lineHeight: '1.72',
          padding: '30px clamp(28px, 6vw, 88px) 96px',
        },
        '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--accent)' },
        '&.cm-focused': { outline: 'none' },
        '.cm-scroller': { overflow: 'auto' },
        '.cm-gutters': {
          backgroundColor: 'var(--surface-input)',
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
            if (!update.docChanged) return;
            onDirtyChange(!update.state.doc.eq(savedDocument));
            onStatisticsChange({
              lines: update.state.doc.lines,
              characters: update.state.doc.length,
            });
          }),
        ],
      });
      savedDocument = editorState.doc;
      view = new viewModule.EditorView({ state: editorState, parent: container });
      loading = false;
      onStatisticsChange({ lines: editorState.doc.lines, characters: editorState.doc.length });
      onReady({
        discardChanges: () => {
          if (!view) return;
          if (!view.state.doc.eq(savedDocument)) {
            view.dispatch({
              changes: { from: 0, to: view.state.doc.length, insert: savedDocument },
            });
          }
          onDirtyChange(false);
        },
        focus: () => view?.focus(),
        getContent: () => view?.state.doc.toString() ?? '',
        markSaved: () => {
          if (!view) return;
          savedDocument = view.state.doc;
          onDirtyChange(false);
        },
        setEditing: (editing) => {
          if (!view) return;
          view.dispatch({ effects: editMode.reconfigure(editModeExtensions(editing)) });
          if (editing) view.focus();
        },
      });
    }

    return () => {
      cancelled = true;
      view?.destroy();
      view = null;
    };
  });
</script>

<div class="editor-host" class:loading aria-busy={loading} bind:this={container}>
  {#if loading}
    <div class="editor-message">正在按需加载文本编辑器…</div>
  {:else if loadError}
    <div class="editor-message error" role="alert">文本编辑器加载失败，请重新打开文档。</div>
  {/if}
</div>

<style>
  .editor-host {
    background: var(--surface-input);
    height: 100%;
    min-height: 0;
    min-width: 0;
    position: relative;
  }

  .editor-host :global(.cm-editor) {
    height: 100%;
  }

  .editor-message {
    color: var(--text-tertiary);
    font: 500 12px/1.5 var(--font-ui);
    left: 50%;
    position: absolute;
    top: 50%;
    transform: translate(-50%, -50%);
  }

  .editor-message.error {
    color: var(--danger);
  }
</style>
