<script lang="ts">
  import { onDestroy, onMount } from 'svelte';

  import { normalizeAppError } from '../../services/backend';
  import {
    chooseEpubChapterImagePath,
    importEpubChapterImage,
    revertEpubChapterDraft,
    updateEpubChapterDraft,
  } from '../../services/epubDocumentService';
  import type {
    ChapterDraftAccepted,
    ChapterEditDto,
    ImportedChapterImage,
  } from '../../types/epub';
  import type { AppErrorDto } from '../../types/ipc';
  import {
    pruneChapterEditorStates,
    readChapterEditorState,
    rememberChapterEditorState,
    type CachedChapterEditorState,
  } from './chapterEditorStateCache';
  import { ChapterDraftSync, type ChapterSyncStatus } from './chapterDraftSync';
  import { loadEpubEditorModules } from './epubEditorModules';
  import { sanitizeEpubPastedHtml, UnsafePastedContentError } from './epubPasteSanitizer';

  export let chapter: ChapterEditDto;
  export let readingSessionId: string;
  export let saving = false;
  export let onAccepted: (accepted: ChapterDraftAccepted) => void = () => {};
  export let onChapterChange: (index: number) => void = () => {};
  export let onError: (error: AppErrorDto) => void = () => {};
  export let onLocalDirty: (dirty: boolean) => void = () => {};
  export let onReverted: (chapter: ChapterEditDto) => void = () => {};
  export let spineLength: number;

  let editorHost: HTMLDivElement;
  let editor: any = null;
  let editorModules: Awaited<ReturnType<typeof loadEpubEditorModules>> | null = null;
  let activeChapterId = '';
  let syncStatus: ChapterSyncStatus = 'idle';
  let loading = true;
  let loadError: AppErrorDto | null = null;
  let localDirty = chapter.dirty;
  let previewRevision = chapter.previewRevision;
  let layout: 'edit' | 'split' | 'preview' = 'edit';
  let toolbarState = emptyToolbarState();
  let suppressUpdate = false;
  let stateCache = new Map<string, CachedChapterEditorState<any>>();

  const sync = new ChapterDraftSync({
    debounceMs: 550,
    submit: updateEpubChapterDraft,
    onStatus: (status) => (syncStatus = status),
    onAccepted: (accepted) => {
      previewRevision = accepted.previewRevision;
      localDirty = accepted.dirty;
      onLocalDirty(accepted.dirty);
      onAccepted(accepted);
    },
    onError: (error) => onError(normalizeAppError(error)),
  });

  $: previewUrl = chapterPreviewUrl(readingSessionId, chapter.chapterHref, previewRevision);
  $: if (editor && chapter.chapterEditId !== activeChapterId) openChapter(chapter);

  onMount(() => {
    let disposed = false;
    void (async () => {
      try {
        editorModules = await loadEpubEditorModules();
        if (disposed || !editorModules) return;
        createEditor(editorModules);
        openChapter(chapter);
        loading = false;
      } catch (error) {
        loadError = normalizeAppError(error);
        loading = false;
        onError(loadError);
      }
    })();
    return () => {
      disposed = true;
    };
  });

  onDestroy(() => {
    if (editor && activeChapterId) rememberState(activeChapterId);
    sync.destroy();
    editor?.destroy();
    editor = null;
  });

  export async function flushDraft(): Promise<void> {
    await sync.flush();
  }

  export function focusEditor(): void {
    editor?.commands.focus();
  }

  function createEditor(modules: NonNullable<typeof editorModules>): void {
    const XhtmlAttributes = modules.Extension.create({
      name: 'readloomXhtmlAttributes',
      addGlobalAttributes() {
        return [{
          types: ['paragraph', 'heading', 'blockquote'],
          attributes: {
            id: passthroughAttribute('id'),
            class: passthroughAttribute('class'),
            lang: passthroughAttribute('lang'),
            xmlLang: passthroughAttribute('xml:lang'),
            dir: passthroughAttribute('dir'),
            epubType: passthroughAttribute('epub:type'),
            title: passthroughAttribute('title'),
          },
        }];
      },
    });
    const SafeImage = modules.Image.extend({
      addAttributes() {
        return {
          ...this.parent?.(),
          width: passthroughAttribute('width'),
          height: passthroughAttribute('height'),
          id: passthroughAttribute('id'),
          class: passthroughAttribute('class'),
        };
      },
    });
    const PublisherSpan = modules.Mark.create({
      name: 'publisherSpan',
      addAttributes() {
        return {
          id: passthroughAttribute('id'),
          class: passthroughAttribute('class'),
          role: passthroughAttribute('role'),
          lang: passthroughAttribute('lang'),
          xmlLang: passthroughAttribute('xml:lang'),
          dir: passthroughAttribute('dir'),
          epubType: passthroughAttribute('epub:type'),
          title: passthroughAttribute('title'),
        };
      },
      parseHTML() {
        return [{ tag: 'span' }];
      },
      renderHTML({ HTMLAttributes }: { HTMLAttributes: Record<string, string> }) {
        return ['span', HTMLAttributes, 0];
      },
    });
    editor = new modules.Editor({
      element: editorHost,
      editable: chapter.capabilities.canEdit,
      content: chapter.editorDocument,
      extensions: [
        modules.Document,
        modules.Text,
        modules.Paragraph,
        modules.Heading.configure({ levels: [1, 2, 3, 4, 5, 6] }),
        modules.HardBreak,
        modules.Bold,
        modules.Italic,
        modules.Strike,
        modules.Underline,
        modules.Blockquote,
        modules.BulletList,
        modules.OrderedList,
        modules.ListItem,
        modules.ListKeymap,
        modules.HorizontalRule,
        modules.Link.configure({
          openOnClick: false,
          autolink: false,
          linkOnPaste: true,
          protocols: ['http', 'https'],
          HTMLAttributes: { rel: null, target: null },
        }),
        SafeImage.configure({ inline: true, allowBase64: false }),
        modules.Subscript,
        modules.Superscript,
        PublisherSpan,
        modules.TextAlign.configure({
          types: ['heading', 'paragraph'],
          alignments: ['left', 'center', 'right', 'justify'],
        }),
        XhtmlAttributes,
        modules.UndoRedo.configure({ depth: 100, newGroupDelay: 500 }),
      ],
      editorProps: {
        attributes: {
          'aria-label': `编辑章节：${chapter.chapterTitle}`,
          class: 'readloom-epub-prosemirror',
          spellcheck: 'true',
        },
        transformPastedHTML: (source: string) => {
          try {
            return sanitizeEpubPastedHtml(source);
          } catch (error) {
            onError(normalizeAppError(error instanceof UnsafePastedContentError
              ? { code: 'UNSAFE_PASTED_CONTENT', message: error.message, recoverable: true }
              : error));
            return '';
          }
        },
        handleClick: (_view: unknown, _position: number, event: MouseEvent) => {
          if ((event.target as Element | null)?.closest('a[href]')) {
            event.preventDefault();
            return true;
          }
          return false;
        },
        handleDOMEvents: {
          compositionstart: () => {
            sync.compositionStart();
            return false;
          },
          compositionend: () => {
            sync.compositionEnd();
            return false;
          },
        },
      },
      onUpdate: ({ editor: current }: { editor: any }) => {
        if (suppressUpdate || !chapter.capabilities.canEdit) return;
        localDirty = true;
        onLocalDirty(true);
        sync.update(current.getJSON() as Record<string, unknown>);
        refreshToolbarState(current);
      },
      onSelectionUpdate: ({ editor: current }: { editor: any }) => refreshToolbarState(current),
      onTransaction: ({ editor: current }: { editor: any }) => refreshToolbarState(current),
    });
  }

  function passthroughAttribute(htmlName: string) {
    return {
      default: null,
      parseHTML: (element: HTMLElement) => element.getAttribute(htmlName),
      renderHTML: (attributes: Record<string, unknown>) => {
        const key = htmlName === 'xml:lang' ? 'xmlLang' : htmlName === 'epub:type' ? 'epubType' : htmlName;
        const value = attributes[key];
        return typeof value === 'string' && value ? { [htmlName]: value } : {};
      },
    };
  }

  function openChapter(next: ChapterEditDto): void {
    if (!editor) return;
    if (activeChapterId) rememberState(activeChapterId);
    suppressUpdate = true;
    const cached = readChapterEditorState(stateCache, next.chapterEditId);
    if (cached && cached.schema === editor.schema) {
      editor.view.updateState(cached);
    } else {
      editor.commands.setContent(next.editorDocument, { emitUpdate: false });
    }
    editor.setEditable(next.capabilities.canEdit);
    editor.view.dom.setAttribute('aria-label', `编辑章节：${next.chapterTitle}`);
    suppressUpdate = false;
    activeChapterId = next.chapterEditId;
    localDirty = next.dirty;
    previewRevision = next.previewRevision;
    sync.open(next);
    pruneStateCache();
    refreshToolbarState();
  }

  function rememberState(chapterEditId: string): void {
    if (!editor) return;
    rememberChapterEditorState(stateCache, chapterEditId, editor.state);
  }

  function pruneStateCache(): void {
    pruneChapterEditorStates(stateCache, activeChapterId);
  }

  function run(command: (chain: any) => any): void {
    if (!editor || !chapter.capabilities.canEdit || saving) return;
    command(editor.chain().focus()).run();
  }

  function setHeading(value: string): void {
    if (value === 'paragraph') run((chain) => chain.setParagraph());
    else run((chain) => chain.toggleHeading({ level: Number(value) }));
  }

  function editLink(): void {
    if (!editor || saving) return;
    const previous = editor.getAttributes('link').href ?? '';
    const href = window.prompt('链接地址（章节 fragment、EPUB 内部路径或 http/https）', previous);
    if (href === null) return;
    if (!href.trim()) {
      run((chain) => chain.extendMarkRange('link').unsetLink());
      return;
    }
    run((chain) => chain.extendMarkRange('link').setLink({ href: href.trim() }));
  }

  async function importImage(): Promise<void> {
    if (!editor || saving) return;
    try {
      const selected = await chooseEpubChapterImagePath();
      if (!selected) return;
      const imported = await importEpubChapterImage(
        chapter.editSessionId,
        chapter.chapterEditId,
        selected,
      );
      let alt = window.prompt('图片替代文本（装饰性图片可明确留空）', '');
      if (alt === null) return;
      if (!alt && !window.confirm('确认这是装饰性图片并使用空 alt？')) return;
      insertImportedImage(imported, alt);
    } catch (error) {
      onError(normalizeAppError(error));
    }
  }

  function insertImportedImage(imported: ImportedChapterImage, alt: string): void {
    run((chain) => chain.setImage({
      src: imported.editorSrc,
      alt,
      title: null,
      width: String(imported.width),
      height: String(imported.height),
    }));
  }

  function editImageAlt(): void {
    if (!editor || !editor.isActive('image')) return;
    const current = editor.getAttributes('image').alt ?? '';
    const alt = window.prompt('图片替代文本', current);
    if (alt === null) return;
    editor.chain().focus().updateAttributes('image', { alt }).run();
  }

  async function revertChapter(): Promise<void> {
    if (!chapter.capabilities.canRevert || saving) return;
    if (!window.confirm(`恢复“${chapter.chapterTitle}”到原始正文？其他章节修改不会受影响。`)) return;
    try {
      await sync.flush();
      const reverted = await revertEpubChapterDraft(chapter.chapterEditId);
      stateCache.delete(chapter.chapterEditId);
      activeChapterId = '';
      openChapter(reverted);
      localDirty = reverted.dirty;
      onLocalDirty(reverted.dirty);
      onReverted(reverted);
    } catch (error) {
      onError(normalizeAppError(error));
    }
  }

  function chapterPreviewUrl(sessionId: string, resourceId: string, revision: number): string {
    const path = resourceId.split('/').map(encodeURIComponent).join('/');
    return `http://readloom-epub.localhost/${sessionId}/${path}?draftRevision=${revision}`;
  }

  function statusLabel(status: ChapterSyncStatus): string {
    return {
      idle: '没有本地修改',
      typing: '正在输入（组合输入期间不发送）',
      waiting: '等待同步到 Rust 草稿',
      syncing: '正在同步到 Rust 草稿',
      synced: '已同步到草稿（尚未写入 EPUB 文件）',
      warning: '草稿已同步，存在校验警告',
      failed: '同步失败，编辑器内容仍保留',
      conflict: 'revision 冲突，较新内容仍保留',
    }[status];
  }

  function emptyToolbarState() {
    return {
      canUndo: false,
      canRedo: false,
      bold: false,
      italic: false,
      underline: false,
      strike: false,
      subscript: false,
      superscript: false,
      bulletList: false,
      orderedList: false,
      blockquote: false,
      link: false,
      image: false,
      textAlign: '',
    };
  }

  function refreshToolbarState(current = editor): void {
    if (!current) {
      toolbarState = emptyToolbarState();
      return;
    }
    toolbarState = {
      canUndo: current.can().undo(),
      canRedo: current.can().redo(),
      bold: current.isActive('bold'),
      italic: current.isActive('italic'),
      underline: current.isActive('underline'),
      strike: current.isActive('strike'),
      subscript: current.isActive('subscript'),
      superscript: current.isActive('superscript'),
      bulletList: current.isActive('bulletList'),
      orderedList: current.isActive('orderedList'),
      blockquote: current.isActive('blockquote'),
      link: current.isActive('link'),
      image: current.isActive('image'),
      textAlign: ['left', 'center', 'right', 'justify']
        .find((align) => current.isActive({ textAlign: align })) ?? '',
    };
  }
