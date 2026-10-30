# 桌面平台与原生验收（2026-10-30）

## 当前平台范围

当前仅维护 **Windows、macOS、Linux**。已移除 iOS/Android 的打包命令、模拟器/设备 CI、原生验收脚本及独立 iOS 签名工作流；README、用户手册、打包文档与官网只提供三个桌面平台的当前入口。

iOS 验收按用户要求停止，不再是本轮待完成项。之前的 iOS 运行记录与已发布的移动端产物保留为历史证据，不改写成通过，也不追溯删除。平台收敛不改动应用生产源码、兼容性分支或已有发布包。

**已发布 1.2.0 不含加密同步，本轮不重新发布版本。** Windows 加密同步验收使用 feature CI **40908795739 / attempt 1** 的原始 NSIS 包，应用源码为 `614e2b21ea59b7256d09c7f34b30f6aeecd9064c`；验收脚本提交单独记录。CI 必须确认应用源码、配置、前端资源、依赖与版本未变，并校验 GitHub artifact 摘要、包内清单和安装后 EXE 摘要，才允许复用旧二进制。

## Windows 验收机制

只在一次性 GitHub-hosted Windows runner 上运行，拒绝预先存在的应用数据库；不在日常账户运行或安装被测应用。

CDP 仅绑定 loopback。[微软确认](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5645#issuecomment-4934355430) WebView2 150 起会忽略管理员进程的环境变量和 HKCU 调试覆盖；GitHub-hosted Windows runner 正处于该环境。当前脚本使用微软支持的 **HKLM、仅限应用标识/EXE 名称** 的临时覆盖，不使用通配应用策略、不降级运行时。必须先通过一次性 runner 和实际管理员权限检查。原值及类型在内存中保存，并在 `finally` 中恢复；已有注册表键不重建，恢复或卸载失败都会使验收失败。安装包和可执行文件不修改。

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

## 官网隔离验证

官网浏览器验证作为独立 Linux CI job 执行，不安装应用或恢复前端依赖。仅使用一次性浏览器配置和 loopback HTTP/CDP，检查中英文、三个桌面下载卡片、三种窗口宽度、文档链接、语言切换及持久化，并保存 DOM 状态、首页及平台区截图和浏览器版本。CI 安装 Noto CJK 字库，并通过 CDP 检查中文标题实际使用 CJK 字体，避免仅 DOM 翻译正确但截图显示缺字框。

本机测试因 Kaspersky 注入跨域资源而失败；该记录保留。不关闭安全软件、不放宽“零跨域资源”断言，也不把受污染的本地运行计作通过。CI 的实际结果单独记录。

## 已有结果

- 本地打包、产物安全解包、桌面平台范围与验收脚本回归：**74/74 通过**；PowerShell 5.1 语法和原生控件互操作 C# 编译检查通过，未在本机执行 GUI 或 Win32 控件操作。
- Windows 第二轮原生 CI：[41014384150 / attempt 1](https://github.com/MiyanoShihoo/XY-switch/actions/runs/41014384150)，脚本提交 `d75623d301326f0c6bec9ee6f7c94e33792d0a6a`。原始 EXE 摘要匹配，实际供应商页面已加载，卸载完成；由于 CDP 未监听，尚未执行加密同步交互，**该轮失败**。
- 上述 Windows 诊断 artifact **12959165985** 的 ZIP SHA-256 为 `5a327efb05b115856dbe2d5657cc1df25e27ec9e9b19973326410d90c93f33c1`，本地下载已核验。保留进程树、命令行、监听状态和实际窗口截图。
- iOS 旧运行的截图显示供应商界面已加载；旧测试把原生 Switch 错当成 Button，不能据此声称应用永久卡在初始化。该项已移出当前平台范围，不再追加 iOS 修复/验收运行。

- 第三轮 [41019279833 / attempt 1](https://github.com/MiyanoShihoo/XY-switch/actions/runs/41019279833)，脚本 `e20b9acc917f510be7a9498badc06570c44b95da`：官网 8 项 DOM/交互检查通过，但截图复核发现 CJK 字库缺失；已补充字体与实际渲染检查，等待新结果。Windows 的 HKCU 覆盖仍被运行时忽略，未进入同步 UI，卸载和策略恢复均完成；该轮失败。Windows artifact `12961375593`，ZIP SHA-256 `51847eb4055c2196d604cbb9280f2b9c7ae728f3b0b185ac83685179c0ce4fe4`，下载已核验。

- 第四轮 [41020451774 / attempt 1](https://github.com/MiyanoShihoo/XY-switch/actions/runs/41020451774)，脚本 `23c90ebf0cc1d0b4ba6f014c3fda845e44b35aac`：官网 8/8 通过；中文标题确实使用 Noto Sans CJK SC，1180/820/390 宽度的平台区截图已复核。官网 artifact `12961701030`，ZIP SHA-256 `437cccb28dffb5b7191fd1adda007c6b4cb067a6341f50a47fd3908fea3e19b8`，下载已核验。
- 同轮 Windows 的应用专属 HKLM 设置已生效：WebView2 `155.0.4283.62` 的 CDP 仅监听 `127.0.0.1`。测试在最初的 `about:blank` 页面过早断言 origin，因此未开始同步交互；已加入真实页面/生产 IPC/供应商工作区就绪等待和导航上下文切换回归，不手动导航或注入 mock。该轮仍记失败；原始 EXE 摘要一致，卸载及策略恢复通过。

- 第五轮 [41021900095 / attempt 1](https://github.com/MiyanoShihoo/XY-switch/actions/runs/41021900095)，脚本 `249940ea4ce8f3c96bd13aef12e235accac6aa67`：已等待到真实应用页面，生产 IPC 和合成测试记录步骤通过，已打开 PID 绑定的原生 Save As 窗口；初始 UIA 树把原生 Edit/Button 报告为 Pane，限定 ControlType 的选择器未匹配。此时先补充控件树轮询及失败截图，用下一轮区分加载时序和控件类型问题。Windows artifact `12961957063`，ZIP SHA-256 `f7004a124004dac5742e4aa2d572bcc5680b3ee19d81acc2fa7ca12d779301dc`，下载已核验；卸载及 HKLM 策略恢复通过。

- 第六轮 [41022727924 / attempt 1](https://github.com/MiyanoShihoo/XY-switch/actions/runs/41022727924)，脚本 `8a6402042ed2a2b48ce38df85991aa0fe26467cc`：75 秒、272 次 UIA 控件读取后仍失败。原生 Save As 截图证明窗口完整可操作；Edit `1001` 和 Button `1/2` 始终暴露为 Pane，并非仍在加载。现按实际 Win32 类名和控件 ID 识别，同时核验 HWND、进程、对话框子孙关系及可见/启用状态；优先 UIA Pattern，否则向同一真实控件发送有界 WM_SETTEXT / BM_CLICK，并回读文件名、验证窗口关闭及截图。Windows artifact `12962401452`，ZIP SHA-256 `c81f6c521e9c33453b9671bccc2d23d8e95c644122cfaa79c6fdfb3b55fb99cb`，下载及原生截图已核验；卸载及策略恢复通过。

- 第七轮 [41023437110 / attempt 1](https://github.com/MiyanoShihoo/XY-switch/actions/runs/41023437110)，脚本 `a708b0a3cdb881708e47b061a40a7a6d364dd7a8`：Win32 控件识别已生效，原生保存窗口 341 ms 就绪，路径回读、原生按钮调用、窗口关闭均通过，应用提示保存完成；但请求路径下未找到文件，因此独立解密尚未执行，仍记失败。文件名输入改用 EM_SETSEL + EM_REPLACESEL，让 shell 收到实际编辑变更通知，而非仅 WM_SETTEXT；增加失败时任务临时目录和 Documents 顶层 `.xysync` 文件位置诊断，不自动接受其他路径。Windows artifact `12962184149`，ZIP SHA-256 `905640785351f5aa9ac6f95ed1a694986e46f961f745df69c401e29ddcb8825e`，下载及原生截图已核验；官网、来源检查、卸载及策略恢复通过。

新的 Windows 原生运行结果须单独核验后再记录；本地脚本回归不能替代实际文件选择器、导入与回滚验收。
