<script lang="ts">
  import type { BooksBackupResultDto, BooksRestoreResultDto } from '../types/backup';
  import type { ThemePreference } from '../stores/theme';
  import { defaultReadingTypographySettings } from '../stores/appSettings';
  import {
    readingFontOptions,
    type AppSettings,
    type LibraryColumns,
    type ReadingFont,
    type ReadingTypographySettings,
    type ShortcutSettings,
  } from '../types/settings';
  import ThemeControl from './ThemeControl.svelte';
  import BackgroundReadabilityControl from './settings/BackgroundReadabilityControl.svelte';
  import BackupSettingsPanel from './settings/BackupSettingsPanel.svelte';
  import ReadingTypographyPanel from './settings/ReadingTypographyPanel.svelte';
  import ShortcutSettingsPanel from './settings/ShortcutSettingsPanel.svelte';

  export let settings: AppSettings;
  export let backgroundKey: string | null = null;
  export let backgroundUrl: string | null = null;
  export let theme: ThemePreference;
  export let headingPattern: string;
  export let headingPatternError: string | null = null;
  export let backupPath: string | null = null;
  export let backupResult: BooksBackupResultDto | null = null;
  export let restoreResult: BooksRestoreResultDto | null = null;
  export let backupBusy = false;
  export let onSettingsChange: (patch: Partial<AppSettings>) => void = () => {};
  export let onThemeChange: (preference: ThemePreference) => void = () => {};
  export let onChooseBackground: () => void = () => {};
  export let onClearBackground: () => void = () => {};
  export let onHeadingPatternChange: (pattern: string) => void = () => {};
  export let onResetHeadingPattern: () => void = () => {};
  export let onChooseBackupPath: () => void = () => {};
  export let onCreateBackup: () => void = () => {};
  export let onRestoreBackup: () => void = () => {};

  const sections = [
    { label: '外观', items: [['appearance.theme', '主题'], ['appearance.font', '字体'], ['appearance.layout', '页面布局']] },
    { label: '阅读', items: [['reading.typography', '阅读排版']] },
    { label: '操作', items: [['controls.shortcuts', '快捷键']] },
    { label: '书籍', items: [['books.chapters', '章节识别']] },
    { label: '数据', items: [['data.backup', '备份']] },
    { label: '高级', items: [['advanced.association', '文件关联'], ['advanced.cache', '缓存'], ['advanced.hardware', '硬件加速'], ['advanced.dpi', 'DPI']] },
  ] as const;

  type SettingItemId = (typeof sections)[number]['items'][number][0];
  let selected: SettingItemId = 'reading.typography';

  $: selectedSection = sections.find((section) => section.items.some(([id]) => id === selected));
  $: selectedLabel = selectedSection?.items.find(([id]) => id === selected)?.[1] ?? '设置';
  $: selectedGroup = selectedSection?.label ?? '设置';

  function updateReading(value: ReadingTypographySettings): void {
    onSettingsChange({ reading: value });
  }

  function updateFont(fontFamily: ReadingFont): void {
    updateReading({ ...settings.reading, fontFamily });
  }

  function updateShortcuts(shortcuts: ShortcutSettings): void {
    onSettingsChange({ shortcuts });
  }

  function resetReading(): void {
    updateReading({
      ...defaultReadingTypographySettings,
      txt: { ...defaultReadingTypographySettings.txt },
      epub: { ...defaultReadingTypographySettings.epub },
    });
  }

  function placeholderDescription(id: SettingItemId): string {
    const descriptions: Partial<Record<SettingItemId, string>> = {
      'advanced.association': '当前不会自动修改 Windows 文件关联，避免覆盖用户现有选择。',
      'advanced.cache': 'EPUB 资源按章节读取，封面与背景通过受控本地协议提供。',
      'advanced.hardware': '当前遵循 WebView2 系统硬件加速策略。',
      'advanced.dpi': '界面遵循 Windows DPI 缩放，并在窄窗口自动收拢。',
    };
    return descriptions[id] ?? '当前使用内置安全策略。';
  }
</script>

