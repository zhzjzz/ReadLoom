export interface LibraryDocumentDto {
  path: string;
  documentKind: 'txt' | 'epub';
  displayTitle: string;
  author: string | null;
  fingerprint: string | null;
  lastOpenedAtMs: number;
  available: boolean;
  groupId: string | null;
  coverKey: string | null;
}

export interface LibraryGroupDto {
  groupId: string;
  name: string;
  position: number;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface LibrarySnapshotDto {
  documents: LibraryDocumentDto[];
  groups: LibraryGroupDto[];
}

export interface LibraryImportFailureDto {
  path: string;
  code: string;
  message: string;
}

export interface LibraryImportResultDto {
  imported: number;
  skipped: number;
  failed: LibraryImportFailureDto[];
}

export interface LibraryImportCandidateDto {
  path: string;
  fileName: string;
  documentKind: 'txt' | 'epub';
  sizeBytes: number;
  alreadyImported: boolean;
}

export interface LibraryImportPreviewDto {
  rootPath: string | null;
  candidates: LibraryImportCandidateDto[];
  totalSizeBytes: number;
  importable: number;
  alreadyImported: number;
}
