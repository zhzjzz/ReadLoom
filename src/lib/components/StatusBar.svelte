<script lang="ts">
  import type { BackendConnection } from '../types/ipc';

  export let connection: BackendConnection;

  $: statusText =
    connection.status === 'connected'
      ? `核心 ${connection.probe.appVersion}`
      : connection.status === 'checking'
        ? '正在检查核心'
        : connection.status === 'browser-preview'
          ? '前端预览'
          : '核心连接异常';
</script>

<footer class="statusbar">
  <span>就绪</span>
  <div class="status-spacer"></div>
  <span>本地优先</span>
  <span>UTF-8</span>
  <span>{statusText}</span>
</footer>

<style>
  .statusbar {
    align-items: center;
    background: var(--surface-chrome);
    border-top: 1px solid var(--border-subtle);
    color: var(--text-tertiary);
    display: flex;
    font: 500 10px/1 var(--font-ui);
    gap: 18px;
    padding: 0 12px;
  }

  .status-spacer {
    flex: 1;
  }
</style>

