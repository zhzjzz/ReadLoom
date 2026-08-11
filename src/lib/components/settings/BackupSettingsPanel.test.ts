import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import BackupSettingsPanel from './BackupSettingsPanel.svelte';

describe('BackupSettingsPanel', () => {
  it('warns that reader state is excluded and invokes real workflow callbacks', async () => {
    const onChooseBackupPath = vi.fn();
    const onCreateBackup = vi.fn();
    const onRestore = vi.fn();
    const { rerender } = render(BackupSettingsPanel, {
      onChooseBackupPath,
      onCreateBackup,
      onRestore,
    });

    expect(screen.getByText(/书签、阅读进度、分组、设置和阅读记录不会写入备份/)).toBeTruthy();
    expect((screen.getByRole('button', { name: '立即备份所有书籍' }) as HTMLButtonElement).disabled).toBe(true);
    await fireEvent.click(screen.getByRole('button', { name: '选择位置' }));
    expect(onChooseBackupPath).toHaveBeenCalledOnce();

    await rerender({ backupPath: 'D:\\Backups\\books.readloom-backup' });
    await fireEvent.click(screen.getByRole('button', { name: '立即备份所有书籍' }));
    await fireEvent.click(screen.getByRole('button', { name: '选择备份文件并恢复' }));
    expect(onCreateBackup).toHaveBeenCalledOnce();
    expect(onRestore).toHaveBeenCalledOnce();
  });
});
