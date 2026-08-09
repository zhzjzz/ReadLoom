# 阶段 4A EPUB 安全编辑验收记录

记录时间：2026-08-08（Asia/Shanghai）

## 结论

阶段 4A 已在最终 Windows release 上完成 EPUB 2/3 运行时草稿、元数据编辑、PNG/JPEG/WebP 封面替换、安全流式重打包、另存为、输出复验、外部修改检测和多标签 dirty/关闭保护。源 EPUB 在成功、失败和取消路径都不会被覆盖；fixed-layout、加密、DRM 和不满足能力约束的出版物保持只读。

## 设计与安全边界

- Publication Draft 只在用户打开编辑面板时惰性创建。元数据和封面使用结构化 Modification Overlay，撤销到原值后恢复 clean；前端提交 revision，Rust 拒绝过期更新。
- 支持标题、多个作者、语言、出版者和描述。OPF 使用 XML 事件级修改，拒绝 DOCTYPE 和过深结构，保留未知节点、属性、refinement、唯一标识关系以及未修改资源。
- 封面按文件内容识别 PNG/JPEG/WebP，并校验扩展名、尺寸、像素数和输入大小。字节只进入 Rust 内存，前端仅持有受会话和资源协议约束的预览 URL，不传 Base64。
- EPUB 2 同时更新 `meta name="cover"` 与封面 manifest/document 引用；EPUB 3 使用 `cover-image` properties。替换时保留旧资源，避免破坏第三方引用。
- 重打包先重新执行 SafeZIP，保持首项 `mimetype` 为精确、未压缩内容，按原顺序和压缩方式逐项 `io::copy`；只覆盖 OPF、选定封面和必要的 cover XHTML。
- 保存前后比较源文件大小、mtime 和 BLAKE3。另存目标不能是源路径；覆盖既有目标需要绑定 draft revision 与目标指纹、120 秒有效且单次使用的确认令牌。
- 临时文件位于目标同目录。输出先重新通过 SafeZIP、EPUB 解析、manifest/spine/元数据/封面检查和未修改资源摘要核对，之后才原子提交。失败保留草稿和既有目标，并清理临时文件。
- 保存命令在 blocking worker 中运行，可在流复制阶段和提交前取消；原子提交阶段不可中断。当前 UI 提供阶段状态和取消，不提供逐字节百分比。
- 资源协议只对已验证图片添加允许 Tauri host 渲染的 CORP；只对安全字体 MIME 添加 CORP/CORS。XHTML/CSS 仍保持 same-origin 隔离，主窗口 CSP 只新增受控 EPUB 图片源。

## 自动化结果

最终执行并通过：

```powershell
npm run check
npm test -- --run
npm run build
cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check
cargo test --all-targets --all-features --manifest-path src-tauri/Cargo.toml
cargo clippy --all-targets --all-features --manifest-path src-tauri/Cargo.toml -- -D warnings
npm run tauri -- build --no-bundle
```

- `svelte-check`：0 error、0 warning。
- Vitest：21 files、57 tests 全部通过。
- Rust：73 passed、0 failed、1 个 release 性能探针按设计 ignored；该探针另行以 release/ignored 模式通过。
- Clippy：`-D warnings` 通过；Rustfmt check 通过。
- Vite：169 modules，编辑面板保持独立动态块。
- Tauri：最终 release 构建成功，产物 `src-tauri/target/release/readloom.exe`。
- Rust 测试命令中的唯一 warning 是 MSVC 链接器中文“正在创建库”stdout 被 rustc 归类为 `linker_messages`；没有源码或 Clippy warning。

定向测试覆盖 lazy/clean/revert、revision 冲突、语言校验、EPUB 2/3 OPF、无封面新增、PNG/JPEG/WebP 内容识别、伪装图片、cover XHTML、强制重打包、SafeZIP 重新打开、manifest/spine、未改资源摘要、取消、外部修改、源路径拒绝、既有目标不受失败影响、单次覆盖令牌、临时文件清理和 TXT 保存回归。

## 最终 release UI 验收

使用 `scripts/create-stage4a-epub-fixtures.ps1` 生成 EPUB 2、EPUB 3、无封面、PNG 和 JPEG 夹具；`scripts/test-stage4a-epub-edit-ui.ps1` 操作最终 release、Windows 原生文件对话框和真实 WebView2/OOPIF，不以浏览器预览替代桌面应用。

