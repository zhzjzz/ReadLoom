<script lang="ts">
  import { resolvedTheme, type ThemePreference } from '../stores/theme';
  import ThemeControl from './ThemeControl.svelte';

  export let headingPattern: string;
  export let headingPatternError: string | null = null;
  export let theme: ThemePreference;
  export let onClose: () => void;
  export let onHeadingPatternChange: (pattern: string) => void;
  export let onResetHeadingPattern: () => void;
  export let onThemeChange: (preference: ThemePreference) => void;
</script>

<aside aria-label="设置" class="inspector-pane">
  <header>
    <h2>设置</h2>
    <button aria-label="关闭设置" onclick={onClose} title="关闭设置" type="button">×</button>
  </header>

  <section>
    <div class="section-title">
      <h3>外观</h3>
      <span>当前 {$resolvedTheme === 'dark' ? '暗色' : '亮色'}</span>
    </div>
    <ThemeControl value={theme} onChange={onThemeChange} />
  </section>

  <section>
    <div class="section-title"><h3>TXT 标题识别</h3></div>
    <p class="description">每行按此正则识别目录标题；自动使用多行和全局匹配。</p>
    <textarea
      aria-invalid={headingPatternError ? 'true' : 'false'}
      aria-label="TXT 标题识别正则"
      oninput={(event) => onHeadingPatternChange(event.currentTarget.value)}
      rows="8"
      spellcheck="false"
      value={headingPattern}
    ></textarea>
    {#if headingPatternError}
      <p class="pattern-error" role="alert">{headingPatternError}</p>
    {/if}
    <button class="reset-button" onclick={onResetHeadingPattern} type="button">恢复默认规则</button>
  </section>
</aside>

<style>
  .inspector-pane {
    background: var(--surface-pane);
    border-left: 1px solid var(--border-subtle);
    height: 100%;
    min-height: 0;
    overflow-y: auto;
  }

  header {
    align-items: center;
    border-bottom: 1px solid var(--border-subtle);
    display: flex;
    justify-content: space-between;
    min-height: 52px;
    padding: 0 18px;
  }

  header h2 {
    color: var(--text-primary);
    font: 650 14px/1 var(--font-ui);
    margin: 0;
  }

  header button {
    background: transparent;
    border: 0;
    border-radius: var(--radius-sm);
    color: var(--text-tertiary);
    font: 500 18px/1 var(--font-ui);
    height: 30px;
    width: 30px;
  }

  header button:hover { background: var(--surface-hover); color: var(--text-primary); }
  section { padding: 18px; }
  section + section { border-top: 1px solid var(--border-subtle); }

  .section-title {
    align-items: baseline;
    display: flex;
    justify-content: space-between;
    margin-bottom: 12px;
  }

  h3 { color: var(--text-primary); font: 650 12px/1 var(--font-ui); margin: 0; }
  .section-title span { color: var(--text-tertiary); font: 500 10px/1 var(--font-ui); }

  .description {
    color: var(--text-tertiary);
    font: 400 11px/1.5 var(--font-ui);
    margin: 0 0 10px;
  }

  textarea {
    background: var(--surface-control);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    font: 500 10px/1.5 var(--font-mono);
    padding: 9px;
    resize: vertical;
    width: 100%;
  }

  textarea[aria-invalid='true'] { border-color: var(--danger); }
  .pattern-error { color: var(--danger); font: 500 10px/1.45 var(--font-ui); margin: 7px 0 0; }

  .reset-button {
    background: var(--surface-control);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    font: 600 11px/1 var(--font-ui);
    margin-top: 10px;
    min-height: 32px;
    padding: 0 11px;
  }

  .reset-button:hover { background: var(--surface-hover); color: var(--text-primary); }
</style>
