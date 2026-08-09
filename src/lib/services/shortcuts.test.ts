import { describe, expect, it, vi } from 'vitest';

import { createShortcutHandler } from './shortcuts';

describe('document shortcuts', () => {
  it('routes open, save, save-as and close once', () => {
    const actions = { open: vi.fn(), save: vi.fn(), saveAs: vi.fn(), close: vi.fn() };
    const handler = createShortcutHandler(actions);

    handler(new KeyboardEvent('keydown', { key: 'o', ctrlKey: true, cancelable: true }));
    handler(new KeyboardEvent('keydown', { key: 's', ctrlKey: true, cancelable: true }));
    handler(new KeyboardEvent('keydown', { key: 'S', ctrlKey: true, shiftKey: true, cancelable: true }));
    handler(new KeyboardEvent('keydown', { key: 'w', ctrlKey: true, cancelable: true }));

    expect(actions.open).toHaveBeenCalledOnce();
    expect(actions.save).toHaveBeenCalledOnce();
    expect(actions.saveAs).toHaveBeenCalledOnce();
    expect(actions.close).toHaveBeenCalledOnce();
  });

  it('does not trigger commands during IME composition or key repeat', () => {
    const actions = { open: vi.fn(), save: vi.fn(), saveAs: vi.fn(), close: vi.fn() };
    const handler = createShortcutHandler(actions);
    const composing = new KeyboardEvent('keydown', { key: 's', ctrlKey: true });
    Object.defineProperty(composing, 'isComposing', { value: true });

    handler(composing);
    handler(new KeyboardEvent('keydown', { key: 's', ctrlKey: true, repeat: true }));

    expect(actions.save).not.toHaveBeenCalled();
  });

  it('routes chapter mode and navigation while allowing rich-text Ctrl+B to continue', () => {
    const actions = {
      open: vi.fn(),
      save: vi.fn(),
      saveAs: vi.fn(),
      close: vi.fn(),
      toggleEdit: vi.fn(),
      previousChapter: vi.fn(),
      nextChapter: vi.fn(),
      bookmark: vi.fn(() => false),
    };
    const handler = createShortcutHandler(actions);
    const edit = new KeyboardEvent('keydown', { key: 'e', ctrlKey: true, cancelable: true });
    const previous = new KeyboardEvent('keydown', { key: 'ArrowUp', altKey: true, cancelable: true });
    const next = new KeyboardEvent('keydown', { key: 'ArrowDown', altKey: true, cancelable: true });
    const bold = new KeyboardEvent('keydown', { key: 'b', ctrlKey: true, cancelable: true });

    handler(edit);
    handler(previous);
    handler(next);
    handler(bold);

    expect(actions.toggleEdit).toHaveBeenCalledOnce();
    expect(actions.previousChapter).toHaveBeenCalledOnce();
    expect(actions.nextChapter).toHaveBeenCalledOnce();
    expect(actions.bookmark).toHaveBeenCalledOnce();
    expect(bold.defaultPrevented).toBe(false);
  });
});
