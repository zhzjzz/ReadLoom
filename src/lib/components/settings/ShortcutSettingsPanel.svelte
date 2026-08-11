<script lang="ts">
  import { shortcutFromEvent, shortcutLabels } from '../../services/shortcuts';
  import { shortcutActionIds } from '../../stores/appSettings';
  import type { ShortcutActionId, ShortcutSettings } from '../../types/settings';

  export let value: ShortcutSettings;
  export let onChange: (value: ShortcutSettings) => void = () => {};
  let capturing: ShortcutActionId | null = null;
  let conflict: string | null = null;

  function capture(action: ShortcutActionId, event: KeyboardEvent): void {
    event.preventDefault();
    event.stopPropagation();
    if (event.key === 'Escape') {
      capturing = null;
      conflict = null;
      return;
    }
    if (event.key === 'Backspace' || event.key === 'Delete') {
      setShortcut(action, null);
      return;
    }
    const shortcut = shortcutFromEvent(event);
    if (!shortcut) return;
    const duplicate = shortcutActionIds.find((id) => id !== action && value[id] === shortcut);
    if (duplicate) {
      conflict = `“${shortcut}”已用于“${shortcutLabels[duplicate]}”。`;
      return;
    }
    setShortcut(action, shortcut);
  }

  function setShortcut(action: ShortcutActionId, shortcut: string | null): void {
    onChange({ ...value, [action]: shortcut });
    capturing = null;
    conflict = null;
  }

  function resetAll(): void {
    onChange(Object.fromEntries(shortcutActionIds.map((id) => [id, null])) as ShortcutSettings);
    capturing = null;
    conflict = null;
  }
</script>

<div class="shortcuts-panel">
  <div class="panel-heading"><div><h3>快捷键</h3><p>默认全部为“无”，点击右侧按键框后直接按下新组合。</p></div><button onclick={resetAll} type="button">全部清除</button></div>
  <div class="shortcut-list">
    {#each shortcutActionIds as action}
      <div class="shortcut-row">
        <span>{shortcutLabels[action]}</span>
        <button
          aria-label={`${shortcutLabels[action]}快捷键`}
          class:capturing={capturing === action}
          onblur={() => (capturing = null)}
          onclick={() => { capturing = action; conflict = null; }}
          onkeydown={(event) => capture(action, event)}
          type="button"
        >{capturing === action ? '请按下组合键…' : value[action] ?? '无'}</button>
        <button aria-label={`清除${shortcutLabels[action]}快捷键`} class="clear" disabled={!value[action]} onclick={() => setShortcut(action, null)} type="button">清除</button>
      </div>
    {/each}
  </div>
  {#if conflict}<p class="conflict" role="alert">{conflict}</p>{/if}
  <p class="tip">Esc 取消录入；Backspace 或 Delete 清除当前快捷键。输入法组合期间不会触发应用快捷键。</p>
</div>

<style>
  .shortcuts-panel { margin:0 auto; max-width:860px; }
  .panel-heading { align-items:flex-start; display:flex; justify-content:space-between; margin-bottom:16px; }
  h3 { color:var(--text-primary); font:680 15px/1.2 var(--font-ui); margin:0; }
  p { color:var(--text-tertiary); font:500 10px/1.5 var(--font-ui); margin:6px 0 0; }
  .panel-heading button, .shortcut-row button { background:var(--surface-control); border:1px solid var(--border-strong); border-radius:8px; color:var(--text-secondary); font:620 10px/1 var(--font-ui); min-height:33px; padding:0 13px; }
  .shortcut-list { border-top:1px solid var(--border-subtle); }
  .shortcut-row { align-items:center; border-bottom:1px solid var(--border-subtle); display:grid; gap:12px; grid-template-columns:minmax(180px,1fr) 190px 56px; min-height:58px; }
  .shortcut-row > span { color:var(--text-secondary); font:600 11px/1.2 var(--font-ui); }
  .shortcut-row button.capturing { border-color:var(--accent); box-shadow:0 0 0 3px var(--accent-focus); color:var(--accent-strong); }
  .shortcut-row button.clear { background:transparent; border:0; color:var(--text-tertiary); padding:0; }
  button:disabled { color:var(--text-disabled); }
  .conflict { color:var(--danger); }
  .tip { margin-top:14px; }
  @media (max-width:620px) { .shortcut-row { grid-template-columns:1fr 150px 48px; } }
</style>
