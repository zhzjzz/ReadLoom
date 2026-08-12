# Slint 阶段 3 TXT/EPUB 阅读编辑、完整设置与 Windows 行为验收

记录时间：2026-08-11（Asia/Shanghai）

## 结论

Slint 原生 release 已补齐三个可用纵向切片：TXT 阅读与编辑、EPUB 连续阅读、设置持久化。TXT 打开时记录编码、BOM、检测换行、主换行和 BLAKE3 文件指纹；编辑后可覆盖保存或原生“另存为”，保存使用同目录临时文件、备份替换和写后校验，并拒绝覆盖外部修改。UTF-8、UTF-16 LE/BE、GBK 与 GB18030 均沿用无替换字符的严格编码策略。

EPUB 2/3 先经过 ZIP 容器、条目数量/大小/压缩比、路径穿越、Unicode/大小写冲突、加密与 DRM 标记检查，再由 `rbook` 解析阅读顺序；XHTML 只转换为 Readloom 自有的标题、段落和已验证图片资源，出版社 HTML、CSS 和脚本不会直接进入 Slint。目录标题优先采用 EPUB 2 NCX / EPUB 3 Navigation 标签；PNG/JPEG/WebP/GIF 正文图片进入封闭布局，标准封面或明确的封面章节资源会写入书库封面缓存。目录、全文搜索、段落跳转、关闭后阅读位置恢复均复用原生阅读工作区。

设置页默认进入“阅读 / 阅读排版”，内部导航按“外观、阅读、操作、书籍、数据、高级”显示分组标题，并拆成主题、字体、页面布局、阅读排版、快捷键、章节识别、备份、文件关联、缓存、硬件加速、DPI 共 11 个独立子页面。设置写入 schema v4 的 `app_preferences` 表，键迁移为 `readloom.app-settings.v2`，读取时兼容第一版原生 JSON。主题支持亮色、暗色与读取 Windows `AppsUseLightTheme` 的系统模式；六种字体只引用本机安装字体并回退到系统中文字体。背景会校验 PNG/JPEG/WebP 签名和 20 MiB 上限，再复制到应用数据目录；“主题”和“页面布局”绑定同一份背景数据，背景强度、3/4/5 列书库和关闭/最小化到托盘均立即保存。

阅读排版恢复了字号、字重、字距、首行缩进、行距、em 段距、对齐、版心、边距和单双栏的独立设置，以及实时示例卡。TXT 的段首、三种空行、保守错误换行合并和三种标题样式会重新构建封闭阅读模型但不改变源文本；章节正则在核心层编译校验。EPUB 原始样式、内嵌字体以及字体/字号/缩进/行距/段距五个正文覆盖项彼此独立。十项快捷键默认空白，保存时执行大小写不敏感冲突检查，并实际分派到打开、保存、另存、关闭、编辑、章节、书签、书库和设置命令。

数据页使用 ZIP Deflate 流式写入 `.readloom-backup`，逐文件计算 SHA-256 并按内容去重；每次备份可在原生另存对话框中命名，界面先显示“正在备份”。恢复既可多选备份，也可选择目录并默认读取目录内全部备份，校验清单、路径、大小和内容哈希，在不同备份之间继续去重，只恢复书籍文件并重新经过 TXT/EPUB 打开校验。高级页按要求只显示文件关联、缓存、硬件加速和 DPI 状态。

界面已回归旧版 Readloom 的工具型工作台风格：48px 顶栏、左对齐的工作区入口、蓝灰色控件层级、居中的书库内容、文档工具栏和状态栏。工作区、TXT/EPUB 目录、设置导航与阅读辅助侧栏均可拖动调整宽度；搜索、书签和书籍信息是三个独立的右侧面板，默认关闭。书卡支持整卡或“打开”按钮单击打开，并提供应用内静音确认的“移除”按钮；确认后清除书库记录、阅读进度、书签和最近打开信息，但不删除源文件。阅读区支持多书标签，返回书库不会销毁会话；再次进入时显示第一本，关闭全部后显示“请打开书籍”。超大 EPUB 的已生成 Slint 模型保存在会话内存缓存中，标签切换不重复解码图片和重建数万行。设置详情独立滚动，阅读排版在宽窗采用“表单 + 实时预览”，较窄时预览自动移到表单下方；数值、按钮和开关采用固定标签列与统一卡片间距。

