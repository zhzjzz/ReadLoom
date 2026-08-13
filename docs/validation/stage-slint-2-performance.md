# Slint 阶段 2 EPUB 图片滚动性能与文档会话收敛

记录时间：2026-08-12（Asia/Shanghai）

## 结论

本阶段修复了 EPUB 正文图片首次进入可见模型时的 UI 线程长任务。修复前，`ReaderParagraphModel::row_data` 会同步解码和缩放图片；1,600×800 PNG 的可重复测试记录到首次模型读取约 580.6 ms。修复后，同一路径立即返回占位项并登记后台任务，首次模型读取约 0.41 ms；调试构建中的图片解码约 674 ms，但全部发生在专用后台线程。完成结果由 40 ms 后台结果定时器接收，仅在 UI 线程把已缩小的共享 RGBA 像素转换为 Slint `Image`，并通过 `ModelNotify` 刷新对应行。

性能反馈环：

```powershell
cargo test -p readloom-slint epub_image_rows_do_not_decode_on_the_ui_thread -- --nocapture
```

该测试真实创建包含大 PNG 的 EPUB，经 `ReadloomCore::open_epub` 进入 `ReaderParagraphModel::row_data`，验证首次读取只返回占位项，随后等待后台结果并确认同一模型从缓存取得真实图片。它不使用易受机器负载影响的硬编码毫秒阈值；耗时只作为诊断输出，行为断言负责判红。

## 图片流水线边界

新增 `crates/readloom-slint/src/reader_images.rs`，用 `ReaderImagePipeline` 封装以下职责：

- 单一后台解码线程，Slint 模型不再执行图片解码或缩放；
- 请求通道和结果通道容量均为 8，队列满时立即返回占位，不阻塞 UI；
- 以书籍指纹、章节和图片索引作为缓存键，跨阅读窗口复用同一解码结果；
- 待处理键去重，避免同一可见行重复排队；
- 已解码图片维持 32 MiB 字节预算 LRU；
- 失败图片维持最多 128 个键的负缓存，避免损坏资源反复解码；
- 输入图片限制为最大单边 8,192 像素、解码分配 128 MiB，超限安全降级为空图；
- 最终进入 Slint 前缩放到不超过 1,200×1,200；
- `EpubImageResource.bytes` 改为 `Arc<[u8]>`，后台请求共享压缩字节，不复制整张图片。

调试构建会输出 `[readloom:perf]` 图片解码耗时，release 不输出该诊断。新增测试覆盖 UI 线程占位/后台完成链、缩放、尺寸拒绝、字节预算 LRU 和有界失败缓存。

## 文档会话模块

新增 `crates/readloom-slint/src/document_workspace.rs`。`DocumentWorkspace` 现在拥有当前 Document Session 与打开标签集合之间的不变量，向启动回调暴露 `active`、`contains`、`select`、`upsert`、`activate`、`replace_path`、`close` 和只读快照。关闭当前标签会原子选择下一本或进入空状态；保存、另存、结构化 TXT 设置刷新和标签切换都经过同一接口。

该模块不持有 `MainWindow`。Slint 回调仍只持有弱窗口句柄，因此没有引入 UI/会话强引用循环。对应测试覆盖路径去重、激活切换和关闭当前标签后的生命周期。

## 当前范围与后续测量

本阶段只修复了已量化的 EPUB 图片 UI 线程热点，没有宣称所有滚动场景已达到固定帧率。仍需用真实 release 窗口继续测量：

- 超长 TXT 持续滚动时 `reader-viewport-y` 回调频率和单次任务耗时；
- 多个后台打开/搜索/切章结果同时到达时，40 ms 轮询任务的排空耗时；
- 阅读位置的 SQLite 持久化在慢磁盘上的 UI 线程长尾；
- 含大量独立图片的 EPUB 快速滚动时占位体验和缓存命中率。

## 自动化结果

最终结果以本阶段结束时执行的仓库验收命令为准：

```powershell
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p readloom-slint
```

workspace 测试总数为 63：`readloom-core` 36 项、`readloom-slint` 27 项。

## 阅读背景与书库分组回归

本阶段同时修复了两个可复现的交互问题：

- 阅读器活动段落不再绘制不透明浅色底，而是使用随主题变化的半透明强调层，背景图片会连续透出；回归测试覆盖单栏和双栏布局。
- “换分组”不再按索引轮换到下一个分组。点击后弹出目标分组选择器，显式列出“未分组”和所有自建分组，选中后才提交移动；目标列表在 Rust 侧预先过滤，避免 Slint `ListView` 隐藏代理项造成行重叠。

使用原生调试窗口完成了视觉复测：背景图片连续显示，分组选择器可同时显示“未分组”、`a`、`b` 三个独立目标。复测过程中只打开并取消弹窗，没有移动书库条目。
