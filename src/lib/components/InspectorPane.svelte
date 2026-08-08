<script lang="ts">
  import { resolvedTheme, type ThemePreference } from '../stores/theme';
  import type { BackendConnection } from '../types/ipc';
  import type { DocumentSession } from '../types/document';
  import Icon from './Icon.svelte';
  import ThemeControl from './ThemeControl.svelte';

  export let connection: BackendConnection;
  export let document: DocumentSession | null = null;
  export let theme: ThemePreference;
  export let onThemeChange: (preference: ThemePreference) => void;
  export let onRetry: () => void;

  $: connectionLabel =
    connection.status === 'connected'
      ? '已连接'
      : connection.status === 'checking'
        ? '正在检查'
        : connection.status === 'browser-preview'
          ? '仅前端预览'
          : '连接失败';
</script>

<aside aria-label="文档与外观" class="inspector-pane">
  <header>
    <h2>文档与外观</h2>
  </header>

  <section>
    <div class="section-title"><h3>当前文档</h3></div>
    {#if document}
      <dl class="runtime-list">
        <div><dt>文件名</dt><dd>{document.fileName}</dd></div>
        <div><dt>路径</dt><dd title={document.displayPath}>{document.displayPath}</dd></div>
        <div><dt>检测编码</dt><dd>{document.savedEncoding}{document.savedHasBom ? ' + BOM' : ''}</dd></div>
        <div><dt>检测换行</dt><dd>{document.lineEnding}</dd></div>
        <div><dt>写入状态</dt><dd>{document.readOnly ? '只读' : '可写'}</dd></div>
        <div><dt>版本</dt><dd>{document.revision}</dd></div>
      </dl>
    {:else}
      <div class="runtime-note">
        <Icon name="document" size={16} />
        <p>打开 TXT 后，这里会显示 Rust 检测到的编码、BOM 和换行格式。</p>
      </div>
    {/if}
  </section>

  <section class="runtime-section">
    <div class="section-title">
      <h3>主题</h3>
      <span>当前 {$resolvedTheme === 'dark' ? '暗色' : '亮色'}</span>
    </div>
    <ThemeControl value={theme} onChange={onThemeChange} />
  </section>

  <section>
    <div class="section-title">
      <h3>Rust 通道</h3>
    </div>

    <dl class="runtime-list">
      <div>
        <dt>连接状态</dt>
        <dd class="connection-value" data-status={connection.status}>
          <span class="status-dot"></span>{connectionLabel}
        </dd>
      </div>

      {#if connection.status === 'connected'}
        <div><dt>后端版本</dt><dd>{connection.probe.appVersion}</dd></div>
        <div><dt>运行目标</dt><dd>{connection.probe.platform} / {connection.probe.architecture}</dd></div>
        <div><dt>协议版本</dt><dd>{connection.probe.protocolVersion}</dd></div>
        <div><dt>前端就绪</dt><dd>{connection.startup.mainToFrontendReadyMs} ms</dd></div>
      {:else if connection.status === 'browser-preview'}
        <div class="runtime-note">
          <Icon name="info" size={16} />
          <p>浏览器预览不包含 Tauri IPC；桌面构建中会连接 Rust 核心。</p>
        </div>
      {:else if connection.status === 'error'}
        <div class="error-message" role="alert">
          <strong>{connection.error.message}</strong>
          {#if connection.error.suggestedAction}
            <p>{connection.error.suggestedAction}</p>
          {/if}
          <code>{connection.error.code}</code>
        </div>
      {/if}
    </dl>

    {#if connection.status !== 'browser-preview'}
      <button class="retry-button" disabled={connection.status === 'checking'} onclick={onRetry} type="button">
        {connection.status === 'checking' ? '正在连接…' : '重新检查'}
      </button>
    {/if}
  </section>

  <section>
    <div class="section-title">
      <h3>阶段 1 边界</h3>
    </div>
    <ul>
      <li><Icon name="check" size={14} /><span>40 MiB 以上打开前确认</span></li>
      <li><Icon name="check" size={14} /><span>160 MiB 以上拒绝完整编辑</span></li>
      <li><Icon name="check" size={14} /><span>替换前检查外部修改</span></li>
    </ul>
  </section>
</aside>

<style>
  .inspector-pane {
    background: var(--surface-pane);
    border-left: 1px solid var(--border-subtle);
    min-height: 0;
    overflow-y: auto;
  }

  header {
    align-items: center;
    border-bottom: 1px solid var(--border-subtle);
    display: flex;
    min-height: 52px;
    padding: 0 18px;
  }

  header h2 {
    color: var(--text-primary);
    font: 650 14px/1 var(--font-ui);
    margin: 0;
  }

  section {
    padding: 18px;
  }

  section + section {
    border-top: 1px solid var(--border-subtle);
  }

  .section-title {
    align-items: baseline;
    display: flex;
    justify-content: space-between;
    margin-bottom: 12px;
  }

  h3 {
    color: var(--text-primary);
    font: 650 12px/1 var(--font-ui);
    margin: 0;
  }

  .section-title span {
    color: var(--text-tertiary);
    font: 500 10px/1 var(--font-ui);
  }

  .runtime-list {
    display: grid;
    gap: 10px;
    margin: 0;
  }

  .runtime-list > div:not(.runtime-note, .error-message) {
    align-items: baseline;
    display: grid;
    gap: 10px;
    grid-template-columns: 76px minmax(0, 1fr);
  }

  dt {
    color: var(--text-tertiary);
    font: 500 11px/1.35 var(--font-ui);
  }

  dd {
    color: var(--text-secondary);
    font: 500 11px/1.35 var(--font-ui);
    margin: 0;
    overflow-wrap: anywhere;
  }

  .connection-value {
    align-items: center;
    display: flex;
    gap: 7px;
  }

  .status-dot {
    background: var(--warning);
    border-radius: 999px;
    flex: 0 0 auto;
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

  .runtime-note {
    align-items: start;
    background: var(--surface-subtle);
    border-radius: var(--radius-sm);
    color: var(--text-tertiary);
    display: flex;
    gap: 8px;
    padding: 10px;
  }

  .runtime-note p,
  .error-message p {
    font: 400 11px/1.5 var(--font-ui);
    margin: 0;
  }

  .error-message {
    background: var(--danger-soft);
    border-left: 2px solid var(--danger);
    color: var(--text-secondary);
    padding: 10px 11px;
  }

  .error-message strong {
    display: block;
    font: 600 11px/1.45 var(--font-ui);
  }

  .error-message p {
    margin-top: 4px;
  }

  .error-message code {
    color: var(--text-tertiary);
    display: block;
    font: 500 9px/1 var(--font-mono);
    margin-top: 7px;
  }

  .retry-button {
    background: var(--surface-control);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    font: 600 11px/1 var(--font-ui);
    margin-top: 13px;
    min-height: 32px;
    width: 100%;
  }

  .retry-button:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--text-primary);
  }

  .retry-button:disabled {
    color: var(--text-disabled);
  }

  ul {
    display: grid;
    gap: 9px;
    list-style: none;
    margin: 0;
    padding: 0;
  }

  li {
    align-items: start;
    color: var(--text-tertiary);
    display: flex;
    font: 400 11px/1.4 var(--font-ui);
    gap: 7px;
  }

  li :global(.icon) {
    color: var(--success);
    margin-top: 1px;
  }
</style>
