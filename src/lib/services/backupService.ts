import type { BooksBackupResultDto, BooksRestoreResultDto } from '../types/backup';
import { invokeChecked } from './backend';

export async function createBooksBackup(targetPath: string): Promise<BooksBackupResultDto> {
  return invokeChecked('create_books_backup', { request: { targetPath } });
}

export async function restoreBooksBackup(
  backupPaths: string[],
  targetDirectory: string,
): Promise<BooksRestoreResultDto> {
  return invokeChecked('restore_books_backup', { request: { backupPaths, targetDirectory } });
}