<section aria-label="设置" class="settings-view">
  <aside aria-label="设置分类" class="settings-navigation">
    <h1>设置</h1>
    <div class="settings-scroll">
      {#each sections as section}
        <section>
          <h2>{section.label}</h2>
          <div>
            {#each section.items as item}
              <button aria-current={selected === item[0] ? 'page' : undefined} class:active={selected === item[0]} onclick={() => (selected = item[0])} type="button">{item[1]}</button>
            {/each}
          </div>
        </section>
      {/each}
    </div>
  </aside>

  <div class="settings-detail">
    <header>
      <h2>{selectedGroup} / {selectedLabel}</h2>
      <p>{selected === 'reading.typography' ? '统一设置字体、段落、页面，并分别控制 TXT 预处理与 EPUB 样式覆盖。' : selected === 'data.backup' ? '只备份书籍内容；恢复多个备份时按内容去重。' : '修改会立即保存到本机。'}</p>
    </header>

    {#if selected === 'appearance.theme'}
      <div class="setting-section">
        <div class="section-heading"><div><h3>应用主题</h3><p>选择亮色、暗色或跟随 Windows。</p></div></div>
        <ThemeControl value={theme} onChange={onThemeChange} />
      </div>
      <div class="setting-section">
        <div class="section-heading"><div><h3>背景与可读性</h3><p>与“页面布局”共用同一份背景设置，背景会延伸到正文下方。</p></div></div>
        <BackgroundReadabilityControl {backgroundKey} {backgroundUrl} opacity={settings.backgroundOpacity} onChoose={onChooseBackground} onClear={onClearBackground} onOpacityChange={(backgroundOpacity) => onSettingsChange({ backgroundOpacity })} />
      </div>
    {:else if selected === 'appearance.font'}
      <div class="setting-section font-section">
        <div class="font-list">
          {#each readingFontOptions as option}
            <button aria-pressed={settings.reading.fontFamily === option.id} class:active={settings.reading.fontFamily === option.id} onclick={() => updateFont(option.id)} type="button">
              <span style={`font-family:${option.stack}`}><strong>{option.label}</strong><em>山高水长 · Readloom</em></span>
              <small>{option.license}</small>
            </button>
          {/each}
        </div>
        <p class="font-note">推荐安装思源宋体、思源黑体、Noto CJK 或霞鹜文楷。出现生僻字缺字时会自动回退到系统中文字体。</p>
      </div>
    {:else if selected === 'appearance.layout'}
      <div class="setting-section">
        <div class="section-heading"><div><h3>书库布局</h3><p>封面标题字数和字号会随每行本数自动调整。</p></div></div>
        <div class="setting-row"><div><strong>书库每行显示</strong><span>可在 3 到 5 本之间选择。</span></div><div aria-label="书库每行显示" class="segmented" role="radiogroup">{#each [3,4,5] as columns}<button aria-checked={settings.libraryColumns === columns} class:active={settings.libraryColumns === columns} onclick={() => onSettingsChange({ libraryColumns: columns as LibraryColumns })} role="radio" type="button">{columns} 本</button>{/each}</div></div>
      </div>
      <div class="setting-section">
        <div class="section-heading"><div><h3>背景与可读性</h3><p>与“主题”共用同一份设置，在任一位置修改都会同步。</p></div></div>
        <BackgroundReadabilityControl {backgroundKey} {backgroundUrl} opacity={settings.backgroundOpacity} onChoose={onChooseBackground} onClear={onClearBackground} onOpacityChange={(backgroundOpacity) => onSettingsChange({ backgroundOpacity })} />
      </div>
      <div class="setting-section">
        <div class="section-heading"><div><h3>窗口行为</h3><p>托盘图标可用于隐藏后重新显示 Readloom。</p></div></div>
        <div class="setting-row"><div><strong>关闭窗口时</strong><span>有未保存修改时仍会先请求确认。</span></div><div aria-label="关闭窗口行为" class="segmented" role="radiogroup"><button aria-checked={settings.closeAction === 'exit'} class:active={settings.closeAction === 'exit'} onclick={() => onSettingsChange({ closeAction:'exit' })} role="radio" type="button">退出</button><button aria-checked={settings.closeAction === 'tray'} class:active={settings.closeAction === 'tray'} onclick={() => onSettingsChange({ closeAction:'tray' })} role="radio" type="button">最小化到托盘</button></div></div>
        <label class="switch-row"><span><strong>最小化到托盘</strong><small>点击系统最小化按钮时隐藏到托盘。</small></span><input checked={settings.minimizeToTray} onchange={(event) => onSettingsChange({ minimizeToTray:event.currentTarget.checked })} type="checkbox" /></label>
      </div>
    {:else if selected === 'reading.typography'}
      <ReadingTypographyPanel value={settings.reading} {backgroundUrl} backgroundOpacity={settings.backgroundOpacity} onChange={updateReading} onReset={resetReading} />
    {:else if selected === 'controls.shortcuts'}
      <ShortcutSettingsPanel value={settings.shortcuts} onChange={updateShortcuts} />
    {:else if selected === 'books.chapters'}
      <div class="setting-section">
        <div class="section-heading"><div><h3>TXT 章节识别</h3><p>这是结构识别规则，不负责删除广告或更改正文排版。</p></div></div>
        <textarea aria-invalid={headingPatternError ? 'true' : 'false'} aria-label="TXT 标题识别正则" oninput={(event) => onHeadingPatternChange(event.currentTarget.value)} rows="8" spellcheck="false" value={headingPattern}></textarea>
        {#if headingPatternError}<p class="pattern-error" role="alert">{headingPatternError}</p>{/if}
        <button class="reset-button" onclick={onResetHeadingPattern} type="button">恢复默认规则</button>
      </div>
    {:else if selected === 'data.backup'}
      <BackupSettingsPanel {backupPath} {backupResult} {restoreResult} busy={backupBusy} onChooseBackupPath={onChooseBackupPath} onCreateBackup={onCreateBackup} onRestore={onRestoreBackup} />
    {:else}
      <div class="placeholder-section"><div aria-hidden="true">R</div><h3>{selectedLabel}</h3><p>{placeholderDescription(selected)}</p><span>高级项继续使用安全默认值。</span></div>
    {/if}
  </div>
</section>

<style>
  .settings-view { background:color-mix(in srgb,var(--surface-canvas) 96%,transparent); display:grid; grid-template-columns:220px minmax(0,1fr); height:100%; min-height:0; }
  .settings-navigation { background:color-mix(in srgb,var(--surface-pane) 97%,transparent); border-right:1px solid var(--border-subtle); display:flex; flex-direction:column; min-height:0; padding:20px 12px; }
  .settings-navigation > h1 { color:var(--text-primary); font:720 22px/1.2 var(--font-ui); letter-spacing:-.025em; margin:0 10px 18px; }
  .settings-scroll { overflow:auto; padding-right:3px; }
  .settings-navigation section + section { margin-top:16px; }
  .settings-navigation section h2 { color:var(--text-tertiary); font:700 10px/1 var(--font-ui); letter-spacing:.08em; margin:0 10px 6px; }
  .settings-navigation section > div { display:grid; gap:2px; }
  .settings-navigation button { background:transparent; border:0; border-radius:8px; color:var(--text-secondary); font:560 12px/1 var(--font-ui); min-height:33px; padding:0 11px; position:relative; text-align:left; }
  .settings-navigation button:hover { background:var(--surface-hover); color:var(--text-primary); }
  .settings-navigation button.active { background:var(--accent-soft); color:var(--accent-strong); font-weight:670; }
  .settings-navigation button.active::before { background:var(--accent); border-radius:99px; content:''; inset-block:8px; left:0; position:absolute; width:3px; }
  .settings-detail { min-width:0; overflow:auto; padding:clamp(26px,3.2vw,48px); }
  .settings-detail > header { margin:0 auto 26px; max-width:1240px; }
  .settings-detail > header h2 { color:var(--text-primary); font:730 25px/1.2 var(--font-ui); letter-spacing:-.025em; margin:0; }
  .settings-detail > header p { color:var(--text-tertiary); font:500 11px/1.5 var(--font-ui); margin:8px 0 0; }
  .setting-section { border-top:1px solid var(--border-subtle); margin:0 auto; max-width:980px; padding:22px 0; }
  .section-heading { margin-bottom:14px; }
  .section-heading h3, .placeholder-section h3 { color:var(--text-primary); font:680 15px/1.3 var(--font-ui); margin:0; }
  .section-heading p { color:var(--text-tertiary); font:500 10px/1.5 var(--font-ui); margin:5px 0 0; }
  .setting-row, .switch-row { align-items:center; display:flex; gap:24px; justify-content:space-between; min-height:66px; }
  .setting-row > div:first-child, .switch-row > span { display:grid; gap:5px; }
  .setting-row strong, .switch-row strong { color:var(--text-secondary); font:620 11px/1.2 var(--font-ui); }
  .setting-row span, .switch-row small { color:var(--text-tertiary); font:500 9px/1.4 var(--font-ui); }
  .segmented { border:1px solid var(--border-strong); border-radius:8px; display:flex; overflow:hidden; }
  .segmented button { background:var(--surface-control); border:0; color:var(--text-secondary); font:620 10px/1 var(--font-ui); min-height:34px; padding:0 18px; }
  .segmented button + button { border-left:1px solid var(--border-strong); }
  .segmented button.active { background:var(--accent); color:white; }
  .switch-row { border-top:1px solid var(--border-subtle); }
  .switch-row input { accent-color:var(--accent); height:18px; width:36px; }
  .font-list { display:grid; gap:1px; }
  .font-list button { align-items:center; background:transparent; border:0; border-bottom:1px solid var(--border-subtle); color:var(--text-secondary); display:flex; justify-content:space-between; min-height:70px; padding:10px 12px; text-align:left; }
  .font-list button:hover { background:var(--surface-hover); }
  .font-list button.active { background:var(--accent-soft); box-shadow:inset 3px 0 var(--accent); }
  .font-list button > span { display:grid; gap:5px; }
  .font-list strong { color:var(--text-primary); font-size:14px; }
  .font-list em { font-size:12px; font-style:normal; }
  .font-list small, .font-note { color:var(--text-tertiary); font:500 9px/1.5 var(--font-ui); }
  .font-note { margin:14px 0 0; }
  textarea { background:var(--surface-control); border:1px solid var(--border-strong); border-radius:8px; color:var(--text-secondary); font:500 11px/1.55 var(--font-mono); padding:12px; resize:vertical; width:100%; }
  textarea[aria-invalid='true'] { border-color:var(--danger); }
  .pattern-error { color:var(--danger); font:500 10px/1.4 var(--font-ui); }
  .reset-button { background:var(--surface-control); border:1px solid var(--border-strong); border-radius:8px; color:var(--text-secondary); font:620 10px/1 var(--font-ui); margin-top:10px; min-height:34px; padding:0 14px; }
  .placeholder-section { align-items:center; border-top:1px solid var(--border-subtle); display:flex; flex-direction:column; margin:0 auto; max-width:940px; padding:80px 20px; text-align:center; }
  .placeholder-section > div { align-items:center; background:var(--accent-soft); border-radius:14px; color:var(--accent-strong); display:flex; font:750 18px/1 var(--font-ui); height:54px; justify-content:center; margin-bottom:17px; width:54px; }
  .placeholder-section p { color:var(--text-secondary); font:500 12px/1.6 var(--font-ui); margin:8px 0; max-width:520px; }
  .placeholder-section span { color:var(--text-disabled); font:500 10px/1.4 var(--font-ui); }
  @media (max-width:820px) { .settings-view { grid-template-columns:180px minmax(0,1fr); } .settings-detail { padding:24px 18px; } }
  @media (max-width:620px) { .settings-view { display:block; overflow:auto; } .settings-navigation { border-bottom:1px solid var(--border-subtle); border-right:0; max-height:235px; } .settings-detail { overflow:visible; } .setting-row, .switch-row { align-items:flex-start; flex-direction:column; padding:13px 0; } }
</style>
