<script lang="ts">
  import type { LibraryImportPreviewDto } from '../types/library';

  export let preview: LibraryImportPreviewDto;
  export let importing = false;
  export let onCancel: () => void = () => {};
  export let onConfirm: (paths: string[]) => void = () => {};

  type Filter = 'all' | 'importable' | 'existing';
  let filter: Filter = 'all';
  let query = '';
  let selected = new Set(preview.candidates.filter((item) => !item.alreadyImported).map((item) => item.path));

  $: normalizedQuery = query.trim().toLocaleLowerCase();
  $: visible = preview.candidates.filter((item) => {
    if (filter === 'importable' && item.alreadyImported) return false;
    if (filter === 'existing' && !item.alreadyImported) return false;
    return !normalizedQuery || `${item.fileName}\n${item.path}`.toLocaleLowerCase().includes(normalizedQuery);
  });
  $: importableVisible = visible.filter((item) => !item.alreadyImported);
  $: allVisibleSelected = importableVisible.length > 0 && importableVisible.every((item) => selected.has(item.path));
  $: selectedCandidates = preview.candidates.filter((item) => selected.has(item.path) && !item.alreadyImported);
  $: selectedBytes = selectedCandidates.reduce((total, item) => total + item.sizeBytes, 0);

  function togglePath(path: string, checked: boolean): void {
    const next = new Set(selected);
    if (checked) next.add(path);
    else next.delete(path);
    selected = next;
  }

  function toggleVisible(): void {
    const next = new Set(selected);
    if (allVisibleSelected) importableVisible.forEach((item) => next.delete(item.path));
    else importableVisible.forEach((item) => next.add(item.path));
    selected = next;
  }

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
    return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
  }
</script>

