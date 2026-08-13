# Readloom（阅织）

Readloom 是一个面向 Windows 10/11 的本地优先阅读与编辑器，支持 TXT 安全编辑与搜索、TXT/EPUB 多标签、SQLite 本地状态、EPUB 2/3 隔离阅读，以及 EPUB 元数据、PNG/JPEG/WebP 封面与兼容章节正文的可视化编辑和安全另存为。

EPUB 不执行出版者脚本、不整包解压、不静默联网。章节正文先由 Rust 转成受限的 Tiptap/ProseMirror JSON 文档，再由白名单序列化器写回 XHTML；不支持或无法无损往返的结构会明确降级为受限或只读，不会以浏览器 `innerHTML` 作为保存源。编辑使用内存草稿和流式重打包，只允许另存为新的 `.epub`，不会覆盖源书；保存前后均检查源文件指纹，输出还会重新通过 SafeZIP、解析器和结构校验。加密、DRM、fixed-layout 和不满足安全能力的 EPUB 仍为只读。项目不包含跨书全文索引、自动保存、云同步、EPUB 目录/CSS/字体编辑或大型 TXT 增量编辑器。TXT 超过 40 MiB 需确认，超过 160 MiB 拒绝完整编辑。

“打开文件”和窗口拖拽共用一条导入路由：`.epub` 使用隔离阅读器，其他扩展名或无扩展名文件按文本打开。

应用默认进入本地书库；从书库移除图书时会清除对应的书库记录、最近打开信息、阅读进度和书签，但始终保留磁盘上的源文件。书库会解析并以受限只读协议显示 EPUB 原封面；没有可用封面时使用固定 2:3 占位封面，以多行书名代替单字，书名长度、字号和封面尺寸会随每行 3、4、5 本的设置自动调整，溢出时显示省略号。书库可一键清理已移动或删除的无效记录。导入图书支持 Ctrl/Shift 多选 EPUB、TXT，也可递归扫描指定目录（跳过符号链接，一次最多 2,000 本），导入只加入书库，不会批量打开标签页。书籍可按普通书架形式分组，支持新建、重命名、删除分组和逐书移动；删除分组只会把其中书籍移回“未分组”。

TXT 会先按空行、标题和保守的硬换行规则识别段落，再进入统一排版层：原始段首空格、空行与 CSS 段距分别处理，自动合并错误换行默认关闭。阅读正文采用带估算高度和二分定位的窗口化渲染，无论全书有多少段，DOM 同时最多保留 600 个正文块；滚动、搜索跳转和阅读位置恢复会移动窗口，而不是一次性把整本书插入页面。TXT 大纲、全文搜索和书签显示在正文旁的独立可拉伸侧栏中；EPUB 搜索结果也有独立可拉伸侧栏。两种搜索都会显示结果总数和序号。EPUB 保存章节、段落索引和字符偏移，TXT 保存首个可见段落及字符位置，关闭后再次打开会自动恢复阅读处；字体或窗口宽度变化后不依赖旧页码。

独立设置页按“外观、阅读、操作、书籍、数据、高级”组织。“主题”和“页面布局”复用同一份背景与可读性设置，自定义 PNG/JPEG/WebP 背景会延伸到 TXT/EPUB 正文下方。“阅读 / 阅读排版”统一管理字体、字号、字重、字间距、缩进、行距、段距、对齐、正文宽度、边距和单双栏，并为 TXT 预处理及 EPUB 原书样式/各项覆盖提供独立开关；EPUB 覆盖仅限定正文选择器，不会对全局 `*` 粗暴应用 `!important`。常用免费中文字体以系统安装字体和可靠回退链使用，不在程序内捆绑大型字体文件。快捷键默认均为“无”，由用户按功能逐项配置，并可一键恢复默认排版。

多选文件或扫描目录后会先显示导入预览，列出文件名、格式、大小、状态和完整位置；已在书库的项目不可重复勾选，用户可取消任意新书后再确认。数据备份使用内容型 `.readloom-backup` ZIP：TXT 使用 Deflate，已压缩的 EPUB 直接存储，并按 SHA-256 去重。备份只包含书籍内容，不包含路径、书签、进度、分组、设置或阅读记录；恢复可同时选择多个备份，校验清单、路径、尺寸、压缩比和内容哈希后再写入指定目录，并继续按内容去重。

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

## Slint 原生迁移切片

仓库当前保留上面的 Tauri/Svelte 应用作为功能和回归基线，同时新增两个原生 Rust crate：`readloom-core` 封装书库、TXT 安全编辑、EPUB 受控布局、搜索/阅读位置和设置持久化，`readloom` 提供不依赖 WebView2 的桌面界面。原生版本默认读取旧版的 `%APPDATA%\app.readloom.desktop\readloom-state.sqlite3`，现已支持：

- 原生文件选择器打开 TXT/EPUB，旧书库 3/4/5 列展示；
- TXT 目录、搜索、长文本虚拟化、编辑、保留编码/换行保存与另存为；
- EPUB 2/3 安全 ZIP 校验、XHTML 到封闭段落模型、目录、搜索和阅读位置恢复；
- 原风格六类设置页、Windows 主题、系统字体回退、背景、完整 TXT/EPUB 排版和实时预览；
- 快捷键冲突检测与实际命令分派、TXT 章节正则、SHA-256 内容备份/多备份恢复；
- Windows 系统托盘、关闭/最小化行为，以及不弹出控制台的 GUI 子系统 release EXE。
- 书库多选文件/递归目录导入、即时搜索、类型/分组筛选、排序、新建分组、逐书换分组和无效记录清理；
- 可拖动调整工作区、TXT/EPUB 目录、设置导航和按需打开的 EPUB 搜索侧栏宽度。

TXT 覆盖保存会校验打开时文件指纹，发现外部修改即拒绝覆盖。EPUB 出版社 HTML/CSS/脚本不会直接进入 Slint；当前原生 EPUB 是阅读切片，EPUB 编辑仍由旧版承担。

```powershell
npm run slint:check
npm run slint:run
npm run slint:build
npm run slint:verify
```

Slint release 产物位于 `target\release\readloom.exe`。`slint:verify` 会运行核心测试、构建 release、断言 PE 子系统为 Windows GUI、真实启动窗口、记录启动时间与内存，并断言进程树中没有控制台或 `msedgewebview2.exe` 子进程。

## 验证命令

```powershell
npm run check
npm run test
npm run build

cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --all-targets --all-features --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --all-targets --all-features --manifest-path src-tauri/Cargo.toml
npm run build:exe
```

`src-tauri/tauri.conf.json` 关闭安装包 bundling；`npm run build:exe` 会启用 Tauri 的生产资源协议、生成 release 可执行文件，并真实启动产物确认内嵌前端已经就绪。不要用普通的 `cargo build --release` 代替该命令；项目会拒绝未启用生产协议的 release 构建，避免 WebView 尝试连接未运行的 Vite 开发服务器。该 `.exe` 已包含 Rust 应用核心，并为 `x86_64-pc-windows-msvc` 静态链接 C/C++ 运行库，不要求目标电脑安装 Rust 或 Visual C++ Redistributable；目标 64 位 Windows 10/11 仍需 Microsoft Edge WebView2 Runtime（多数系统已预装）。当前没有安装器、代码签名或自动安装 WebView2 的引导程序。

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
