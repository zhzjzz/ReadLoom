# Readloom（阅织）

Readloom 是一个面向 Windows 10/11 的本地优先阅读与编辑器。当前仓库已完成阶段 1 的 TXT 闭环：Tauri 2、Svelte、TypeScript、Vite、按需加载的 CodeMirror 6、Rust 文档会话、编码/BOM/换行检测、外部修改保护，以及 Windows 安全替换保存。

阶段 1 只支持 TXT，不包含 Tiptap、SQLite、EPUB、全文索引、自动保存、云同步或大型文件增量编辑器。超过 40 MiB 的文件需确认，超过 160 MiB 的文件拒绝完整编辑。

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

阶段 0 基线见 [`docs/performance/stage-0-baseline.md`](docs/performance/stage-0-baseline.md)。阶段 1 实测见 [`docs/performance/stage-1-baseline.md`](docs/performance/stage-1-baseline.md)，TXT 手动验收流程与自动化结果见 [`docs/validation/stage-1-txt.md`](docs/validation/stage-1-txt.md)。
