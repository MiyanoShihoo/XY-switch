# XY Switch

本地 AI 配置管理与协议路由工作台。集中管理供应商、模型、MCP、提示词和 Skills，减少在不同 AI 客户端之间反复修改配置的工作。

**技术栈：** Tauri 2 · Rust · React 19 · TypeScript · SQLite

**下载：** [Releases](https://github.com/MiyanoShihoo/XY-switch/releases) · **源码：** [MiyanoShihoo/XY-switch](https://github.com/MiyanoShihoo/XY-switch) · **许可证：** [MIT](LICENSE)

> Windows / Linux 提供桌面安装包构建；iOS 提供移动端适配、模拟器验证与独立签名构建入口。**模拟器归档不是可安装到 iPhone 的 IPA**，真机分发需要 Apple 证书与匹配的描述文件。使用上游模型需要自行配置服务地址与 API Key。浏览器预览不等于原生应用。

## 下载与平台差异

从 [Releases](https://github.com/MiyanoShihoo/XY-switch/releases) 下载对应系统的附件，并核对同页 SHA-256。具体版本的已验证范围以发布说明和 [Actions](https://github.com/MiyanoShihoo/XY-switch/actions) 实际结果为准，不把构建入口等同于构建通过。

| 平台 | 包格式 | 凭据存储 | 平台边界 |
| --- | --- | --- | --- |
| Windows x64 | NSIS `.exe` / `.msi` | Windows Credential Manager | 支持客户端接管、系统托盘和开机自启 |
| Linux x64 | `.deb` / `.AppImage` | Secret Service | 需要已解锁的系统密钥库；托盘取决于桌面环境；暂无开机自启配置 |
| iOS | 模拟器 `.app.zip` / 配置签名后 `.ipa` | Apple Keychain | 不能接管其他 App 的 CLI、扫描其 Skills/会话目录，也不保证代理后台常驻 |

Linux 发布工作流使用 Ubuntu 22.04 构建。AppImage 仍需要兼容的系统库和 Secret Service。iOS 真机签名说明见 [多平台打包](docs/packaging.md)。

## 能做什么

| 功能 | 用途 | 说明 |
| --- | --- | --- |
| 供应商与模型 | 管理端点、凭据、模型映射、请求头与请求体覆盖 | 保存配置、设为当前、连接检测分别执行 |
| 本地协议路由 | 连接 Responses、Chat Completions、Anthropic Messages 等接口 | 按客户端与上游实际能力选择协议，不保证任意服务商完全兼容 |
| 客户端接管 | 为 Codex、Claude Code、Gemini CLI 等客户端写入路由配置 | 需要用户明确启用；部分客户端需重启后读取新配置 |
| MCP 与提示词 | 统一管理服务器配置和可复用提示词 | 本机凭据与普通配置分开管理 |
| 本地 Skills | 读取已授权的技能目录，导入单个技能或包含多个技能的文件夹 | 以 `SKILL.md` 为识别入口，支持扫描子目录并展示导入结果 |
| 会话与用量 | 浏览会话，按请求、模型、供应商查看 Token、缓存与成本记录 | 缺失的上游用量保持未知，不按零消耗处理 |
| 模型响应核验 | 检查型号声明、协议形态、工具调用及计量异常 | 接口能力与底层模型身份分别判断 |
| 数据与界面 | 导入/导出配置、查看数据路径、切换主题与工作台布局 | 桌面数据保存在本机；导入配置会修改当前数据 |

## 开始使用

### 1. 启动桌面程序

使用 Releases 中对应平台的构建，或按下文从源码构建。替换程序前先退出已有 XY Switch 实例；Windows 安装包需要 WebView2 Runtime。

首次使用可先在「设置」中查看数据位置、凭据状态和本地路由设置。不要把浏览器预览当作已连接本机后端的桌面程序。

### 2. 添加并检查供应商

1. 打开供应商页面，填写名称、API 地址、API Key 和模型。
2. 根据服务商文档选择上游协议。支持 Chat 的服务不应仅因模型名称相似就改走 Responses。
3. 保存后执行连接或模型探测，检查 HTTP 状态、返回型号、用量及错误信息。探测可能产生上游费用。
4. 确认配置后点击「设为当前」。点击列表项只查看详情，不会自动切换供应商。

如果填写的是完整请求 URL，使用界面中的对应选项，避免重复追加路径。遇到 401/403 时检查凭据与权限；遇到 404 时优先检查端点和协议。

### 3. 按需启用客户端接管（桌面端）

在「设置 → 本地路由」中检查监听状态，再为需要接管的客户端启用配置写入。修改后按客户端要求重新启动或重新载入配置。

保存成功、设为当前、代理运行、上游请求成功是不同状态；不能仅凭其中一项判断整条调用链已正常工作。

### 4. 导入 Skills（桌面端）

在 Skills 页面授权所需平台目录，然后读取目录或选择本地文件夹导入。可以选择技能自身目录，也可以选择包含多个技能的父目录。

找不到技能时，先确认文件名为 `SKILL.md`、文件可读且格式有效，再查看扫描结果中的错误或跳过原因。仅创建一个同名文件夹不等于已经安装了技能。

## 模型核验与 Token 预算

XY Switch 将三类结论分开呈现：

- **接口与工具能力：** 请求是否完整成功，工具参数、调用 ID 和回执是否形成有效闭环。
- **型号声明：** 请求型号与返回字段是否一致，是否存在声明冲突或路由变化。
- **身份认证：** 是否存在能通过独立上游渠道核对的来源证据。

HTTP 错误、缺失状态、解析失败、流式响应不完整或证据截断，不会被认定为有效型号匹配。计量异常会单独提示，不直接推导为模型冒充。不同供应商、配置上下文和型号的历史观察不会混为同一基线。

**返回的 `model` 字段、模型自称或工具调用成功，都不能单独证明底层权重身份；缺少独立身份凭据也不等于假模型。** 少量成功请求不等于已经验证长期稳定性或相同输入的确定性。

低 Token 模型验证的软预算支持 **0–10000 Token**，默认 **100**；设为 **0** 时不允许该功能执行收费复核。此预算只约束对应的主动复核机制，不是普通业务调用、其他手动探测或服务商账单的全局硬上限。未知用量不计为零；已包含在输出用量中的推理 Token 不重复相加。

## 数据与凭据

- 供应商密钥使用系统原生凭据存储：Windows Credential Manager、Linux Secret Service、Apple Keychain。密钥库不可用或锁定时返回错误，不回退到明文文件。
- 数据库、用户目录及 Codex 配置目录可在「设置 → 数据」查看，以当前进程实际返回的位置为准。
- 配置导出用于迁移普通配置，不是完整的凭据备份；MCP 的敏感请求头不会随普通导出保留。迁移后应重新检查凭据。
- 不要公开 API Key、客户端认证文件、真实请求日志或包含私有提示词的导出文件。仓库忽略规则用于减少误提交，但不能代替提交前检查。

安全相关说明见 [SECURITY.md](SECURITY.md)。

## 从源码运行

### 环境要求

- Windows：Microsoft C++ Build Tools 的「使用 C++ 的桌面开发」、Windows SDK、WebView2 Runtime。
- Linux：WebKitGTK 4.1、AppIndicator、D-Bus、Secret Service 及编译依赖，安装命令见 [打包指南](docs/packaging.md)。
- iOS：macOS + Xcode + CocoaPods；Windows/Linux 可使用仓库的 macOS CI 工作流。
- Rust stable；Windows 使用 MSVC 工具链。
- Node.js 22.12 或更新的兼容版本；当前 Vite 7 依赖的引擎范围为 `^20.19.0 || >=22.12.0`。
- pnpm 10，以及 Git。

以 [Tauri 环境准备文档](https://v2.tauri.app/start/prerequisites/) 和仓库内的依赖锁文件为准。不要在构建脚本中写入某台机器专用的 SDK 或用户目录。

### 安装、开发与检查

在仓库根目录执行：

```powershell
pnpm install --frozen-lockfile
pnpm dev
```

常用检查命令：

```powershell
# TypeScript 类型检查
pnpm lint

# 前端单元与组件回归
pnpm test

# Rust 回归
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

只开发界面时运行 `pnpm dev:renderer`。浏览器端只提供预览，不会替代桌面 IPC；需要验证凭据、客户端接管或系统路径时使用桌面构建。

部分 Rust 测试依赖交互式凭据存储或单独安装的 Codex CLI，默认标记为忽略。Linux 原生密钥库可用 `bash scripts/test-linux-keyring.sh` 在隔离环境中单独回归。未运行这些测试时，不应把它们算作通过。界面自动化的环境要求见 [工作台验收说明](docs/workbench-ui-acceptance-20261017.md)。

### 构建安装包

Windows PowerShell：

```powershell
pnpm build:windows --tools-dir "E:\AI tools\xy-switch\windows\bundlers"
```

Linux / WSL：

```bash
pnpm build:linux --tools-dir "$HOME/AI-tools/xy-switch/bundlers"
```

macOS 的 iOS 模拟器：

```bash
pnpm build:ios:simulator
bash scripts/smoke-ios-simulator.sh
```

产物按版本和平台写入 `release/<version>/<platform>-<architecture>/`，同时生成 `SHA256SUMS.txt`；iOS 子目录另有 `simulator` / `device` 标记。Windows/Linux 包装脚本会重新构建前端和 Rust，拒绝把旧包改名当作新包。脚本测试命令为 `pnpm test:packaging`。

`--tools-dir` 或 `XY_SWITCH_TOOLS_DIR` 控制下载的安装包工具缓存；默认使用被 Git 忽略的 `.build-tools/`。Node、Rust 和系统 SDK 是独立的预先安装依赖，其路径由各自工具链配置决定。当前 Windows 工作环境的新工具统一存放在 `E:\AI tools`。

完整命令、Linux 依赖、缓存规则、iOS 签名参数与常见故障见 [多平台打包指南](docs/packaging.md)。前端资源由 Tauri 嵌入应用；`frontendDist` 使用相对于 Tauri 配置的路径，不直接传入 Windows 盘符绝对路径。

## 项目结构

```text
src/
  features/          业务页面与功能组件
  lib/               前端服务、验证与判定逻辑
  types/             IPC 和业务数据类型
  ui/                通用界面组件
src-tauri/
  src/               原生命令、代理、凭据、Skills 与模型核验
  migrations/        SQLite 数据结构
  capabilities/      Tauri 权限配置
scripts/             构建与回归辅助脚本
docs/                使用、架构与历史验证文档
public/              图标及静态资源
```

应用「关于」页从 `package.json` 的 `repository` 字段读取仓库地址，避免在界面代码和文档中维护多个互相矛盾的地址。未配置有效地址时不会展示未经确认的仓库链接。

## 文档与参与开发

- [多平台打包与 iOS 签名](docs/packaging.md)
- [中文使用指南](docs/user-guide-zh.md)
- [English user guide](docs/user-guide-en.md)
- [架构说明](docs/architecture.md)
- [模型审计实现说明](docs/model-audit-implementation.md)
- [工作台界面验收说明](docs/workbench-ui-acceptance-20261017.md)
- [变更记录](CHANGELOG.md)

历史验收文档记录的是对应日期的快照，不代表当前每项功能都已完成相同范围的验证。提交改动时请附上复现步骤与实际运行的测试结果；涉及真实 API 的日志需先脱敏。

项目采用 [MIT License](LICENSE)。第三方供应商图标的来源与说明见 [public/providers/NOTICE.md](public/providers/NOTICE.md)。
