<script lang="ts">
  import type { EpubEditDraft, EpubMetadataPatch } from '../../types/epub';

  export let draft: EpubEditDraft;
  export let previewUrl: string | null;
  export let saving = false;
  export let onMetadataChange: (patch: EpubMetadataPatch) => void;
  export let onReplaceCover: () => void;
  export let onRestoreCover: () => void;
  export let onSaveAs: () => void;
  export let onCancelSave: () => void;
  export let onDiscard: () => void;
  export let onClose: () => void;

  let lastRevision = -1;
  let title = '';
  let creators = '';
  let contributors = '';
  let language = '';
  let publisher = '';
  let description = '';
  let identifier = '';
  let publicationDate = '';
  let subjects = '';
  let rights = '';

  $: if (draft.revision !== lastRevision) {
    lastRevision = draft.revision;
    title = draft.metadata.title;
    creators = draft.metadata.creators.join('\n');
    contributors = draft.metadata.contributors.join('\n');
    language = draft.metadata.language;
    publisher = draft.metadata.publisher ?? '';
    description = draft.metadata.description ?? '';
    identifier = draft.metadata.identifier;
    publicationDate = draft.metadata.publicationDate ?? '';
    subjects = draft.metadata.subjects.join('\n');
    rights = draft.metadata.rights.join('\n');
  }

  function lines(value: string): string[] {
    return value.split(/\r?\n/).map((item) => item.trim()).filter(Boolean);
  }

  function applyMetadata(): void {
    onMetadataChange({
      title,
      creators: lines(creators),
      contributors: lines(contributors),
      language,
      publisher: publisher.trim() || null,
      description: description.trim() || null,
      identifier,
      publicationDate: publicationDate.trim() || null,
      subjects: lines(subjects),
      rights: lines(rights),
    });
  }
</script>