实际通过的矩阵：

| 场景 | 结果 |
| --- | --- |
| EPUB 2 + 原封面 | clean 草稿、PNG/JPEG 预览、元数据另存、重新打开、NCX/spine/CSS/图片/内嵌字体均通过；源 SHA-256 未变，exit 0 |
| EPUB 3 + 原封面 | clean 草稿、PNG/JPEG 预览、元数据另存、两章/目录/CSS/图片均通过；源 SHA-256 未变，exit 0 |
| EPUB 3 + 无封面 | 正确显示无原封面，新增封面后另存并重新打开，章节/CSS/图片均通过；源 SHA-256 未变，exit 0 |
| 阶段 3 回归 | 章节切换、隔离 frame、内部资源、外部资源阻断、搜索、书签、未知扩展名 TXT 回退、跨格式标签和窗口退出均通过 |

保存耗时分别为 EPUB 2 861.48 ms、EPUB 3 893.29 ms、无封面 895 ms。截图位于 `target/validation/stage4a-epub2-edited.png`、`stage4a-epub3-edited.png`、`stage4a-no-cover-edited.png`，三张均已视觉检查。

外部修改、保存到源路径、既有目标失败安全和取消边界由直接调用真实文件系统与重打包器的 Rust 集成测试验证；没有把这些结果冒充为鼠标手工操作。系统没有安装 `epubcheck`，因此未运行第三方 EPUBCheck；内部 SafeZIP、解析器、结构和摘要复验全部通过。

## 工作区后续验收

在同一最终 release 上另行打开真实 TXT 夹具，验证了本阶段后的工作区增强：最近文件的小叉会调用 Rust 删除单条 SQLite 历史记录并立即更新界面，磁盘源文件仍保留；TXT 大纲识别“序章”“第一章”“第  十二  章”“999　数字标题”和“尾声”，并排除“正文完”；左右辅助栏均可收起、展开，左分隔条可由 220 px 拖至 300 px，正文同步伸缩。应用通过 Windows 原生文件对话框打开文件，并在验收后正常退出。

浏览器开发预览也完成桌面布局回归：左右栏收起/展开、分隔条鼠标拖动及键盘调宽均通过，控制台无 warning 或 error。最终 release 截图为 `target/validation/workspace-outline-layout.png`。

后续布局回归进一步验证了左栏收起状态：导航隐藏后顶部品牌宽 56 px、左分隔条 8 px、正文区 978 px、右侧栏 286 px，未再发生五列网格子项前移导致正文只剩 8 px 的错位。EPUB 目录新增独立横向分隔条，真实 release 中从 220 px 拖至 300 px，章节视口同步从 530 px 缩至 450 px；同时覆盖方向键、Home 和 End 键盘调宽。验收截图为 `target/validation/workspace-left-collapsed.png` 与 `target/validation/epub-toc-resized.png`。

产品界面随后移除了开发阶段、文档 revision、后端/协议版本和启动耗时等内部信息。左右两处“文件安全”区域也已删除；外观与可自定义的 TXT 标题识别正则默认隐藏，通过顶部“设置”按钮打开。最终 release 的可见文本扫描未发现阶段编号或内部版本文案。

TXT 阅读区最终统一采用 32 px 顶部留白，不再根据短文档高度做垂直居中；包括 `.env` 在内的小文件从编辑器顶部开始显示，长文档保持正常滚动。动态均衡布局辅助模块及其测试已删除。

最终设置面板与 `.env` 顶部对齐验收截图为 `target/validation/settings-env-top.png`。

## 已知限制

- 阶段 4A 不编辑正文、目录、CSS、字体或其他结构资源，也不覆盖原 EPUB。
- fixed-layout 仍按阶段 3 尽力只读呈现，不开放编辑；加密和 DRM 不尝试绕过。
- 图片验证读取格式头和尺寸边界，不做完整像素解码或 PNG CRC 重算；保存后的资源仍须通过协议 MIME 签名和 EPUB 结构复验。
- 进度是粗粒度阶段提示，不是逐字节百分比；原子替换的最终提交不可取消。
- 安装器仍未生成，因为 `bundle.active = false`；签名、图标和安装包属于后续阶段。
