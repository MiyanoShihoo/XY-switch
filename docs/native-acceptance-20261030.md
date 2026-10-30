# 桌面平台与原生验收（2026-10-30）

## 当前平台范围

当前仅维护 **Windows、macOS、Linux**。已移除 iOS/Android 的打包命令、模拟器/设备 CI、原生验收脚本及独立 iOS 签名工作流；README、用户手册、打包文档与官网只提供三个桌面平台的当前入口。

iOS 验收按用户要求停止，不再是本轮待完成项。之前的 iOS 运行记录与已发布的移动端产物保留为历史证据，不改写成通过，也不追溯删除。平台收敛不改动应用生产源码、兼容性分支或已有发布包。

**已发布 1.2.0 不含加密同步，本轮不重新发布版本。** Windows 加密同步验收使用 feature CI **40908795739 / attempt 1** 的原始 NSIS 包，应用源码为 `614e2b21ea59b7256d09c7f34b30f6aeecd9064c`；验收脚本提交单独记录。CI 必须确认应用源码、配置、前端资源、依赖与版本未变，并校验 GitHub artifact 摘要、包内清单和安装后 EXE 摘要，才允许复用旧二进制。

## Windows 验收机制

只在一次性 GitHub-hosted Windows runner 上运行，拒绝预先存在的应用数据库；不在日常账户运行或安装被测应用。

CDP 仅绑定 loopback。原始环境变量方式未在该 runner 的 WebView2 中生效，因此当前脚本同时设置被测子进程环境和 **仅限应用标识/EXE 名称** 的临时 WebView2 注册表覆盖，不使用通配应用策略。原值在内存中保存，并在 `finally` 中恢复；恢复或卸载失败都会使验收失败。安装包和可执行文件不修改。

- 使用生产 IPC 构造 `.invalid` 域名的合成供应商、收藏和自定义价格；不读取或配置真实供应商密钥，不调用付费接口。
- 导出、导入、取消及回滚操作实际 UI。独立进程通过 Windows UI Automation 操作属于被测 PID 的 `#32770` 文件对话框，保存控件树和截图，不替换 IPC 或文件选择器。
- 独立解密实际保存的 `.xysync`，验证 AES-256-GCM / PBKDF2 参数、白名单、同步身份隔离、错误口令与 tag 篡改拒绝。
- 验证取消与错误口令不写入；正确预览不应用配置；逐项明确选择，覆盖接受与保留本地。
- 验证同目标合并保留本机 advanced，变更目标保留原供应商并创建新禁用副本；验证备份和回滚。同步新增副本在回滚后仍保留为禁用，这是产品约定。
- 口令随机生成且不持久化；检查输入框、WebView 存储与 SQLite。结束后停止进程、检查数据库完整性并卸载。

这不是 MSI 专项同步验收，也不是 macOS/Linux 的原生同步 UI 验收。此前三个桌面平台的真实后端与安装包测试作为独立证据保留，不混为一次新验收。

## 复现

在 **Build and verify** 工作流派发：

- `native_acceptance_only: true`
- `native_source_run: 40908795739`
- `native_source_commit: 614e2b21ea59b7256d09c7f34b30f6aeecd9064c`

仅验收模式现在只下载 Windows 产物、执行 Windows 原生同步验证，不依赖移动端 artifact。普通模式仅执行前端检查，以及 Windows、Linux、macOS 构建/验证，不包含移动端 job。macOS 保留 Universal（Intel + Apple Silicon）打包。

本地无需恢复应用构建依赖即可运行：

```bash
npm run test:packaging
```

## 已有结果

- 本地打包、产物安全解包、桌面平台范围与验收脚本回归：**66/66 通过**；PowerShell 语法检查通过。
- Windows 第二轮原生 CI：[41014384150 / attempt 1](https://github.com/MiyanoShihoo/XY-switch/actions/runs/41014384150)，脚本提交 `d75623d301326f0c6bec9ee6f7c94e33792d0a6a`。原始 EXE 摘要匹配，实际供应商页面已加载，卸载完成；由于 CDP 未监听，尚未执行加密同步交互，**该轮失败**。
- 上述 Windows 诊断 artifact **12959165985** 的 ZIP SHA-256 为 `5a327efb05b115856dbe2d5657cc1df25e27ec9e9b19973326410d90c93f33c1`，本地下载已核验。保留进程树、命令行、监听状态和实际窗口截图。
- iOS 旧运行的截图显示供应商界面已加载；旧测试把原生 Switch 错当成 Button，不能据此声称应用永久卡在初始化。该项已移出当前平台范围，不再追加 iOS 修复/验收运行。

新的 Windows 原生运行结果须单独核验后再记录；本地脚本回归不能替代实际文件选择器、导入与回滚验收。
