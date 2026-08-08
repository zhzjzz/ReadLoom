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
});
