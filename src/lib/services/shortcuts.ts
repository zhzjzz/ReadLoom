import type { ShortcutActionId, ShortcutSettings } from '../types/settings';

export interface ShortcutActions {
  open(): void;
  save(): void;
  saveAs(): void;
  close(): void;
  toggleEdit?(): void;
  previousChapter?(): void;
  nextChapter?(): void;
  bookmark?(): void | boolean;
  showLibrary?(): void;
  showSettings?(): void;
}

export const shortcutLabels: Readonly<Record<ShortcutActionId, string>> = {
  open: '打开文件',
  save: '保存',
  saveAs: '另存为',
  close: '关闭当前图书',
  toggleEdit: '切换编辑模式',
  previousChapter: '上一章',
  nextChapter: '下一章',
  bookmark: '添加书签',
  showLibrary: '打开书库',
  showSettings: '打开设置',
};

export function createShortcutHandler(
  actions: ShortcutActions,
  getSettings: () => ShortcutSettings,
): (event: KeyboardEvent) => void {
  return (event) => {
    if (event.isComposing || event.keyCode === 229 || event.repeat) return;
    const pressed = shortcutFromEvent(event);
    if (!pressed) return;
    const settings = getSettings();
    const action = (Object.keys(settings) as ShortcutActionId[])
      .find((id) => settings[id] === pressed);
    if (!action) return;
    const handler = actions[action];
    if (!handler) return;
    const handled = handler();
    if (handled !== false) event.preventDefault();
  };
}

export function shortcutFromEvent(event: KeyboardEvent): string | null {
  if (event.isComposing || event.keyCode === 229) return null;
  const key = normalizedKey(event.key);
  if (!key || ['Control', 'Meta', 'Alt', 'Shift'].includes(key)) return null;
  const parts: string[] = [];
  if (event.ctrlKey || event.metaKey) parts.push('Ctrl');
  if (event.altKey) parts.push('Alt');
  if (event.shiftKey) parts.push('Shift');
  parts.push(key);
  return parts.join('+');
}

function normalizedKey(key: string): string {
  if (key === ' ') return 'Space';
  if (key.length === 1) return key.toUpperCase();
  return key.startsWith('Arrow') ? key.slice(5) : key;
}
