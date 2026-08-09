<script lang="ts">
  import { onMount } from 'svelte';

  interface CompositionLogEntry {
    id: number;
    time: string;
    event: string;
    data: string;
  }

  let text = '';
  let eventId = 0;
  let eventLog: CompositionLogEntry[] = [];
  let viewportWidth = 0;
  let viewportHeight = 0;
  let devicePixelRatio = 1;
  let screenResolution = '—';
  let rootFontSize = '—';

  $: characterCount = Array.from(text).length;

  onMount(() => {
    const updateViewport = () => {
      viewportWidth = window.innerWidth;
      viewportHeight = window.innerHeight;
      devicePixelRatio = window.devicePixelRatio;
      screenResolution = `${window.screen.width} × ${window.screen.height}`;
      rootFontSize = getComputedStyle(document.documentElement).fontSize;
    };

    updateViewport();
    window.addEventListener('resize', updateViewport);
    return () => window.removeEventListener('resize', updateViewport);
  });

  function recordEvent(event: string, data: string | null): void {
    const timestamp = new Date();
    const time = `${timestamp.toLocaleTimeString('zh-CN', { hour12: false })}.${String(
      timestamp.getMilliseconds(),
    ).padStart(3, '0')}`;

    eventLog = [
      { id: ++eventId, time, event, data: data || '—' },
      ...eventLog,
    ].slice(0, 6);
  }

  function recordInputEvent(event: Event): void {
    const inputType = event instanceof InputEvent ? event.inputType : 'input';
    recordEvent('input', inputType);
  }
</script>

