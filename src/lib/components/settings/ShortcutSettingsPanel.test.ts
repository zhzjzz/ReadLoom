import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { defaultShortcutSettings } from '../../stores/appSettings';
import ShortcutSettingsPanel from './ShortcutSettingsPanel.svelte';

describe('ShortcutSettingsPanel', () => {
  it('shows no defaults and captures a user-selected combination', async () => {
    const onChange = vi.fn();
    render(ShortcutSettingsPanel, { value: defaultShortcutSettings, onChange });

    const open = screen.getByRole('button', { name: '打开文件快捷键' });
    expect(open.textContent).toBe('无');
    await fireEvent.click(open);
    await fireEvent.keyDown(open, { key: 'o', ctrlKey: true });

    expect(onChange).toHaveBeenCalledWith({ ...defaultShortcutSettings, open: 'Ctrl+O' });
  });

  it('rejects a combination already assigned to another action', async () => {
    const onChange = vi.fn();
    render(ShortcutSettingsPanel, {
      value: { ...defaultShortcutSettings, save: 'Ctrl+S' },
      onChange,
    });

    const open = screen.getByRole('button', { name: '打开文件快捷键' });
    await fireEvent.click(open);
    await fireEvent.keyDown(open, { key: 's', ctrlKey: true });

    expect(screen.getByRole('alert').textContent).toContain('已用于“保存”');
    expect(onChange).not.toHaveBeenCalled();
  });
});
