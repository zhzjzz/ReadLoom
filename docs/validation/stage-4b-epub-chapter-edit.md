# 阶段 4B EPUB 章节正文编辑验收记录

记录时间：2026-08-09（Asia/Shanghai）

## 结论

阶段 4B 已实现兼容 EPUB 2/3 spine 章节的实时可视化编辑，并复用阶段 4A 的 Publication Draft、安全另存为、外部修改检测、流式重打包和输出复验。普通阅读不会加载 Tiptap；进入编辑后只创建一个 EditorView，并以修订化多章节草稿、550 ms 防抖、IME composition gate、单 in-flight/coalescing 队列同步 Rust。源 EPUB 在测试的成功、失败、撤回和多章节路径中保持字节不变。

## 安全模型与降级

- Rust 解析 XHTML 并保留 XML 声明、`html/head/body` 外壳、命名空间、head/CSS 引用和未编辑外围字节；正文转换为封闭的 ProseMirror JSON 词汇。
- 完整支持段落、H1–H6、换行、粗体、斜体、删除线、下划线、上下标、引用、无序/有序列表、分隔线、文本对齐、安全链接和 manifest 内部图片。
- `full` 可正常编辑；仅含可说明样式损失的章节为 `limited`；表格、Ruby、MathML、SVG、音视频、未知结构或无法证明无损的属性为 `read-only`；无正文或无法安全解析为 `unsupported`。UI 展示原因，不静默删除结构。
- 脚本、事件属性、form/iframe/object/embed、DOCTYPE 外部实体、危险 URI、外部图片、路径逃逸和超限内容不能进入可保存草稿。粘贴 HTML 会先去除 Word 噪声、隐藏/跟踪节点、脚本样式和外部/data 图片，再由 ProseMirror schema 解析。
- 保存只接受 Rust 白名单序列化器生成的 XHTML，不读取编辑器 `innerHTML`。内部链接和图片必须解析到当前 manifest 或本次已导入资源。
- 导入 PNG/JPEG/WebP 复用内容签名、扩展名、尺寸、像素数和大小校验；未被章节引用的临时导入不会写入输出。已引用图片使用唯一安全路径和 manifest ID，并在 OPF 中加入相对 href。

## 草稿、同步与保存

- `ChapterEditDto` 携带章节身份、资源 hash、兼容级别、能力、警告、editor document、accepted revision、preview revision 与 dirty 状态。
- 前端同步请求含 `chapterEditId/baseRevision/clientRevision/requestId`；Rust 拒绝过期或倒退 revision。失败保留本地状态，状态栏区分“本地未同步、同步中、已同步未另存、已另存、同步失败”。
- 快速输入期间只保留一个定时器和一个进行中请求；新状态覆盖待发状态。切章、退出编辑、另存为和关闭前都显式 flush，失败则阻止动作。
- 多章节草稿彼此独立，目录展示未另存修改点；内存只缓存最近 3 个 ProseMirror EditorState，但 DOM 中始终只有一个 EditorView。
- 另存时章节 XHTML、OPF、封面和被引用导入图片组成统一 Modification Overlay。保存完成后重新打开输出，复验全部修改章节和未修改资源摘要，然后才提交目标。

## 自动化结果

最终通过：

```powershell
npm run check
npm test -- --run
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --all-targets --all-features --manifest-path src-tauri/Cargo.toml -- -D warnings
npm run tauri -- build --no-bundle
```

- Svelte/TypeScript：0 error、0 warning。
- Vitest：25 files、69 tests 全部通过；其中专门覆盖 4 章穿过 3 个 EditorState 缓存上限、最近访问刷新和活动章不被淘汰。
- Rust：79 passed、0 failed、2 个 release 性能探针按设计 ignored；阶段 4B 探针另行以 release/ignored 模式通过。
- Clippy：全 target/feature 且 `-D warnings` 通过；Rustfmt check 通过。
- Vite：215 modules；普通阅读入口与章节编辑器分块，未安装 React/Vue/Tiptap 框架适配器。
- Tauri：最终 release 成功，产物 `src-tauri/target/release/readloom.exe`；仅出现 MSVC 链接器中文 stdout 被 rustc 归类为 `linker_messages`。

