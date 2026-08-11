import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { OpenedTextDocumentDto } from './lib/types/document';

const closeHarness = vi.hoisted(() => ({
  closeDocument: vi.fn<() => Promise<void>>(),
  closeRequested: null as ((event: { preventDefault(): void }) => void) | null,
  destroy: vi.fn<() => Promise<void>>(),
  hide: vi.fn<() => Promise<void>>(),
  unlisten: vi.fn(),
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    destroy: closeHarness.destroy,
    hide: closeHarness.hide,
    onCloseRequested: async (handler: (event: { preventDefault(): void }) => void) => {
      closeHarness.closeRequested = handler;
      return closeHarness.unlisten;
    },
    onDragDropEvent: async () => vi.fn(),
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => vi.fn()),
}));

vi.mock('./lib/services/backend', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./lib/services/backend')>()),
  hasTauriRuntime: () => true,
  probeBackend: vi.fn().mockRejectedValue(new Error('not needed by this test')),
  reportFrontendReady: vi.fn().mockRejectedValue(new Error('not needed by this test')),
}));

vi.mock('./lib/services/appearanceService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./lib/services/appearanceService')>()),
  getBackgroundImage: vi.fn(async () => null),
  applyWindowBehavior: vi.fn(async () => {}),
}));

vi.mock('./lib/services/textDocumentService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./lib/services/textDocumentService')>()),
  closeTextDocument: closeHarness.closeDocument,
}));

import App from './App.svelte';
import { documentStore } from './lib/stores/documentStore';

const openedDocument: OpenedTextDocumentDto = {
  documentId: 'doc-window-close',
  fileName: '未保存.txt',
  displayPath: 'C:\\未保存.txt',
  content: '等待保存的内容',
  encoding: 'utf8',
  hasBom: false,
  lineEnding: 'crlf',
  sizeBytes: 24,
  readOnly: false,
  revision: 0,
  bookmarks: [],
  initialCharacterOffset: 0,
};

describe('native window close', () => {
  beforeEach(() => {
    localStorage.clear();
    closeHarness.closeRequested = null;
    closeHarness.closeDocument.mockReset();
    closeHarness.closeDocument.mockImplementation(() => new Promise<void>(() => {}));
    closeHarness.destroy.mockReset();
    closeHarness.destroy.mockResolvedValue();
    closeHarness.hide.mockReset();
    closeHarness.hide.mockResolvedValue();
    closeHarness.unlisten.mockReset();
  });

  it('destroys the window even when document-session cleanup never resolves', async () => {
    render(App);
    await waitFor(() => expect(closeHarness.closeRequested).not.toBeNull());

    documentStore.open(openedDocument);
    documentStore.markContentDirty(true);
    await screen.findAllByText('未保存.txt');

    const preventDefault = vi.fn();
    closeHarness.closeRequested!({ preventDefault });
    expect(preventDefault).toHaveBeenCalledOnce();
    await fireEvent.click(await screen.findByText('不保存'));

    await waitFor(() => expect(closeHarness.destroy).toHaveBeenCalledOnce());
    expect(closeHarness.closeDocument).not.toHaveBeenCalled();
  });

  it('does not intercept the native close request when there are no unsaved changes', async () => {
    render(App);
    await waitFor(() => expect(closeHarness.closeRequested).not.toBeNull());

    const preventDefault = vi.fn();
    closeHarness.closeRequested!({ preventDefault });

    expect(preventDefault).not.toHaveBeenCalled();
    expect(closeHarness.destroy).not.toHaveBeenCalled();
  });

  it('hides to the tray when that close behavior is selected', async () => {
    localStorage.setItem('readloom.app-settings.v1', JSON.stringify({
      libraryColumns: 4,
      backgroundOpacity: 0.14,
      minimizeToTray: false,
      closeAction: 'tray',
    }));
    render(App);
    await waitFor(() => expect(closeHarness.closeRequested).not.toBeNull());

    const preventDefault = vi.fn();
    closeHarness.closeRequested!({ preventDefault });

    expect(preventDefault).toHaveBeenCalledOnce();
    expect(closeHarness.hide).toHaveBeenCalledOnce();
    expect(closeHarness.destroy).not.toHaveBeenCalled();
  });
});
