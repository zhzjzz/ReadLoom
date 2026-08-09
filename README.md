# Readloom（阅织）

Readloom 是一个面向 Windows 10/11 的本地优先阅读与编辑器，支持 TXT 安全编辑与搜索、TXT/EPUB 多标签、SQLite 本地状态、EPUB 2/3 隔离阅读，以及 EPUB 元数据、PNG/JPEG/WebP 封面与兼容章节正文的可视化编辑和安全另存为。

EPUB 不执行出版者脚本、不整包解压、不静默联网。章节正文先由 Rust 转成受限的 Tiptap/ProseMirror JSON 文档，再由白名单序列化器写回 XHTML；不支持或无法无损往返的结构会明确降级为受限或只读，不会以浏览器 `innerHTML` 作为保存源。编辑使用内存草稿和流式重打包，只允许另存为新的 `.epub`，不会覆盖源书；保存前后均检查源文件指纹，输出还会重新通过 SafeZIP、解析器和结构校验。加密、DRM、fixed-layout 和不满足安全能力的 EPUB 仍为只读。项目不包含跨书全文索引、自动保存、云同步、EPUB 目录/CSS/字体编辑或大型 TXT 增量编辑器。TXT 超过 40 MiB 需确认，超过 160 MiB 拒绝完整编辑。

“打开文件”和窗口拖拽共用一条导入路由：`.epub` 使用隔离阅读器，其他扩展名或无扩展名文件按文本打开。

工作区支持从“最近文件”列表用小叉移除单条历史记录（不会删除磁盘文件）；TXT 会按章节标题语法生成可跳转大纲。左侧导航可收起并拖动调宽；设置默认隐藏，通过顶部“设置”按钮打开，其中包含外观和可自定义的 TXT 标题识别正则。EPUB 目录也可单独横向拖动调宽。TXT 内容统一从顶部开始显示并正常滚动。

## 环境

- Node.js 24+
- npm 11+
- Rust stable，目标 `x86_64-pc-windows-msvc`
- Microsoft C++ Build Tools
- Microsoft Edge WebView2 Runtime

## 开发命令

```powershell
npm ci
npm run dev
npm run tauri dev
```

## 验证命令

```powershell
npm run check
npm run test
npm run build

cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --all-targets --all-features --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --all-targets --all-features --manifest-path src-tauri/Cargo.toml
npm run tauri build
```

`src-tauri/tauri.conf.json` 在阶段 0 关闭安装包 bundling；`tauri build` 仍会生成 release 可执行文件。安装器、图标和签名属于阶段 6。

## 性能基线

release 构建完成后运行：

```powershell
powershell -ExecutionPolicy Bypass -File scripts/measure-stage0.ps1
```

脚本输出 JSON，包含可执行文件大小、前端资源大小、启动到前端就绪时间，以及 Readloom 与其 WebView2 子进程的空闲内存。第一次启动仅是“冷启动代理值”：它不会清空 Windows 文件缓存，不能当作重启系统后的严格冷启动。

阶段 0/1 基线见 [`stage-0-baseline.md`](docs/performance/stage-0-baseline.md) 和 [`stage-1-baseline.md`](docs/performance/stage-1-baseline.md)。阶段 3 实测见 [`stage-3-baseline.md`](docs/performance/stage-3-baseline.md)。阶段 4A 的重打包性能见 [`stage-4a-baseline.md`](docs/performance/stage-4a-baseline.md)，安全编辑和 release UI 验收见 [`stage-4a-epub-edit.md`](docs/validation/stage-4a-epub-edit.md)。阶段 4B 的正文编辑性能和验收分别见 [`stage-4b-baseline.md`](docs/performance/stage-4b-baseline.md) 与 [`stage-4b-epub-chapter-edit.md`](docs/validation/stage-4b-epub-chapter-edit.md)。

阶段 4A 本地验收夹具和真实桌面脚本：

```powershell
powershell -ExecutionPolicy Bypass -File scripts/create-stage4a-epub-fixtures.ps1
powershell -ExecutionPolicy Bypass -File scripts/test-stage4a-epub-edit-ui.ps1
```

阶段 4B 夹具包含正常四章、只读降级、1.68 MiB XHTML 大章和图片导入样本。为确保中文与 emoji 按 UTF-8 生成，使用 PowerShell 7：

```powershell
pwsh -NoProfile -File scripts/create-stage4b-epub-fixtures.ps1
pwsh -NoProfile -File scripts/test-stage4b-epub-chapter-ui.ps1
```