<main class="workspace">
  <header class="workspace-header">
    <div>
      <h1>输入与缩放检查</h1>
      <p>验证 WebView2 中的中文组合输入、焦点行为与 DPI 缩放。</p>
    </div>
    <span class="validation-label">WebView2 验证</span>
  </header>

  <section aria-labelledby="ime-heading" class="validation-section ime-section">
    <div class="section-heading">
      <div>
        <h2 id="ime-heading">中文输入测试</h2>
        <p>请使用拼音或五笔输入一段中文，并测试候选词、标点、换行和删除。</p>
      </div>
      <span aria-live="polite">{characterCount} 字符</span>
    </div>

    <textarea
      aria-describedby="ime-help"
      aria-label="中文输入测试区域"
      bind:value={text}
      oncompositionend={(event) => recordEvent('compositionend', event.data)}
      oncompositionstart={(event) => recordEvent('compositionstart', event.data)}
      oncompositionupdate={(event) => recordEvent('compositionupdate', event.data)}
      oninput={recordInputEvent}
      placeholder="在此输入：阅织中文输入测试……"
      spellcheck="false"
    ></textarea>
    <p class="sr-only" id="ime-help">输入事件将显示在下方的组合事件日志中。</p>

    <div class="event-log">
      <div class="event-log-title">
        <h3>组合事件日志</h3>
        <span>Composition Events</span>
      </div>
      <div class="event-table" role="table" aria-label="中文输入组合事件">
        <div class="event-row event-header" role="row">
          <span role="columnheader">时间</span>
          <span role="columnheader">事件</span>
          <span role="columnheader">数据</span>
        </div>
        {#if eventLog.length === 0}
          <div class="empty-log">等待输入事件</div>
        {:else}
          {#each eventLog as entry (entry.id)}
            <div class="event-row" role="row">
              <span role="cell">{entry.time}</span>
              <span role="cell"><code>{entry.event}</code></span>
              <span class="event-data" role="cell">{entry.data}</span>
            </div>
          {/each}
        {/if}
      </div>
    </div>
  </section>

  <section aria-labelledby="dpi-heading" class="validation-section dpi-section">
    <div class="section-heading dpi-title">
      <div>
        <h2 id="dpi-heading">DPI 与窗口诊断</h2>
        <p>标尺宽度为 100 CSS 像素，用于检查不同系统缩放比例下的清晰度。</p>
      </div>
    </div>

    <div class="css-ruler" aria-label="100 CSS 像素标尺">
      <span>0</span>
      <span>100 CSS px</span>
    </div>

    <dl class="diagnostics">
      <div>
        <dt>设备像素比</dt>
        <dd>{devicePixelRatio.toFixed(2)}</dd>
      </div>
      <div>
        <dt>视口尺寸</dt>
        <dd>{viewportWidth} × {viewportHeight}</dd>
      </div>
      <div>
        <dt>屏幕逻辑分辨率</dt>
        <dd>{screenResolution}</dd>
      </div>
      <div>
        <dt>根字号</dt>
        <dd>{rootFontSize}</dd>
      </div>
    </dl>
  </section>
</main>

<style>
  .workspace {
    background: var(--surface-canvas);
    min-width: 0;
    overflow: auto;
    padding: 27px clamp(24px, 3vw, 42px) 36px;
  }

  .workspace-header {
    align-items: start;
    border-bottom: 1px solid var(--border-subtle);
    display: flex;
    justify-content: space-between;
    margin-bottom: 22px;
    padding-bottom: 19px;
  }

  h1 {
    color: var(--text-primary);
    font: 650 25px/1.2 var(--font-ui);
    letter-spacing: -0.02em;
    margin: 0 0 7px;
  }

  .workspace-header p,
  .section-heading p {
    color: var(--text-tertiary);
    font: 400 12px/1.55 var(--font-ui);
    margin: 0;
  }

  .validation-label {
    color: var(--text-tertiary);
    font: 600 11px/1 var(--font-ui);
    padding-top: 7px;
  }

  .validation-section + .validation-section {
    border-top: 1px solid var(--border-subtle);
    margin-top: 24px;
    padding-top: 22px;
  }

  .section-heading {
    align-items: end;
    display: flex;
    justify-content: space-between;
    margin-bottom: 12px;
  }

  h2 {
    color: var(--text-primary);
    font: 650 16px/1.3 var(--font-ui);
    margin: 0 0 4px;
  }

  .section-heading > span {
    color: var(--text-tertiary);
    font: 500 11px/1 var(--font-ui);
  }

  textarea {
    background: var(--surface-input);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    font: 400 15px/1.75 var(--font-content);
    min-height: 150px;
    padding: 13px 15px;
    resize: vertical;
    width: 100%;
  }

  textarea:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-focus);
    outline: none;
  }

  textarea::placeholder {
    color: var(--text-disabled);
  }

  .event-log {
    margin-top: 14px;
  }

  .event-log-title {
    align-items: baseline;
    display: flex;
    gap: 9px;
    margin-bottom: 8px;
  }

  .event-log-title h3 {
    color: var(--text-secondary);
    font: 650 12px/1 var(--font-ui);
    margin: 0;
  }

  .event-log-title span {
    color: var(--text-tertiary);
    font: 500 10px/1 var(--font-ui);
  }

  .event-table {
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    min-height: 116px;
    overflow: hidden;
  }

  .event-row {
    border-top: 1px solid var(--border-subtle);
    display: grid;
    font: 400 11px/1.25 var(--font-ui);
    grid-template-columns: 118px minmax(150px, 0.7fr) minmax(120px, 1fr);
    min-height: 28px;
  }

  .event-row:first-child {
    border-top: 0;
  }

  .event-row > * {
    align-items: center;
    border-right: 1px solid var(--border-subtle);
    display: flex;
    min-width: 0;
    overflow: hidden;
    padding: 5px 9px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .event-row > *:last-child {
    border-right: 0;
  }

  .event-header {
    background: var(--surface-subtle);
    color: var(--text-secondary);
    font-weight: 650;
  }

  .event-row code {
    color: var(--accent-strong);
    font-family: var(--font-mono);
  }

  .event-data {
    color: var(--text-secondary);
  }

  .empty-log {
    align-items: center;
    color: var(--text-disabled);
    display: flex;
    font: 500 11px/1 var(--font-ui);
    justify-content: center;
    min-height: 84px;
  }

  .dpi-title {
    align-items: start;
  }

  .css-ruler {
    align-items: end;
    background-image: repeating-linear-gradient(
      to right,
      var(--border-strong) 0,
      var(--border-strong) 1px,
      transparent 1px,
      transparent 10px
    );
    background-position: bottom;
    background-repeat: no-repeat;
    background-size: 100px 8px;
    border-bottom: 1px solid var(--border-strong);
    color: var(--text-tertiary);
    display: flex;
    font: 500 10px/1 var(--font-ui);
    height: 28px;
    justify-content: space-between;
    margin-bottom: 13px;
    width: 100px;
  }

  .diagnostics {
    background: var(--surface-subtle);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    margin: 0;
  }

  .diagnostics > div {
    border-right: 1px solid var(--border-subtle);
    min-width: 0;
    padding: 12px 14px;
  }

  .diagnostics > div:last-child {
    border-right: 0;
  }

  dt {
    color: var(--text-tertiary);
    font: 500 10px/1.2 var(--font-ui);
    margin-bottom: 6px;
  }

  dd {
    color: var(--text-primary);
    font: 600 12px/1.2 var(--font-ui);
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  @media (max-width: 760px) {
    .workspace {
      padding-inline: 18px;
    }

    .workspace-header p,
    .section-heading p,
    .validation-label {
      display: none;
    }

    .diagnostics {
      grid-template-columns: repeat(2, 1fr);
    }

    .diagnostics > div:nth-child(2) {
      border-right: 0;
    }

    .diagnostics > div:nth-child(n + 3) {
      border-top: 1px solid var(--border-subtle);
    }
  }
</style>