</script>

<section class="chapter-editor" aria-label="EPUB 章节编辑器">
  <header class="chapter-toolbar">
    <button aria-label="上一章" disabled={saving || chapter.spineIndex <= 0} onclick={() => onChapterChange(chapter.spineIndex - 1)} type="button">←</button>
    <div class="chapter-title">
      <strong>{chapter.chapterTitle}</strong>
      <span>{chapter.spineIndex + 1} / {spineLength}</span>
    </div>
    <button aria-label="下一章" disabled={saving || chapter.spineIndex >= spineLength - 1} onclick={() => onChapterChange(chapter.spineIndex + 1)} type="button">→</button>
    <span class:limited={chapter.compatibilityLevel === 'limited'} class="compatibility">{chapter.compatibilityLevel === 'full' ? '完整兼容' : chapter.compatibilityLevel === 'limited' ? '有限兼容' : chapter.compatibilityLevel === 'readOnly' ? '只读' : '不支持'}</span>
    <div class="layout-switch" aria-label="编辑预览布局">
      <button class:active={layout === 'edit'} onclick={() => (layout = 'edit')} type="button">仅编辑</button>
      <button class:active={layout === 'split'} disabled={!chapter.capabilities.canPreview} onclick={() => (layout = 'split')} type="button">编辑 + 预览</button>
      <button class:active={layout === 'preview'} disabled={!chapter.capabilities.canPreview} onclick={() => (layout = 'preview')} type="button">安全预览</button>
    </div>
  </header>

  {#if chapter.warnings.length}
    <div class="chapter-warning" role="status">
      {#each chapter.warnings as warning}<span>{warning.message}</span>{/each}
    </div>
  {/if}

  {#if chapter.capabilities.canEdit}
    <nav aria-label="章节格式工具栏" class="format-toolbar">
      <button aria-label="撤销" disabled={saving || !toolbarState.canUndo} onclick={() => run((chain) => chain.undo())} type="button">↶</button>
      <button aria-label="重做" disabled={saving || !toolbarState.canRedo} onclick={() => run((chain) => chain.redo())} type="button">↷</button>
      <select aria-label="段落或标题级别" disabled={saving} onchange={(event) => setHeading(event.currentTarget.value)}>
        <option value="paragraph">正文</option>
        {#each [1, 2, 3, 4, 5, 6] as level}<option value={level}>标题 {level}</option>{/each}
      </select>
      <button aria-pressed={toolbarState.bold} class:active={toolbarState.bold} onclick={() => run((chain) => chain.toggleBold())} type="button"><strong>B</strong></button>
      <button aria-pressed={toolbarState.italic} class:active={toolbarState.italic} onclick={() => run((chain) => chain.toggleItalic())} type="button"><em>I</em></button>
      <button aria-pressed={toolbarState.underline} class:active={toolbarState.underline} onclick={() => run((chain) => chain.toggleUnderline())} type="button"><u>U</u></button>
      <button aria-pressed={toolbarState.strike} class:active={toolbarState.strike} onclick={() => run((chain) => chain.toggleStrike())} type="button"><s>S</s></button>
      <button aria-pressed={toolbarState.subscript} class:active={toolbarState.subscript} onclick={() => run((chain) => chain.toggleSubscript())} type="button">X₂</button>
      <button aria-pressed={toolbarState.superscript} class:active={toolbarState.superscript} onclick={() => run((chain) => chain.toggleSuperscript())} type="button">X²</button>
      <button class:active={toolbarState.bulletList} onclick={() => run((chain) => chain.toggleBulletList())} type="button">• 列表</button>
      <button class:active={toolbarState.orderedList} onclick={() => run((chain) => chain.toggleOrderedList())} type="button">1. 列表</button>
      <button class:active={toolbarState.blockquote} onclick={() => run((chain) => chain.toggleBlockquote())} type="button">引用</button>
      <button onclick={() => run((chain) => chain.setHorizontalRule())} type="button">分隔线</button>
      <button class:active={toolbarState.link} onclick={editLink} type="button">链接</button>
      <button onclick={() => void importImage()} type="button">导入图片</button>
      <button disabled={!toolbarState.image} onclick={editImageAlt} type="button">图片 alt</button>
      <button disabled={!toolbarState.image} onclick={() => run((chain) => chain.deleteSelection())} type="button">删除图片引用</button>
      {#each ['left', 'center', 'right', 'justify'] as align}
        <button aria-label={`文本${align}对齐`} class:active={toolbarState.textAlign === align} onclick={() => run((chain) => chain.setTextAlign(align))} type="button">{align === 'left' ? '左' : align === 'center' ? '中' : align === 'right' ? '右' : '齐'}</button>
      {/each}
      <button class="revert" disabled={saving || !chapter.capabilities.canRevert && !localDirty} onclick={() => void revertChapter()} type="button">恢复本章</button>
    </nav>
  {/if}

  <div class:preview-only={layout === 'preview'} class:split={layout === 'split'} class="editor-body">
    {#if layout !== 'preview'}
      <div class="editor-pane">
        {#if loading}<div class="loading" role="status">正在按需加载章节编辑器…</div>{/if}
        {#if loadError}<div class="loading" role="alert">{loadError.message}</div>{/if}
        <div bind:this={editorHost} class:hidden={loading || Boolean(loadError)} class="editor-host"></div>
      </div>
    {/if}
    {#if layout !== 'edit'}
      <div class="preview-pane">
        <iframe
          allow="camera 'none'; microphone 'none'; geolocation 'none'; clipboard-read 'none'; clipboard-write 'none'"
          referrerpolicy="no-referrer"
          sandbox="allow-scripts"
          src={previewUrl}
          title={`Rust 已接受草稿预览：${chapter.chapterTitle}`}
        ></iframe>
      </div>
    {/if}
  </div>

  <footer class="sync-status" data-status={syncStatus}>
    <span>{statusLabel(syncStatus)}</span>
    <span>{localDirty ? '本章有修改' : '本章与保存基线一致'}</span>
    <strong>只有“另存为”才会生成新的 EPUB 文件</strong>
  </footer>
</section>

<style>
  .chapter-editor { display:grid; grid-template-rows:auto auto auto minmax(0,1fr) auto; height:100%; min-height:0; }
  button, select { background:var(--surface-control); border:1px solid var(--border-strong); border-radius:var(--radius-sm); color:var(--text-secondary); font:550 10px/1 var(--font-ui); min-height:28px; padding:0 8px; }
  button:hover:not(:disabled), button.active { background:var(--accent-soft); color:var(--accent-strong); }
  button:disabled { color:var(--text-disabled); }
  .chapter-toolbar, .format-toolbar, .sync-status { align-items:center; background:var(--surface-chrome); border-bottom:1px solid var(--border-subtle); display:flex; gap:5px; padding:6px 10px; }
  .chapter-title { display:grid; min-width:120px; }
  .chapter-title strong { color:var(--text-primary); font:650 12px/1.25 var(--font-ui); }
  .chapter-title span, .compatibility { color:var(--text-tertiary); font:500 9px/1.3 var(--font-ui); }
  .compatibility { background:var(--success-soft,#e8f6ee); border-radius:999px; color:var(--success,#26734d); padding:4px 8px; }
  .compatibility.limited { background:var(--warning-soft,#fff5d6); color:var(--warning,#8a5d00); }
  .layout-switch { display:flex; gap:3px; margin-left:auto; }
  .chapter-warning { background:var(--warning-soft,#fff5d6); border-bottom:1px solid var(--warning,#d29b24); color:var(--text-secondary); display:grid; font:500 10px/1.4 var(--font-ui); gap:2px; padding:7px 12px; }
  .format-toolbar { flex-wrap:wrap; max-height:76px; overflow:auto; }
  .format-toolbar .revert { margin-left:auto; }
  .editor-body { display:grid; grid-template-columns:minmax(0,1fr); min-height:0; }
  .editor-body.split { grid-template-columns:minmax(0,1fr) minmax(320px,.8fr); }
  .editor-body.preview-only { grid-template-columns:minmax(0,1fr); }
  .editor-pane, .preview-pane { background:var(--surface-canvas); min-height:0; overflow:auto; padding:12px; }
  .preview-pane { border-left:1px solid var(--border-subtle); }
  .preview-pane iframe { background:white; border:1px solid var(--border-subtle); border-radius:var(--radius-sm); height:100%; width:100%; }
  .editor-host { background:var(--surface-pane); border:1px solid var(--border-subtle); border-radius:var(--radius-sm); box-shadow:var(--shadow-sm); margin:0 auto; max-width:860px; min-height:100%; }
  .editor-host.hidden { display:none; }
  .editor-host :global(.readloom-epub-prosemirror) { color:var(--text-primary); font:400 17px/1.75 Georgia,'Noto Serif SC',serif; min-height:calc(100vh - 250px); outline:none; padding:44px 54px 120px; overflow-wrap:anywhere; }
  .editor-host :global(.readloom-epub-prosemirror p) { margin:0 0 1em; }
  .editor-host :global(.readloom-epub-prosemirror h1), .editor-host :global(.readloom-epub-prosemirror h2), .editor-host :global(.readloom-epub-prosemirror h3) { line-height:1.3; margin:1.4em 0 .7em; }
  .editor-host :global(.readloom-epub-prosemirror img) { height:auto; max-width:100%; }
  .editor-host :global(.readloom-epub-prosemirror blockquote) { border-left:3px solid var(--accent); color:var(--text-secondary); margin:1em 0; padding-left:1em; }
  .editor-host :global(.ProseMirror-selectednode) { outline:2px solid var(--accent); }
  .loading { color:var(--text-secondary); display:grid; font:500 11px/1.4 var(--font-ui); min-height:180px; place-items:center; }
  .sync-status { border-bottom:0; border-top:1px solid var(--border-subtle); color:var(--text-tertiary); font:500 9px/1.2 var(--font-ui); justify-content:space-between; }
  .sync-status strong { color:var(--text-secondary); font-weight:600; }
  .sync-status[data-status='failed'], .sync-status[data-status='conflict'] { color:var(--danger,#b33a3a); }
  @media (max-width:900px) { .editor-body.split { grid-template-columns:minmax(0,1fr); grid-template-rows:1fr 1fr; } .preview-pane { border-left:0; border-top:1px solid var(--border-subtle); } .format-toolbar button:nth-of-type(n+14) { display:none; } }
  @media (max-width:640px) { .layout-switch button:nth-child(2) { display:none; } .editor-host :global(.readloom-epub-prosemirror) { padding:28px 24px 96px; } }
</style>
