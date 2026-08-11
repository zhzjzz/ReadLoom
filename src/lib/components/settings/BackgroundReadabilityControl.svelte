<script lang="ts">
  export let backgroundKey: string | null = null;
  export let backgroundUrl: string | null = null;
  export let opacity = 0.2;
  export let onChoose: () => void = () => {};
  export let onClear: () => void = () => {};
  export let onOpacityChange: (value: number) => void = () => {};

  $: opacityPercent = Math.round(opacity * 100);
</script>

<div class="background-control">
  <div
    aria-label={backgroundKey ? '当前自定义背景预览' : '尚未设置自定义背景'}
    class:empty={!backgroundKey}
    class="background-preview"
    style={backgroundUrl ? `background-image:url('${backgroundUrl}')` : undefined}
  >
    {#if !backgroundKey}<span>暂无背景</span>{/if}
    <div class="preview-copy"><strong>正文预览</strong><span>背景会延伸到阅读页下方。</span></div>
  </div>
  <div class="background-meta">
    <div class="actions">
      <button onclick={onChoose} type="button">选择图片</button>
      <button class="danger" disabled={!backgroundKey} onclick={onClear} type="button">清除背景</button>
    </div>
    <label>
      <span><strong>背景显示强度</strong><small>正文上会自动叠加可读性遮罩。</small></span>
      <input
        aria-label="背景显示强度"
        max="100"
        min="0"
        oninput={(event) => onOpacityChange(Number(event.currentTarget.value) / 100)}
        step="1"
        type="range"
        value={opacityPercent}
      />
      <output>{opacityPercent}%</output>
    </label>
  </div>
</div>

<style>
  .background-control { align-items:stretch; display:grid; gap:18px; grid-template-columns:minmax(260px,420px) minmax(280px,1fr); }
  .background-preview { background-position:center; background-size:cover; border:1px solid var(--border-subtle); border-radius:12px; box-shadow:var(--shadow-sm); display:flex; flex-direction:column; height:164px; justify-content:flex-end; overflow:hidden; position:relative; }
  .background-preview::after { background:color-mix(in srgb,var(--surface-pane) 78%,transparent); content:''; inset:0; position:absolute; }
  .background-preview.empty { align-items:center; background:var(--surface-subtle); color:var(--text-disabled); justify-content:center; }
  .background-preview.empty::after { display:none; }
  .background-preview > span { font:600 11px/1 var(--font-ui); }
  .preview-copy { display:grid; gap:4px; padding:14px; position:relative; z-index:1; }
  .preview-copy strong { color:var(--text-primary); font:680 13px/1.2 var(--font-ui); }
  .preview-copy span { color:var(--text-tertiary); font:500 10px/1.4 var(--font-ui); }
  .empty .preview-copy { display:none; }
  .background-meta { display:flex; flex-direction:column; justify-content:space-between; padding:4px 0; }
  .actions { display:flex; gap:8px; }
  button { background:var(--surface-control); border:1px solid var(--border-strong); border-radius:8px; color:var(--text-secondary); font:620 11px/1 var(--font-ui); min-height:34px; padding:0 15px; }
  button:hover:not(:disabled) { background:var(--surface-hover); color:var(--text-primary); }
  button.danger { color:var(--danger); }
  button:disabled { color:var(--text-disabled); }
  label { align-items:center; display:grid; gap:12px; grid-template-columns:minmax(150px,1fr) minmax(120px,220px) 42px; }
  label > span { display:grid; gap:4px; }
  label strong { color:var(--text-secondary); font:620 11px/1.2 var(--font-ui); }
  label small { color:var(--text-tertiary); font:500 9px/1.4 var(--font-ui); }
  input { accent-color:var(--accent); width:100%; }
  output { color:var(--text-secondary); font:650 10px/1 var(--font-mono); }
  @media (max-width:900px) { .background-control { grid-template-columns:1fr; } .background-preview { height:140px; max-width:none; } }
  @media (max-width:620px) { label { grid-template-columns:1fr 42px; } label > span { grid-column:1 / -1; } }
</style>
