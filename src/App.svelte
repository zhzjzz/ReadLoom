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
    chooseEpubFile,
    closeEpubDocument,
    listRecentDocuments,
    openEpubDocument,
  } from './lib/services/epubDocumentService';
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
  import type { OpenedEpubDocumentDto, RecentDocumentDto } from './lib/types/epub';
  import type { AppErrorDto, BackendConnection } from './lib/types/ipc';
  import type { WorkspaceTab, WorkspaceTabSummary } from './lib/types/workspace';

  type PendingAction = 'close' | 'exit-edit' | 'open' | 'open-epub' | 'exit' | 'reopen';

  const desktopRuntime = hasTauriRuntime();
  let connection: BackendConnection = { status: 'checking' };
  let initialContent = '';
  let editorHandle: TextEditorHandle | null = null;
  let statistics: EditorStatistics = { lines: 0, characters: 0 };
  let pendingAction: PendingAction | null = null;
  let encodingRecoveryPath: string | null = null;
  let editing = false;
  let epubDocument: OpenedEpubDocumentDto | null = null;
  let epubError: AppErrorDto | null = null;
  let epubSpineIndex = 0;
  let EpubReaderComponent: typeof import('./lib/readers/epub/EpubReader.svelte').default | null = null;
  let epubReaderHandle: { flushProgress(): Promise<void> } | null = null;
  let tabs: WorkspaceTab[] = [];
  let activeTabId: string | null = null;
  let editorInstanceKey = 0;
  let recentDocuments: RecentDocumentDto[] = [];

  $: activeDocument = $documentStore.active;
  $: saving = $documentStore.saveStatus === 'saving';
  $: dirty = isDirty(activeDocument);
  $: workspaceError = epubError ?? $documentStore.error;
  $: tabSummaries = summarizeTabs(tabs, activeTabId, activeDocument, epubSpineIndex);

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
          const dirtyDocumentId = firstDirtyTabId();
          if (dirtyDocumentId || saving) {
            event.preventDefault();
            if (dirtyDocumentId && dirtyDocumentId !== activeTabId) {
              void activateTab(dirtyDocumentId).then(() => (pendingAction = 'exit'));
            } else {
              pendingAction = 'exit';
            }
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
      const probe = await probeBackend('阅织阶段 3 EPUB 通信正常');
      connection = { status: 'connected', startup, probe };
      await refreshRecentDocuments();
    } catch (error) {
      connection = { status: 'error', error: normalizeAppError(error) };
    }
  }

  async function refreshRecentDocuments(): Promise<void> {
    if (!desktopRuntime) return;
    try {
      recentDocuments = await listRecentDocuments(12);
    } catch {
      // 最近文件是辅助状态，失败不能阻止打开和阅读。
    }
  }

  function openRecent(document: RecentDocumentDto): void {
    if (document.documentKind === 'epub') void openEpubPath(document.path);
    else void openPath(document.path, null, false);
  }

  function summarizeTabs(
    workspaceTabs: WorkspaceTab[],
    currentTabId: string | null,
    currentTextDocument: typeof activeDocument,
    currentEpubSpineIndex: number,
  ): WorkspaceTabSummary[] {
    return workspaceTabs.map((tab) => {
      if (tab.kind === 'txt') {
        const session = tab.documentId === currentTabId && currentTextDocument
          ? currentTextDocument
          : tab.session;
        return {
          id: tab.documentId,
          kind: tab.kind,
          title: session.fileName,
          path: session.displayPath,
          detail: null,
          dirty: isDirty(session),
        };
      }
      const index = tab.documentId === currentTabId ? currentEpubSpineIndex : tab.spineIndex;
      return {
        id: tab.documentId,
        kind: tab.kind,
        title: tab.document.document.metadata.title,
        path: tab.document.displayPath,
        detail: `${index + 1}/${tab.document.document.spine.length}`,
        dirty: false,
      };
    });
  }

  function firstDirtyTabId(): string | null {
    for (const tab of tabs) {
      if (tab.kind !== 'txt') continue;
      const session = tab.documentId === activeTabId && activeDocument
        ? activeDocument
        : tab.session;
      if (isDirty(session)) return tab.documentId;
    }
    return isDirty(activeDocument) ? activeDocument?.documentId ?? null : null;
  }

  function snapshotActiveTab(): void {
    if (!activeTabId) return;
    tabs = tabs.map((tab) => {
      if (tab.documentId !== activeTabId) return tab;
      if (tab.kind === 'txt' && activeDocument) {
        return {
          ...tab,
          session: { ...activeDocument },
          content: editorHandle?.getContent() ?? tab.content,
        };
      }
      if (tab.kind === 'epub' && epubDocument) {
        return { ...tab, document: epubDocument, spineIndex: epubSpineIndex };
      }
      return tab;
    });
  }

  async function activateTab(documentId: string): Promise<void> {
    if (documentId === activeTabId) return;
    await epubReaderHandle?.flushProgress().catch(() => {});
    snapshotActiveTab();
    const target = tabs.find((tab) => tab.documentId === documentId);
    if (!target) return;

    activeTabId = target.documentId;
    epubError = null;
    editing = false;
    editorHandle = null;
    statistics = { lines: 0, characters: 0 };
    if (target.kind === 'txt') {
      epubDocument = null;
      epubReaderHandle = null;
      initialContent = target.content;
      editorInstanceKey += 1;
      documentStore.restore({ ...target.session });
      return;
    }

    EpubReaderComponent ??= (await import('./lib/readers/epub/EpubReader.svelte')).default;
    initialContent = '';
    documentStore.close();
    epubDocument = target.document;
    epubSpineIndex = target.spineIndex;
  }

  async function requestCloseTab(documentId: string): Promise<void> {
    if (documentId !== activeTabId) await activateTab(documentId);
    requestClose();
  }

  function updateActiveEpubLocator(locator: OpenedEpubDocumentDto['initialLocator']): void {
    if (!epubDocument || !locator) return;
    epubDocument = { ...epubDocument, initialLocator: locator };
    tabs = tabs.map((tab) =>
      tab.kind === 'epub' && tab.documentId === epubDocument?.documentId
        ? { ...tab, document: epubDocument }
        : tab,
    );
  }

  function updateActiveEpubBookmarks(bookmarks: OpenedEpubDocumentDto['bookmarks']): void {
    if (!epubDocument) return;
    epubDocument = { ...epubDocument, bookmarks };
    tabs = tabs.map((tab) =>
      tab.kind === 'epub' && tab.documentId === epubDocument?.documentId
        ? { ...tab, document: epubDocument }
        : tab,
    );
  }

  function changeEpubSpine(index: number): void {
    epubSpineIndex = index;
    tabs = tabs.map((tab) =>
      tab.kind === 'epub' && tab.documentId === activeTabId
        ? { ...tab, spineIndex: index }
        : tab,
    );
  }

  function requestOpenEpub(): void {
    if (!desktopRuntime || saving) return;
    void performOpenEpub();
  }

  async function performOpenEpub(): Promise<void> {
    try {
      const path = await chooseEpubFile();
      if (!path) return;
      await openEpubPath(path);
    } catch (error) {
      epubError = normalizeAppError(error);
    }
  }

  async function openEpubPath(path: string): Promise<void> {
    try {
      const opened = await openEpubDocument(path);
      if (tabs.some((tab) => tab.documentId === opened.documentId)) {
        await activateTab(opened.documentId);
        return;
      }
      await epubReaderHandle?.flushProgress().catch(() => {});
      snapshotActiveTab();

      EpubReaderComponent ??= (await import('./lib/readers/epub/EpubReader.svelte')).default;
      editing = false;
      editorHandle = null;
      initialContent = '';
      statistics = { lines: 0, characters: 0 };
      encodingRecoveryPath = null;
      epubError = null;
      epubSpineIndex = opened.initialLocator?.spineIndex ?? 0;
      epubDocument = opened;
      activeTabId = opened.documentId;
      tabs = [
        ...tabs,
        {
          kind: 'epub',
          documentId: opened.documentId,
          document: opened,
          spineIndex: epubSpineIndex,
        },
      ];
      documentStore.close();
      await refreshRecentDocuments();
    } catch (error) {
      epubError = normalizeAppError(error);
    }
  }

  function requestOpen(): void {
    if (!desktopRuntime || saving) return;
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
    try {
      const opened = await openTextDocument(path, encoding, allowLarge);
      if (tabs.some((tab) => tab.documentId === opened.documentId)) {
        await activateTab(opened.documentId);
        return;
      }
      encodingRecoveryPath = null;
      epubError = null;
      await epubReaderHandle?.flushProgress().catch(() => {});
      snapshotActiveTab();
      epubDocument = null;
      epubReaderHandle = null;
      adoptOpenedDocument(opened);
      activeTabId = opened.documentId;
      const openedSession = $documentStore.active;
      if (openedSession) {
        tabs = [
          ...tabs,
          {
            kind: 'txt',
            documentId: opened.documentId,
            session: { ...openedSession },
            content: opened.content,
          },
        ];
      }
      await refreshRecentDocuments();
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
    editorInstanceKey += 1;
    documentStore.open(opened);
    const restored = $documentStore.active;
    if (restored) {
      tabs = tabs.map((tab) =>
        tab.kind === 'txt' && tab.documentId === opened.documentId
          ? { ...tab, session: { ...restored }, content: opened.content }
          : tab,
      );
    }
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
    if ((!activeDocument && !epubDocument) || saving) return;
    if (dirty) {
      pendingAction = 'close';
      return;
    }
    void closeCurrentDocument();
  }

  async function closeCurrentDocument(): Promise<void> {
    if (epubDocument) {
      const closing = epubDocument;
      const closedIndex = tabs.findIndex((tab) => tab.documentId === closing.documentId);
      try {
        await epubReaderHandle?.flushProgress().catch(() => {});
        await closeEpubDocument(closing.documentId);
        epubDocument = null;
        epubReaderHandle = null;
        epubSpineIndex = 0;
        epubError = null;
        tabs = tabs.filter((tab) => tab.documentId !== closing.documentId);
        activeTabId = null;
        await activateNeighborAfterClose(closedIndex);
      } catch (error) {
        epubError = normalizeAppError(error);
      }
      return;
    }
    const documentId = activeDocument?.documentId;
    if (!documentId) return;
    const closedIndex = tabs.findIndex((tab) => tab.documentId === documentId);
    try {
      await closeTextDocument(documentId);
      editing = false;
      editorHandle = null;
      initialContent = '';
      statistics = { lines: 0, characters: 0 };
      documentStore.close();
      tabs = tabs.filter((tab) => tab.documentId !== documentId);
      activeTabId = null;
      await activateNeighborAfterClose(closedIndex);
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    }
  }

  async function activateNeighborAfterClose(closedIndex: number): Promise<void> {
    const next = tabs[Math.max(0, closedIndex)] ?? tabs.at(-1);
    if (next) await activateTab(next.documentId);
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
    else if (action === 'open-epub') await performOpenEpub();
  }

  async function continueAction(action: PendingAction): Promise<void> {
    if (action === 'exit-edit') finishEditing();
    else if (action === 'close') await closeCurrentDocument();
    else if (action === 'open') await performOpen();
    else if (action === 'open-epub') await performOpenEpub();
    else if (action === 'reopen') await performReopen();
    else await continueExit();
  }

  async function continueExit(): Promise<void> {
    snapshotActiveTab();
    const dirtyDocumentId = firstDirtyTabId();
    if (dirtyDocumentId) {
      if (dirtyDocumentId !== activeTabId) await activateTab(dirtyDocumentId);
      pendingAction = 'exit';
      return;
    }
    await getCurrentWindow().destroy();
  }

  function changeTheme(preference: ThemePreference): void {
    setTheme(preference);
  }

  function dismissWorkspaceError(): void {
    epubError = null;
    documentStore.clearError();
  }
</script>

<div class="app-shell">
  <TopBar
    activeTabId={activeTabId}
    {connection}
    document={activeDocument}
    displayTitle={epubDocument?.document.metadata.title ?? null}
    displayPath={epubDocument?.displayPath ?? null}
    hasDocument={Boolean(epubDocument)}
    onClose={requestClose}
    onCloseTab={(documentId) => void requestCloseTab(documentId)}
    onSelectTab={(documentId) => void activateTab(documentId)}
    tabs={tabSummaries}
  />
  <div class="workspace-grid">
    <NavigationPane
      {desktopRuntime}
      {recentDocuments}
      onOpen={requestOpen}
      onOpenEpub={requestOpenEpub}
      onOpenRecent={openRecent}
    />

    <main class="document-workspace">
      {#if !epubDocument}
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
      {/if}

      {#if workspaceError}
        <div class="error-banner" role="alert">
          <div>
            <strong>{workspaceError.message}</strong>
            {#if workspaceError.suggestedAction}<span>{workspaceError.suggestedAction}</span>{/if}
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
          <button aria-label="关闭错误提示" class="dismiss" onclick={dismissWorkspaceError} type="button">×</button>
        </div>
      {/if}

      <div class="editor-stage">
        {#if epubDocument && EpubReaderComponent}
          <svelte:component
            bind:this={epubReaderHandle}
            this={EpubReaderComponent}
            document={epubDocument}
            onBookmarksChange={updateActiveEpubBookmarks}
            onError={(error: AppErrorDto) => (epubError = error)}
            onLocatorChange={updateActiveEpubLocator}
            spineIndex={epubSpineIndex}
            onSpineChange={changeEpubSpine}
          />
        {:else if activeDocument}
          {#key `${activeDocument.documentId}-${editorInstanceKey}`}
            <TextEditor
              {initialContent}
              onDirtyChange={(value) => documentStore.markContentDirty(value)}
              onReady={editorReady}
              onStatisticsChange={(value) => (statistics = value)}
            />
          {/key}
        {:else}
          <section class="empty-state">
            <div class="empty-mark">R</div>
            <h1>打开一本书，开始阅读或编织</h1>
            <p>TXT 支持安全编辑与保存；EPUB 2/3 以隔离、只读模式打开。</p>
            <div class="empty-actions">
              <button disabled={!desktopRuntime} onclick={requestOpen} type="button">
                {desktopRuntime ? '选择 TXT 文件' : '请在 Readloom 桌面版中打开'}
              </button>
              <button disabled={!desktopRuntime} onclick={requestOpenEpub} type="button">选择 EPUB 文件</button>
            </div>
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
  <EditorStatusBar
    document={activeDocument}
    epubStatus={epubDocument ? `EPUB ${epubDocument.document.version} · 只读` : null}
    {saving}
    {statistics}
  />
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
    min-height: 36px;
    padding: 0 18px;
  }

  .empty-actions {
    display: flex;
    gap: 8px;
    margin-top: 23px;
  }

  .empty-actions button + button {
    background: var(--surface-control);
    border-color: var(--border-strong);
    color: var(--text-primary);
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
