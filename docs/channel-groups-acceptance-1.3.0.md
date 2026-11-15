# 1.3.0 渠道组与 Windows 原生验收

更新：2026-11-15。版本：1.3.0 预发布。

## 结果与证据范围

渠道组通过真实 Windows WebView、Tauri IPC 和本地代理验收：**9 项通过、0 项失败**。六个代理入口均返回 HTTP 400 / `channel_group_pilot_only`，本机上游接收器收到 **0** 次请求；`request_events`、`usage_events`、`budget_ledger` 三张表的记录数在验收前后均为 0。

本次原生程序为隔离的 debug 构建，SHA-256：`ce9a8aa12a9d0a7e6683daea70f12500cebc891c20d599a86b874a0e607c04e9`。验收完成于 `2026-11-15T05:53:25.688Z`。使用临时数据目录、合成供应商引用和三个本机回环接收器，通过真实界面创建与编辑配置。该结果证明这里列出的本机行为；安装、升级、卸载和其他平台需由相应环境的证据验证。

公开摘要与安装包清单随 [v1.3.0 预发布](https://github.com/MiyanoShihoo/XY-switch/releases/tag/v1.3.0) 提供。原始日志、数据库、截图和收费记录保留在本地归档；公开摘要不包含本机路径、凭据或个人会话统计。

## 检测点与验证方式

| 检测点 | 定位方法 | 方案与结果 | 验证方式 |
| --- | --- | --- | --- |
| 配置读写 | 原生界面和真实 IPC | 创建三渠道组、编辑、停用、启用；拒绝过期 revision | 当前 9 项原生验收中的配置断言 |
| 状态与历史 | 渠道组详情和记录入口 | 读取本地状态，运行固定合成请求时间线 | 原生 UI 截图与 IPC 返回值 |
| 共享额度 | 离线引擎 | 未知关系按共享池处理；明确独立后验证独立配额 | 轮询、池耗尽与配额感知断言 |
| 响应与会话 | 合成响应证据 | 429 等待最长有效恢复时间，401 隔离，流式未知结果保留亲和性 | 真实 IPC 调用离线引擎 |
| 真实代理入口 | 六条本地 HTTP 路由 | 全部拒绝试点模型，上游 0 请求，账本 0 增量 | 本地 HTTP 返回值、接收器计数和 SQLite 前后计数 |
| 重启持久化 | 关闭自有进程并重新启动 | 组配置保留，代理关闭，默认供应商保持原值 | 第二次原生启动后读取 IPC 和截图 |
| Rust 回归 | 当前后端源码 | **862 passed / 0 failed / 7 ignored** | 全量库测试日志；忽略测试不计为通过 |
| 前端回归 | 当前前端源码 | **904 passed / 0 failed**，185 个 suite | Vitest JSON 结果 |
| 浏览器辅助层 | 实际修改的隔离与 CDP 工具 | **84 passed / 0 failed** | `test:ui:network` |
| 打包门禁 | 版本、平台、安装器与原生验证工具 | **83 passed / 0 failed** | `test:packaging` |
| 类型与 renderer | 发布构建前置步骤 | TypeScript 和 Vite 构建通过 | `tsc -b && vite build` |

全量测试开始前的产品源码时间戳已核对。其后修改仅涉及验收脚本、测试入口与发布文档；受影响的辅助测试已重新运行。

## Windows 安装包静态核验

本次发行构建于 `2026-11-15T06:33:50.133Z` 完成。NSIS 和 MSI 均通过解包后的 x64 PE、版本资源、SHA-256 和程序内容核对。主程序与构建输入仅有 Tauri 安装类型标记的预期差异；两个包中的 `xy-marketplace.exe` 均与构建输入完全一致。MSI 的产品版本为 1.3.0，开始菜单应用标识为 `com.xyswitch.desktop`，未发现孤立的控件事件。NSIS 安装范围为当前用户。

| 安装包 | 字节数 | SHA-256 |
| --- | ---: | --- |
| `XY-Switch-1.3.0-windows-x64-setup.exe` | 7,802,625 | `16945d146e51fdad68e4b7e89df7c0283037b837ab1253203c910cf3040ff484` |
| `XY-Switch-1.3.0-windows-x64.msi` | 10,653,696 | `b34f3d57f14e066f2a6c40d79939521f4ba19a8a440baae79df6a4c2550405ed` |

两种安装器、主程序与附带程序的 Authenticode 状态均为 `NotSigned`。本机未执行安装或卸载。发布附件包含 `manifest.json`、`SHA256SUMS.txt`、`xy-switch_1.3.0_windows-static.json` 和去除本机路径的 `xy-switch_1.3.0_acceptance.json`；后续 CI 安装器验收以对应提交的运行结果为准。

## 复现入口

在仓库根目录、具有 Node.js、Rust 和 Tauri Windows 构建依赖的环境执行：

```powershell
pnpm test
pnpm lint
pnpm test:ui:network
pnpm test:packaging
cargo test --manifest-path src-tauri/Cargo.toml --locked --lib
pnpm tauri build --debug --no-bundle -- --locked
node scripts/native-acceptance/channel-groups.mjs <debug-executable> out/native-acceptance
```

原生脚本要求可验证的 debug 程序及仓库 `out` 内的输出目录，拒绝使用不支持数据隔离的 release 程序。它仅终止自己启动的进程。参见[原生验收脚本](../scripts/native-acceptance/channel-groups.mjs)、[调度测试](../src-tauri/src/channel_groups/tests.rs)和[代理门禁测试](../src-tauri/src/channel_group_proxy_tests.rs)。

## 功能状态

`offlinePilot`、`liveEnabled=false`、`billingHold=true` 和 `dispatchAllowed=false` 仍由后端固定返回。模拟不写真实账本。真实多渠道上游派发尚未实现；启用渠道组仅改变离线选择资格。

既有 HTTP 525 请求的实际 Token 与费用尚未核实，收费测试继续暂停。本次新增真实模型 POST 为 **0**。未知费用保持未知，预算预留不转换为已结算费用。

架构和完整字段规则见[渠道组架构](channel-groups-architecture.md)与[本地接口契约](channel-groups-contract.md)。

## English summary

Nine isolated Windows native checks pass through the real WebView, IPC and loopback proxy. All six tested proxy routes reject pilot models; upstream receivers see zero requests and all three billing-table counts remain unchanged. Current source validation records 862 Rust passes (7 ignored), 904 frontend passes, 84 browser-helper passes and 83 packaging-gate passes.

The native acceptance executable is a debug build with a recorded SHA-256, not an installed release application. Installation/upgrade/uninstall and other-platform acceptance require their own evidence. Channel groups remain an offline pilot. The unresolved HTTP 525 billing hold stays active, and this acceptance adds zero real model POSTs.
