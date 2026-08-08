# 阶段 3 EPUB 安全阅读验收记录

记录时间：2026-08-08（Asia/Shanghai）

## 结论

阶段 3 已在最终 Windows release 上完成 EPUB 2/3 解析、安全资源协议、隔离阅读、目录/章节、阅读设置、进度、书签、书内搜索、最近文件和 TXT/EPUB 多标签集成。EPUB 始终只读，不显示编辑、保存或另存为能力。

本阶段同时修复了原生标题栏 X 和任务栏关闭不退出的问题。根因是 `onCloseRequested` 在未阻止事件时会自动调用 `window.destroy()`，而主窗口 capability 缺少 `core:window:allow-destroy`。新增配置回归测试锁定该权限，并用 `scripts/test-release-close.ps1` 在前端完全就绪后发送标准 `WM_CLOSE`；最终进程在 5 秒内以 code 0 退出。

## 安全边界

- SafeZIP 在读取语义结构前验证首个未压缩 `mimetype`、UTF-8 名称、绝对/UNC/盘符/反斜杠/遍历、Windows 保留名、路径长度、符号链接、加密、压缩方式、大小、总解压量、压缩率，以及大小写和 Unicode NFKC 冲突。
- 集中限制为：归档 512 MiB、10,000 entries、单 entry 64 MiB、总解压量 1 GiB、压缩率 200:1；XHTML 8 MiB、CSS 4 MiB、图片 32 MiB、字体 16 MiB、XML 4 MiB。
- 不执行整包解压。每次读取都重新检查 manifest 成员、资源类别、单资源上限和 MIME 签名。
- 资源只通过随机 192-bit session 的 `http://readloom-epub.localhost/<session>/...` 暴露；仅主 WebView、GET/HEAD、当前 session 和精确 manifest 路径可访问。关闭 session 后返回 410。
- XHTML 经 Ammonia 清理；CSS token 检查阻止 `@import`、远程/data URL、`expression`；SVG XML 检查阻止脚本、事件、foreignObject、外部引用和过深结构。
- 出版者脚本被删除。章节以 `text/html` 安全包装后加载到 `sandbox="allow-scripts"`、无 `allow-same-origin` 的独立 OOPIF。
- 章节 CSP 从 `default-src 'none'` 开始，只允许当前 session 的图片、样式和字体，以及 SHA-256 精确授权的最小 bridge；主应用 release CSP 不允许外部 connect/script/frame。
- bridge 使用固定 schema、document/session/token 三重身份、event.source 验证、4 KiB 消息上限和滚动节流，不暴露 Tauri API 或文件路径。
- `http/https` 外部链接被转换为不可联网的 `readloom-external:` 标记。主应用显示完整域名和 URL，只提供“复制链接”和“取消”；外部图片、CSS、字体和脚本直接移除。
- `META-INF/encryption.xml` 和 rights/DRM 标记稳定返回 `ENCRYPTED_EPUB` / `DRM_PROTECTED_EPUB`，不尝试绕过。

架构决定见 `docs/adr/0001-isolate-untrusted-epub-content.md`。

## 自动化结果

最终命令与真实结果：

```powershell
npm run check
npm test -- --run
npm run build
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
npm run tauri -- build --no-bundle
```

- `svelte-check`：0 error、0 warning。
- Vitest：16 files、35 tests 全部通过。
- Rust：55 tests 全部通过。
- Clippy：`-D warnings` 通过。
- Vite：164 modules；EPUB reader 维持独立动态块。
- Tauri：release 构建成功，产物 `src-tauri/target/release/readloom.exe`。
- MSVC 链接器只输出中文“正在创建库”信息，被 rustc 记录为 `linker_messages` warning；没有源码或 Clippy warning。

测试覆盖 EPUB 2 NCX、EPUB 3 nav、元数据、封面、spine、fixed-layout 标记、缺失 container、非 2/3 版本、路径规范化、Zip Slip、entry/单项/总量/压缩率、Unicode 冲突、加密/DRM、脚本/事件/远程资源、CSS/SVG、MIME、session 创建/失效、进度指纹、书签、搜索、SQLite migration，以及阶段 1 TXT 保存回归。

前端覆盖目录展开、上一/下一章、opaque iframe、fixed-layout 提示、消息 source/session/token、双重编码路径、Windows 映射协议 URL、外部链接复制/取消、EPUB/TXT 多标签和 CodeMirror/EPUB reader 互斥延迟加载、窗口关闭 capability。

## 最终 release UI 验收

执行：

```powershell
powershell -ExecutionPolicy Bypass -File scripts/create-stage3-epub-fixture.ps1
powershell -ExecutionPolicy Bypass -File scripts/test-stage3-epub-ui.ps1
powershell -ExecutionPolicy Bypass -File scripts/test-release-close.ps1
```

`test-stage3-epub-ui.ps1` 操作真实系统文件对话框，并通过仅本次进程启用的 WebView2 本机调试端口读取主页面和 sandbox OOPIF，不使用浏览器预览替代 Tauri。最终验证：

- 打开中文书名 EPUB 3，显示目录、最近文件和只读状态；编辑/保存控件不存在。
- SQLite 可恢复到第 2 章；能返回第 1 章再前进到第 2 章。
- 第 1、2 章真实 DOM 正文可见；内部 CSS 和 1 张 PNG 从当前 session 加载。
- 出版者脚本未执行，外部图片源为 0，外部网络资源请求为 0，外部链接为 1 个 inert 标记。
- 两章搜索返回 2 个结果；书签保存请求成功。
- iframe sandbox 精确为 `allow-scripts`，章节位于独立 OOPIF。
- 加载 EPUB 后发送 `WM_CLOSE`，进程 5 秒内以 code 0 退出。

性能数据见 `docs/performance/stage-3-baseline.md`。

## 已知兼容性限制

- Fixed-layout 只显示明确警告并尽力流式呈现，不保证漫画、杂志和复杂绝对定位版式。
- 加密、DRM、脚本化、音视频/媒体 overlay、未知压缩方式和未列入安全 MIME 白名单的资源不支持。
- 外部链接阶段 3 只允许复制和取消，不直接调用系统浏览器；出版物不能静默联网。
- 缺失非关键图片会保留无源占位元素，正文继续阅读；缺失 manifest 核心资源会拒绝打开。
- 搜索按需扫描线性 spine，不做跨书搜索或持久全文索引。
- release GUI 自动验收使用自有 EPUB 3；EPUB 2 的 NCX/封面/正文由 Rust 真实归档夹具覆盖，但本轮没有对第三方 EPUB 2 做人工视觉兼容性抽样。
- 没有提交大型 EPUB，因此大型书、多 EPUB 并发标签和复杂字体的性能仍需后续专项样本验证。
