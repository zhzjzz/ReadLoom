# 阶段 4B 性能基线

记录时间：2026-08-09（Asia/Shanghai）

## 环境与口径

- Windows 11，x86_64 MSVC；最终 release 使用仓库既有 size/LTO/panic/strip 配置。
- Rust 探针命令：`cargo test --release --manifest-path src-tauri/Cargo.toml epub_chapter_edit_release_performance_probe -- --ignored --nocapture`。
- 探针打开两章 EPUB，将 250 段中文、emoji 和组合字符组成的 125,666 B 编辑器 JSON 连续同步 40 次，再执行流式重打包、SafeZIP/解析器/章节复验和原子另存。
- 前端大小为 Vite 最终产物逐文件原始大小与 GZipStream 压缩后求和；阶段 4A 对照来自同仓库上一阶段记录。

## 章节草稿与另存实测

| 指标 | release 实测 |
| --- | ---: |
| 创建 Publication Draft | 392 µs |
| 解析并创建 Chapter Edit Draft | 374 µs |
| 编辑器 JSON 大小 | 125,666 B |
| Rust 校验、序列化并接受一次同步（40 次平均） | 581 µs |
| 防抖窗口 | 550 ms |
| 单次同步硬上限 | 3,145,728 B |
| 安全另存 + 全部内部复验 | 12,841 µs |
| 输入 / 输出 EPUB | 1,734 B / 2,760 B |

后端平均同步耗时只占 550 ms 防抖窗口约 0.11%；用户连续输入时前端只保留一个进行中请求并合并后续状态，IME composition 期间不发送。该小型归档的 12.8 ms 另存时间不能外推到接近上限的大书；阶段 4A 已用 64 MiB 流复制样本覆盖大归档路径。

## 构建产物与按需加载

| 指标 | 阶段 4B | 相对阶段 4A 开始前基线 |
| --- | ---: | ---: |
| `readloom.exe` | 6,694,912 B | +260,096 B（+4.04%） |
| 全部前端资源（原始） | 910,052 B | +402,029 B（+79.14%） |
| 全部前端资源（逐文件 gzip） | 299,203 B | +134,642 B（+81.82%） |
| 主入口 JS | 115.61 kB（gzip 39.91 kB） | 约 +5.53 kB（gzip +1.58 kB） |
| 章节编辑组件 JS + CSS | 25.91 kB（gzip 9.52 kB） | 新增动态块 |

Tiptap/ProseMirror 的核心与扩展都由进入章节编辑模式后的动态 import 拉取；普通阅读主入口只增加状态协调代码。全部前端 gzip 增量为 134,642 B，落在阶段目标的 100–200 KB 容许区间。原始体积增量较高是未压缩的 schema、commands、history 和 ProseMirror view 代码；没有安装 React/Vue 适配器或运行时。`jsx-runtime` 小块是 Tiptap 自带的 DOM 规格构造函数，不是 React JSX runtime。

真实 release 壳层的首次编辑器动态加载为 118.69 ms；1.68 MiB XHTML 大章加载为 9,208.08 ms。对应进程树 Working Set 分别为 575,340,544 B 与 721,313,792 B。进程树包含 WebView2 及其 OOPIF，只用于同机回归，不与纯 Rust 探针混合比较。

## 边界

- 产物大小是构建结果，不代表 WebView2 运行时内存；桌面进程树包含共享 WebView2/OOPIF。
- 探针测 Rust 接受和序列化，不包含 550 ms 防抖等待、浏览器排版和原生文件对话框。
- 安装器仍不生成，因为 `bundle.active = false`；因此没有可报告的安装包增量。
