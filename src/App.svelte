<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount } from 'svelte';

  import EditorStatusBar from './lib/components/EditorStatusBar.svelte';
  import EditorToolbar from './lib/components/EditorToolbar.svelte';
  import InspectorPane from './lib/components/InspectorPane.svelte';
  import NavigationPane from './lib/components/NavigationPane.svelte';
  import TopBar from './lib/components/TopBar.svelte';
  import UnsavedChangesDialog from './lib/components/UnsavedChangesDialog.svelte';
  import TextEditor from './lib/editors/TextEditor.svelte';
  import {
    hasTauriRuntime,
    normalizeAppError,
    probeBackend,
    reportFrontendReady,
  } from './lib/services/backend';
  import { createShortcutHandler } from './lib/services/shortcuts';
  import {
    chooseSavePath,
    chooseTextFile,
    closeTextDocument,
    openTextDocument,
    reopenTextDocument,
    saveTextDocument,
    saveTextDocumentAs,
  } from './lib/services/textDocumentService';
  import { documentStore } from './lib/stores/documentStore';
  import {
    initializeTheme,
    setTheme,
    themePreference,
    type ThemePreference,
  } from './lib/stores/theme';
  import type {
    EditorStatistics,
    OpenedTextDocumentDto,
    SaveOptions,
    TextEditorHandle,
    TextEncoding,
  } from './lib/types/document';
  import { isDirty } from './lib/types/document';
  import type { BackendConnection } from './lib/types/ipc';

  type PendingAction = 'close' | 'exit-edit' | 'open' | 'exit' | 'reopen';

  const desktopRuntime = hasTauriRuntime();
  let connection: BackendConnection = { status: 'checking' };
  let initialContent = '';
  let editorHandle: TextEditorHandle | null = null;
  let statistics: EditorStatistics = { lines: 0, characters: 0 };
  let pendingAction: PendingAction | null = null;
  let encodingRecoveryPath: string | null = null;
  let editing = false;

  $: activeDocument = $documentStore.active;
  $: saving = $documentStore.saveStatus === 'saving';
  $: dirty = isDirty(activeDocument);

  onMount(() => {
    documentStore.reset();
    const disposeTheme = initializeTheme();
    void connectBackend();

    const shortcutHandler = createShortcutHandler({
      open: () => requestOpen(),
      save: () => editing && void performSave(false),
      saveAs: () => editing && void performSave(true),
      close: () => requestClose(),
    });
    window.addEventListener('keydown', shortcutHandler, { capture: true });

    let unlistenClose: (() => void) | null = null;
    let disposed = false;
    if (desktopRuntime) {
      void getCurrentWindow()
        .onCloseRequested((event) => {
          if (dirty || saving) {
            event.preventDefault();
            pendingAction = 'exit';
          }
        })
        .then((unlisten) => {
          if (disposed) unlisten();
          else unlistenClose = unlisten;
        });
    }

    return () => {
      disposed = true;
      disposeTheme();
      unlistenClose?.();
      window.removeEventListener('keydown', shortcutHandler, { capture: true });
    };
  });

  async function connectBackend(): Promise<void> {
    if (!desktopRuntime) {
      connection = { status: 'browser-preview' };
      return;
    }
    connection = { status: 'checking' };
    try {
      const startup = await reportFrontendReady();
      const probe = await probeBackend('阅织阶段 1 TXT 通信正常');
      connection = { status: 'connected', startup, probe };
    } catch (error) {
      connection = { status: 'error', error: normalizeAppError(error) };
    }
  }

  function requestOpen(): void {
    if (!desktopRuntime || saving) return;
    if (dirty) {
      pendingAction = 'open';
      return;
    }
    void performOpen();
  }

  async function performOpen(): Promise<void> {
    try {
      const path = await chooseTextFile();
      if (!path) return;
      await openPath(path, null, false);
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    }
  }

  async function openPath(
    path: string,
    encoding: TextEncoding | null,
    allowLarge: boolean,
  ): Promise<void> {
    const previousId = activeDocument?.documentId ?? null;
    try {
      const opened = await openTextDocument(path, encoding, allowLarge);
      encodingRecoveryPath = null;
      adoptOpenedDocument(opened);
      if (previousId && previousId !== opened.documentId) {
        await closeTextDocument(previousId).catch(() => {});
      }
    } catch (error) {
      const appError = normalizeAppError(error);
      if (appError.code === 'LARGE_FILE_CONFIRMATION_REQUIRED' && !allowLarge) {
        const confirmed = window.confirm(`${appError.message}\n\n${appError.suggestedAction ?? ''}`);
        if (confirmed) await openPath(path, encoding, true);
        return;
      }
      if (appError.code === 'LOW_ENCODING_CONFIDENCE') encodingRecoveryPath = path;
      documentStore.failed(appError);
    }
  }

  function adoptOpenedDocument(opened: OpenedTextDocumentDto): void {
    editing = false;
    editorHandle = null;
    statistics = { lines: 0, characters: 0 };
    initialContent = opened.content;
    documentStore.open(opened);
  }

  function editorReady(handle: TextEditorHandle): void {
    editorHandle = handle;
    editorHandle.setEditing(editing);
    initialContent = '';
  }

  function toggleEditing(): void {
    if (!activeDocument || saving) return;
    if (!editing) {
      editing = true;
      editorHandle?.setEditing(true);
      return;
    }
    if (dirty) {
      pendingAction = 'exit-edit';
      return;
    }
    finishEditing();
  }

  function finishEditing(): void {
    editing = false;
    editorHandle?.setEditing(false);
  }

  function discardEditingChanges(): void {
    const document = activeDocument;
    editorHandle?.discardChanges();
    if (document) {
      documentStore.updateSaveOptions({
        encoding: document.savedEncoding,
        hasBom: document.savedHasBom,
        lineEnding: 'preserve',
      });
    }
    documentStore.clearError();
    finishEditing();
  }

  async function performSave(saveAs: boolean): Promise<boolean> {
    const document = activeDocument;
    const editor = editorHandle;
    if (!document || !editor || saving) return false;
    if (document.lineEnding === 'mixed' && document.lineEndingChoice === 'preserve') {
      documentStore.failed({
        code: 'LINE_ENDING_SELECTION_REQUIRED',
        message: '原文件包含混合换行符，保存前需要选择统一格式。',
        recoverable: true,
        suggestedAction: '请在工具栏选择 CRLF 或 LF。',
      });
      return false;
    }

    documentStore.saving();
    try {
      const request = {
        documentId: document.documentId,
        content: editor.getContent(),
        encoding: document.encoding,
        hasBom: document.hasBom,
        lineEnding: document.lineEndingChoice,
        expectedRevision: document.revision,
      };
      const saved = saveAs
        ? await saveAsWithDialog(document.displayPath, request)
        : await saveTextDocument(request);
      if (!saved) {
        documentStore.idle();
        return false;
      }
      editor.markSaved();
      documentStore.saved(saved);
      return true;
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
      return false;
    }
  }

  async function saveAsWithDialog(
    defaultPath: string,
    request: Parameters<typeof saveTextDocument>[0],
  ) {
    const targetPath = await chooseSavePath(defaultPath);
    if (!targetPath) return null;
    return saveTextDocumentAs({ ...request, targetPath, allowOverwrite: true });
  }

  function updateSaveOptions(options: SaveOptions): void {
    documentStore.updateSaveOptions(options);
  }

  function requestReopen(): void {
    if (!activeDocument || saving) return;
    if (activeDocument.contentDirty) {
      pendingAction = 'reopen';
      return;
    }
    void performReopen();
  }

  async function performReopen(): Promise<void> {
    const document = activeDocument;
    if (!document) return;
    try {
      const reopened = await reopenTextDocument(document.documentId, document.encoding);
      adoptOpenedDocument(reopened);
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    }
  }

  function requestClose(): void {
    if (!activeDocument || saving) return;
    if (dirty) {
      pendingAction = 'close';
      return;
    }
    void closeCurrentDocument();
  }

  async function closeCurrentDocument(): Promise<void> {
    const documentId = activeDocument?.documentId;
    if (!documentId) return;
    try {
      await closeTextDocument(documentId);
      editing = false;
      editorHandle = null;
      initialContent = '';
      statistics = { lines: 0, characters: 0 };
      documentStore.close();
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    }
  }

  async function saveAndContinue(): Promise<void> {
    const action = pendingAction;
    if (!action) return;
    if (await performSave(Boolean(activeDocument?.readOnly))) {
      pendingAction = null;
      await continueAction(action);
    }
  }

  async function discardAndContinue(): Promise<void> {
    const action = pendingAction;
    if (!action) return;
    pendingAction = null;
    if (action === 'exit-edit') {
      discardEditingChanges();
      return;
    }
    if (action === 'reopen') {
      await performReopen();
      return;
    }
    if (action === 'exit') {
      await getCurrentWindow().destroy();
      return;
    }
    await closeCurrentDocument();
    if (action === 'open') await performOpen();
  }

  async function continueAction(action: PendingAction): Promise<void> {
    if (action === 'exit-edit') finishEditing();
    else if (action === 'close') await closeCurrentDocument();
    else if (action === 'open') await performOpen();
    else if (action === 'reopen') await performReopen();
    else await getCurrentWindow().destroy();
  }

  function changeTheme(preference: ThemePreference): void {
    setTheme(preference);
  }
