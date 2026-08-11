import type {
    LibraryGroupDto,
    LibraryImportPreviewDto,
    LibraryImportResultDto,
    LibrarySnapshotDto,
} from '../types/library';
import { invokeChecked } from './backend';

export async function listLibrary(maximum = 500): Promise<LibrarySnapshotDto> {
  return invokeChecked('list_library', { request: { maximum } });
}

export async function importLibraryDocuments(paths: string[]): Promise<LibraryImportResultDto> {
  return invokeChecked('import_library_documents', { request: { paths } });
}

export async function previewLibraryDirectory(directory: string): Promise<LibraryImportPreviewDto> {
  return invokeChecked('preview_library_directory', { request: { directory } });
}

export async function previewLibraryDocuments(paths: string[]): Promise<LibraryImportPreviewDto> {
  return invokeChecked('preview_library_documents', { request: { paths } });
}

export function libraryCoverUrl(coverKey: string): string {
  return `http://readloom-library.localhost/${encodeURIComponent(coverKey)}`;
}

export async function createLibraryGroup(name: string): Promise<LibraryGroupDto> {
  return invokeChecked('create_library_group', { request: { name } });
}

export async function renameLibraryGroup(groupId: string, name: string): Promise<void> {
  await invokeChecked('rename_library_group', { request: { groupId, name } });
}

export async function deleteLibraryGroup(groupId: string): Promise<void> {
  await invokeChecked('delete_library_group', { request: { groupId } });
}

export async function assignLibraryGroup(path: string, groupId: string | null): Promise<void> {
  await invokeChecked('assign_library_group', { request: { path, groupId } });
}

export async function removeLibraryDocument(path: string): Promise<void> {
  await invokeChecked('remove_library_document', { request: { path } });
}

export async function removeUnavailableLibraryDocuments(): Promise<number> {
  return invokeChecked('remove_unavailable_library_documents', {});
}
