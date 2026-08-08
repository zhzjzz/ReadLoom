import { open } from '@tauri-apps/plugin-dialog';

export type WorkspaceFileKind = 'epub' | 'text';

export async function chooseDocumentFile(): Promise<string | null> {
  const selected = await open({
    directory: false,
    multiple: false,
    title: '打开文件',
  });
  return typeof selected === 'string' ? selected : null;
}

export function classifyDocumentPath(path: string): WorkspaceFileKind {
  return /\.epub$/i.test(path) ? 'epub' : 'text';
}
