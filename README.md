# Readloom（阅织）

Readloom 是一个面向 Windows 10/11 的本地优先阅读与编辑器，支持 TXT、EPUB、本地书库、多标签阅读和安全编辑。当前主界面使用 Rust + Slint 构建，不依赖 WebView2。

## 为什么做 Readloom

我创建 Readloom，是想解决在电脑上阅读和编辑 TXT、EPUB 时遇到的实际问题。

我尝试过不少现有阅读器，但它们往往存在界面不够美观、阅读体验不统一，或者只能阅读、不能直接编辑等问题。于是我希望做一个界面简洁舒适、排版可调，同时兼顾阅读与编辑的本地工具，让整理和阅读自己的电子书不再需要在多个软件之间切换。

目前我只有 Windows 设备，因此 Readloom 现阶段专注于 Windows 版本。未来是否支持其他平台，将根据开发条件和项目进展再决定。

## 主要功能

- 本地书库：导入 TXT/EPUB、目录扫描、分组、搜索、筛选和排序。
- TXT 阅读与编辑：目录、全文搜索、书签、长文本窗口化渲染、段落编辑、撤销与重做。
- TXT 安全保存：保留原编码、BOM 和换行格式，检测外部文件修改，支持另存为。
- EPUB 阅读与编辑：EPUB 2/3 校验、目录、搜索、图片显示、兼容正文编辑和安全保存。
- 阅读体验：自动恢复阅读位置、字体与排版设置、单双栏、主题和自定义背景。
- 本地数据：SQLite 保存书库、设置、进度和书签，支持内容备份与恢复。

## 安全与限制

- EPUB 内容先由 Rust 校验和解析，不执行出版者脚本，不静默联网。
- 不支持安全往返的 EPUB 结构会明确降级为受限或只读。
- TXT 保存前检查源文件指纹，避免覆盖外部修改。
- TXT 超过 40 MiB 需要确认，超过 160 MiB 不支持完整编辑。
- 当前不提供云同步、自动保存、跨书全文索引或 EPUB CSS/字体编辑。

## 构建环境

- Windows 10/11 x64
- Rust 1.88 或更高版本
- Microsoft C++ Build Tools（MSVC）

## 运行与构建

```powershell
# 开发运行
cargo run -p readloom

# Release 构建
cargo build --release --workspace
```

Release 产物：

```text
target\release\readloom.exe
```

## 检查与测试

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## 项目结构

```text
crates/readloom-core    核心领域、TXT/EPUB、书库和持久化
crates/readloom-slint   Windows 原生桌面应用
ui                      Slint 界面
docs                    架构、性能和验收记录
src / src-tauri         保留的 Tauri/Svelte 功能与回归基线
```

旧版 Tauri/Svelte 开发环境需要 Node.js 24+、npm 11+ 和 WebView2；相关命令与历史验证记录保留在 `package.json` 和 `docs` 中。

## License

MIT
