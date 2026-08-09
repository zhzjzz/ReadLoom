import { writable } from 'svelte/store';

import type {
  DocumentState,
  OpenedTextDocumentDto,
  SavedTextDocumentDto,
  SaveOptions,
} from '../types/document';
import type { AppErrorDto } from '../types/ipc';

const initialState: DocumentState = {
  active: null,
  saveStatus: 'idle',
  error: null,
};

export function createDocumentStore() {
  const { subscribe, set, update } = writable<DocumentState>(initialState);

  return {
    subscribe,
    open(document: OpenedTextDocumentDto): void {
      const { content: _content, bookmarks: _bookmarks, ...metadata } = document;
      set({
        active: {
          ...metadata,
          savedEncoding: metadata.encoding,
          savedHasBom: metadata.hasBom,
          lineEndingChoice: 'preserve',
          contentDirty: false,
          formatDirty: false,
        },
        saveStatus: 'idle',
        error: null,
      });
    },
    restore(active: DocumentState['active']): void {
      set({ active, saveStatus: 'idle', error: null });
    },
    markContentDirty(contentDirty: boolean): void {
      update((state) => ({
        ...state,
        active: state.active ? { ...state.active, contentDirty } : null,
      }));
    },
    updateSaveOptions(options: SaveOptions): void {
      update((state) => {
        if (!state.active) return state;
        const formatDirty =
          options.encoding !== state.active.savedEncoding ||
          options.hasBom !== state.active.savedHasBom ||
          options.lineEnding !== 'preserve';
        return {
          ...state,
          active: {
            ...state.active,
            encoding: options.encoding,
            hasBom: options.hasBom,
            lineEndingChoice: options.lineEnding,
            formatDirty,
          },
        };
      });
    },
    saving(): void {
      update((state) => ({ ...state, saveStatus: 'saving', error: null }));
    },
    idle(): void {
      update((state) => ({ ...state, saveStatus: 'idle' }));
    },
    saved(document: SavedTextDocumentDto): void {
      update((state) => ({
        active: state.active
          ? {
              ...state.active,
              ...document,
              savedEncoding: document.encoding,
              savedHasBom: document.hasBom,
              lineEndingChoice: 'preserve',
              contentDirty: false,
              formatDirty: false,
            }
          : null,
        saveStatus: 'idle',
        error: null,
      }));
    },
    failed(error: AppErrorDto): void {
      update((state) => ({ ...state, saveStatus: 'error', error }));
    },
    clearError(): void {
      update((state) => ({ ...state, saveStatus: state.saveStatus === 'error' ? 'idle' : state.saveStatus, error: null }));
    },
    close(): void {
      set(initialState);
    },
    reset(): void {
      set(initialState);
    },
  };
}

export const documentStore = createDocumentStore();
