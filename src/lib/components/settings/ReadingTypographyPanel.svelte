<script lang="ts">
  import {
    readingFontOptions,
    readingFontStack,
    type ReadingTypographySettings,
  } from '../../types/settings';

  export let value: ReadingTypographySettings;
  export let backgroundUrl: string | null = null;
  export let backgroundOpacity = 0.2;
  export let onChange: (value: ReadingTypographySettings) => void = () => {};
  export let onReset: () => void = () => {};

  function update(patch: Partial<ReadingTypographySettings>): void {
    onChange({ ...value, ...patch });
  }

  function updateTxt(patch: Partial<ReadingTypographySettings['txt']>): void {
    update({ txt: { ...value.txt, ...patch } });
  }

  function updateEpub(patch: Partial<ReadingTypographySettings['epub']>): void {
    update({ epub: { ...value.epub, ...patch } });
  }
</script>

<div aria-label="阅读排版设置" class="typography-layout">
  <div class="form-column">
    <div class="form-actions"><button onclick={onReset} type="button">恢复默认排版</button></div>

    <section>
      <h3>字体</h3>
      <div class="setting-grid">
        <label><span>字体</span><select value={value.fontFamily} onchange={(event) => update({ fontFamily: event.currentTarget.value as ReadingTypographySettings['fontFamily'] })}>{#each readingFontOptions as option}<option value={option.id}>{option.label}</option>{/each}</select></label>
        <label><span>字号</span><span class="unit-input"><input aria-label="阅读字号" max="36" min="12" type="number" value={value.fontSize} oninput={(event) => update({ fontSize: Number(event.currentTarget.value) })} /><em>px</em></span></label>
        <label><span>字重</span><select value={value.fontWeight} onchange={(event) => update({ fontWeight: Number(event.currentTarget.value) })}>{#each [300,400,500,600,700] as weight}<option value={weight}>{weight}</option>{/each}</select></label>
        <label><span>字间距</span><span class="unit-input"><input aria-label="阅读字间距" max="0.3" min="-0.05" step="0.01" type="number" value={value.letterSpacing} oninput={(event) => update({ letterSpacing: Number(event.currentTarget.value) })} /><em>em</em></span></label>
      </div>
      <p class="helper">字体按“所选字体 → 系统中文字体 → 通用字体”回退；未安装时不会联网下载。</p>
    </section>

    <section>
      <h3>段落</h3>
      <div class="setting-grid">
        <label><span>首行缩进</span><span class="unit-input"><input aria-label="首行缩进" max="4" min="0" step="0.25" type="number" value={value.firstLineIndent} oninput={(event) => update({ firstLineIndent: Number(event.currentTarget.value) })} /><em>em</em></span></label>
        <label><span>行间距</span><input aria-label="行间距" max="2.4" min="1.2" step="0.05" type="number" value={value.lineHeight} oninput={(event) => update({ lineHeight: Number(event.currentTarget.value) })} /></label>
        <label><span>段落间距</span><span class="unit-input"><input aria-label="段落间距" max="1.5" min="0" step="0.05" type="number" value={value.paragraphSpacing} oninput={(event) => update({ paragraphSpacing: Number(event.currentTarget.value) })} /><em>em</em></span></label>
        <div aria-label="文本对齐" class="segmented-setting" role="group"><span>文本对齐</span><div class="segmented"><button class:active={value.textAlign === 'justify'} onclick={() => update({ textAlign: 'justify' })} type="button">两端对齐</button><button class:active={value.textAlign === 'start'} onclick={() => update({ textAlign: 'start' })} type="button">左对齐</button></div></div>
      </div>
    </section>

    <section>
      <h3>页面</h3>
      <div class="setting-grid">
        <label><span>正文宽度</span><span class="unit-input"><input aria-label="正文宽度" max="1280" min="480" step="20" type="number" value={value.contentWidth} oninput={(event) => update({ contentWidth: Number(event.currentTarget.value) })} /><em>px</em></span></label>
        <label><span>左右边距</span><span class="unit-input"><input aria-label="左右边距" max="160" min="8" step="4" type="number" value={value.horizontalMargin} oninput={(event) => update({ horizontalMargin: Number(event.currentTarget.value) })} /><em>px</em></span></label>
        <label><span>上下边距</span><span class="unit-input"><input aria-label="上下边距" max="120" min="8" step="4" type="number" value={value.verticalMargin} oninput={(event) => update({ verticalMargin: Number(event.currentTarget.value) })} /><em>px</em></span></label>
        <div aria-label="分栏" class="segmented-setting" role="group"><span>分栏</span><div class="segmented"><button class:active={value.columns === 1} onclick={() => update({ columns: 1 })} type="button">单栏</button><button class:active={value.columns === 2} onclick={() => update({ columns: 2 })} type="button">双栏</button></div></div>
      </div>
    </section>

    <section class="format-section">
      <div class="format-heading"><h3>TXT</h3><span>净化与排版分层处理</span></div>
      <div class="rows">
        <label class="choice-row"><span><strong>段首空格处理</strong><small>先清理原有全角/半角缩进，再统一应用阅读缩进。</small></span><select value={value.txt.leadingIndent} onchange={(event) => updateTxt({ leadingIndent: event.currentTarget.value as ReadingTypographySettings['txt']['leadingIndent'] })}><option value="clean">清理原缩进</option><option value="preserve">保留原空格</option></select></label>
        <label class="choice-row"><span><strong>空行处理</strong><small>空行属于原始结构，不等同于段落间距。</small></span><select value={value.txt.blankLines} onchange={(event) => updateTxt({ blankLines: event.currentTarget.value as ReadingTypographySettings['txt']['blankLines'] })}><option value="preserve">保留</option><option value="single">连续空行合并为一行</option><option value="remove">隐藏空行</option></select></label>
        <label class="switch-row"><span><strong>自动合并错误换行</strong><small>只合并长度相近且没有句末标点的疑似固定宽度换行，默认关闭。</small></span><input checked={value.txt.mergeWrappedLines} onchange={(event) => updateTxt({ mergeWrappedLines: event.currentTarget.checked })} type="checkbox" /></label>
        <label class="choice-row"><span><strong>章节标题排版</strong><small>标题保持独立样式，不套用正文首行缩进。</small></span><select value={value.txt.chapterTitleStyle} onchange={(event) => updateTxt({ chapterTitleStyle: event.currentTarget.value as ReadingTypographySettings['txt']['chapterTitleStyle'] })}><option value="prominent">突出</option><option value="compact">紧凑</option><option value="plain">跟随正文</option></select></label>
      </div>
    </section>

    <section class="format-section">
      <div class="format-heading"><h3>EPUB</h3><span>覆盖项彼此独立</span></div>
      <div class="rows">
        <label class="switch-row"><span><strong>使用书籍原始样式</strong><small>保留标题、引用、脚注、图片和特殊排版。</small></span><input checked={value.epub.usePublisherStyles} onchange={(event) => updateEpub({ usePublisherStyles: event.currentTarget.checked })} type="checkbox" /></label>
        {#each [['overrideFont','覆盖字体'],['overrideFontSize','覆盖字号'],['overrideIndent','覆盖缩进'],['overrideLineHeight','覆盖行间距'],['overrideParagraphSpacing','覆盖段间距']] as option}
          <label class="switch-row"><span><strong>{option[1]}</strong><small>只覆盖正文选择器，不影响代码、脚注、图片与章节标题。</small></span><input checked={value.epub[option[0] as keyof typeof value.epub]} onchange={(event) => updateEpub({ [option[0]]: event.currentTarget.checked })} type="checkbox" /></label>
        {/each}
        <label class="switch-row"><span><strong>使用内嵌字体</strong><small>关闭后优先使用阅读器字体回退链。</small></span><input checked={value.epub.useEmbeddedFonts} onchange={(event) => updateEpub({ useEmbeddedFonts: event.currentTarget.checked })} type="checkbox" /></label>
      </div>
    </section>
  </div>

  <aside class="preview-rail">
    <div
      class:double={value.columns === 2}
      class="reading-preview"
      style={`--preview-font:${readingFontStack(value.fontFamily)};--preview-size:${value.fontSize}px;--preview-weight:${value.fontWeight};--preview-tracking:${value.letterSpacing}em;--preview-indent:${value.firstLineIndent}em;--preview-line:${value.lineHeight};--preview-gap:${value.paragraphSpacing}em;--preview-align:${value.textAlign};--preview-opacity:${backgroundOpacity};${backgroundUrl ? `background-image:linear-gradient(color-mix(in srgb,var(--surface-pane) 82%,transparent),color-mix(in srgb,var(--surface-pane) 82%,transparent)),url('${backgroundUrl}')` : ''}`}
    >
      <article>
        <h4>第一章 初入江湖</h4>
        <p>清晨的雾气还未散尽，山间小道上只听见自己的脚步声。远方的世界正在等待。</p>
        <p>这是一段实时排版预览。字号、行距、缩进和正文宽度会在这里立即体现。</p>
        <p>切换字体后，阅读位置仍以章节和文本偏移保存，不依赖会随窗口变化的页码。</p>
      </article>
    </div>
    <p>实时预览只重排示例正文；返回书籍后再应用到当前章节。</p>
  </aside>
</div>

<style>
  .typography-layout { align-items:start; display:grid; gap:34px; grid-template-columns:minmax(520px,760px) minmax(300px,430px); margin:0 auto; max-width:1240px; }
  .form-column { min-width:0; }
  .form-actions { display:flex; justify-content:flex-end; margin-bottom:8px; }
  .form-actions button { background:var(--surface-control); border:1px solid var(--border-strong); border-radius:8px; color:var(--text-secondary); font:620 10px/1 var(--font-ui); min-height:32px; padding:0 13px; }
  section { border-top:1px solid var(--border-subtle); padding:20px 0; }
  h3 { color:var(--text-primary); font:680 14px/1.2 var(--font-ui); margin:0 0 14px; }
  .setting-grid { display:grid; gap:13px 20px; grid-template-columns:repeat(2,minmax(0,1fr)); }
  .setting-grid > label, .segmented-setting { align-items:center; display:grid; gap:12px; grid-template-columns:105px minmax(0,1fr); }
  label > span:first-child, .segmented-setting > span { color:var(--text-secondary); font:600 11px/1.2 var(--font-ui); }
  input[type='number'], select { background:var(--surface-control); border:1px solid var(--border-strong); border-radius:8px; color:var(--text-primary); font:550 11px/1 var(--font-ui); height:34px; min-width:0; padding:0 10px; width:100%; }
  select { cursor:pointer; }
  .unit-input { align-items:center; display:flex; position:relative; }
  .unit-input input { padding-right:38px; }
  .unit-input em { color:var(--text-tertiary); font:550 9px/1 var(--font-mono); position:absolute; right:10px; }
  .segmented-setting { min-width:0; }
  .segmented { border:1px solid var(--border-strong); border-radius:8px; display:flex; overflow:hidden; }
  .segmented button { background:var(--surface-control); border:0; color:var(--text-secondary); flex:1; font:600 10px/1 var(--font-ui); height:32px; min-width:max-content; padding:0 12px; white-space:nowrap; }
  .segmented button + button { border-left:1px solid var(--border-strong); }
  .segmented button.active { background:var(--accent); color:white; }
  .helper { color:var(--text-tertiary); font:500 9px/1.5 var(--font-ui); margin:12px 0 0 117px; }
  .format-heading { align-items:baseline; display:flex; gap:10px; }
  .format-heading h3 { color:var(--accent-strong); }
  .format-heading span { color:var(--text-tertiary); font:500 9px/1 var(--font-ui); }
  .rows { display:grid; }
  .choice-row, .switch-row { align-items:center; border-top:1px solid var(--border-subtle); display:flex; gap:24px; justify-content:space-between; min-height:58px; }
  .rows > label:first-child { border-top:0; }
  .choice-row > span, .switch-row > span { display:grid; gap:4px; }
  .choice-row strong, .switch-row strong { color:var(--text-secondary); font:620 11px/1.2 var(--font-ui); }
  .choice-row small, .switch-row small { color:var(--text-tertiary); font:500 9px/1.4 var(--font-ui); }
  .choice-row select { max-width:210px; }
  input[type='checkbox'] { accent-color:var(--accent); height:18px; width:34px; }
  .preview-rail { position:sticky; top:20px; }
  .reading-preview { background-color:var(--surface-pane); background-position:center; background-size:cover; border:1px solid var(--border-subtle); border-radius:12px; box-shadow:var(--shadow-sm); min-height:610px; overflow:hidden; padding:54px 34px; }
  .reading-preview article { column-count:1; column-gap:28px; font-family:var(--preview-font); font-size:var(--preview-size); font-weight:var(--preview-weight); letter-spacing:var(--preview-tracking); line-height:var(--preview-line); text-align:var(--preview-align); }
  .reading-preview.double article { column-count:2; }
  .reading-preview h4 { column-span:all; color:var(--text-primary); font:700 1.45em/1.35 var(--preview-font); letter-spacing:0; margin:0 0 1.6em; text-align:center; text-indent:0; }
  .reading-preview p { color:var(--text-primary); margin:0 0 var(--preview-gap); text-indent:var(--preview-indent); }
  .preview-rail > p { color:var(--text-tertiary); font:500 9px/1.5 var(--font-ui); margin:9px 4px 0; }
  @media (max-width:1500px) { .typography-layout { grid-template-columns:1fr; } .preview-rail { position:static; } .reading-preview { min-height:420px; } }
  @media (max-width:720px) { .setting-grid { grid-template-columns:1fr; } .helper { margin-left:0; } .choice-row, .switch-row { align-items:flex-start; flex-direction:column; padding:12px 0; } .choice-row select { max-width:none; } }
</style>
