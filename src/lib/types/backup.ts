export interface BooksBackupResultDto {
  targetPath: string;
  bookCount: number;
  uniqueContentCount: number;
  unavailableSkipped: number;
  sourceBytes: number;
  backupBytes: number;
}

export interface BooksRestoreFailureDto {
  backupPath: string;
  fileName: string;
  message: string;
}

export interface BooksRestoreResultDto {
  targetDirectory: string;
  restored: number;
  duplicateContentSkipped: number;
  existingContentSkipped: number;
  restoredBytes: number;
  failed: BooksRestoreFailureDto[];
}
