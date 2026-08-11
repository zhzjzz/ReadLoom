import { describe, expect, it, vi } from 'vitest';

import { defaultShortcutSettings } from '../stores/appSettings';
import { createShortcutHandler, shortcutFromEvent } from './shortcuts';

describe('document shortcuts', () => {
  it('routes only explicitly configured shortcuts', () => {
    const actions = { open: vi.fn(), save: vi.fn(), saveAs: vi.fn(), close: vi.fn() };
    const settings = {
      ...defaultShortcutSettings,
      open: 'Ctrl+O',
      save: 'Ctrl+S',
      saveAs: 'Ctrl+Shift+S',
      close: 'Ctrl+W',
    };
    const handler = createShortcutHandler(actions, () => settings);

    handler(new KeyboardEvent('keydown', { key: 'o', ctrlKey: true, cancelable: true }));
    handler(new KeyboardEvent('keydown', { key: 's', ctrlKey: true, cancelable: true }));
    handler(new KeyboardEvent('keydown', { key: 'S', ctrlKey: true, shiftKey: true, cancelable: true }));
    handler(new KeyboardEvent('keydown', { key: 'w', ctrlKey: true, cancelable: true }));

    expect(actions.open).toHaveBeenCalledOnce();
    expect(actions.save).toHaveBeenCalledOnce();
    expect(actions.saveAs).toHaveBeenCalledOnce();
    expect(actions.close).toHaveBeenCalledOnce();
  });

  it('keeps every shortcut disabled by default', () => {
    const actions = { open: vi.fn(), save: vi.fn(), saveAs: vi.fn(), close: vi.fn() };
    const handler = createShortcutHandler(actions, () => defaultShortcutSettings);

    handler(new KeyboardEvent('keydown', { key: 'o', ctrlKey: true, cancelable: true }));

    expect(actions.open).not.toHaveBeenCalled();
  });

  it('does not trigger commands during IME composition or key repeat', () => {
    const actions = { open: vi.fn(), save: vi.fn(), saveAs: vi.fn(), close: vi.fn() };
    const handler = createShortcutHandler(actions, () => ({
      ...defaultShortcutSettings,
      save: 'Ctrl+S',
    }));
    const composing = new KeyboardEvent('keydown', { key: 's', ctrlKey: true });
    Object.defineProperty(composing, 'isComposing', { value: true });

    handler(composing);
    handler(new KeyboardEvent('keydown', { key: 's', ctrlKey: true, repeat: true }));

    expect(actions.save).not.toHaveBeenCalled();
  });

  it('routes chapter navigation and lets a rejected bookmark shortcut continue', () => {
    const actions = {
      open: vi.fn(),
      save: vi.fn(),
      saveAs: vi.fn(),
      close: vi.fn(),
      previousChapter: vi.fn(),
      nextChapter: vi.fn(),
      bookmark: vi.fn(() => false),
    };
    const handler = createShortcutHandler(actions, () => ({
      ...defaultShortcutSettings,
      previousChapter: 'Alt+Up',
      nextChapter: 'Alt+Down',
      bookmark: 'Ctrl+B',
    }));
    const previous = new KeyboardEvent('keydown', { key: 'ArrowUp', altKey: true, cancelable: true });
    const next = new KeyboardEvent('keydown', { key: 'ArrowDown', altKey: true, cancelable: true });
    const bold = new KeyboardEvent('keydown', { key: 'b', ctrlKey: true, cancelable: true });

    handler(previous);
    handler(next);
    handler(bold);

    expect(actions.previousChapter).toHaveBeenCalledOnce();
    expect(actions.nextChapter).toHaveBeenCalledOnce();
    expect(actions.bookmark).toHaveBeenCalledOnce();
    expect(bold.defaultPrevented).toBe(false);
  });

  it('normalizes captured combinations for storage and duplicate checks', () => {
    expect(shortcutFromEvent(new KeyboardEvent('keydown', {
      key: 'ArrowLeft',
      ctrlKey: true,
      shiftKey: true,
    }))).toBe('Ctrl+Shift+Left');
  });
});
