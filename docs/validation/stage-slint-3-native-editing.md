# Slint 阶段 3 TXT / EPUB 原生编辑

记录时间：2026-08-13（Asia/Shanghai）

## 结论

阅读视图现在直接承载 Slint 原生 `TextInput`，编辑状态不会切换到全屏字符串编辑器。背景、EPUB 图片、单/双栏正文与 `ScrollView` 保持原位；输入只更新活动标签的 `EditSession` 草稿，保存完成也不重建当前展示模型。

每个标签拥有独立草稿、撤销/重做栈和 `ViewAnchor`。锚点由稳定 `BlockId`、章节键、UTF-16 光标偏移和相对视口顶部的精确像素偏移组成。恢复滚动使用 Slint 上报的实际块几何，不再以固定行高估算；编辑器重新挂载时会把 UTF-16 光标转换为合法 UTF-8 字节边界并恢复原生焦点/选区。

## 保存边界

- TXT 草稿保留原始字节的编码、BOM 和换行风格，只物化并替换修改过的源区间；覆盖保存前检查文件指纹。
- EPUB 只编辑当前活动章节中的文本块。XHTML 文本会执行 XML 转义并保留已有行内元素和属性；未修改 ZIP 条目通过 raw copy 原样搬运。
- EPUB 重打包保持 `mimetype` 为首条且不压缩，保留图片、CSS、字体、导航、NCX、元数据和未知条目，并在替换原文件前重新打开归档、校验结构和正文结果。
- TXT 与 EPUB 都写入同目录临时文件，验证通过后才替换目标；只读、外部修改冲突或验证失败不会破坏原文件。冲突界面提供另存为或取消。
- 保存使用修订票据：保存修订 N 期间发生修订 N+1 时，旧保存成功不会错误清除脏状态。

## 交互范围

统一工具栏提供编辑、保存、另存为、撤销、重做和取消；支持 `Ctrl+S`、`Ctrl+Shift+S`、`Ctrl+Z`、`Ctrl+Y` 与 `Ctrl+Shift+Z`。关闭标签、切换书库/设置、移除书籍和退出窗口都会在脏草稿存在时要求保存、放弃或取消。

本阶段明确不包含富文本样式、图片和 CSS 编辑。`Enter` 作为当前文本块内的真实换行；为避免错误改写 EPUB DOM，目前不支持跨块合并或在 EPUB 中用回车创建新的兄弟元素。脏的 EPUB 活动章节在保存或取消前不能切换章节。

## 自动化结果

最终验收命令：

```powershell
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p readloom
```

结果：`readloom-core` 50 项测试、`readloom` 44 项测试全部通过；release 二进制成功生成。覆盖内容包括 TXT 的 UTF-8 BOM、UTF-16 LE/BE、GBK/GB18030、LF/CRLF、外部修改冲突和只读保护，以及 EPUB 的 XHTML 转义、行内元素保留、ZIP 条目保留、`mimetype` 约束、重开验证、冲突另存和失败回滚。

## 原生窗口验证状态

最终 release 二进制以隔离状态库启动，Windows 报告进程响应正常，并在启动时返回非零 `MainWindowHandle`。但是本次 Codex Computer Use 会话的可寻址窗口清单未暴露 Readloom，只返回了其他桌面应用；直接让 Computer Use 启动该可执行文件的尝试又在应用批准阶段超时。

因此本阶段确认了 release 原生进程/窗口能够启动，但没有完成可复核的鼠标、键盘、IME 和截图视觉验收，也不把它误报为已完成。需要在 Computer Use 能识别该 Slint 窗口后，继续执行 TXT/EPUB 打开、长文深处编辑、保存不跳顶、标签切换光标恢复、冲突、撤销重做与关闭拦截的真实窗口检查。

## 2026-08-13 打开文档退出修复

Windows 事件日志确认 release 进程以 `0xc0000409` 退出；stderr 将原因定位为 Slint `properties.rs` 的 `Recursion detected`。最小复现只需打开一个单行 TXT 或一章一段的 EPUB，与文件大小、图片、旧数据库和阅读设置无关。

根因是正文代理项在 `changed y` / `changed height` 属性处理器中同步读取自身 `y` / `height` 进行锚点几何上报。release 优化下，这会在属性尚处于求值状态时重入同一属性。现改为独立的布局测量代次：模型挂载、窗口缩放、排版设置变化或文本编辑后，Rust 定时器在下一次事件循环中递增测量代次；代理项只在该代次变化时读取已经稳定的实际几何。因此仍保留精确像素锚点，没有退回固定行高估算。

修复后的 release 外部进程复现分别打开最小 TXT、最小 EPUB、原先稳定崩溃的真实 EPUB 和大型合成 TXT，每个观察 5 秒；四个进程都保持存活并持有非零窗口句柄，stderr 均未再出现属性递归。正式回归测试同时禁止在 `y` / `height` 自身 change handler 中执行几何上报。

## 2026-08-13 TXT 快速滚轮卡顿修复

TXT 的视口变化回调曾通过 `OpenDocument::paragraphs()` 把全文复制成新的 `Arc<Vec<_>>`，随后从第一个段落开始线性查找当前锚点；阅读位置回调还会再复制一次全文。EPUB 返回已经共享的当前章节 `Arc`，所以没有同等卡顿。

确定性反馈环使用 99,999 个阅读块、96 个展示块：修复前一次滚轮锚点查找检查 99,999 个块并失败；修复后只在 `ReaderParagraphModel` 的展示窗口内定位，最多检查 96 个块。活动章节和锚点更新也直接读取同一有界模型，不再在滚轮热路径生成 TXT 全文快照。阅读位置仍按原有 350 ms 单次定时器合并保存，持久化语义不变。
