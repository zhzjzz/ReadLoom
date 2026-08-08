<script lang="ts">
  import type { DocumentSession, SaveOptions, TextEncoding } from '../types/document';

  export let document: DocumentSession | null;
  export let saving = false;
  export let editing = false;
  export let desktopRuntime = true;
  export let onOpen: () => void;
  export let onToggleEditing: () => void;
  export let onSave: () => void;
  export let onSaveAs: () => void;
  export let onClose: () => void;
  export let onReopen: () => void;
  export let onOptionsChange: (options: SaveOptions) => void;

  $: encodingChoice = document
    ? document.encoding === 'utf8' && document.hasBom
      ? 'utf8Bom'
      : document.encoding
    : 'utf8';

  function changeEncoding(event: Event): void {
    if (!document) return;
    const choice = (event.currentTarget as HTMLSelectElement).value;
    let encoding: TextEncoding;
    let hasBom: boolean;
    if (choice === 'utf8Bom') {
      encoding = 'utf8';
      hasBom = true;
    } else {
      encoding = choice as TextEncoding;
      hasBom = choice === 'utf16Le' || choice === 'utf16Be';
    }
    onOptionsChange({
      encoding,
      hasBom,
      lineEnding: document.lineEndingChoice,
    });
  }

  function changeLineEnding(event: Event): void {
    if (!document) return;
    onOptionsChange({
      encoding: document.encoding,
      hasBom: document.hasBom,
      lineEnding: (event.currentTarget as HTMLSelectElement).value as SaveOptions['lineEnding'],
    });
  }
</script>

<nav aria-label="文本文件操作" class="editor-toolbar">
  <div class="action-group">
    <button disabled={!desktopRuntime || saving} onclick={onOpen} title="打开文件（Ctrl+O）" type="button">
      打开
    </button>
    <button
      class:editing
      disabled={!document || saving}
      onclick={onToggleEditing}
      title={editing ? '退出编辑' : '开始编辑'}
      type="button"
    >
      {editing ? '退出编辑' : '开始编辑'}
    </button>
    <button disabled={!document || !editing || saving || document.readOnly} onclick={onSave} title="保存（Ctrl+S）" type="button">
      {saving ? '保存中…' : '保存'}
    </button>
    <button disabled={!document || !editing || saving || !desktopRuntime} onclick={onSaveAs} title="另存为（Ctrl+Shift+S）" type="button">
      另存为
    </button>
    <button disabled={!document || saving} onclick={onClose} title="关闭（Ctrl+W）" type="button">
      关闭
    </button>
  </div>

  <div class="toolbar-spacer"></div>

  <label>
    <span>保存编码</span>
    <select
      aria-label="保存编码"
      disabled={!document || !editing || saving}
      onchange={changeEncoding}
      value={encodingChoice}
    >
      <option value="utf8">UTF-8</option>
      <option value="utf8Bom">UTF-8 BOM</option>
      <option value="utf16Le">UTF-16 LE BOM</option>
      <option value="utf16Be">UTF-16 BE BOM</option>
      <option value="gbk">GBK</option>
      <option value="gb18030">GB18030</option>
    </select>
  </label>

  <label>
    <span>换行符</span>
    <select
      aria-label="保存换行符"
      disabled={!document || !editing || saving}
      onchange={changeLineEnding}
      value={document?.lineEndingChoice ?? 'preserve'}
    >
      <option disabled={document?.lineEnding === 'mixed'} value="preserve">
        {document?.lineEnding === 'mixed' ? 'Mixed（请选择）' : '保留原格式'}
      </option>
      <option value="crlf">CRLF</option>
      <option value="lf">LF</option>
    </select>
  </label>

  <button class="reload-button" disabled={!document || !editing || saving} onclick={onReopen} title="按当前选择的编码重新解码文件" type="button">
    按编码重载
  </button>
</nav>

<style>
  .editor-toolbar {
    align-items: center;
    background: var(--surface-chrome);
    border-bottom: 1px solid var(--border-subtle);
    display: flex;
    gap: 14px;
    min-height: 45px;
    padding: 6px 10px;
  }

  .action-group {
    display: flex;
    gap: 5px;
  }

  button,
  select {
    background: var(--surface-control);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    font: 600 11px/1 var(--font-ui);
    min-height: 30px;
  }

  button {
    padding: 0 12px;
  }

  button.editing {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent-strong);
  }

  .reload-button {
    white-space: nowrap;
  }

  button:hover:not(:disabled),
  select:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--text-primary);
  }

  button:disabled,
  select:disabled {
    color: var(--text-disabled);
    cursor: default;
  }

  .toolbar-spacer {
    flex: 1;
  }

  label {
    align-items: center;
    color: var(--text-tertiary);
    display: flex;
    font: 500 10px/1 var(--font-ui);
    gap: 7px;
  }

  select {
    padding: 0 24px 0 8px;
  }

  @media (max-width: 760px) {
    label span {
      display: none;
    }
  }
</style>