</script>

<div class="app-shell">
  <TopBar {connection} document={activeDocument} onClose={requestClose} />
  <div class="workspace-grid">
    <NavigationPane {desktopRuntime} onOpen={requestOpen} />

    <main class="document-workspace">
      <EditorToolbar
        {desktopRuntime}
        document={activeDocument}
        {editing}
        {saving}
        onClose={requestClose}
        onOpen={requestOpen}
        onOptionsChange={updateSaveOptions}
        onReopen={requestReopen}
        onSave={() => void performSave(false)}
        onSaveAs={() => void performSave(true)}
        onToggleEditing={toggleEditing}
      />

      {#if $documentStore.error}
        <div class="error-banner" role="alert">
          <div>
            <strong>{$documentStore.error.message}</strong>
            {#if $documentStore.error.suggestedAction}<span>{$documentStore.error.suggestedAction}</span>{/if}
          </div>
          {#if encodingRecoveryPath}
            <div class="encoding-actions" aria-label="手动选择文件编码">
              {#each ['utf8', 'utf16Le', 'utf16Be', 'gbk', 'gb18030'] as encoding}
                <button onclick={() => void openPath(encodingRecoveryPath!, encoding as TextEncoding, false)} type="button">
                  {encoding}
                </button>
              {/each}
            </div>
          {/if}
          <button aria-label="关闭错误提示" class="dismiss" onclick={() => documentStore.clearError()} type="button">×</button>
        </div>
      {/if}

      <div class="editor-stage">
        {#if activeDocument}
          {#key activeDocument.documentId}
            <TextEditor
              {initialContent}
              onDirtyChange={(value) => documentStore.markContentDirty(value)}
              onReady={editorReady}
              onStatisticsChange={(value) => (statistics = value)}
            />
          {/key}
        {:else}
          <section class="empty-state">
            <div class="empty-mark">TXT</div>
            <h1>打开一篇文字，开始编织</h1>
            <p>支持 UTF-8、UTF-16、GBK / GB18030。编辑器只在打开文档后按需加载。</p>
            <button disabled={!desktopRuntime} onclick={requestOpen} type="button">
              {desktopRuntime ? '选择 TXT 文件' : '请在 Readloom 桌面版中打开'}
            </button>
            <span>Ctrl+O 打开 · Ctrl+S 保存 · Ctrl+Shift+S 另存为</span>
          </section>
        {/if}
      </div>
    </main>

    <InspectorPane
      {connection}
      document={activeDocument}
      onRetry={connectBackend}
      onThemeChange={changeTheme}
      theme={$themePreference}
    />
  </div>
  <EditorStatusBar document={activeDocument} {saving} {statistics} />
</div>

{#if pendingAction && activeDocument}
  <UnsavedChangesDialog
    fileName={activeDocument.fileName}
    {saving}
    onCancel={() => (pendingAction = null)}
    onDiscard={() => void discardAndContinue()}
    onSave={() => void saveAndContinue()}
  />
{/if}

<style>
  .app-shell {
    background: var(--surface-canvas);
    display: grid;
    grid-template-rows: var(--topbar-height) minmax(0, 1fr) var(--statusbar-height);
    height: 100%;
    min-height: 0;
    min-width: 0;
  }

  .workspace-grid {
    display: grid;
    grid-template-columns: var(--left-pane-width) minmax(0, 1fr) var(--right-pane-width);
    min-height: 0;
    min-width: 0;
  }

  .document-workspace {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
  }

  .editor-stage {
    flex: 1;
    min-height: 0;
    min-width: 0;
  }

  .empty-state {
    align-items: center;
    background:
      linear-gradient(var(--border-subtle) 1px, transparent 1px),
      linear-gradient(90deg, var(--border-subtle) 1px, transparent 1px),
      var(--surface-canvas);
    background-size: 32px 32px;
    display: flex;
    flex-direction: column;
    height: 100%;
    justify-content: center;
    padding: 40px;
    text-align: center;
  }

  .empty-mark {
    align-items: center;
    background: var(--surface-pane);
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    color: var(--accent-strong);
    display: flex;
    font: 700 13px/1 var(--font-mono);
    height: 58px;
    justify-content: center;
    letter-spacing: 0.08em;
    width: 58px;
  }

  h1 {
    color: var(--text-primary);
    font: 650 22px/1.25 var(--font-ui);
    margin: 20px 0 8px;
  }

  .empty-state p {
    color: var(--text-tertiary);
    font: 400 12px/1.6 var(--font-ui);
    margin: 0;
    max-width: 460px;
  }

  .empty-state button {
    background: var(--accent);
    border: 1px solid var(--accent);
    border-radius: var(--radius-sm);
    color: white;
    font: 650 12px/1 var(--font-ui);
    margin-top: 23px;
    min-height: 36px;
    padding: 0 18px;
  }

  .empty-state button:disabled {
    background: var(--surface-control);
    border-color: var(--border-strong);
    color: var(--text-disabled);
    cursor: default;
  }

  .empty-state span {
    color: var(--text-disabled);
    font: 500 10px/1 var(--font-ui);
    margin-top: 13px;
  }

  .error-banner {
    align-items: center;
    background: var(--danger-soft);
    border-bottom: 1px solid var(--danger);
    color: var(--text-secondary);
    display: flex;
    gap: 12px;
    padding: 9px 12px;
  }

  .error-banner > div:first-child {
    display: grid;
    flex: 1;
    gap: 3px;
  }

  .error-banner strong {
    font: 650 11px/1.35 var(--font-ui);
  }

  .error-banner span {
    color: var(--text-tertiary);
    font: 400 10px/1.35 var(--font-ui);
  }

  .encoding-actions {
    display: flex;
    gap: 4px;
  }

  .encoding-actions button,
  .dismiss {
    background: var(--surface-control);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    font: 600 10px/1 var(--font-ui);
    min-height: 26px;
  }

  .dismiss {
    border: 0;
    font-size: 18px;
    width: 28px;
  }

  @media (max-width: 980px) {
    .workspace-grid {
      grid-template-columns: var(--left-pane-width) minmax(0, 1fr);
    }

    .workspace-grid :global(.inspector-pane) {
      display: none;
    }
  }
</style>
