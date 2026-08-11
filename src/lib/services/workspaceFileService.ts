import { open, save } from '@tauri-apps/plugin-dialog';

export type WorkspaceFileKind = 'epub' | 'text';

export async function chooseDocumentFile(): Promise<string | null> {
  const selected = await open({
    directory: false,
    multiple: false,
    title: '打开文件',
  });
  return typeof selected === 'string' ? selected : null;
}

export async function chooseLibraryFiles(): Promise<string[]> {
  const selected = await open({
    directory: false,
    filters: [{ name: '图书', extensions: ['epub', 'txt'] }],
    multiple: true,
    title: '批量导入图书',
  });
  if (Array.isArray(selected)) return selected;
  return typeof selected === 'string' ? [selected] : [];
}

export async function chooseLibraryDirectory(): Promise<string | null> {
  const selected = await open({
    directory: true,
    multiple: false,
    title: '选择图书目录',
  });
  return typeof selected === 'string' ? selected : null;
}

export async function chooseBackgroundImage(): Promise<string | null> {
  const selected = await open({
    title: '选择 Readloom 背景图片',
    multiple: false,
    directory: false,
    filters: [{ name: '图片', extensions: ['png', 'jpg', 'jpeg', 'webp'] }],
  });
  return typeof selected === 'string' ? selected : null;
}

export async function chooseBooksBackupPath(): Promise<string | null> {
  const date = new Date().toISOString().slice(0, 10);
  const selected = await save({
    defaultPath: `Readloom-books-${date}.readloom-backup`,
    filters: [{ name: 'Readloom 图书内容备份', extensions: ['readloom-backup'] }],
    title: '选择图书内容备份位置',
  });
  return typeof selected === 'string' ? selected : null;
}

export async function chooseBooksBackupFiles(): Promise<string[]> {
  const selected = await open({
    directory: false,
    filters: [{ name: 'Readloom 图书内容备份', extensions: ['readloom-backup'] }],
    multiple: true,
    title: '选择一个或多个图书内容备份',
  });
  if (Array.isArray(selected)) return selected;
  return typeof selected === 'string' ? [selected] : [];
}

export async function chooseBooksRestoreDirectory(): Promise<string | null> {
  const selected = await open({
    directory: true,
    multiple: false,
    title: '选择恢复图书的文件夹',
  });
  return typeof selected === 'string' ? selected : null;
}

export function classifyDocumentPath(path: string): WorkspaceFileKind {
  return /\.epub$/i.test(path) ? 'epub' : 'text';
}
