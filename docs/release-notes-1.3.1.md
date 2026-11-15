# XY Switch 1.3.1 — Validation and packaging fixes / 验收与打包修复

## 简体中文

- 修复窄屏导航抽屉中键盘焦点框被滚动容器裁切的问题，保留正向与反向键盘导航。
- 更新真实浏览器验收，覆盖当前客户端首页、供应商独立编辑页、切换确认、MCP 详情、Skills 来源管理和设置任务目录；保留取消、失败恢复、未保存更改、凭据及计费相关断言。
- 修复 Windows 目录联接测试的路径格式，并在创建失败时保留诊断输出。
- macOS Universal 打包先构建两个架构，合并并核验 `xy-marketplace` 附带程序，再生成 App 与 DMG；缺失架构、合并失败或无输出时终止打包。
- 将应用、Tauri 与 Cargo 版本统一为 1.3.1。既有 `v1.3.0` 标签与报告保留其原始提交和验收范围。

### 发布证据

本版的安装包、SHA-256、构建清单、浏览器及原生验收摘要随 [v1.3.1 预发布](https://github.com/MiyanoShihoo/XY-switch/releases/tag/v1.3.1) 提供。发布记录绑定准确的源码提交；完整 CI 的各平台作业给出安装、启动、卸载与打包检查的实际结果。Windows 隔离调试程序的 WebView / IPC 报告与安装包静态核验分别记录。

渠道组继续为 `offlinePilot`，`liveEnabled=false`、`dispatchAllowed=false`。`xy-group/` 虚拟模型请求在本地返回 `channel_group_pilot_only`，真实上游多渠道派发尚未实现。2026 年 11 月 11 日上游 525 请求的实际 Token 与费用仍未知，`billingHold=true`，收费测试继续暂停。

渠道组功能与历史验证详见 [1.3.0 说明](release-notes-1.3.0.md)、[架构](channel-groups-architecture.md)及[接口契约](channel-groups-contract.md)。历史报告中的数量和二进制哈希只属于对应版本；1.3.1 以本版发布附件为准。

## English

- Keep keyboard focus rings visible in the compact navigation drawer, including forward and reverse traversal.
- Update real-browser acceptance for the client home, dedicated provider editor, switch confirmation, MCP details, Skill source management and settings task directory. Preserve cancellation, recovery, unsaved-change, credential and billing assertions.
- Normalize paths in the Windows junction test fixture and retain command diagnostics on failure.
- Build both macOS architectures, merge and verify the `xy-marketplace` companion, then bundle the universal App and DMG. Missing slices, failed merges or missing output stop packaging.
- Align the application, Tauri and Cargo versions at 1.3.1 while preserving the existing `v1.3.0` tag and evidence.

The [1.3.1 prerelease](https://github.com/MiyanoShihoo/XY-switch/releases/tag/v1.3.1) carries package hashes, build metadata and acceptance summaries tied to the exact source commit. CI job results establish each platform's package and installation checks. Isolated Windows debug WebView/IPC acceptance and static installer inspection are reported separately.

Channel groups remain an offline pilot with live dispatch disabled. Virtual-model requests fail locally with `channel_group_pilot_only`; live multi-channel upstream dispatch is not implemented. The billing hold for the November 11, 2026 HTTP 525 event remains active because actual token usage and charges are unknown. Historical reports and binary hashes apply only to their recorded versions.