<aside aria-label="编辑书籍信息" class="epub-edit-panel">
  <header>
    <div>
      <strong>编辑书籍信息</strong>
      <span>安全另存为，不会覆盖原 EPUB</span>
    </div>
    <button aria-label="关闭书籍信息面板" disabled={saving} onclick={onClose} type="button">×</button>
  </header>

  <div class="panel-scroll">
    <section class="cover-section">
      <div class="cover-preview">
        {#if previewUrl}
          <img alt="当前 EPUB 封面预览" src={previewUrl} />
        {:else}
          <span>无封面</span>
        {/if}
      </div>
      <div class="cover-actions">
        <strong>封面</strong>
        {#if draft.cover.width && draft.cover.height}
          <span>{draft.cover.mediaType} · {draft.cover.width} × {draft.cover.height}</span>
        {:else}
          <span>{draft.cover.originalResourceId ? '使用原封面' : '出版物没有显式封面'}</span>
        {/if}
        <button disabled={saving} onclick={onReplaceCover} type="button">替换封面</button>
        <button disabled={saving || draft.cover.state === 'unchanged'} onclick={onRestoreCover} type="button">
          恢复原封面
        </button>
      </div>
    </section>

    <form onsubmit={(event) => { event.preventDefault(); applyMetadata(); }}>
      <label><span>书名 *</span><input aria-label="书名" bind:value={title} maxlength="512" required /></label>
      <label><span>作者（每行一个）</span><textarea aria-label="作者列表" bind:value={creators} rows="2"></textarea></label>
      <label><span>贡献者（每行一个）</span><textarea aria-label="贡献者列表" bind:value={contributors} rows="2"></textarea></label>
      <div class="field-row">
        <label><span>语言 *</span><input aria-label="语言" bind:value={language} maxlength="63" required /></label>
        <label><span>出版日期</span><input aria-label="出版日期" bind:value={publicationDate} maxlength="64" /></label>
      </div>
      <label><span>出版社</span><input aria-label="出版社" bind:value={publisher} maxlength="512" /></label>
      <label><span>简介（保存为安全纯文本）</span><textarea aria-label="简介" bind:value={description} maxlength="16384" rows="4"></textarea></label>
      <label><span>Identifier *</span><input aria-label="Identifier" bind:value={identifier} maxlength="1024" required /></label>
      <label><span>主题（每行一个）</span><textarea aria-label="主题标签" bind:value={subjects} rows="2"></textarea></label>
      <label><span>版权信息（每行一个）</span><textarea aria-label="版权信息" bind:value={rights} rows="2"></textarea></label>
      <button class="apply" disabled={saving} type="submit">应用元数据</button>
    </form>

    <section class="validation" aria-live="polite">
      <div>
        <strong>{draft.dirty ? '有未保存修改' : '没有未保存修改'}</strong>
      </div>
      {#if draft.validation.errors.length > 0}
        {#each draft.validation.errors as issue}<p class="error">{issue.message}</p>{/each}
      {:else if draft.dirty}
        <p>保存前检查通过；另存后还会重新打开并核对资源摘要。</p>
      {:else}
        <p>修改元数据或封面后可另存为新的 EPUB。</p>
      {/if}
      {#each draft.validation.warnings as issue}<p class="warning">{issue.message}</p>{/each}
    </section>
  </div>

  <footer>
    <button class="discard" disabled={saving || !draft.dirty} onclick={onDiscard} type="button">放弃修改</button>
    {#if saving}
      <button onclick={onCancelSave} type="button">取消保存</button>
    {/if}
    <button
      class="primary"
      disabled={saving || !draft.dirty || !draft.validation.canSave}
      onclick={onSaveAs}
      type="button"
    >
      {saving ? '正在安全另存…' : '另存为 EPUB'}
    </button>
  </footer>
</aside>

<style>
  .epub-edit-panel {
    background: var(--surface-pane);
    border-left: 1px solid var(--border-strong);
    box-shadow: -12px 0 28px rgb(0 0 0 / 10%);
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    height: 100%;
    inset: 0 0 0 auto;
    position: absolute;
    width: min(420px, 48vw);
    z-index: 20;
  }

  header, footer {
    align-items: center;
    border-bottom: 1px solid var(--border-subtle);
    display: flex;
    gap: 10px;
    justify-content: space-between;
    padding: 12px 14px;
  }

  header > div { display: grid; gap: 4px; }
  header strong, .cover-actions strong, .validation strong { color: var(--text-primary); font: 650 12px/1.3 var(--font-ui); }
  header span, .cover-actions span { color: var(--text-tertiary); font: 500 9px/1.4 var(--font-ui); }
  header button { border: 0; font-size: 18px; width: 30px; }
  .panel-scroll { min-height: 0; overflow: auto; padding: 14px; }
  .cover-section { display: grid; gap: 14px; grid-template-columns: 92px 1fr; margin-bottom: 16px; }
  .cover-preview { align-items: center; aspect-ratio: 3 / 4; background: var(--surface-control); border: 1px solid var(--border-strong); display: flex; justify-content: center; overflow: hidden; }
  .cover-preview img { height: 100%; object-fit: contain; width: 100%; }
  .cover-preview span { color: var(--text-disabled); font: 600 10px/1 var(--font-ui); }
  .cover-actions { align-content: start; display: grid; gap: 7px; }
  form { display: grid; gap: 10px; }
  label { display: grid; gap: 5px; }
  label span { color: var(--text-secondary); font: 600 10px/1.2 var(--font-ui); }
  input, textarea { background: var(--surface-control); border: 1px solid var(--border-strong); border-radius: var(--radius-sm); color: var(--text-primary); font: 400 11px/1.45 var(--font-ui); padding: 7px 8px; resize: vertical; }
  .field-row { display: grid; gap: 8px; grid-template-columns: 1fr 1fr; }
  button { background: var(--surface-control); border: 1px solid var(--border-strong); border-radius: var(--radius-sm); color: var(--text-secondary); font: 600 10px/1 var(--font-ui); min-height: 30px; padding: 0 10px; }
  button:disabled { color: var(--text-disabled); cursor: default; }
  button.apply { justify-self: start; }
  .validation { background: var(--surface-control); border: 1px solid var(--border-subtle); display: grid; gap: 5px; margin-top: 14px; padding: 10px; }
  .validation > div { display: flex; justify-content: space-between; }
  .validation p { color: var(--text-tertiary); font: 400 10px/1.45 var(--font-ui); margin: 0; }
  .validation p.error { color: var(--danger); }
  .validation p.warning { color: var(--warning); }
  footer { border-bottom: 0; border-top: 1px solid var(--border-subtle); justify-content: flex-end; }
  footer .discard { color: var(--danger); margin-right: auto; }
  footer .primary { background: var(--accent); border-color: var(--accent); color: white; }
</style>
