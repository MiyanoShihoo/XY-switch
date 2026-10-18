# 托盘事件 ACL 修复（2026-09-21）

## 用户可见故障

截图显示：`托盘事件监听失败：Command plugin:event|listen not allowed by ACL`。

这次故障位于本地桌面事件订阅权限，不是模型接口返回的错误。无需为此更换密钥或删除用户配置。

## 已确认根因

- `E:\XY switch\src\App.tsx` 注册 `provider-changed` 和 `provider-switch-error` 托盘事件，并在清理时注销监听。
- 项目原先没有 `src-tauri/capabilities` 目录，构建生成的 capabilities 为 `{}`。
- 缺少主窗口 `core:event:allow-listen` 和 `core:event:allow-unlisten` 权限。
- 前一轮测试没有覆盖实际编译后的 ACL，这是此前验收遗漏。

## 修复范围

1. 新增 `E:\XY switch\src-tauri\capabilities\main-events.json`：仅本地 `main` 窗口可监听/注销事件。
2. 修改 `E:\XY switch\src-tauri\tauri.conf.json`：显式启用 `main-events`。
3. 新增 `E:\XY switch\src-tauri\src\acl_regression_tests.rs`，并在 `E:\XY switch\src-tauri\src\lib.rs` 中以 cfg(test) 注册。
4. 没有用删除错误提示、关闭 ACL 或授予全部权限的方式绕过问题。

## 回归方式

测试通过 `tauri::generate_context!()` 获得编译后的权限上下文，调用实际 `RuntimeAuthority::resolve_access`。不构建运行中的 App、不执行应用 setup、不打开窗口、不打开用户数据库或凭证存储。

先加测试、后修复：
- 修复前：5 项中 3 通过、2 失败，失败项正是 main 窗口 listen/unlisten。
- 修复后：5 项全部通过。
- 验证 main 本地窗口可 listen/unlisten。
- 验证其他窗口、远程 origin 不被授权。
- 验证 main 不获得 emit、emit_to、shell execute/spawn/open 权限。

本轮自动化结果：
- 前端：46 passed，11 个文件。
- 后端：77 passed，0 failed，0 ignored，4 filtered out。
- 4 项过滤是 DPAPI 往返、Windows Credential Manager 往返和两项真实 Codex CLI 测试。本次不运行凭证存储测试，也不再次启动 Codex；这些项目此前的结果不能冒充本轮测试结果。
- 其余隔离合成数据协议回归仍执行，包括 Grok/Kimi 命名的本地模拟上游测试。

证据：
- `E:\XY switch\artifacts\2026-09-21-acl-fix\acl-before.log`
- `E:\XY switch\artifacts\2026-09-21-acl-fix\rust-tests.log`
- `E:\XY switch\artifacts\2026-09-21-acl-fix\frontend-tests.log`
- `E:\XY switch\artifacts\2026-09-21-acl-fix\windows-build.log`

## 隐私与验收边界

本轮未查询、读取或复制真实密钥、证书、密码、私人配置或供应商数据库；未启动正式应用；未上传文件；未替换已运行的旧程序。只检查项目源码、框架依赖源码、合成测试和构建产物。

本轮未做正式桌面 GUI 点击验收、实际供应商云端验证或安装器测试。ACL 测试证明编译权限解析已修复，不冒充完整 GUI 端到端测试。

## 用户操作

使用新文件名中带 `codex-compat-acl-fix` 的程序。先自行正常退出旧版（包括托盘），再打开新 EXE；若旧进程仍在运行，旧进程不会自动获得新权限。

新包保留此前 Codex 兼容性修复。原生 Responses 与 Chat 桥接选择规则不变，Chat 桥接仍需保持本地代理运行。不应为这个 ACL 故障重置密钥、删除数据或重装 Codex。

## 最终交付及完整性结果

`pnpm tauri build --no-bundle` 成功，release 构建耗时 3 分 33 秒。发布包仅包含 EXE 与说明。

- EXE：`E:\XY switch\release\XY-Switch-1.0.0-codex-compat-acl-fix-20260921.exe`
- EXE 大小：13,807,616 字节。
- EXE SHA256：`86A35F430A99C2D80967B2939637DB475983B73149DD554A2653F685836571AA`
- ZIP：`E:\XY switch\release\XY-Switch-1.0.0-codex-compat-acl-fix-20260921.zip`
- ZIP SHA256：`A30DF3FE1951B65387D31EB18B8C52D13BBAA79261600D182740D655CF278996`
- 完整性记录：`E:\XY switch\artifacts\2026-09-21-acl-fix\release-integrity.json`

已检查 Windows x64 PE32+ GUI 文件结构及段边界、前端资源与图标、源码/权限配置早于 EXE、复制与压缩前后 EXE 哈希一致；同时检查构建生成的 capability 包含且仅包含本地主窗口所需 listen/unlisten 权限。哈希校验不等于代码签名或安全审计。
