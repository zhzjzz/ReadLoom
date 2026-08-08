<script lang="ts">
  export let fileName: string;
  export let saving = false;
  export let onSave: () => void;
  export let onDiscard: () => void;
  export let onCancel: () => void;
</script>

<div aria-labelledby="unsaved-title" aria-modal="true" class="dialog-backdrop" role="dialog">
  <section class="dialog-card">
    <h2 id="unsaved-title">保存对“{fileName}”的更改？</h2>
    <p>如果不保存，本次编辑内容将丢失。</p>
    <div class="dialog-actions">
      <button disabled={saving} onclick={onCancel} type="button">取消</button>
      <button class="danger" disabled={saving} onclick={onDiscard} type="button">不保存</button>
      <button class="primary" disabled={saving} onclick={onSave} type="button">
        {saving ? '保存中…' : '保存'}
      </button>
    </div>
  </section>
</div>

<style>
  .dialog-backdrop {
    align-items: center;
    background: rgb(0 0 0 / 38%);
    display: flex;
    inset: 0;
    justify-content: center;
    padding: 24px;
    position: fixed;
    z-index: 100;
  }

  .dialog-card {
    background: var(--surface-pane);
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    box-shadow: 0 18px 50px rgb(0 0 0 / 24%);
    max-width: 440px;
    padding: 22px;
    width: 100%;
  }

  h2 {
    color: var(--text-primary);
    font: 650 16px/1.35 var(--font-ui);
    margin: 0;
  }

  p {
    color: var(--text-tertiary);
    font: 400 12px/1.55 var(--font-ui);
    margin: 9px 0 20px;
  }

  .dialog-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }

  button {
    background: var(--surface-control);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    font: 600 11px/1 var(--font-ui);
    min-height: 32px;
    padding: 0 14px;
  }

  button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
  }

  button.danger {
    color: var(--danger);
  }
</style>
