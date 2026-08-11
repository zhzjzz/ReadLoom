<script lang="ts">
  import Icon, { type IconName } from './Icon.svelte';

  export let desktopRuntime = true;
  export let onOpen: () => void;
  export let activeView: 'workspace' | 'library' | 'settings' = 'workspace';
  export let onSelectWorkspace: () => void = () => {};
  export let onSelectLibrary: () => void = () => {};
  export let onSelectSettings: () => void = () => {};

  const sections: Array<{
    label: string;
    items: Array<{ id: 'workspace' | 'library' | 'settings'; label: string; icon: IconName }>;
  }> = [
    {
      label: '工作区',
      items: [
        { id: 'library', label: '书库', icon: 'library' },
        { id: 'workspace', label: '阅读与编辑', icon: 'document' },
        { id: 'settings', label: '设置', icon: 'settings' },
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
    {#each sections as section}
      <section>
        <h2>{section.label}</h2>
        <div class="nav-items">
          {#each section.items as item}
            <button
              aria-current={activeView === item.id ? 'page' : undefined}
              class:active={activeView === item.id}
              onclick={item.id === 'workspace'
                ? onSelectWorkspace
                : item.id === 'library' ? onSelectLibrary : onSelectSettings}
              title={item.label}
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

</style>
