<script lang="ts">
  import type { ThemePreference } from '../stores/theme';
  import Icon, { type IconName } from './Icon.svelte';

  export let value: ThemePreference;
  export let onChange: (preference: ThemePreference) => void;

  const options: Array<{ value: ThemePreference; label: string; icon: IconName }> = [
    { value: 'system', label: '跟随系统', icon: 'monitor' },
    { value: 'light', label: '亮色', icon: 'sun' },
    { value: 'dark', label: '暗色', icon: 'moon' },
  ];
</script>

<div aria-label="应用主题" class="theme-control" role="radiogroup">
  {#each options as option}
    <button
      aria-checked={value === option.value}
      class:active={value === option.value}
      onclick={() => onChange(option.value)}
      role="radio"
      type="button"
    >
      <Icon name={option.icon} size={17} />
      <span>{option.label}</span>
    </button>
  {/each}
</div>

<style>
  .theme-control {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 6px;
  }

  button {
    align-items: center;
    background: var(--surface-control);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    display: flex;
    flex-direction: column;
    font: 500 12px/1.2 var(--font-ui);
    gap: 7px;
    min-height: 62px;
    justify-content: center;
    padding: 8px 5px;
  }

  button:hover {
    background: var(--surface-hover);
    color: var(--text-primary);
  }

  button.active {
    background: var(--accent-soft);
    border-color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent);
    color: var(--accent-strong);
  }
</style>
