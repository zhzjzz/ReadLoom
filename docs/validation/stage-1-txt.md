# 阶段 1 TXT 验收记录

记录时间：2026-08-08（Asia/Shanghai）

## 自动化结果

- Rust 通过应用服务真实打开、编码、同目录临时写入、`ReplaceFileW` 替换、重开与摘要校验；测试夹具只位于 `src-tauri/target` 或项目 `target`。
- 覆盖 UTF-8（有/无 BOM）、UTF-16 LE/BE BOM、GBK/GB18030、空文件、只有 BOM、中文/英文/emoji、CRLF/LF/CR/Mixed/None。
- GBK 无法表示 emoji 时返回 `UNREPRESENTABLE_CHARACTERS`，编码发生在任何磁盘替换之前，原文件保持不变。
- 外部写入后保存返回 `EXTERNAL_MODIFICATION`，磁盘上的外部版本保持不变。
- 另存为测试确认新文件创建成功且原文件字节不变；唯一临时文件名测试确认连续创建不会冲突。
- 前端组件测试覆盖正文显示、dirty、保存成功/失败、编码与换行状态、快捷键、未保存确认、错误提示和 CodeMirror 延迟加载。
- 浏览器生产预览在默认窗口与 900 × 640 下无页面级横向溢出；最小窗口下右侧检查器折叠、工具栏 `scrollWidth` 与 `clientWidth` 均为 836 px。
- 亮色与暗色根主题属性同步；装载、主题切换与响应式复测后控制台为 0 error / 0 warning。
- release 性能脚本连续收到 5 次由 Svelte 调用 Rust `frontend_ready` 产生的标记，证明最终 exe 内的前端挂载和 IPC 链路可用。

## Windows 安全保存边界

1. Rust 在目标目录用 `create_new` 创建唯一临时文件。
2. 严格编码后执行 `write_all`、`flush`、`sync_all`，再读回并比较完整字节。
3. 替换前重新计算大小、修改时间和 BLAKE3 指纹；不一致即停止。
4. 对已有文件使用 Windows `ReplaceFileW`，同时让 Windows 生成恢复备份；对新文件使用同卷 `MoveFileExW(MOVEFILE_WRITE_THROUGH)`。
5. 替换前写入同目录恢复日志，替换后再次校验 BLAKE3；成功才更新会话 revision 并清理日志、临时文件和备份。
6. 启动或再次保存时会检查中断日志：可判定为旧版本或新版本时安全清理，目标异常且备份完整时恢复旧版本；无法可靠判断时返回 `RECOVERY_AVAILABLE` 并保留现场。

`ReplaceFileW` 仍不能保证磁盘硬件、文件系统驱动或突然断电下的绝对事务语义；`REPLACEFILE_IGNORE_MERGE_ERRORS` 允许 Windows 在无法合并附加文件元数据时继续替换。正文完整性由恢复备份、日志和替换后摘要共同保护。

## 人工端到端流程

先生成项目内夹具并启动 release：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/create-stage1-manual-fixture.ps1
src-tauri/target/release/readloom.exe
```

然后执行：

1. 按 `Ctrl+O`，打开 `target/manual-stage1/stage1-utf8.txt`。
2. 确认右侧显示 UTF-8、无 BOM、CRLF，状态栏显示 CRLF。
3. 用 Windows 中文输入法输入 `阅织中文输入 😀`，确认候选窗、回车、退格、复制和粘贴正常。
4. 确认标题与状态栏立即显示未保存；按 `Ctrl+Z` 回到保存版本时标记消失，再按 `Ctrl+Y` 恢复编辑。
5. 用 `Ctrl+F` 查找“中文”，用 `Ctrl+H` 打开替换面板。
6. 按 `Ctrl+S`，确认未保存标记消失；`Ctrl+W` 关闭并重新打开，确认内容、UTF-8、无 BOM 与 CRLF 不变。
7. 再编辑但不保存，按 `Ctrl+W`，分别验证“取消”“不保存”“保存”三个分支。
8. 按 `Ctrl+Shift+S` 保存到同目录的新 `.txt`，确认原文件不变、新路径成为当前会话。
9. 保持 Readloom 中存在未保存修改，用记事本修改当前磁盘文件；回到 Readloom 按 `Ctrl+S`，确认出现外部修改提示且磁盘内容未被覆盖。

本次自动执行环境能启动 release 并收到前端就绪标记，但 GUI 子进程没有向自动化会话暴露可交互 HWND，Windows 自动化插件同时返回 `spawn EPERM`。因此上述需要真实文件对话框与输入法候选窗的步骤保留为人工复核，未标记为自动通过。
