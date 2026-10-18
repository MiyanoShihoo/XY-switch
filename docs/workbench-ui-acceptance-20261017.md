# XY Switch「对位工作台」UI 验收报告

日期：2026-10-17（Asia/Shanghai）  
工作区：`E:\XY switch`  
接续聊天：`codex://threads/01a14862-9925-71e0-815e-d19d3aaf8151`

## 结论

**七页 UI 重写已接入实际应用，前端回归与生产 renderer 验收通过；后续同日已补齐 SDK 并成功构建新的 Windows x64 release EXE。** 新产物为 [XY-Switch-1.0.0-workbench-20261017.exe](E:/XY%20switch/release/XY-Switch-1.0.0-workbench-20261017.exe)，详见 [原生发布记录](E:/XY%20switch/docs/native-release-20261017.md)。以下保留 UI 验收及早期构建失败的历史证据；生产 EXE 启动和真实网站/Key 链路仍未验证。

UI 验收阶段仅使用现存依赖和工具，未安装依赖、组件库、技能或 SDK。随后根据用户“生成最新版 exe，缺少工具下载到 E:\AI tools”的要求补齐了 SDK，并完成原生编译。两个阶段均未运行真实供应商请求、未启动生产原生应用、未写入真实凭据、注册表自启或客户端配置；未执行 Git reset、clean、stage 或 commit。

## 1. 已落地范围

| 模块 | 实现与接入 |
| --- | --- |
| 统一外壳 | [App.tsx](E:/XY%20switch/src/App.tsx) 接入七个业务页面；[main.tsx](E:/XY%20switch/src/main.tsx) 仅加载新设计系统；统一侧栏、上下文栏和运行状态栏。 |
| 设计系统 | [设计样式](E:/XY%20switch/src/design/index.css) 与 [基础控件](E:/XY%20switch/src/ui/index.ts)：中性底色、节制蓝色、独立状态色；无渐变、无毛玻璃；统一间距、边框、焦点和反馈。 |
| 供应商 | [ProvidersPage.tsx](E:/XY%20switch/src/features/providers/ProvidersPage.tsx)：筛选、排序、详情、紧凑抽屉、编辑、模型映射、连接测试、独立激活。 |
| MCP | [McpPage.tsx](E:/XY%20switch/src/features/mcp/McpPage.tsx)：stdio/HTTP 配置、四客户端挂载、编辑与删除确认。 |
| 提示词 | [PromptsPage.tsx](E:/XY%20switch/src/features/prompts/PromptsPage.tsx)：列表、编辑、应用与删除；不添加后端不存在的 app scope。 |
| Skills | [SkillsPage.tsx](E:/XY%20switch/src/features/skills/SkillsPage.tsx)：目录导入、Tauri 原生文件拖放、启停与卸载反馈。 |
| 会话 | [SessionsPage.tsx](E:/XY%20switch/src/features/sessions/SessionsPage.tsx)：筛选、详情、时间格式与删除反馈。 |
| 用量与审计 | [UsagePage.tsx](E:/XY%20switch/src/features/usage/UsagePage.tsx)：筛选、摘要、趋势、分页、供应商/模型汇总、请求详情、流式统计、被动审计与影子差分。 |
| 设置 | [SettingsPage.tsx](E:/XY%20switch/src/features/settings/SettingsPage.tsx)：通用、本地路由、凭据、数据、关于；复用现有后端服务。 |

默认窗口 1180×760、最小窗口 820×600；深浅色均覆盖。原有 Tauri 窗口配置未改。删除的 27 个旧文件为已替换的旧页面/审计组件、旧 CSS 及两份代码快照；不是删除对应业务功能。

## 2. 保留的契约与修复

