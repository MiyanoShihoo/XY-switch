# XY Switch

[English](README.md) | 简体中文

基于 Tauri 2、Rust、React 和 TypeScript 的本地 AI 配置管理与协议路由工作台。集中管理供应商、模型映射、MCP、提示词与本地 Skills，查看代理用量和模型响应证据，减少在不同客户端之间反复修改配置的工作。

**1.1.0 新增中英文界面。** 在「设置 → 常规」选择「跟随系统 / 简体中文 / English」。原生应用保存本地偏好并同步桌面托盘，浏览器预览使用 localStorage；用户名称、提示词、Skills、会话正文、模型 ID 和原始诊断不会被翻译。

## 下载与平台范围

在 [GitHub Releases](https://github.com/MiyanoShihoo/XY-switch/releases) 下载安装包、校验和、原生截图与验证报告。源码 ZIP 不是安装包。

| 平台 | 产物 | 凭据存储 | 实际边界 |
| --- | --- | --- | --- |
| Windows x64 | NSIS `.exe` / `.msi` | Windows Credential Manager | 未签名；需要 WebView2；本次没有 portable ZIP |
| Linux x64 | `.deb` / `.AppImage` | Secret Service | Ubuntu 22.04 构建基线；需要兼容系统库与解锁的密钥库；托盘取决于桌面环境 |
| macOS Intel + Apple Silicon | Universal `.app.zip` / `.dmg` | Apple Keychain | ad-hoc 签名，非 Developer ID 签名，未公证 |

当前仅维护 **Windows、macOS、Linux**（2026-10-30 起）。iOS 与 Android 的打包、模拟器验收和签名工作流已移除；旧版 Release 中的移动端产物仅作为历史记录保留，不代表当前支持。最低构建目标不代表所有系统/架构均已实测。具体命令和签名限制见[桌面平台打包](docs/packaging.md)。

## 已实现功能

| 领域 | 已交付行为 | 不应混淆的边界 |
| --- | --- | --- |
| 供应商与模型 | 端点/凭据、当前供应商、模型探索/映射、请求头/请求体覆盖 | 保存、激活、连接检查与上游请求成功是不同状态 |
| 协议路由 | 本地 OpenAI Chat Completions、Responses、Anthropic Messages、Gemini 适配 | 协议/工具能力与模型声明不等于权重身份认证 |
| 桌面客户端接管 | **Claude Code、Codex、Gemini CLI** 的明确启用接管与恢复 | 其他客户端图标不代表已实现接管 |
| MCP | 服务器管理与受支持客户端的导入/同步 | 仍受平台权限与客户端能力限制 |
| 提示词 | 本地增删改、启用/禁用；未发布源码支持显式挂载 | **不会自动**写入 `CLAUDE.md`、`AGENTS.md` 或 `GEMINI.md`；每次写入需预览 diff、确认并备份 |
| Skills | 桌面源显式授权扫描、手工引用 | 源默认全关闭；移除手工引用不删源文件；排除插件缓存 |
| 用量与响应证据 | 代理请求/token/缓存/成本、模型字段、Codex 与 DeepSeek Harness 本地用量读取 | 缺失价格/用量保持未知；本地会话用量不能提供真实 HTTP 延迟或账单证据 |
| 本地会话（未发布） | 三种桌面 CLI 日志、Codex 重命名标题、自定义客户端目录、搜索、消息/工具结果和受限日志删除 | 不改写客户端索引，不云同步，不自动执行恢复命令 |
| 模型定价（未发布） | 持久化自定义价格、models.dev 公开参考价同步、历史费用快照与 v2 配置备份 | 按完整模型 ID 精确匹配；估算不是供应商账单；浏览器预览不能写入本机数据 |
| 安全与验收（未发布） | 本地令牌/权限、共享限流与并发、费用/Token 预留准入、六类合成验收、计划监控与审计 | 认证和预算需显式开启；预留不是供应商账单保证；HTTP 200 也须通过内容断言 |
| 配置与显式挂载（未发布） | 手动配置快照、diff、事务回滚；提示词/Skills 规则文件预览、备份与回滚 | 不自动写规则文件；不执行 Skills 脚本；快照不是加密同步包 |
| 只读监控（未发布） | 本机用量、供应商状态、验收收据；仅回环 `GET /xy/status` | 状态接口始终需要本地令牌；未实现跨设备远程监控 |
| 界面与数据 | 中英文/系统语言、主题、普通配置导入导出、实际数据位置 | 普通导出不是完整凭据备份 |

**未发布源码检修（2026-10-27）：** 当前工作区已支持 Codex、Claude Code、Gemini CLI 的日志列表、消息与工具结果查看、恢复命令复制及指定日志删除；删除不改写原客户端数据库/索引。浏览器预览不提供本机会话访问。本轮还新增 **用量与审计 → 模型定价**、自定义价格备份（导出 v2、兼容导入 v1）和不随改价重算的历史费用快照。这些改动尚未进入已发布的 1.1.0 安装包，见[检修报告](docs/maintenance-20261027.md)。

**1.2.0 安全与验收（2026-10-29）：** 侧栏新增「安全与验收」，提供访问控制、预留硬预算、有界合成监控、配置版本和规则文件显式挂载。真实回环代理验收已达到 **Chat 上游 6/6、Responses 上游修复后 6/6**。见[真实验收跟进](docs/governance-live-followup-20261029.md)与[1.2.0 发布说明](docs/release-notes-1.2.0.md)；安装包是否可下载及原生验证状态，以对应 Release 资产为准，不由源码说明推断。

## 开始使用

1. 安装对应平台的原生程序。浏览器预览不能替代原生凭据、文件和代理操作；代理冷启动保持关闭。
2. 「设置 → 常规」切换语言。系统模式使用首选语言列表的第一项：以 `zh` 开头用中文，否则英文。原生偏好保存失败时保留旧语言并允许重试。
3. 添加供应商、填写端点与凭据、保存后执行所需连接/模型检查。**点击已启用、非当前的供应商卡片会查看并激活它**；详情图标只查看。以当前状态标记判断激活结果。
4. 按需启用本地代理和桌面接管，分别核对当前上游、代理状态、客户端配置及真实请求结果；部分客户端需重启。
5. 使用桌面 Skills 时先授权目标源，预览扫描再确认导入；递归包含 `.system`，排除插件缓存。

完整说明见 [中文指南](docs/user-guide-zh.md) / [English user guide](docs/user-guide-en.md)。

## 数据、凭据和核验

普通配置保存在本机。供应商密钥使用 Windows Credential Manager、Linux Secret Service 或 macOS Keychain，不进入普通配置备份。密钥库不可用会明确失败，不回退明文。

在「设置 → 数据」查看实际路径。普通导出不含供应商密钥和敏感 MCP 请求头；迁移后需重新录入。接管恢复只恢复该操作管理的配置，不等于回滚全部应用数据。

模型响应核验提供可观察字段与协议行为证据，不提供权重的密码学认证。未知成本不是 0。主动核验 token 预算默认 100、范围 0–10,000，是该机制的**软预算**，不是全局账单硬上限。

新增的全局「硬预算闸」独立于上述模型复核软预算：启用后，对受支持请求的每次上游尝试做持久化预扣，超出本地 Token 或费用预留额度则拒绝。它不是通用模型的精确计数或供应商账单上限；未知价格、不可界定的载荷和原生 Gemini 上游会被拒绝，而非静默放行。

## 从源码运行

复用兼容 Node.js（建议 22.12+）、pnpm 10、Rust 与平台 SDK。CI 固定 Rust **1.98.1**、Node **24**。Windows 需 MSVC/Windows SDK/WebView2；Linux 需 WebKitGTK 4.1/AppIndicator/D-Bus/Secret Service；macOS 需 Xcode Command Line Tools。

```bash
pnpm install --frozen-lockfile
pnpm dev:renderer           # 仅浏览器预览
pnpm dev                    # 原生开发
pnpm lint
pnpm test
pnpm test:packaging
pnpm build:renderer
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

交互式凭据库及外部 Codex CLI 测试可能默认忽略；需另跑对应隔离凭据测试，不能把忽略算作通过。

```bash
pnpm build:windows
pnpm build:linux
pnpm build:macos:universal
```

输出为 `release/<version>/<platform>-<architecture>/`。`--tools-dir` 或 `XY_SWITCH_TOOLS_DIR` 只选择包装工具缓存，不重定位 Node/Rust/SDK。仅提供三个桌面平台的构建入口。

## 仓库与文档

- `src/`：React 工作台、公共 UI、功能模块与 `i18n/`。
- `src-tauri/`：Rust 命令、协议、代理、存储、原生插件与回归。
- `scripts/`：打包、隔离原生验收、synthetic 界面自动化。
- `tests/`：前端与词库回归。
- `website/`：不依赖外部 CDN 的双语静态项目页。
- `.github/workflows/`：多平台验证及真机签名入口。

[使用指南](docs/user-guide-zh.md) · [打包](docs/packaging.md) · [1.1.0 发布说明](docs/release-notes-1.1.0.md) · [安全](SECURITY.md) · [更新日志](CHANGELOG.md) · [仓库信息](docs/github-description.md)

日期性验收报告是各自历史快照，不代表所有后续版本的验证状态。源码放 Git，安装包及验证证据放 Releases；不提交凭据、数据库、个人配置、签名材料、日志或缓存。

## 许可证与来源

[MIT](LICENSE)。XY Switch 是独立项目，并非 OpenAI、Anthropic、Google 或上游模型的官方客户端。供应商图标权利属于各自所有者，见 [素材说明](public/providers/NOTICE.md)。
