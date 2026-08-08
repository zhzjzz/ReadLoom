<script lang="ts">
  import Icon, { type IconName } from './Icon.svelte';
  import type { RecentDocumentDto } from '../types/epub';

  export let desktopRuntime = true;
  export let onOpen: () => void;
  export let recentDocuments: RecentDocumentDto[] = [];
  export let onOpenRecent: (document: RecentDocumentDto) => void = () => {};

  const sections: Array<{
    label: string;
    items: Array<{ label: string; icon: IconName; active?: boolean; disabled?: boolean }>;
  }> = [
    {
      label: '工作区',
      items: [
        { label: 'TXT 编辑器', icon: 'document', active: true },
        { label: '书库', icon: 'library', disabled: true },
      ],
    },
    {
      label: '阶段 1',
      items: [
        { label: '安全保存', icon: 'check' },
        { label: '只读大文件', icon: 'settings', disabled: true },
      ],
    },
  ];
</script>

<aside aria-label="主导航" class="navigation-pane">
  <div class="open-area">
    <button class="open-button" disabled={!desktopRuntime} onclick={onOpen} type="button">
      <span>打开文件</span>
    </button>
  </div>
  <div class="nav-scroll">
    {#if recentDocuments.length}
      <section class="recent-section">
        <h2>最近文件</h2>
        <div class="recent-items">
          {#each recentDocuments as document}
            <button onclick={() => onOpenRecent(document)} title={document.path} type="button">
              <span class="recent-kind">{document.documentKind.toUpperCase()}</span>
              <span class="recent-copy"><strong>{document.displayTitle}</strong>{#if document.author}<small>{document.author}</small>{/if}</span>
            </button>
          {/each}
        </div>
      </section>
    {/if}
    {#each sections as section}
      <section>
        <h2>{section.label}</h2>
        <div class="nav-items">
          {#each section.items as item}
            <button
              aria-current={item.active ? 'page' : undefined}
              class:active={item.active}
              disabled={item.disabled}
              title={item.disabled ? `${item.label}将在后续阶段提供` : item.label}
              type="button"
            >
              <Icon name={item.icon} size={18} />
              <span>{item.label}</span>
            </button>
          {/each}
        </div>
      </section>
    {/each}
  </div>

  <div class="local-status">
    <span class="status-dot"></span>
    <span>本地优先</span>
  </div>
</aside>

<style>
  .navigation-pane {
    background: var(--surface-pane);
    border-right: 1px solid var(--border-subtle);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .nav-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 19px 10px;
  }

  .open-area {
    border-bottom: 1px solid var(--border-subtle);
    padding: 11px 10px;
    display: grid;
  }

  .open-button {
    background: var(--accent);
    color: white;
    justify-content: center;
  }

  .open-button:hover:not(:disabled) {
    background: var(--accent-strong);
    color: white;
  }

  section + section {
    border-top: 1px solid var(--border-subtle);
    margin-top: 17px;
    padding-top: 17px;
  }

  h2 {
    color: var(--text-tertiary);
    font: 600 11px/1.2 var(--font-ui);
    letter-spacing: 0.04em;
    margin: 0 9px 8px;
    text-transform: uppercase;
  }

  .nav-items {
    display: grid;
    gap: 3px;
  }

  .recent-items {
    display: grid;
    gap: 3px;
  }

  .recent-items button {
    gap: 7px;
    min-width: 0;
  }

  .recent-kind {
    color: var(--text-disabled);
    flex: 0 0 28px;
    font: 700 8px/1 var(--font-mono);
  }

  .recent-copy {
    display: grid;
    min-width: 0;
  }

  .recent-copy strong,
  .recent-copy small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .recent-copy strong {
    font: 550 11px/1.3 var(--font-ui);
  }

  .recent-copy small {
    color: var(--text-tertiary);
    font: 400 9px/1.25 var(--font-ui);
  }

  button {
    align-items: center;
    background: transparent;
    border: 0;
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    display: flex;
    font: 500 13px/1 var(--font-ui);
    gap: 10px;
    min-height: 36px;
    padding: 0 10px;
    text-align: left;
    width: 100%;
  }

  button:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--text-primary);
  }

  button.active {
    background: var(--accent-soft);
    color: var(--accent-strong);
    position: relative;
  }

  button.active::before {
    background: var(--accent);
    border-radius: 999px;
    bottom: 8px;
    content: '';
    left: 0;
    position: absolute;
    top: 8px;
    width: 2px;
  }

  button:disabled {
    color: var(--text-disabled);
    cursor: default;
  }

  .local-status {
    align-items: center;
    border-top: 1px solid var(--border-subtle);
    color: var(--text-tertiary);
    display: flex;
    font: 500 11px/1 var(--font-ui);
    gap: 8px;
    min-height: 44px;
    padding: 0 18px;
  }

  .status-dot {
    background: var(--success);
    border-radius: 999px;
    height: 7px;
    width: 7px;
  }

  @media (max-width: 1050px) {
    .nav-scroll {
      padding-inline: 8px;
    }

    h2,
    button span,
    .local-status span:last-child {
      display: none;
    }

    button,
    .local-status {
      justify-content: center;
      padding-inline: 0;
    }
  }
</style>