- **供应商**：保存不等于激活；点击行只查看详情；空 API Key 不覆盖现有密钥；复制不复制密钥；连接测试与配置切换状态分开。
- **MCP**：后端整体覆盖保存时保留 `env`、`headers`、`builtIn`、`secretsConfigured`；环境变量值中的空格和等号不被截断；非法 URL 不提交后端。
- **危险操作**：取消不提交；失败保留确认框与错误；提交中禁止重复操作及 Escape 退出；完成后恢复焦点。会话删除返回 0 不报成功。
- **路由状态**：底栏和设置页共用互斥；用 generation 防止旧读取覆盖新状态；操作失败后回读；回读也失败时明确显示“状态未知”，而非永久“读取中”或伪造停止状态。
- **配置导入**：导入成功立即关闭确认框；后续刷新失败只允许重试刷新，避免再次导入。
- **自启与凭据**：自启读取失败是未知状态，写入后依据实际回读反馈；凭据仅查询是否存在，不读出密钥；单条查询失败显示未知，不伪装成“未配置”。
- **用量**：`successRate=98.5` 显示 98.5%，不再乘 100；0 成本保留为 0；后端 `1d` 的语义是昨天；用量时间戳按毫秒处理。新筛选失败不显示旧筛选数据，同筛选刷新失败保留已有数据并展示错误。
- **审计**：UTF-8 字节不当作字符数；硬证据与模型名称差异分开；上游未自报、reasoning 未知不当作零；样本不足不判可比；影子采样默认关闭，区分 consistent / suspicious / insufficient；过期分析不得跨筛选回填。
- **键盘**：[useDialogFocus.ts](E:/XY%20switch/src/ui/useDialogFocus.ts) 将 `summary` 纳入焦点循环，排除负 tabindex、隐藏控件和关闭的 `details` 内部元素；修复审计弹窗无法 Tab 到展开入口的问题。
- **模型编辑**：上下文窗口必须为空或安全正整数；启用 1M 时压缩阈值须为范围内整数；保留重排、发现模型、单项/批量去重及 Claude 配置。边界新增 13 项单元测试。
- **浏览器预览**：无 Tauri 时只读，禁止导入、自启等本机操作，主题仍可切换。未给后端尚未实现的价格保存能力制作假成功入口。

## 3. 验证结果与证据

| 验证项 | 结果 | 证据/范围 |
| --- | --- | --- |
| TypeScript + Vite production | 通过 | [构建日志](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/native-build.log) 的 beforeBuild 阶段：`tsc -b`、Vite build 完成。 |
| 前端单元/组件测试 | **21 文件，149 passed** | [frontend-tests.log](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/frontend-tests.log)。 |
| Rust 单元测试 | **198 passed，0 failed，3 ignored** | 接续阶段已执行 `cargo test --offline --lib` 的工具结果；未另存该轮日志。此后未修改 Rust 产品代码，未重复运行已通过的测试。 |
| 开发宿主浏览器回归 | **13/13 组通过** | [最终 results.json](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/browser-qa/results.json)、[browser-tests.log](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/browser-tests.log)。 |
| 生产 renderer 浏览器回归 | **10 个适用组通过** | [生产验收汇总](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/production-browser-qa/summary.json)；9 个业务/布局组与单独执行的 preview 组，使用实际 `dist`。 |
| 验收脚本语法 | 7/7 通过 | `node --check` 检查四个兼容入口、主 UI 脚本、CDP 工具与 fixture。 |
| Windows 原生 release | **未通过：环境阻塞** | [MSVC 构建日志](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/native-build-msvc.log)：`LINK : fatal error LNK1181`，无法打开 `kernel32.lib`。 |
| 测试进程清理 | 完成 | [cleanup.json](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/cleanup.json)：仅关闭本次三个根进程；1431、1432、9231 不再监听。 |

开发宿主 13 组：layout、providers、prompts、mcp、skills、sessions、usage、settings、routing、audit、model-editor、preview、error-boundary。每个业务组检查新增运行时异常；error-boundary 组的错误是宿主故意抛出。

生产 10 组为上述列表除 audit、model-editor、error-boundary 的其余组。后三者使用专用开发 QA 宿主，不作为 production entry 发布；专用审计宿主在生产环境的一次探测超时保存在原始 [生产运行记录](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/production-browser-qa/results.json) 中，未被改写为通过。其审计组件功能由开发宿主回归验证；生产汇总明确列出排除项，不将其算作生产通过项。

布局矩阵分别覆盖七页 × 两主题 × 两种窗口，共 28 张截图；另检查 1280×800 用量页。没有根级横向溢出。浏览器用内存 fixture 模拟 Tauri IPC、错误、延迟与原生拖放事件，**不等同于 WebView2/原生 IPC 实机测试**。

### 关键浏览器断言

- 删除取消、失败留框、busy Escape、Tab 焦点循环和恢复；紧凑模式供应商抽屉。
- MCP 四客户端挂载、保留字段与表单校验；Skills 拖放/卸载失败；会话 0 删除数。
- 用量分页和跳页、98.5%、零成本、空态、旧请求丢弃、新筛选失败隔离、同筛选刷新保留数据。
- 导入成功后刷新失败不再导入；自启未知和回读不匹配；凭据逐项未知；路由全局互斥与状态未知。
- 审计汉字 12 个对应 36 UTF-8 字节；6 / 36、token/byte 0.167、bytes/frame 3.000、frames/token 2.000；硬证据、名称差异、未自报和未知 reasoning 分开表达。
- 被动审计的跨时间/渠道样本及样本不足；影子采样开关和三种结果；延迟结果丢弃。
- 模型窗口 0 不可保存、重排、重复导入去重、42 行窄窗口滚动、保存回读及 Claude 配置。

### 构建产物说明

本次 Vite 输出：

