<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, tick } from 'svelte';

  import EditorStatusBar from './lib/components/EditorStatusBar.svelte';
  import EditorToolbar from './lib/components/EditorToolbar.svelte';
  import LibraryImportReviewDialog from './lib/components/LibraryImportReviewDialog.svelte';
  import LibraryView from './lib/components/LibraryView.svelte';
  import NavigationPane from './lib/components/NavigationPane.svelte';
  import SettingsView from './lib/components/SettingsView.svelte';
  import TextToolsPane from './lib/components/TextToolsPane.svelte';
  import TopBar from './lib/components/TopBar.svelte';
  import UnsavedChangesDialog from './lib/components/UnsavedChangesDialog.svelte';
  import TextEditor from './lib/editors/TextEditor.svelte';
  import { describeTextPosition } from './lib/readers/text/textSearch';
  import {
    DEFAULT_TEXT_HEADING_PATTERN,
    type TextHeading,
  } from './lib/editors/textHeadings';
  import { resizedPaneWidth, resizedPaneWidthFromKeyboard } from './lib/layout/workspaceLayout';
  import {
    applyWindowBehavior,
    backgroundImageUrl,
    clearBackgroundImage,
    getBackgroundImage,
    setBackgroundImage,
  } from './lib/services/appearanceService';
  import {
    hasTauriRuntime,
    normalizeAppError,
    probeBackend,
    reportFrontendReady,
  } from './lib/services/backend';
  import { createBooksBackup, restoreBooksBackup } from './lib/services/backupService';
  import { createShortcutHandler } from './lib/services/shortcuts';
  import {
    beginEpubEdit,
    beginEpubChapterEdit,
    cancelEpubSave,
    chooseEpubCoverPath,
    chooseEpubSavePath,
    closeEpubDocument,
    discardEpubDraft,
    epubResourceUrl,
    getEpubEditDraft,
    openEpubDocument,
    prepareEpubOverwriteConfirmation,
    removeEpubCoverChange,
    replaceEpubCover,
    saveEpubAs,
    updateEpubMetadata,
    validateEpubChapterDraft,
  } from './lib/services/epubDocumentService';
  import {
    assignLibraryGroup,
    createLibraryGroup,
    deleteLibraryGroup,
    importLibraryDocuments,
    listLibrary,
    removeLibraryDocument,
    removeUnavailableLibraryDocuments,
    renameLibraryGroup,
    previewLibraryDirectory,
    previewLibraryDocuments,
  } from './lib/services/libraryService';
  import {
    chooseSavePath,
    closeTextDocument,
    deleteTextBookmark,
    openTextDocument,
    reopenTextDocument,
    saveTextDocument,
    saveTextDocumentAs,
    saveTextProgress,
    saveTextBookmark,
  } from './lib/services/textDocumentService';
  import {
    chooseDocumentFile,
    chooseBackgroundImage,
    chooseBooksBackupFiles,
    chooseBooksBackupPath,
    chooseBooksRestoreDirectory,
    chooseLibraryDirectory,
    chooseLibraryFiles,
    classifyDocumentPath,
  } from './lib/services/workspaceFileService';
  import { documentStore } from './lib/stores/documentStore';
  import { loadAppSettings, normalizeAppSettings, persistAppSettings } from './lib/stores/appSettings';
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
    TextBookmark,
  } from './lib/types/document';
  import { isDirty } from './lib/types/document';
  import type {
    ChapterDraftAccepted,
    ChapterEditDto,
    EpubEditDraft,
    EpubMetadataPatch,
    OpenedEpubDocumentDto,
  } from './lib/types/epub';
  import type { AppErrorDto, BackendConnection } from './lib/types/ipc';
  import type {
    BooksBackupResultDto,
    BooksRestoreResultDto,
  } from './lib/types/backup';
  import type {
    LibraryDocumentDto,
    LibraryGroupDto,
    LibraryImportResultDto,
    LibraryImportPreviewDto,
  } from './lib/types/library';
  import type { AppSettings, BackgroundImageDto } from './lib/types/settings';
  import type { WorkspaceTab, WorkspaceTabSummary } from './lib/types/workspace';

  type PendingAction = 'close' | 'exit-edit' | 'open' | 'exit' | 'reopen';

  const desktopRuntime = hasTauriRuntime();
  const headingPatternStorageKey = 'readloom.txt-heading-pattern';
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
  let EpubEditPanelComponent: typeof import('./lib/readers/epub/EpubEditPanel.svelte').default | null = null;
  let EpubChapterEditorComponent: typeof import('./lib/readers/epub/EpubChapterEditor.svelte').default | null = null;
  let epubReaderHandle: { flushProgress(): Promise<void>; addBookmark(): Promise<void> } | null = null;
  let epubChapterEditorHandle: { flushDraft(): Promise<void>; focusEditor(): void } | null = null;
  let epubEditDraft: EpubEditDraft | null = null;
  let epubEditPanelOpen = false;
  let epubChapterEditMode = false;
  let epubChapterDraft: ChapterEditDto | null = null;
  let epubChapterLocalDirty = false;
  let epubSaving = false;
  let tabs: WorkspaceTab[] = [];
  let activeTabId: string | null = null;
  let editorInstanceKey = 0;
  let libraryDocuments: LibraryDocumentDto[] = [];
  let libraryGroups: LibraryGroupDto[] = [];
  let activeView: 'workspace' | 'library' | 'settings' = 'library';
  let libraryLoading = false;
  let libraryImportStatus: string | null = null;
  let libraryImportPreview: LibraryImportPreviewDto | null = null;
  let libraryImporting = false;
  let textHeadings: TextHeading[] = [];
  let textBookmarks: TextBookmark[] = [];
  let headingPattern = DEFAULT_TEXT_HEADING_PATTERN;
  let headingPatternDraft = DEFAULT_TEXT_HEADING_PATTERN;
  let headingPatternError: string | null = null;
  let appSettings: AppSettings = loadAppSettings();
  let backgroundImage: BackgroundImageDto | null = null;
  let backupPath: string | null = null;
  let backupResult: BooksBackupResultDto | null = null;
  let restoreResult: BooksRestoreResultDto | null = null;
  let backupBusy = false;
  let initialTextOffset = 0;
  let textProgressTimer: ReturnType<typeof setTimeout> | null = null;
  let dragActive = false;
  let leftPaneWidth = 220;
  let leftPaneCollapsed = false;
  let resizingPane = false;
  let stopPaneResize: (() => void) | null = null;

  $: activeDocument = $documentStore.active;
  $: saving = $documentStore.saveStatus === 'saving' || epubSaving;
  $: dirty = isDirty(activeDocument) || Boolean(epubEditDraft?.dirty) || epubChapterLocalDirty;
  $: workspaceError = epubError ?? $documentStore.error;
  $: tabSummaries = summarizeTabs(
    tabs,
    activeTabId,
    activeDocument,
    epubSpineIndex,
    epubEditDraft,
    epubChapterLocalDirty,
  );
  $: currentBackgroundUrl = backgroundImage ? backgroundImageUrl(backgroundImage.key) : null;

  onMount(() => {
    documentStore.reset();
    const disposeTheme = initializeTheme();
    restoreHeadingPattern();
    void connectBackend();

    const shortcutHandler = createShortcutHandler({
      open: () => requestOpen(),
      save: () => activeView === 'workspace' && (epubDocument ? void performEpubSaveAs() : editing && void performSave(false)),
      saveAs: () => activeView === 'workspace' && (epubDocument ? void performEpubSaveAs() : editing && void performSave(true)),
      close: () => activeView === 'workspace' && requestClose(),
      toggleEdit: () => activeView === 'workspace' && (epubDocument ? void toggleEpubChapterEditing() : toggleEditing()),
      previousChapter: () => activeView === 'workspace' && epubDocument && epubSpineIndex > 0 && void changeEpubSpine(epubSpineIndex - 1),
      nextChapter: () => activeView === 'workspace' && epubDocument && epubSpineIndex < epubDocument.document.spine.length - 1 && void changeEpubSpine(epubSpineIndex + 1),
      bookmark: () => {
        if (activeView !== 'workspace') return false;
        if (epubDocument && !epubChapterEditMode) {
          void epubReaderHandle?.addBookmark();
          return true;
        }
        if (activeDocument && editorHandle) {
          void addActiveTextBookmark();
          return true;
        }
        return false;
      },
      showLibrary: () => void showLibrary(),
      showSettings: () => void showSettings(),
    }, () => appSettings.shortcuts);
    window.addEventListener('keydown', shortcutHandler, { capture: true });

    let unlistenClose: (() => void) | null = null;
    let unlistenDragDrop: (() => void) | null = null;
    let unlistenTrayExit: (() => void) | null = null;
    let disposed = false;
    if (desktopRuntime) {
      const currentWindow = getCurrentWindow();
      void currentWindow
        .onCloseRequested((event) => {
          if (appSettings.closeAction === 'tray') {
            event.preventDefault();
            void currentWindow.hide();
            return;
          }
          const dirtyDocumentId = firstDirtyTabId();
          if (dirtyDocumentId || saving) {
            event.preventDefault();
            if (dirtyDocumentId && dirtyDocumentId !== activeTabId) {
              void activateTab(dirtyDocumentId).then(() => (pendingAction = 'exit'));
            } else {
              pendingAction = 'exit';
            }
          } else if (activeDocument || epubDocument) {
            event.preventDefault();
            void continueExit();
          }
        })
        .then((unlisten) => {
          if (disposed) unlisten();
          else unlistenClose = unlisten;
        });
      void currentWindow
        .onDragDropEvent((event) => {
          if (event.payload.type === 'enter' || event.payload.type === 'over') {
            dragActive = true;
            return;
          }
          dragActive = false;
          if (event.payload.type === 'drop') void openDroppedPaths(event.payload.paths);
        })
        .then((unlisten) => {
          if (disposed) unlisten();
          else unlistenDragDrop = unlisten;
        })
        .catch((error) => {
          documentStore.failed(normalizeAppError(error));
        });
      void listen('readloom-request-exit', () => {
        const dirtyDocumentId = firstDirtyTabId();
        if (dirtyDocumentId || saving) {
          if (dirtyDocumentId && dirtyDocumentId !== activeTabId) {
            void activateTab(dirtyDocumentId).then(() => (pendingAction = 'exit'));
          } else {
            pendingAction = 'exit';
          }
          return;
        }
        void continueExit();
      }).then((unlisten) => {
        if (disposed) unlisten();
        else unlistenTrayExit = unlisten;
      });
    }

    return () => {
      disposed = true;
      disposeTheme();
      unlistenClose?.();
      unlistenDragDrop?.();
      unlistenTrayExit?.();
      stopPaneResize?.();
      if (textProgressTimer) clearTimeout(textProgressTimer);
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
      const probe = await probeBackend('阅织 EPUB 通信正常');
      connection = { status: 'connected', startup, probe };
      backgroundImage = await getBackgroundImage();
      await applyWindowBehavior(appSettings);
      await refreshLibrary();
    } catch (error) {
      connection = { status: 'error', error: normalizeAppError(error) };
    }
  }

  async function refreshLibrary(): Promise<void> {
    if (!desktopRuntime) return;
    libraryLoading = true;
    try {
      const snapshot = await listLibrary(500);
      libraryDocuments = snapshot.documents;
      libraryGroups = snapshot.groups;
    } catch {
      // 书库状态失败不能阻止直接打开和阅读本地文件。
    } finally {
      libraryLoading = false;
    }
  }

  async function importSelectedLibraryFiles(): Promise<void> {
    if (!desktopRuntime || libraryLoading) return;
    try {
      const paths = await chooseLibraryFiles();
      if (!paths.length) return;
      libraryLoading = true;
      libraryImportStatus = `正在检查 ${paths.length} 个图书文件…`;
      libraryImportPreview = await previewLibraryDocuments(paths);
      libraryImportStatus = null;
    } catch (error) {
      const appError = normalizeAppError(error);
      libraryImportStatus = appError.message;
      documentStore.failed(appError);
    } finally {
      libraryLoading = false;
    }
  }

  async function importSelectedLibraryDirectory(): Promise<void> {
    if (!desktopRuntime || libraryLoading) return;
    try {
      const directory = await chooseLibraryDirectory();
      if (!directory) return;
      libraryLoading = true;
      libraryImportStatus = '正在扫描目录中的 EPUB / TXT…';
      libraryImportPreview = await previewLibraryDirectory(directory);
      libraryImportStatus = null;
    } catch (error) {
      const appError = normalizeAppError(error);
      libraryImportStatus = appError.message;
      documentStore.failed(appError);
    } finally {
      libraryLoading = false;
    }
  }

  async function confirmLibraryImport(paths: string[]): Promise<void> {
    if (!paths.length || libraryImporting) return;
    libraryImporting = true;
    try {
      libraryImportStatus = `正在解析并导入 ${paths.length} 本图书…`;
      const result = await importLibraryDocuments(paths);
      libraryImportStatus = describeLibraryImport(result);
      libraryImportPreview = null;
      await refreshLibrary();
    } catch (error) {
      const appError = normalizeAppError(error);
      libraryImportStatus = appError.message;
      documentStore.failed(appError);
    } finally {
      libraryImporting = false;
    }
  }

  function describeLibraryImport(result: LibraryImportResultDto): string {
    const details = [`已导入 ${result.imported} 本`];
    if (result.skipped) details.push(`跳过 ${result.skipped} 个重复选择`);
    if (result.failed.length) {
      details.push(`${result.failed.length} 个文件失败：${result.failed[0].message}`);
    }
    return details.join(' · ');
  }

  function openLibraryDocument(document: LibraryDocumentDto): void {
    if (!document.available) return;
    void openDocumentPath(document.path);
  }

  async function showLibrary(): Promise<void> {
    await showStaticView('library');
  }

  async function showSettings(): Promise<void> {
    await showStaticView('settings');
  }

  async function showStaticView(target: 'library' | 'settings'): Promise<void> {
    if (activeView === target) return;
    if (!(await flushActiveEpubChapterDraft())) return;
    await epubReaderHandle?.flushProgress().catch(() => {});
    await flushActiveTextProgress();
    snapshotActiveTab();
    if (activeDocument) {
      const activeTextTab = tabs.find((tab) => tab.kind === 'txt' && tab.documentId === activeTabId);
      if (activeTextTab?.kind === 'txt') initialContent = activeTextTab.content;
      editorHandle = null;
      textHeadings = [];
      editorInstanceKey += 1;
    }
    activeView = target;
  }

  function showWorkspace(): void {
    activeView = 'workspace';
  }

  function toggleSettingsView(): void {
    if (activeView === 'settings') {
      if (activeTabId) showWorkspace();
      else void showLibrary();
      return;
    }
    void showSettings();
  }

  async function selectWorkspaceTab(documentId: string): Promise<void> {
    if (documentId !== activeTabId) await activateTab(documentId);
    activeView = 'workspace';
  }

  function maximumPaneWidth(): number {
    return Math.max(180, Math.min(420, window.innerWidth - 480));
  }

  function beginPaneResize(event: PointerEvent): void {
    if (event.button !== 0 || leftPaneCollapsed) return;
    event.preventDefault();
    stopPaneResize?.();
    const startPointerX = event.clientX;
    const startWidth = leftPaneWidth;
    resizingPane = true;

    const move = (moveEvent: PointerEvent) => {
      leftPaneWidth = resizedPaneWidth(
        'left',
        startWidth,
        startPointerX,
        moveEvent.clientX,
        180,
        maximumPaneWidth(),
      );
    };
    const finish = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', finish);
      resizingPane = false;
      stopPaneResize = null;
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', finish);
    window.addEventListener('pointercancel', finish);
    stopPaneResize = finish;
  }

  function resizePaneFromKeyboard(event: KeyboardEvent): void {
    const requested = resizedPaneWidthFromKeyboard(
      'left',
      leftPaneWidth,
      event.key,
      180,
      maximumPaneWidth(),
    );
    if (requested === null) return;
    event.preventDefault();
    leftPaneWidth = requested;
  }

  async function removeLibraryBook(document: LibraryDocumentDto): Promise<void> {
    try {
      await removeLibraryDocument(document.path);
      libraryDocuments = libraryDocuments.filter((item) => item.path !== document.path);
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    }
  }

  async function cleanupUnavailableLibraryBooks(): Promise<void> {
    const count = libraryDocuments.filter((document) => !document.available).length;
    if (!count || !window.confirm(`从书库移除 ${count} 本已移动或删除的无效书籍？原文件不会被删除。`)) return;
    try {
      const removed = await removeUnavailableLibraryDocuments();
      libraryDocuments = libraryDocuments.filter((document) => document.available);
      libraryImportStatus = `已清理 ${removed} 本无效书籍`;
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    }
  }

  function changeAppSettings(patch: Partial<AppSettings>): void {
    appSettings = normalizeAppSettings({ ...appSettings, ...patch });
    persistAppSettings(appSettings);
    if (desktopRuntime) {
      void applyWindowBehavior(appSettings).catch((error) => {
        documentStore.failed(normalizeAppError(error));
      });
    }
  }

  async function chooseCustomBackground(): Promise<void> {
    if (!desktopRuntime) return;
    try {
      const path = await chooseBackgroundImage();
      if (!path) return;
      backgroundImage = await setBackgroundImage(path);
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    }
  }

  async function clearCustomBackground(): Promise<void> {
    if (!desktopRuntime || !backgroundImage) return;
    try {
      await clearBackgroundImage();
      backgroundImage = null;
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    }
  }

  async function selectBackupPath(): Promise<void> {
    if (!desktopRuntime || backupBusy) return;
    const selected = await chooseBooksBackupPath();
    if (selected) {
      backupPath = selected.toLocaleLowerCase().endsWith('.readloom-backup')
        ? selected
        : `${selected}.readloom-backup`;
      backupResult = null;
    }
  }

  async function backupAllBooks(): Promise<void> {
    if (!desktopRuntime || !backupPath || backupBusy) return;
    if (!window.confirm('备份只保留图书文件内容，不包含书签、阅读进度、分组、设置或阅读记录。继续吗？')) return;
    backupBusy = true;
    try {
      backupResult = await createBooksBackup(backupPath);
      restoreResult = null;
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    } finally {
      backupBusy = false;
    }
  }

  async function restoreBookBackups(): Promise<void> {
    if (!desktopRuntime || backupBusy) return;
    try {
      const backups = await chooseBooksBackupFiles();
      if (!backups.length) return;
      const directory = await chooseBooksRestoreDirectory();
      if (!directory) return;
      if (!window.confirm(`将从 ${backups.length} 个备份中恢复图书内容到：\n${directory}\n\n重复内容会自动跳过；书签、阅读进度和设置不会恢复。继续吗？`)) return;
      backupBusy = true;
      restoreResult = await restoreBooksBackup(backups, directory);
      backupResult = null;
      await refreshLibrary();
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    } finally {
      backupBusy = false;
    }
  }

  async function addLibraryGroup(name: string): Promise<void> {
    try {
      const group = await createLibraryGroup(name);
      libraryGroups = [...libraryGroups, group].sort((left, right) => left.position - right.position);
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    }
  }

  async function renameLibraryShelf(group: LibraryGroupDto, name: string): Promise<void> {
    try {
      await renameLibraryGroup(group.groupId, name);
      libraryGroups = libraryGroups.map((item) => item.groupId === group.groupId
        ? { ...item, name }
        : item);
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    }
  }

  async function deleteLibraryShelf(group: LibraryGroupDto): Promise<void> {
    try {
      await deleteLibraryGroup(group.groupId);
      libraryGroups = libraryGroups.filter((item) => item.groupId !== group.groupId);
      libraryDocuments = libraryDocuments.map((document) => document.groupId === group.groupId
        ? { ...document, groupId: null }
        : document);
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    }
  }

  async function moveLibraryBook(document: LibraryDocumentDto, groupId: string | null): Promise<void> {
    if (document.groupId === groupId) return;
    try {
      await assignLibraryGroup(document.path, groupId);
      libraryDocuments = libraryDocuments.map((item) => item.path === document.path
        ? { ...item, groupId }
        : item);
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    }
  }

  function summarizeTabs(
    workspaceTabs: WorkspaceTab[],
    currentTabId: string | null,
    currentTextDocument: typeof activeDocument,
    currentEpubSpineIndex: number,
    currentEpubDraft: EpubEditDraft | null,
    currentChapterLocalDirty: boolean,
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
        dirty: tab.documentId === currentTabId
          ? Boolean(currentEpubDraft?.dirty) || currentChapterLocalDirty
          : Boolean(tab.editDraft?.dirty),
      };
    });
  }

  function firstDirtyTabId(): string | null {
    for (const tab of tabs) {
      if (tab.kind === 'txt') {
        const session = tab.documentId === activeTabId && activeDocument
          ? activeDocument
          : tab.session;
        if (isDirty(session)) return tab.documentId;
      } else {
        const draft = tab.documentId === activeTabId ? epubEditDraft : tab.editDraft;
        if (draft?.dirty || (tab.documentId === activeTabId && epubChapterLocalDirty)) return tab.documentId;
      }
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
          bookmarks: [...textBookmarks],
          readingOffset: editorHandle?.getReadingOffset() ?? tab.readingOffset,
        };
      }
      if (tab.kind === 'epub' && epubDocument) {
        return {
          ...tab,
          document: epubDocument,
          spineIndex: epubSpineIndex,
          editDraft: epubEditDraft,
          editPanelOpen: epubEditPanelOpen,
          chapterEditMode: epubChapterEditMode,
          activeChapterDraft: epubChapterDraft,
          saving: epubSaving,
        };
      }
      return tab;
    });
  }

  async function activateTab(documentId: string): Promise<void> {
    if (documentId === activeTabId) return;
    if (!(await flushActiveEpubChapterDraft())) return;
    await epubReaderHandle?.flushProgress().catch(() => {});
    await flushActiveTextProgress();
    snapshotActiveTab();
    const target = tabs.find((tab) => tab.documentId === documentId);
    if (!target) return;

    activeTabId = target.documentId;
    epubError = null;
    editing = false;
    editorHandle = null;
    textHeadings = [];
    textBookmarks = [];
    statistics = { lines: 0, characters: 0 };
    if (target.kind === 'txt') {
      epubDocument = null;
      epubReaderHandle = null;
      epubEditDraft = null;
      epubEditPanelOpen = false;
      epubChapterEditMode = false;
      epubChapterDraft = null;
      epubChapterEditorHandle = null;
      epubChapterLocalDirty = false;
      epubSaving = false;
      initialContent = target.content;
      initialTextOffset = target.readingOffset;
      textBookmarks = [...target.bookmarks];
      editorInstanceKey += 1;
      documentStore.restore({ ...target.session });
      return;
    }

    EpubReaderComponent ??= (await import('./lib/readers/epub/EpubReader.svelte')).default;
    initialContent = '';
    documentStore.close();
    epubDocument = target.document;
    epubSpineIndex = target.spineIndex;
    epubEditDraft = target.editDraft;
    epubEditPanelOpen = target.editPanelOpen;
    epubChapterEditMode = target.chapterEditMode;
    epubChapterDraft = target.activeChapterDraft;
    epubChapterLocalDirty = false;
    epubSaving = target.saving;
    if (epubEditPanelOpen) {
      EpubEditPanelComponent ??= (await import('./lib/readers/epub/EpubEditPanel.svelte')).default;
    }
    if (epubChapterEditMode) {
      EpubChapterEditorComponent ??= (await import('./lib/readers/epub/EpubChapterEditor.svelte')).default;
    }
  }

  async function flushActiveEpubChapterDraft(): Promise<boolean> {
    if (!epubChapterEditMode || !epubChapterEditorHandle) return true;
    try {
      await epubChapterEditorHandle.flushDraft();
      epubChapterLocalDirty = false;
      return true;
    } catch (error) {
      epubError = normalizeAppError(error);
      return false;
    }
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

  async function changeEpubSpine(index: number): Promise<void> {
    if (!epubDocument || index < 0 || index >= epubDocument.document.spine.length) return;
    if (epubChapterEditMode && !(await flushActiveEpubChapterDraft())) return;
    if (epubChapterEditMode) {
      try {
        const editDraft = epubEditDraft ?? await beginEpubEdit(epubDocument.documentId);
        updateActiveEpubDraft(editDraft);
        const nextChapter = await beginEpubChapterEdit(editDraft.editSessionId, index);
        if (!nextChapter.capabilities.canEdit) {
          epubChapterEditMode = false;
          epubChapterEditorHandle = null;
          epubError = {
            code: nextChapter.compatibilityLevel === 'unsupported' ? 'CHAPTER_EDITING_NOT_SUPPORTED' : 'CHAPTER_READ_ONLY',
            message: nextChapter.warnings[0]?.message ?? '此章节无法安全进行可视化编辑。',
            recoverable: true,
            suggestedAction: '已切换到安全阅读模式，原章节草稿仍然保留。',
          };
        }
        epubChapterDraft = nextChapter;
      } catch (error) {
        epubError = normalizeAppError(error);
        return;
      }
    }
    epubSpineIndex = index;
    tabs = tabs.map((tab) =>
      tab.kind === 'epub' && tab.documentId === activeTabId
        ? { ...tab, spineIndex: index }
        : tab,
    );
  }

  async function toggleEpubChapterEditing(): Promise<void> {
    if (!epubDocument || saving || !epubDocument.document.capabilities.canEditText) return;
    if (epubChapterEditMode) {
      if (!(await flushActiveEpubChapterDraft())) return;
      epubChapterEditMode = false;
      epubChapterEditorHandle = null;
      snapshotActiveTab();
      return;
    }
    try {
      const editDraft = epubEditDraft ?? await beginEpubEdit(epubDocument.documentId);
      updateActiveEpubDraft(editDraft);
      const chapterDraft = await beginEpubChapterEdit(editDraft.editSessionId, epubSpineIndex);
      epubChapterDraft = chapterDraft;
      if (!chapterDraft.capabilities.canEdit) {
        epubError = {
          code: chapterDraft.compatibilityLevel === 'unsupported' ? 'CHAPTER_EDITING_NOT_SUPPORTED' : 'CHAPTER_READ_ONLY',
          message: chapterDraft.warnings[0]?.message ?? '此章节无法安全进行可视化编辑。',
          recoverable: true,
          suggestedAction: '本章保持安全阅读模式；不会静默删除不支持结构。',
        };
        return;
      }
      EpubChapterEditorComponent ??= (await import('./lib/readers/epub/EpubChapterEditor.svelte')).default;
      epubChapterEditMode = true;
      epubChapterLocalDirty = chapterDraft.dirty;
      epubEditPanelOpen = false;
      epubError = null;
      snapshotActiveTab();
      await tick();
      epubChapterEditorHandle?.focusEditor();
    } catch (error) {
      epubError = normalizeAppError(error);
    }
  }

  function acceptEpubChapterDraft(accepted: ChapterDraftAccepted): void {
    if (!epubChapterDraft || accepted.chapterEditId !== epubChapterDraft.chapterEditId) return;
    epubChapterDraft = {
      ...epubChapterDraft,
      revision: accepted.clientRevision,
      acceptedRevision: accepted.acceptedRevision,
      previewRevision: accepted.previewRevision,
      dirty: accepted.dirty,
      warnings: accepted.warnings,
      validationState: accepted.warnings.length ? 'warning' : 'valid',
      capabilities: {
        ...epubChapterDraft.capabilities,
        canRevert: accepted.dirty,
      },
    };
    if (epubEditDraft) {
      const modified = new Set(epubEditDraft.changes.modifiedChapters);
      if (accepted.dirty) modified.add(epubChapterDraft.spineIndex);
      else modified.delete(epubChapterDraft.spineIndex);
      const modifiedChapters = [...modified].sort((left, right) => left - right);
      updateActiveEpubDraft({
        ...epubEditDraft,
        revision: accepted.publicationRevision,
        dirty: Boolean(
          epubEditDraft.changes.metadataFields.length
          || epubEditDraft.changes.coverChanged
          || modifiedChapters.length
        ),
        changes: { ...epubEditDraft.changes, modifiedChapters },
      });
    }
    epubChapterLocalDirty = false;
  }

  async function refreshDraftAfterChapterRevert(reverted: ChapterEditDto): Promise<void> {
    epubChapterDraft = reverted;
    epubChapterLocalDirty = reverted.dirty;
    if (!epubEditDraft) return;
    try {
      updateActiveEpubDraft(await getEpubEditDraft(epubEditDraft.editSessionId));
    } catch (error) {
      epubError = normalizeAppError(error);
    }
  }

  function updateActiveEpubDraft(draft: EpubEditDraft | null): void {
    epubEditDraft = draft;
    tabs = tabs.map((tab) =>
      tab.kind === 'epub' && tab.documentId === activeTabId
        ? { ...tab, editDraft: draft, editPanelOpen: epubEditPanelOpen, saving: epubSaving }
        : tab,
    );
  }

  async function openEpubEditPanel(): Promise<void> {
    if (!epubDocument?.document.capabilities.canEditMetadata || saving) return;
    try {
      const draft = epubEditDraft ?? await beginEpubEdit(epubDocument.documentId);
      EpubEditPanelComponent ??= (await import('./lib/readers/epub/EpubEditPanel.svelte')).default;
      epubEditPanelOpen = true;
      updateActiveEpubDraft(draft);
    } catch (error) {
      epubError = normalizeAppError(error);
    }
  }

  async function changeEpubMetadata(metadataPatch: EpubMetadataPatch): Promise<void> {
    const draft = epubEditDraft;
    if (!draft || saving) return;
    try {
      updateActiveEpubDraft(await updateEpubMetadata(
        draft.editSessionId,
        draft.revision,
        metadataPatch,
      ));
      epubError = null;
    } catch (error) {
      epubError = normalizeAppError(error);
    }
  }

  async function chooseAndReplaceEpubCover(): Promise<void> {
    const draft = epubEditDraft;
    if (!draft || saving) return;
    try {
      const selectedPath = await chooseEpubCoverPath();
      if (!selectedPath) return;
      updateActiveEpubDraft(await replaceEpubCover(
        draft.editSessionId,
        draft.revision,
        selectedPath,
      ));
      epubError = null;
    } catch (error) {
      epubError = normalizeAppError(error);
    }
  }

  async function restoreOriginalEpubCover(): Promise<void> {
    const draft = epubEditDraft;
    if (!draft || saving) return;
    try {
      updateActiveEpubDraft(await removeEpubCoverChange(draft.editSessionId, draft.revision));
      epubError = null;
    } catch (error) {
      epubError = normalizeAppError(error);
    }
  }

  function defaultEpubSavePath(path: string): string {
    return path.replace(/\.epub$/i, '.edited.epub');
  }

  async function performEpubSaveAs(): Promise<boolean> {
    const document = epubDocument;
    if (!document || saving) return false;
    if (!(await flushActiveEpubChapterDraft())) return false;
    const draft = epubEditDraft;
    if (!draft?.dirty) return false;
    try {
      const targetPath = await chooseEpubSavePath(defaultEpubSavePath(document.displayPath));
      if (!targetPath) return false;
      epubSaving = true;
      updateActiveEpubDraft({ ...draft, saving: true });
      let saved;
      try {
        saved = await saveEpubAs(draft.editSessionId, draft.revision, targetPath);
      } catch (error) {
        const appError = normalizeAppError(error);
        if (appError.code !== 'TARGET_ALREADY_EXISTS') throw appError;
        const confirmed = window.confirm(`目标文件已存在：\n${targetPath}\n\n是否覆盖该目标？原 EPUB 不会被修改。`);
        if (!confirmed) return false;
        const token = await prepareEpubOverwriteConfirmation(
          draft.editSessionId,
          draft.revision,
          targetPath,
        );
        saved = await saveEpubAs(draft.editSessionId, draft.revision, targetPath, token);
      }
      epubSaving = false;
      updateActiveEpubDraft(saved.draft);
      if (epubChapterDraft) {
        epubChapterDraft = await validateEpubChapterDraft(epubChapterDraft.chapterEditId);
        epubChapterLocalDirty = false;
      }
      await refreshLibrary();
      if (window.confirm(`已安全另存为：\n${saved.targetPath}\n\n是否在新标签中打开？`)) {
        await openEpubPath(saved.targetPath);
      }
      return true;
    } catch (error) {
      epubError = normalizeAppError(error);
      return false;
    } finally {
      epubSaving = false;
      if (epubEditDraft?.saving) updateActiveEpubDraft({ ...epubEditDraft, saving: false });
    }
  }

  async function cancelActiveEpubSave(): Promise<void> {
    if (!epubEditDraft || !epubSaving) return;
    await cancelEpubSave(epubEditDraft.editSessionId).catch(() => {});
  }

  async function discardActiveEpubDraft(confirm = true): Promise<boolean> {
    const draft = epubEditDraft;
    if (!draft || saving) return false;
    if (confirm && (draft.dirty || epubChapterLocalDirty) && !window.confirm('确定放弃全部 EPUB 元数据、封面和章节正文修改吗？')) {
      return false;
    }
    try {
      await discardEpubDraft(draft.editSessionId);
      epubEditPanelOpen = false;
      epubChapterEditMode = false;
      epubChapterDraft = null;
      epubChapterEditorHandle = null;
      epubChapterLocalDirty = false;
      updateActiveEpubDraft(null);
      return true;
    } catch (error) {
      epubError = normalizeAppError(error);
      return false;
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
      await flushActiveTextProgress();
      snapshotActiveTab();

      EpubReaderComponent ??= (await import('./lib/readers/epub/EpubReader.svelte')).default;
      editing = false;
      editorHandle = null;
      textHeadings = [];
      textBookmarks = [];
      initialContent = '';
      statistics = { lines: 0, characters: 0 };
      encodingRecoveryPath = null;
      epubError = null;
      epubSpineIndex = opened.initialLocator?.spineIndex ?? 0;
      epubDocument = opened;
      epubEditDraft = null;
      epubEditPanelOpen = false;
      epubChapterEditMode = false;
      epubChapterDraft = null;
      epubChapterEditorHandle = null;
      epubChapterLocalDirty = false;
      epubSaving = false;
      activeTabId = opened.documentId;
      tabs = [
        ...tabs,
        {
          kind: 'epub',
          documentId: opened.documentId,
          document: opened,
          spineIndex: epubSpineIndex,
          editDraft: null,
          editPanelOpen: false,
          chapterEditMode: false,
          activeChapterDraft: null,
          saving: false,
        },
      ];
      documentStore.close();
      await refreshLibrary();
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
      const path = await chooseDocumentFile();
      if (!path) return;
      await openDocumentPath(path);
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    }
  }

  async function openDocumentPath(path: string): Promise<void> {
    activeView = 'workspace';
    const normalizedPath = comparableDocumentPath(path);
    const openTab = tabs.find((tab) => {
      const tabPath = tab.kind === 'txt' ? tab.session.displayPath : tab.document.displayPath;
      return comparableDocumentPath(tabPath) === normalizedPath;
    });
    if (openTab) {
      await selectWorkspaceTab(openTab.documentId);
      return;
    }
    if (classifyDocumentPath(path) === 'epub') await openEpubPath(path);
    else await openPath(path, null, false);
  }

  function comparableDocumentPath(path: string): string {
    return path
      .replaceAll('/', '\\')
      .replace(/^\\\\\?\\/, '')
      .toLocaleLowerCase('en-US');
  }

  async function openDroppedPaths(paths: string[]): Promise<void> {
    if (saving) return;
    for (const path of paths) await openDocumentPath(path);
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
      await flushActiveTextProgress();
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
            bookmarks: [...opened.bookmarks],
            readingOffset: opened.initialCharacterOffset,
          },
        ];
      }
      await refreshLibrary();
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
    textHeadings = [];
    textBookmarks = [...opened.bookmarks];
    statistics = { lines: 0, characters: 0 };
    initialContent = opened.content;
    initialTextOffset = opened.initialCharacterOffset;
    editorInstanceKey += 1;
    documentStore.open(opened);
    const restored = $documentStore.active;
    if (restored) {
      tabs = tabs.map((tab) =>
        tab.kind === 'txt' && tab.documentId === opened.documentId
          ? { ...tab, session: { ...restored }, content: opened.content, bookmarks: [...opened.bookmarks], readingOffset: opened.initialCharacterOffset }
          : tab,
      );
    }
  }

  function editorReady(handle: TextEditorHandle): void {
    editorHandle = handle;
    editorHandle.setEditing(editing);
    if (initialTextOffset > 0) editorHandle.revealOffset(initialTextOffset, false);
    initialContent = '';
  }

  function queueTextProgress(offset: number): void {
    if (!activeDocument) return;
    tabs = tabs.map((tab) => tab.kind === 'txt' && tab.documentId === activeDocument?.documentId
      ? { ...tab, readingOffset: offset }
      : tab);
    if (textProgressTimer) clearTimeout(textProgressTimer);
    textProgressTimer = setTimeout(() => {
      textProgressTimer = null;
      void persistActiveTextProgress(offset);
    }, 700);
  }

  async function persistActiveTextProgress(offset?: number): Promise<void> {
    const document = activeDocument;
    const editor = editorHandle;
    if (!document || !editor) return;
    const content = editor.getContent();
    const position = describeTextPosition(content, offset ?? editor.getReadingOffset());
    await saveTextProgress(document.documentId, position.characterOffset, position.lineNumber);
  }

  async function flushActiveTextProgress(): Promise<void> {
    if (textProgressTimer) {
      clearTimeout(textProgressTimer);
      textProgressTimer = null;
    }
    await persistActiveTextProgress().catch(() => {});
  }

  function updateActiveTextBookmarks(bookmarks: TextBookmark[]): void {
    textBookmarks = bookmarks;
    tabs = tabs.map((tab) =>
      tab.kind === 'txt' && tab.documentId === activeDocument?.documentId
        ? { ...tab, bookmarks: [...bookmarks] }
        : tab,
    );
  }

  async function addActiveTextBookmark(): Promise<void> {
    const document = activeDocument;
    if (!document || !editorHandle) return;
    const content = editorHandle.getContent();
    const position = describeTextPosition(content, editorHandle.getCursorOffset());
    try {
      const bookmark = await saveTextBookmark(
        document.documentId,
        position.characterOffset,
        position.lineNumber,
        null,
        position.preview,
      );
      updateActiveTextBookmarks([...textBookmarks, bookmark]);
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    }
  }

  async function renameActiveTextBookmark(bookmark: TextBookmark): Promise<void> {
    const document = activeDocument;
    if (!document) return;
    const title = window.prompt('书签标题', bookmark.title ?? `第 ${bookmark.lineNumber} 行`);
    if (title === null) return;
    try {
      const updated = await saveTextBookmark(
        document.documentId,
        bookmark.characterOffset,
        bookmark.lineNumber,
        title,
        bookmark.preview,
        bookmark.bookmarkId,
      );
      updateActiveTextBookmarks(textBookmarks.map((item) =>
        item.bookmarkId === bookmark.bookmarkId ? updated : item,
      ));
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    }
  }

  async function removeActiveTextBookmark(bookmark: TextBookmark): Promise<void> {
    const document = activeDocument;
    if (!document || !window.confirm(`删除书签“${bookmark.title ?? `第 ${bookmark.lineNumber} 行`}”？`)) return;
    try {
      await deleteTextBookmark(document.documentId, bookmark.bookmarkId);
      updateActiveTextBookmarks(textBookmarks.filter((item) => item.bookmarkId !== bookmark.bookmarkId));
    } catch (error) {
      documentStore.failed(normalizeAppError(error));
    }
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
    documentStore.markContentDirty(false);
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
        epubEditDraft = null;
        epubEditPanelOpen = false;
        epubChapterEditMode = false;
        epubChapterDraft = null;
        epubChapterEditorHandle = null;
        epubChapterLocalDirty = false;
        epubSaving = false;
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
      await flushActiveTextProgress();
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
    const saved = epubDocument ? await performEpubSaveAs() : await performSave(Boolean(activeDocument?.readOnly));
    if (saved) {
      pendingAction = null;
      await continueAction(action);
    }
  }

  async function discardAndContinue(): Promise<void> {
    const action = pendingAction;
    if (!action) return;
    pendingAction = null;
    if (epubEditDraft && !(await discardActiveEpubDraft(false))) return;
    if (activeDocument && isDirty(activeDocument)) discardEditingChanges();
    if (action === 'exit-edit') {
      discardEditingChanges();
      return;
    }
    if (action === 'reopen') {
      await performReopen();
      return;
    }
    if (action === 'exit') {
      await tick();
      return continueExit();
    }
    await closeCurrentDocument();
    if (action === 'open') await performOpen();
  }

  async function continueAction(action: PendingAction): Promise<void> {
    if (action === 'exit-edit') finishEditing();
    else if (action === 'close') await closeCurrentDocument();
    else if (action === 'open') await performOpen();
    else if (action === 'reopen') await performReopen();
    else await continueExit();
  }

  async function continueExit(): Promise<void> {
    await epubReaderHandle?.flushProgress().catch(() => {});
    await flushActiveTextProgress();
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

  function restoreHeadingPattern(): void {
    try {
      const savedPattern = localStorage.getItem(headingPatternStorageKey);
      if (savedPattern) changeHeadingPattern(savedPattern, false);
    } catch {
      // 设置存储不可用时继续使用内置规则。
    }
  }

  function changeHeadingPattern(value: string, persist = true): void {
    headingPatternDraft = value;
    if (!value.trim()) {
      headingPatternError = '标题识别正则不能为空。';
      return;
    }
    try {
      new RegExp(value, 'gm');
    } catch {
      headingPatternError = '正则表达式无效，仍继续使用上一次有效规则。';
      return;
    }
    headingPattern = value;
    headingPatternError = null;
    if (persist) {
      try {
        localStorage.setItem(headingPatternStorageKey, value);
      } catch {
        // 设置仍在当前窗口生效。
      }
    }
  }

  function resetHeadingPattern(): void {
    changeHeadingPattern(DEFAULT_TEXT_HEADING_PATTERN);
  }

  function dismissWorkspaceError(): void {
    epubError = null;
    documentStore.clearError();
  }
</script>

<div
  class:has-custom-background={Boolean(currentBackgroundUrl)}
  class:resizing={resizingPane}
  class="app-shell"
  style={`--left-pane-width:${leftPaneCollapsed ? 0 : leftPaneWidth}px;--app-background-image:${currentBackgroundUrl ? `url(${currentBackgroundUrl})` : 'none'};--app-background-opacity:${appSettings.backgroundOpacity}`}
>
  <TopBar
    activeTabId={activeTabId}
    compact={leftPaneCollapsed}
    {connection}
    document={activeDocument}
    displayTitle={epubDocument?.document.metadata.title ?? null}
    displayPath={epubDocument?.displayPath ?? null}
    hasDocument={Boolean(epubDocument)}
    settingsOpen={activeView === 'settings'}
    onClose={requestClose}
    onCloseTab={(documentId) => void requestCloseTab(documentId)}
    onSelectTab={(documentId) => void selectWorkspaceTab(documentId)}
    onToggleSettings={toggleSettingsView}
    tabs={tabSummaries}
  />
  <div class="workspace-grid">
    {#if !leftPaneCollapsed}
      <NavigationPane
        {desktopRuntime}
        activeView={activeView}
        onOpen={requestOpen}
        onSelectLibrary={() => void showLibrary()}
        onSelectSettings={() => void showSettings()}
        onSelectWorkspace={showWorkspace}
      />
    {/if}
    <div class="pane-divider left-divider" style="grid-column: 2">
      <!-- svelte-ignore a11y_no_noninteractive_tabindex a11y_no_noninteractive_element_interactions (ARIA Window Splitter pattern) -->
      <div
        aria-label="调整左侧栏宽度"
        aria-orientation="vertical"
        aria-valuemax={maximumPaneWidth()}
        aria-valuemin="180"
        aria-valuenow={leftPaneWidth}
        class="resize-grip"
        onkeydown={resizePaneFromKeyboard}
        onpointerdown={beginPaneResize}
        role="separator"
        tabindex={leftPaneCollapsed ? -1 : 0}
      ></div>
      <button
        aria-label={leftPaneCollapsed ? '展开左侧栏' : '收起左侧栏'}
        class="pane-toggle"
        onclick={() => (leftPaneCollapsed = !leftPaneCollapsed)}
        title={leftPaneCollapsed ? '展开左侧栏' : '收起左侧栏'}
        type="button"
      >{leftPaneCollapsed ? '›' : '‹'}</button>
    </div>

    <main class="document-workspace" style="grid-column: 3">
      {#if activeView === 'library'}
        <div class="library-stage">
          <LibraryView
            {desktopRuntime}
            columns={appSettings.libraryColumns}
            documents={libraryDocuments}
            groups={libraryGroups}
            importStatus={libraryImportStatus}
            loading={libraryLoading}
            onCreateGroup={(name) => void addLibraryGroup(name)}
            onDeleteGroup={(group) => void deleteLibraryShelf(group)}
            onImportDirectory={() => void importSelectedLibraryDirectory()}
            onImportFiles={() => void importSelectedLibraryFiles()}
            onMoveToGroup={(document, groupId) => void moveLibraryBook(document, groupId)}
            onOpen={openLibraryDocument}
            onRefresh={() => void refreshLibrary()}
            onRemove={(document) => void removeLibraryBook(document)}
            onRemoveUnavailable={() => void cleanupUnavailableLibraryBooks()}
            onRenameGroup={(group, name) => void renameLibraryShelf(group, name)}
          />
        </div>
      {:else if activeView === 'settings'}
        <SettingsView
          {backupBusy}
          {backupPath}
          {backupResult}
          {restoreResult}
          backgroundKey={backgroundImage?.key ?? null}
          backgroundUrl={currentBackgroundUrl}
          {headingPatternError}
          headingPattern={headingPatternDraft}
          onChooseBackground={() => void chooseCustomBackground()}
          onClearBackground={() => void clearCustomBackground()}
          onChooseBackupPath={() => void selectBackupPath()}
          onCreateBackup={() => void backupAllBooks()}
          onHeadingPatternChange={changeHeadingPattern}
          onResetHeadingPattern={resetHeadingPattern}
          onRestoreBackup={() => void restoreBookBackups()}
          onSettingsChange={changeAppSettings}
          onThemeChange={changeTheme}
          settings={appSettings}
          theme={$themePreference}
        />
      {:else}
      {#if !epubDocument}
        <EditorToolbar
          {desktopRuntime}
          document={activeDocument}
          {editing}
          {saving}
          shortcuts={appSettings.shortcuts}
          onClose={requestClose}
          onOpen={requestOpen}
          onOptionsChange={updateSaveOptions}
          onReopen={requestReopen}
          onSave={() => void performSave(false)}
          onSaveAs={() => void performSave(true)}
          onToggleEditing={toggleEditing}
        />
      {:else}
        <nav aria-label="EPUB 文件操作" class="epub-toolbar">
          <button disabled={!desktopRuntime || saving} onclick={requestOpen} type="button">打开</button>
          {#if epubDocument.document.capabilities.canEditMetadata}
            <button
              class:editing={epubEditPanelOpen}
              disabled={saving || epubChapterEditMode}
              onclick={() => void openEpubEditPanel()}
              type="button"
            >
              编辑书籍信息
            </button>
          {/if}
          {#if epubDocument.document.capabilities.canEditText}
            <button
              class:editing={epubChapterEditMode}
              disabled={saving}
              onclick={() => void toggleEpubChapterEditing()}
              type="button"
            >
              {epubChapterEditMode ? '退出章节编辑' : '编辑当前章节'}
            </button>
          {/if}
          <span>仅另存为 · 原 EPUB 始终只读</span>
          <button disabled={saving || !dirty} onclick={() => void performEpubSaveAs()} type="button">另存为</button>
          <button disabled={saving || !dirty} onclick={() => void discardActiveEpubDraft()} type="button">放弃全部修改</button>
          <button disabled={saving} onclick={requestClose} type="button">关闭</button>
        </nav>
      {/if}

      <div class="editor-stage">
        {#if epubDocument && epubChapterEditMode && epubChapterDraft && EpubChapterEditorComponent}
          <svelte:component
            bind:this={epubChapterEditorHandle}
            this={EpubChapterEditorComponent}
            chapter={epubChapterDraft}
            onAccepted={acceptEpubChapterDraft}
            onChapterChange={(index: number) => void changeEpubSpine(index)}
            onError={(error: AppErrorDto) => (epubError = error)}
            onLocalDirty={(value: boolean) => (epubChapterLocalDirty = value)}
            onReverted={(reverted: ChapterEditDto) => void refreshDraftAfterChapterRevert(reverted)}
            readingSessionId={epubDocument.sessionId}
            saving={epubSaving}
            spineLength={epubDocument.document.spine.length}
          />
        {:else if epubDocument && EpubReaderComponent}
          <svelte:component
            bind:this={epubReaderHandle}
            this={EpubReaderComponent}
            document={epubDocument}
            onBookmarksChange={updateActiveEpubBookmarks}
            onError={(error: AppErrorDto) => (epubError = error)}
            onLocatorChange={updateActiveEpubLocator}
            modifiedSpineIndices={epubEditDraft?.changes.modifiedChapters ?? []}
            spineIndex={epubSpineIndex}
            onSpineChange={(index: number) => void changeEpubSpine(index)}
            readingSettings={appSettings.reading}
            hasCustomBackground={Boolean(currentBackgroundUrl)}
          />
          {#if epubEditPanelOpen && epubEditDraft && EpubEditPanelComponent}
            <svelte:component
              this={EpubEditPanelComponent}
              draft={epubEditDraft}
              previewUrl={epubEditDraft.cover.previewResourceId
                ? epubResourceUrl(epubDocument.sessionId, epubEditDraft.cover.previewResourceId)
                : null}
              saving={epubSaving}
              onCancelSave={() => void cancelActiveEpubSave()}
              onClose={() => {
                epubEditPanelOpen = false;
                snapshotActiveTab();
              }}
              onDiscard={() => void discardActiveEpubDraft()}
              onMetadataChange={(patch: EpubMetadataPatch) => void changeEpubMetadata(patch)}
              onReplaceCover={() => void chooseAndReplaceEpubCover()}
              onRestoreCover={() => void restoreOriginalEpubCover()}
              onSaveAs={() => void performEpubSaveAs()}
            />
          {/if}
        {:else if activeDocument}
          <div class="text-reader-body">
            <TextToolsPane
              getTextContent={() => editorHandle?.getContent() ?? initialContent}
              onAddTextBookmark={() => void addActiveTextBookmark()}
              onDeleteTextBookmark={(bookmark) => void removeActiveTextBookmark(bookmark)}
              onRenameTextBookmark={(bookmark) => void renameActiveTextBookmark(bookmark)}
              onRevealTextOffset={(offset) => editorHandle?.revealOffset(offset)}
              {textBookmarks}
              {textHeadings}
            />
            <div class="text-editor-slot">
              {#key `${activeDocument.documentId}-${editorInstanceKey}`}
                <TextEditor
                  {headingPattern}
                  hasCustomBackground={Boolean(currentBackgroundUrl)}
                  {initialContent}
                  onDirtyChange={(value) => documentStore.markContentDirty(value)}
                  onHeadingsChange={(value) => (textHeadings = value)}
                  onReadingPositionChange={queueTextProgress}
                  onReady={editorReady}
                  onStatisticsChange={(value) => (statistics = value)}
                  readingSettings={appSettings.reading}
                />
              {/key}
            </div>
          </div>
        {:else}
          <section class="empty-state">
            <div class="empty-mark">R</div>
            <h1>打开一本书，开始阅读或编织</h1>
            <p>EPUB 2/3 以隔离模式阅读；其他文件按文本打开并支持安全编辑。</p>
            <div class="empty-actions">
              <button disabled={!desktopRuntime} onclick={requestOpen} type="button">
                {desktopRuntime ? '选择文件' : '请在 Readloom 桌面版中打开'}
              </button>
            </div>
            <span>可拖入文件；常用操作可在“设置 / 操作 / 快捷键”中自行绑定</span>
          </section>
        {/if}
      </div>
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
    </main>

  </div>
  <EditorStatusBar
    document={activeView === 'workspace' ? activeDocument : null}
    epubStatus={activeView === 'workspace' && epubDocument
      ? `EPUB ${epubDocument.document.version} · ${epubChapterEditMode ? '章节编辑' : '安全阅读'} · ${dirty ? '有未另存修改' : '原文件只读'}`
      : null}
    workspaceStatus={activeView === 'library'
      ? `书库 · ${libraryDocuments.length} 本地书籍 · ${libraryGroups.length} 个分组`
      : activeView === 'settings' ? '设置 · 本地配置' : null}
    {saving}
    {statistics}
  />
  {#if dragActive}
    <div class="drop-overlay" role="status">
      <div>
        <strong>松开以打开文件</strong>
        <span>EPUB 作为电子书打开，其他格式作为文本打开</span>
      </div>
    </div>
  {/if}
</div>

{#if libraryImportPreview}
  <LibraryImportReviewDialog
    importing={libraryImporting}
    preview={libraryImportPreview}
    onCancel={() => { if (!libraryImporting) libraryImportPreview = null; }}
    onConfirm={(paths) => void confirmLibraryImport(paths)}
  />
{/if}

{#if pendingAction && (activeDocument || epubDocument)}
  <UnsavedChangesDialog
    fileName={activeDocument?.fileName ?? epubDocument?.fileName ?? 'EPUB'}
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
    position: relative;
  }

  .workspace-grid {
    display: grid;
    grid-template-columns: var(--left-pane-width) 8px minmax(0, 1fr);
    min-height: 0;
    min-width: 0;
  }

  .pane-divider {
    background: var(--surface-chrome);
    position: relative;
    z-index: 5;
  }

  .resize-grip {
    background: transparent;
    border: 0;
    cursor: col-resize;
    inset: 0;
    outline: none;
    padding: 0;
    position: absolute;
    width: 100%;
  }

  .resize-grip::after {
    background: transparent;
    content: '';
    inset: 0 3px;
    position: absolute;
    transition: background var(--motion-fast);
  }

  .resize-grip:hover::after,
  .resize-grip:focus-visible::after,
  .resizing .resize-grip::after {
    background: var(--accent);
  }

  .app-shell.resizing {
    cursor: col-resize;
    user-select: none;
  }

  .pane-toggle {
    align-items: center;
    background: var(--surface-control);
    border: 1px solid var(--border-strong);
    border-radius: 999px;
    color: var(--text-tertiary);
    display: flex;
    font: 600 16px/1 var(--font-ui);
    height: 28px;
    justify-content: center;
    left: 50%;
    padding: 0;
    position: absolute;
    top: 12px;
    transform: translateX(-50%);
    width: 22px;
    z-index: 1;
  }

  .pane-toggle:hover {
    background: var(--surface-hover);
    color: var(--accent-strong);
  }

  .document-workspace {
    background: var(--surface-canvas);
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    position: relative;
  }

  .library-stage {
    flex: 1;
    min-height: 0;
    min-width: 0;
  }

  .editor-stage {
    flex: 1;
    min-height: 0;
    min-width: 0;
    position: relative;
  }

  .text-reader-body {
    display: flex;
    height: 100%;
    min-height: 0;
    min-width: 0;
    position: relative;
  }

  .has-custom-background .text-reader-body {
    background: transparent;
  }

  .document-workspace::before {
    background-image: var(--app-background-image);
    background-position: center;
    background-size: cover;
    content: '';
    inset: 0;
    opacity: var(--app-background-opacity);
    pointer-events: none;
    position: absolute;
  }

  .document-workspace > :global(*) {
    position: relative;
    z-index: 1;
  }

  .text-editor-slot {
    flex: 1;
    min-height: 0;
    min-width: 0;
  }

  .epub-toolbar {
    align-items: center;
    background: var(--surface-pane);
    border-bottom: 1px solid var(--border-subtle);
    display: flex;
    gap: 7px;
    min-height: 39px;
    padding: 0 10px;
  }

  .epub-toolbar button {
    background: var(--surface-control);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    font: 600 10px/1 var(--font-ui);
    min-height: 28px;
    padding: 0 10px;
  }

  .epub-toolbar button.editing {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent-strong);
  }

  .epub-toolbar span {
    color: var(--text-disabled);
    flex: 1;
    font: 500 9px/1 var(--font-ui);
    text-align: right;
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

  .drop-overlay {
    align-items: center;
    background: color-mix(in srgb, var(--surface-canvas) 88%, transparent);
    border: 2px dashed var(--accent);
    display: flex;
    inset: 10px;
    justify-content: center;
    position: absolute;
    z-index: 100;
  }

  .drop-overlay > div {
    background: var(--surface-pane);
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    box-shadow: 0 16px 40px rgb(0 0 0 / 18%);
    display: grid;
    gap: 8px;
    padding: 24px 32px;
    text-align: center;
  }

  .drop-overlay strong {
    color: var(--text-primary);
    font: 650 16px/1.2 var(--font-ui);
  }

  .drop-overlay span {
    color: var(--text-tertiary);
    font: 500 11px/1.4 var(--font-ui);
  }

</style>
