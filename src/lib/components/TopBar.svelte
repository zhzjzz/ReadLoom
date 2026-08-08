<script lang="ts">
  import type { BackendConnection } from '../types/ipc';
  import type { DocumentSession } from '../types/document';
  import { isDirty } from '../types/document';
  import type { WorkspaceTabSummary } from '../types/workspace';
  import Icon from './Icon.svelte';

  export let connection: BackendConnection;
  export let document: DocumentSession | null = null;
  export let displayTitle: string | null = null;
  export let displayPath: string | null = null;
  export let hasDocument = false;
  export let onClose: () => void = () => {};
  export let tabs: WorkspaceTabSummary[] = [];
  export let activeTabId: string | null = null;
  export let onSelectTab: (tabId: string) => void = () => {};
  export let onCloseTab: (tabId: string) => void = () => {};

  $: connectionLabel =
    connection.status === 'connected'
      ? 'Rust 已连接'
      : connection.status === 'checking'
        ? '正在连接'
        : connection.status === 'browser-preview'
          ? '浏览器预览'
          : '连接异常';
  $: resolvedTitle = displayTitle ?? document?.fileName ?? '文档工作区';
  $: resolvedPath = displayPath ?? document?.displayPath ?? '文档工作区';
  $: documentOpen = hasDocument || Boolean(document);
</script>

<header class="topbar">
  <div class="brand" aria-label="Readloom 阅织">
    <span class="brand-mark" aria-hidden="true">R</span>
    <span class="brand-name">Readloom</span>
    <span class="brand-cn">阅织</span>
  </div>

  {#if tabs.length}
    <div aria-label="文档标签" class="tabs-strip">
      {#each tabs as item}
        <div class:active-tab={item.id === activeTabId} class="tab" title={item.path}>
          <button class="tab-select" onclick={() => onSelectTab(item.id)} type="button">
            <span class="tab-kind">{item.kind.toUpperCase()}</span>
            <span class="tab-title">{item.title}</span>
            {#if item.detail}<span class="tab-detail">{item.detail}</span>{/if}
            {#if item.dirty}<span aria-label="未保存" class="dirty-mark">●</span>{/if}
          </button>
          <button aria-label={`关闭 ${item.title}`} class="tab-close" onclick={() => onCloseTab(item.id)} title="关闭文档" type="button">×</button>
        </div>
      {/each}
    </div>
  {:else}
    <div class="active-tab tab" aria-current="page" title={resolvedPath}>
      <Icon name="document" size={16} />
      <span class="tab-title">{resolvedTitle}</span>
      {#if isDirty(document)}<span aria-label="未保存" class="dirty-mark">●</span>{/if}
      {#if documentOpen}
        <button aria-label={`关闭 ${resolvedTitle}`} class="tab-close" onclick={onClose} title="关闭文档" type="button">×</button>
      {/if}
    </div>
  {/if}

  <div class="topbar-spacer"></div>

  <div class="connection-summary" data-status={connection.status}>
    <span class="status-dot"></span>
    <span>{connectionLabel}</span>
  </div>
</header>

<style>
  .topbar {
    align-items: stretch;
    background: var(--surface-chrome);
    border-bottom: 1px solid var(--border-subtle);
    display: flex;
    min-width: 0;
  }

  .brand {
    align-items: center;
    border-right: 1px solid var(--border-subtle);
    display: flex;
    gap: 9px;
    padding: 0 17px;
    width: var(--left-pane-width);
  }

  .brand-mark {
    align-items: center;
    background: var(--text-primary);
    border-radius: 4px;
    color: var(--surface-canvas);
    display: inline-flex;
    font: 650 12px/1 var(--font-ui);
    height: 22px;
    justify-content: center;
    letter-spacing: -0.04em;
    width: 22px;
  }

  .brand-name {
    color: var(--text-primary);
    font: 600 14px/1 var(--font-ui);
  }

  .brand-cn {
    color: var(--text-tertiary);
    font: 500 12px/1 var(--font-ui);
  }

  .tab {
    align-items: center;
    border-right: 1px solid var(--border-subtle);
    color: var(--accent-strong);
    display: flex;
    font: 600 13px/1 var(--font-ui);
    gap: 8px;
    flex: 0 0 auto;
    max-width: min(340px, 34vw);
    min-width: 0;
    padding: 0 18px;
    position: relative;
  }

  .tabs-strip {
    display: flex;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: thin;
  }

  .tab-select {
    align-items: center;
    background: transparent;
    border: 0;
    color: inherit;
    display: flex;
    font: inherit;
    gap: 7px;
    min-width: 0;
    padding: 0;
  }

  .tab-kind {
    color: var(--text-disabled);
    font: 700 8px/1 var(--font-mono);
    letter-spacing: 0.04em;
  }

  .tab-detail {
    color: var(--text-tertiary);
    font: 500 9px/1 var(--font-ui);
    max-width: 84px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tab-title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tab::after {
    display: none;
  }

  .tab.active-tab::after {
    background: var(--accent);
    bottom: -1px;
    content: '';
    height: 2px;
    left: 0;
    position: absolute;
    right: 0;
    display: block;
  }

  .tab:not(.active-tab) {
    color: var(--text-secondary);
  }

  .dirty-mark {
    color: var(--warning);
    font-size: 8px;
  }

  .tab-close {
    align-items: center;
    background: transparent;
    border: 0;
    border-radius: var(--radius-sm);
    color: var(--text-tertiary);
    display: inline-flex;
    font: 500 17px/1 var(--font-ui);
    height: 24px;
    justify-content: center;
    margin-left: 1px;
    padding: 0;
    flex: 0 0 auto;
    width: 24px;
  }

  .tab-close:hover {
    background: var(--surface-hover);
    color: var(--text-primary);
  }

  .topbar-spacer {
    flex: 1;
  }

  .connection-summary {
    align-items: center;
    color: var(--text-tertiary);
    display: flex;
    font: 500 11px/1 var(--font-ui);
    gap: 7px;
    padding: 0 16px;
  }

  .status-dot {
    background: var(--warning);
    border-radius: 999px;
    height: 7px;
    width: 7px;
  }

  [data-status='connected'] .status-dot {
    background: var(--success);
  }

  [data-status='error'] .status-dot {
    background: var(--danger);
  }

  [data-status='browser-preview'] .status-dot {
    background: var(--text-tertiary);
  }

  @media (max-width: 1050px) {
    .brand {
      justify-content: center;
      padding: 0;
    }

    .brand-name,
    .brand-cn {
      display: none;
    }
  }

  @media (max-width: 780px) {
    .connection-summary {
      display: none;
    }
  }
</style>
