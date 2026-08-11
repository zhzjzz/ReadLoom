<script lang="ts">
  import type { BooksBackupResultDto, BooksRestoreResultDto } from '../../types/backup';

  export let backupPath: string | null = null;
  export let backupResult: BooksBackupResultDto | null = null;
  export let restoreResult: BooksRestoreResultDto | null = null;
  export let busy = false;
  export let onChooseBackupPath: () => void = () => {};
  export let onCreateBackup: () => void = () => {};
  export let onRestore: () => void = () => {};

  function formatBytes(bytes: number): string {
    if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
    return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
  }
</script>

<div class="backup-panel">
  <section>
    <div class="section-heading"><div><h3>书籍内容备份</h3><p>使用 ZIP Deflate 流式压缩，并按内容哈希去重。</p></div></div>
    <div class="warning" role="note"><strong>只备份书籍内容</strong><span>书签、阅读进度、分组、设置和阅读记录不会写入备份，也不会在恢复时还原。</span></div>
    <div class="path-row">
      <div><strong>备份位置</strong><span title={backupPath ?? ''}>{backupPath ?? '尚未选择 .readloom-backup 文件'}</span></div>
      <button disabled={busy} onclick={onChooseBackupPath} type="button">选择位置</button>
    </div>
    <button class="primary" disabled={!backupPath || busy} onclick={onCreateBackup} type="button">{busy ? '正在处理…' : '立即备份所有书籍'}</button>
    {#if backupResult}
      <p class="result success">备份完成：{backupResult.bookCount} 本，{backupResult.uniqueContentCount} 份唯一内容，压缩后 {formatBytes(backupResult.backupBytes)}{backupResult.unavailableSkipped ? `；跳过 ${backupResult.unavailableSkipped} 本无效书籍` : ''}。</p>
    {/if}
  </section>

  <section>
    <div class="section-heading"><div><h3>读取备份</h3><p>可同时选择多个备份，再选择恢复目录；重复内容只恢复一次。</p></div></div>
    <button disabled={busy} onclick={onRestore} type="button">选择备份文件并恢复</button>
    {#if restoreResult}
      <p class:warning-result={restoreResult.failed.length > 0} class="result success">已恢复 {restoreResult.restored} 本到 {restoreResult.targetDirectory}；跨备份去重 {restoreResult.duplicateContentSkipped} 本，目标目录已有 {restoreResult.existingContentSkipped} 本。{restoreResult.failed.length ? `有 ${restoreResult.failed.length} 项失败：${restoreResult.failed[0].message}` : ''}</p>
    {/if}
    <p class="dedupe-note">恢复时校验每本书的 SHA-256；同一内容即使来自多个备份或文件名不同，也不会重复写入。</p>
  </section>
</div>

<style>
  .backup-panel { margin:0 auto; max-width:900px; }
  section { border-top:1px solid var(--border-subtle); padding:22px 0 26px; }
  .section-heading h3 { color:var(--text-primary); font:680 15px/1.3 var(--font-ui); margin:0; }
  .section-heading p { color:var(--text-tertiary); font:500 10px/1.5 var(--font-ui); margin:5px 0 0; }
  .warning { background:color-mix(in srgb,var(--warning-soft,#fff5d6) 72%,var(--surface-pane)); border:1px solid var(--warning,#d39b18); border-radius:9px; display:grid; gap:5px; margin:16px 0; padding:13px 15px; }
  .warning strong { color:var(--text-primary); font:650 11px/1.2 var(--font-ui); }
  .warning span { color:var(--text-secondary); font:520 10px/1.45 var(--font-ui); }
  .path-row { align-items:center; border-bottom:1px solid var(--border-subtle); border-top:1px solid var(--border-subtle); display:flex; gap:20px; justify-content:space-between; margin:14px 0; min-height:70px; }
  .path-row > div { display:grid; gap:5px; min-width:0; }
  .path-row strong { color:var(--text-secondary); font:620 11px/1.2 var(--font-ui); }
  .path-row span { color:var(--text-tertiary); font:500 9px/1.4 var(--font-mono); overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  button { background:var(--surface-control); border:1px solid var(--border-strong); border-radius:8px; color:var(--text-secondary); font:640 10px/1 var(--font-ui); min-height:36px; padding:0 16px; }
  button.primary { background:var(--accent); border-color:var(--accent); color:white; }
  button:disabled { opacity:.5; }
  .result { font:540 10px/1.5 var(--font-ui); margin:14px 0 0; }
  .result.success { color:#247b51; }
  .result.warning-result { color:var(--danger); }
  .dedupe-note { color:var(--text-tertiary); font:500 9px/1.5 var(--font-ui); margin:14px 0 0; }
</style>