测试覆盖 XHTML 中文/emoji/组合字符、命名空间/head/CSS/`epub:type` 往返，危险与不支持结构降级，脚本/外部图片/JavaScript 链接拒绝，粘贴净化，IME gate、防抖、单 in-flight、coalescing、失败保留、revision 冲突、多章节独立 dirty/revert、图片 manifest/ZIP 写回、源文件不变、另存后重新打开，以及 TXT/EPUB 工作区回归。

针对真实文件 `D:\a\为美好的世界献上祝福.epub` 的兼容性回归扫描了 94 个 `OPS/content*.xhtml`：全部分析为 `full`。新增回归覆盖出版方常见的 `span[role]`、图片 `id/class`、包裹图片的 `span[class]` 以及 XML 预定义实体；这些属性和内联层级会安全往返，不再误降级为只读。

## 真实 release 壳层验收

在最终功能实现上运行 `scripts/test-stage4b-epub-chapter-ui.ps1` 的完整成功记录覆盖：普通阅读不加载编辑器、进入编辑后恰好一个 EditorView、DOM composition gate、章节同步、格式按钮与撤销/重做、脏稿关闭取消、危险粘贴清洗、图片导入与 alt、隔离分栏预览、两章独立草稿恢复、退出后的修改标记、另存为且源 hash 不变、输出 XHTML/OPF/图片验证、输出重开、只读降级和 1.68 MiB XHTML 大章。结果为：

| 指标 | release 实测 |
| --- | ---: |
| 首次按需加载章节编辑器 | 118.69 ms |
| 原生另存为与输出复验 | 2,586.15 ms |
| 大章编辑器加载 | 9,208.08 ms |
| 普通编辑进程树 Working Set / Private | 575,340,544 B / 417,394,688 B |
| 大章进程树 Working Set / Private | 721,313,792 B / 555,335,680 B |

截图为 `target/validation/stage4b-normal-edited.png`。另行运行的阶段 4A release 回归验证元数据、封面和安全另存链路；阶段 3/TXT release 回归验证 EPUB 隔离阅读、内部/外部资源策略、章节、搜索、书签、未知扩展名文本回退和多标签保留。生产预览在未进入章节编辑时没有请求 Tiptap/ProseMirror 动态块，控制台无 error/warning。

同一 release 还对上述真实文件执行了兼容性壳层探针：自动跳到第 5/95 章并进入可视化编辑，确认 `span[role="heading"]` 与 `img#image-26.s2.s3` 保留、旧的“不可靠保留元素属性”只读警告消失、编辑器仍按需加载，且源文件 SHA-256 不变。截图为 `target/validation/konosuba-compat-probe.png`。

成功记录使用两章真实 EPUB；随后验收夹具和壳层脚本已扩展为四章 LRU 往返，且缓存算法的四章自动化回归已通过。扩展后的最后一次壳层复跑被本机 WebView2 Runtime 在创建页面前的进程崩溃阻断，未产生四章 GUI 成功记录；这不是应用断言失败，但仍按未完成的额外实机覆盖记录，不将其计为通过。

IME 验收通过真实 ProseMirror DOM 的 `compositionstart/compositionend` 事件和同步单元测试验证发送门控；本轮没有使用 Windows 拼音候选窗完成物理键盘选词，因此不声称覆盖输入法 UI 本身。

## 交互与快捷键

- 工具栏包含撤销/重做、标题、粗体、斜体、删除线、下划线、引用、列表、分隔线、链接、图片导入/替换/alt/删除和对齐；按钮状态直接取自 EditorState。
- `Ctrl+E` 切换阅读/章节编辑，`Alt+↑/↓` 切换章节；编辑器内 `Ctrl+B` 为加粗，阅读器内仍为书签，不发生全局快捷键抢占。
- 编辑、分栏、预览三种布局共享同一草稿；预览 iframe 仍使用无 `allow-same-origin` 的 sandbox 和受控自定义协议。

## 已知限制

- 不编辑目录、spine 顺序、CSS、字体、脚注模型、表格、Ruby、MathML、SVG、音视频或 fixed-layout 正文；这些结构保持阅读或只读降级。
- 当前是章节级安全往返，不承诺对任意出版者 HTML/CSS 做像素级 WYSIWYG；编辑区提供可预测语义样式，预览区展示保存前 XHTML。
- 导入图片不做裁剪、压缩或 EXIF 旋转；仅支持 PNG/JPEG/WebP。删除引用后资源不会写入下一次输出。
- 仍然只允许另存为；不自动保存、不覆盖源 EPUB，也没有第三方 EPUBCheck。