<div aria-labelledby="library-import-title" aria-modal="true" class="dialog-backdrop" role="dialog">
  <section class="review-dialog">
    <header>
      <div>
        <h2 id="library-import-title">导入前确认</h2>
        <p>扫描到 {preview.candidates.length} 本 · 可导入 {preview.importable} 本 · 已在书库 {preview.alreadyImported} 本</p>
      </div>
      <button aria-label="关闭导入确认" disabled={importing} onclick={onCancel} type="button">×</button>
    </header>

    <div class="review-toolbar">
      <label class="select-all"><input checked={allVisibleSelected} disabled={!importableVisible.length || importing} onchange={toggleVisible} type="checkbox" />选择当前可导入项</label>
      <div aria-label="导入状态筛选" class="segmented" role="radiogroup">
        <button aria-checked={filter === 'all'} class:active={filter === 'all'} onclick={() => (filter = 'all')} role="radio" type="button">全部</button>
        <button aria-checked={filter === 'importable'} class:active={filter === 'importable'} onclick={() => (filter = 'importable')} role="radio" type="button">可导入</button>
        <button aria-checked={filter === 'existing'} class:active={filter === 'existing'} onclick={() => (filter = 'existing')} role="radio" type="button">已在书库</button>
      </div>
      <input aria-label="筛选待导入图书" bind:value={query} placeholder="搜索文件名或位置" type="search" />
    </div>

    <div class="table-shell">
      <table>
        <thead><tr><th>勾选</th><th>文件名</th><th>格式</th><th>大小</th><th>状态</th><th>位置</th></tr></thead>
        <tbody>
          {#each visible as item}
            <tr class:existing={item.alreadyImported}>
              <td><input aria-label={`选择 ${item.fileName}`} checked={selected.has(item.path)} disabled={item.alreadyImported || importing} onchange={(event) => togglePath(item.path, event.currentTarget.checked)} type="checkbox" /></td>
              <td title={item.fileName}>{item.fileName}</td>
              <td>{item.documentKind.toUpperCase()}</td>
              <td>{formatBytes(item.sizeBytes)}</td>
              <td><span class:ready={!item.alreadyImported}>{item.alreadyImported ? '已在书库' : '可导入'}</span></td>
              <td class="path" title={item.path}>{item.path}</td>
            </tr>
          {:else}
            <tr><td class="empty" colspan="6">没有符合筛选条件的图书。</td></tr>
          {/each}
        </tbody>
      </table>
    </div>

    <footer>
      <span>将导入 {selectedCandidates.length} 本，共 {formatBytes(selectedBytes)}</span>
      <div><button disabled={importing} onclick={onCancel} type="button">取消</button><button class="primary" disabled={!selectedCandidates.length || importing} onclick={() => onConfirm(selectedCandidates.map((item) => item.path))} type="button">{importing ? '正在导入…' : '导入所选图书'}</button></div>
    </footer>
  </section>
</div>

<style>
  .dialog-backdrop { align-items:center; background:rgb(19 28 45 / 48%); display:flex; inset:0; justify-content:center; padding:24px; position:fixed; z-index:120; }
  .review-dialog { background:var(--surface-pane); border:1px solid var(--border-strong); border-radius:14px; box-shadow:0 24px 80px rgb(0 0 0 / 28%); display:grid; grid-template-rows:auto auto minmax(0,1fr) auto; height:min(760px,calc(100vh - 48px)); overflow:hidden; width:min(1120px,calc(100vw - 48px)); }
  header { align-items:flex-start; display:flex; justify-content:space-between; padding:24px 28px 16px; }
  header h2 { color:var(--text-primary); font:720 23px/1.2 var(--font-ui); letter-spacing:-.02em; margin:0; }
  header p { color:var(--text-tertiary); font:520 11px/1.5 var(--font-ui); margin:7px 0 0; }
  header button { background:transparent; border:0; color:var(--text-tertiary); font:400 22px/1 var(--font-ui); height:30px; width:30px; }
  .review-toolbar { align-items:center; border-bottom:1px solid var(--border-subtle); display:grid; gap:18px; grid-template-columns:1fr auto minmax(210px,300px); padding:0 28px 15px; }
  .select-all { align-items:center; color:var(--text-secondary); display:flex; font:600 10px/1.2 var(--font-ui); gap:7px; }
  input[type='checkbox'] { accent-color:var(--accent); }
  .segmented { border:1px solid var(--border-strong); border-radius:8px; display:flex; overflow:hidden; }
  .segmented button { background:var(--surface-control); border:0; color:var(--text-secondary); font:600 10px/1 var(--font-ui); height:32px; min-width:72px; }
  .segmented button + button { border-left:1px solid var(--border-strong); }
  .segmented button.active { background:var(--accent); color:white; }
  input[type='search'] { background:var(--surface-control); border:1px solid var(--border-strong); border-radius:8px; color:var(--text-primary); font:520 10px/1 var(--font-ui); height:34px; padding:0 11px; width:100%; }
  .table-shell { min-height:0; overflow:auto; }
  table { border-collapse:collapse; table-layout:fixed; width:100%; }
  th { background:var(--surface-subtle); color:var(--text-tertiary); font:650 9px/1 var(--font-ui); padding:10px 8px; position:sticky; text-align:left; top:0; z-index:1; }
  th:first-child, td:first-child { padding-left:28px; width:62px; }
  th:nth-child(2) { width:25%; } th:nth-child(3) { width:70px; } th:nth-child(4) { width:88px; } th:nth-child(5) { width:92px; }
  td { border-bottom:1px solid var(--border-subtle); color:var(--text-secondary); font:520 10px/1.3 var(--font-ui); overflow:hidden; padding:10px 8px; text-overflow:ellipsis; white-space:nowrap; }
  tr.existing td { color:var(--text-disabled); }
  td span.ready { color:#248052; font-weight:650; }
  td.path { color:var(--text-tertiary); font-family:var(--font-mono); font-size:9px; }
  td.empty { color:var(--text-tertiary); padding:48px; text-align:center; }
  footer { align-items:center; border-top:1px solid var(--border-subtle); display:flex; justify-content:space-between; padding:15px 28px; }
  footer > span { color:var(--text-secondary); font:620 11px/1 var(--font-ui); }
  footer > div { display:flex; gap:9px; }
  footer button { background:var(--surface-control); border:1px solid var(--border-strong); border-radius:8px; color:var(--text-secondary); font:650 10px/1 var(--font-ui); min-height:36px; padding:0 18px; }
  footer button.primary { background:var(--accent); border-color:var(--accent); color:white; }
  button:disabled { opacity:.5; }
  @media (max-width:760px) { .dialog-backdrop { padding:10px; } .review-dialog { height:calc(100vh - 20px); width:calc(100vw - 20px); } .review-toolbar { grid-template-columns:1fr; gap:9px; } .table-shell { overflow:auto; } table { min-width:820px; } footer { align-items:flex-start; flex-direction:column; gap:12px; } }
</style>