- `E:\XY switch\dist\assets\index-Cl_3kGYE.js`：469.65 kB，gzip 143.78 kB。
- `E:\XY switch\dist\assets\index-V9FmWrxJ.css`：38.69 kB，gzip 6.90 kB。

这些是构建日志给出的 renderer 资源体积，不是 EXE 体积、安装包体积、启动时间或内存性能。本次未测量原生性能；README 中原有的历史性能数字不能视为这次改版的测量结果。

## 4. 截图

截图中的 Orion、Relay、Claude Lab、用量、延迟和审计证据均来自合成 fixture。

| 场景 | 截图 |
| --- | --- |
| 生产版供应商，1180×760，浅色 | [查看](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/production-browser-qa/layout-1180-light-0.png) |
| 生产版供应商，820×600，深色 | [查看](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/production-browser-qa/layout-820-dark-0.png) |
| 生产版用量，1180×760，浅色 | [查看](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/production-browser-qa/layout-1180-light-5.png) |
| 生产版设置，820×600，浅色 | [查看](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/production-browser-qa/layout-820-light-6.png) |
| 紧凑供应商详情抽屉 | [查看](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/browser-qa/provider-drawer.png) |
| 审计 UTF-8 字节详情 | [查看](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/browser-qa/audit-stream-bytes.png) |
| 被动审计 | [查看](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/browser-qa/audit-passive.png) |
| 影子差分三状态 | [consistent](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/browser-qa/audit-shadow-consistent.png) / [suspicious](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/browser-qa/audit-shadow-suspicious.png) / [insufficient](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/browser-qa/audit-shadow-insufficient.png) |
| 窄窗口模型编辑器 | [浅色](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/browser-qa/model-editor-light.png) / [深色](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/browser-qa/model-editor-dark.png) |
| 错误边界 | [查看](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/browser-qa/error-boundary.png) |

证据目录内保留的早期 `*-failure.*` 是调试记录，不代表最终开发宿主回归结果。以明确链接的最终结果与生产汇总为准。

## 5. 变更清单与统计

比较基准是接续时保存的 **163 文件 SHA-256 快照**，不是 HEAD 或 Git 暂存区。该快照已包含前序聊天完成的设计系统及部分新页面，因此下表只表示本轮接续差异，不声称是所有历史 UI 工作的总量。

| 指标 | 数值 |
| --- | ---: |
| 新增文件 | 20 |
| 修改文件 | 20 |
| 删除旧文件 | 27 |
| 增加行数 | 1539 |
| 删除行数 | 12232 |

逐文件状态、行数与前后 SHA-256 见 [workbench-ui-changes-20261017.json](E:/XY%20switch/docs/workbench-ui-changes-20261017.json)。按路径统计，移动按删除+新增处理，忽略 CRLF 差异；行数不是性能或质量指标。

范围包含完整 src/scripts 树及清单中显式捕获的根配置。README 没有起始快照，已修改但不计算原始行差；本报告与 JSON 清单为新交付文档，未混入上面的源码/脚本统计。`pnpm-lock.yaml`、`Cargo.toml`、`tauri.conf.json`、Vite/Vitest 配置相对快照未变。

## 6. UI 验收复现（原验收阶段未安装工具）

### 单元测试与 renderer 构建

在工作区已有依赖条件下执行；本次没有运行 `pnpm install` 或会安装依赖的 `verify-windows.ps1`。

```powershell
Set-Location -LiteralPath 'E:\XY switch'
& 'E:\node.js\node.exe' 'E:\XY switch\node_modules\vitest\vitest.mjs' run
& 'E:\node.js\node.exe' 'E:\XY switch\node_modules\typescript\bin\tsc' -b
& 'E:\node.js\node.exe' 'E:\XY switch\node_modules\vite\bin\vite.js' build
& 'E:\Rust\.cargo\bin\cargo.exe' test --offline --manifest-path 'E:\XY switch\src-tauri\Cargo.toml' --lib
```

Rust 测试通过不代表从空缓存可编译。原验收阶段的 SDK 缺失问题已在同日后续 EXE 构建中解决；独立 SDK 仅注入构建进程，不会自动改变其他终端的 LIB/INCLUDE/PATH。

### 浏览器回归

终端一启动开发 renderer，完成后在该终端 Ctrl+C 关闭：

```powershell
Set-Location -LiteralPath 'E:\XY switch'
& 'E:\node.js\node.exe' 'E:\XY switch\node_modules\vite\bin\vite.js' --host 127.0.0.1 --port 1431 --strictPort
```

终端二使用独立 profile；端口应为空闲，不要连接个人浏览器调试端口：