EPUB 编辑器仍未迁移；当前 Slint 版本只读显示 EPUB，旧 Tauri/Svelte 应用继续作为 EPUB 编辑功能的基线。

## 自动化结果

最终执行并通过：

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p readloom-core
cargo check --workspace
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\check-windows-gui-subsystem.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\verify-slint-stage1.ps1
npm.cmd test -- --run
npm.cmd run check
cargo test --all-targets --all-features --manifest-path src-tauri/Cargo.toml
```

- workspace：34 passed（核心 30、Slint 4），包括 TXT 编码/换行保真保存、外部修改冲突拒绝、设置默认值/迁移/归一化、快捷键冲突、章节正则、TXT 重排、背景生命周期、书库移除级联清理且不删除源文件、新建/移动分组、无效记录清理、目录扫描、书库筛选、窗口图标、安全 EPUB 路径拒绝、EPUB 导航标题/封面/正文图片/搜索/定位、阅读滚动稳定性，以及 50,000 段 TXT 深层恢复。
- 新 workspace：rustfmt、check、Clippy `-D warnings` 全部通过。
- 旧前端：35 个测试文件、115 项测试通过；`svelte-check` 为 0 errors、0 warnings。
- 旧 Tauri Rust：100 passed、2 个 release 性能探针按设计 ignored。

## 原生 release 启动记录

`scripts/verify-slint-stage1.ps1` 曾使用独立临时 SQLite 真实启动 release，等待主窗口句柄并遍历完整子进程树。窗口、内存和子进程数据来自上一轮启动探针；EXE 大小、SHA-256 和 PE Subsystem 已在本轮重建后重新读取，本轮没有再次启动窗口：

| 指标 | 结果 |
| --- | ---: |
| 窗口就绪 | 264 ms |
| 工作集 | 74,215,424 bytes |
| 私有内存 | 113,688,576 bytes |
| 子进程 | 无 |
| `msedgewebview2.exe` | 0 |
| PE Subsystem | 2 (`Windows GUI`) |
| 当前 EXE 大小 | 11,875,840 bytes |
| 当前 SHA-256 | `5BD7002CB0866C4D66B49143D1A6966688E7B7CA8A272809F97855D0A7C5C58E` |

该启动值使用空白验收数据库，不等同于包含大量封面或 50,000 段正文后的内存。核心长文本恢复已有自动化保护；下一轮应增加带 1,000/50,000 段真实 Slint 窗口的进程内存采样与连续滚动 UI 自动化。

## 视觉验收

本轮使用 Windows Computer Use 对最终 release 执行了实际窗口验收：检查书库筛选布局、空阅读区、两本 EPUB 标签会话、阅读位置恢复、目录、正文图片、独立搜索/书签侧栏、书签位置列表、设置排版、命名备份入口和封面刷新。标题栏实际显示黑底白色 `R`，Slint `Window.icon`、运行时 `WM_SETICON` 与 PE 资源使用同一图标。真实渲染截图包括：

- `target/validation/slint-settings-functional.jpg`
- `target/validation/slint-txt-editor-functional.jpg`
- `target/validation/slint-epub-reader-functional.jpg`
- `target/validation/slint-original-library-complete.png`
- `target/validation/slint-original-reader-complete-final.png`

最终验收中，书库类型与分组控件保持分离；《女侠且慢》的 EPUB 封面在刷新后显示为真实封面，正文图片也能在阅读列表中显示。搜索与书签切换为互斥侧栏，新增书签后立即出现章节名和段落编号；返回书库后两本已打开 EPUB 仍以标签保留。后续仍应把这套坐标/语义混合验收固化为可重复 UI 自动化。

`scripts/check-windows-gui-subsystem.ps1` 在修复前稳定报告 `Subsystem=3` 并失败；加入 release-only `windows_subsystem = "windows"` 后报告 `Subsystem=2`。隔离启动探针的完整子进程列表为空，不再出现 `conhost.exe`，因此双击 release EXE 不会弹出 cmd 窗口。
