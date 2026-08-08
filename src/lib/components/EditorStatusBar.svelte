<script lang="ts">
  import type { DocumentSession, EditorStatistics } from '../types/document';
  import { isDirty } from '../types/document';

  export let document: DocumentSession | null;
  export let statistics: EditorStatistics = { lines: 0, characters: 0 };
  export let saving = false;

  const lineEndingLabels = {
    crlf: 'CRLF',
    lf: 'LF',
    cr: 'CR',
    mixed: 'Mixed',
    none: '无换行',
  } as const;

  $: encodingLabel = document
    ? `${document.encoding === 'utf8' ? 'UTF-8' : document.encoding === 'utf16Le' ? 'UTF-16 LE' : document.encoding === 'utf16Be' ? 'UTF-16 BE' : document.encoding.toUpperCase()}${document.hasBom ? ' BOM' : ''}`
    : '—';
</script>

<footer class="editor-statusbar">
  <span class:dirty={isDirty(document)}>
    {saving ? '正在安全保存' : isDirty(document) ? '未保存' : document ? '已保存' : '就绪'}
  </span>
  <div class="status-spacer"></div>
  {#if document}
    <span>{statistics.lines} 行</span>
    <span>{statistics.characters} 字符</span>
    <span>{lineEndingLabels[document.lineEnding]}</span>
    <span>{encodingLabel}</span>
    <span>{document.sizeBytes < 1024 ? `${document.sizeBytes} B` : `${(document.sizeBytes / 1024).toFixed(1)} KiB`}</span>
  {:else}
    <span>本地优先</span>
  {/if}
</footer>

<style>
  .editor-statusbar {
    align-items: center;
    background: var(--surface-chrome);
    border-top: 1px solid var(--border-subtle);
    color: var(--text-tertiary);
    display: flex;
    font: 500 10px/1 var(--font-ui);
    gap: 18px;
    padding: 0 12px;
  }

  .dirty {
    color: var(--warning);
    font-weight: 650;
  }

  .status-spacer {
    flex: 1;
  }
</style>