```powershell
$ErrorActionPreference = 'Stop'
$qa = Join-Path 'E:\XY switch\artifacts' ('ui-qa-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $qa -Force | Out-Null
$qaChrome = Start-Process -FilePath 'C:\Program Files\Google\Chrome\Application\chrome.exe' -WindowStyle Hidden -PassThru -ArgumentList @(
  '--headless=new', '--no-first-run', '--no-default-browser-check',
  '--disable-extensions', '--disable-background-networking',
  '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=9231',
  ('--user-data-dir="' + $qa + '\chrome"'), 'about:blank'
)
try {
  $ready = $false
  for ($i = 0; $i -lt 50; $i++) {
    try {
      Invoke-RestMethod -Uri 'http://127.0.0.1:9231/json/version' -ErrorAction Stop | Out-Null
      $ready = $true
      break
    } catch { Start-Sleep -Milliseconds 100 }
  }
  if (-not $ready) { throw 'Isolated Chrome did not start' }
  $env:CDP_ENDPOINT = 'http://127.0.0.1:9231'
  $env:APP_URL = 'http://127.0.0.1:1431'
  $env:QA_OUTPUT = Join-Path $qa 'results'
  $env:QA_ONLY = $null
  & 'E:\node.js\node.exe' 'E:\XY switch\scripts\verify-workspace-ui.mjs'
  if ($LASTEXITCODE -ne 0) { throw 'UI regression failed; inspect QA_OUTPUT' }
} finally {
  Stop-Process -InputObject $qaChrome -ErrorAction SilentlyContinue
}
```

也可在上述环境中用 `npm run test:ui` 调用同一入口。执行生产 renderer 验收时，将终端一改为 Vite `preview --host 127.0.0.1 --port 1432 --strictPort`，将 `APP_URL` 改为 `http://127.0.0.1:1432`，并设置：

```powershell
$env:QA_ONLY = 'layout,providers,prompts,mcp,skills,sessions,usage,settings,routing,preview'
```

此脚本使用 Node 内置 WebSocket 和 CDP，不需要安装 Playwright 或浏览器组件。四个兼容验收入口仍保留；`E:\XY switch\scripts\verify-release-renderer.mjs` 本次仅完成语法检查，未在新原生二进制上运行。它的代理变更模式还要求 `VERIFY_PROXY_TOGGLE=1` 与 `QA_ISOLATED=1`，不能对个人配置直接运行。

## 7. 原生构建：早期失败记录与后续解决

> 状态更新：用户随后允许下载缺失工具。SDK 已放入 `E:\AI tools\WindowsSDK\10.0.26100.9169`，2026-10-17 20:37（Asia/Shanghai）原生 release 构建成功。以下为解决前的诊断记录；新产物与当前验证边界以 [原生发布记录](E:/XY%20switch/docs/native-release-20261017.md) 为准。

发现本机已有 MSVC：`E:\build tools\VC\Tools\MSVC\14.44.35207\bin\HostX64\x64\link.exe`。加载现有 VsDevCmd 后链接器可用，但注册的 SDK 根 `C:\Program Files (x86)\Windows Kits\10` 仅见 Catalogs、Redist、UnionMetadata，没有 Lib/Include；已检查的现有工具目录未找到 `kernel32.lib`。构建环境的 LIB 只有 MSVC 库目录。

在 renderer 已构建后，本次原生重试命令为：

```powershell
Set-Location -LiteralPath 'E:\XY switch'
$msvcEnv = & cmd.exe /d /c 'call "E:\build tools\Common7\Tools\VsDevCmd.bat" -no_logo -arch=x64 -host_arch=x64 >nul && set'
foreach ($line in $msvcEnv) {
  if ($line -match '^([^=][^=]*)=(.*)$') {
    [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process')
  }
}
$env:PATH = 'E:\node.js;E:\Rust\.cargo\bin;' + $env:PATH
$env:CARGO_NET_OFFLINE = 'true'
& 'E:\node.js\node.exe' 'E:\XY switch\node_modules\@tauri-apps\cli\tauri.js' build --no-bundle --ci --config '{"build":{"beforeBuildCommand":null}}' -- --offline
```

上述命令在补齐 SDK 前失败于 `kernel32.lib`，当时依据原要求没有下载工具。用户后续允许下载后，已完成 SDK 校验、离线锁定依赖构建、PE 文件与 SHA-256 验证；早期失败日志仍保留。原生运行测试没有因构建成功而视为通过。

**本次不宣称已验证**：新 EXE 启动、WebView2 与真实 Tauri IPC、真实凭据存储、真实注册表自启、真实 CLI 文件写入/接管、真实上游连通性/计费、原生文件拖放实机链路、安装器安装/卸载、正式签名发布（本 EXE 未签名）、原生启动/内存/CPU 性能。

本次测试 Chrome 与两个 Vite 服务已关闭，专用 profile 和截图保留作证据；没有停止其他 Chrome/Node 进程，也未清理用户既有工作区改动。
