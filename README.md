# Readloom（阅织）

Readloom 是一个面向 Windows 10/11 的本地优先阅读与编辑器。当前仓库已完成阶段 3：TXT 安全编辑与搜索、TXT/EPUB 多标签、SQLite 本地状态，以及 EPUB 2/3 的 SafeZIP、隔离只读阅读、目录、进度、书签和书内搜索。

EPUB 不执行出版者脚本、不整包解压、不静默联网，也不支持编辑、回写、加密或 DRM。项目不包含 Tiptap、跨书全文索引、自动保存、云同步或大型 TXT 增量编辑器。TXT 超过 40 MiB 需确认，超过 160 MiB 拒绝完整编辑。

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

阶段 0/1 基线见 [`stage-0-baseline.md`](docs/performance/stage-0-baseline.md) 和 [`stage-1-baseline.md`](docs/performance/stage-1-baseline.md)。阶段 3 实测见 [`stage-3-baseline.md`](docs/performance/stage-3-baseline.md)，EPUB 安全与 release UI 验收见 [`stage-3-epub.md`](docs/validation/stage-3-epub.md)。
