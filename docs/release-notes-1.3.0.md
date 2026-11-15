# XY Switch 1.3.0 — Channel groups preview / 渠道组预发布

## 简体中文

- 供应商管理新增渠道组：创建、编辑、启停虚拟模型组，配置渠道、权重、优先级、限额、预算与冷却。
- 提供轮询、加权轮询、最少负载、延迟优先和配额感知的离线验证；共享额度、会话亲和性、429 冷却与鉴权错误隔离参与调度判断。
- 复用现有供应商、凭据引用和用量记录；未知 Token、余额与费用保持未知。模拟结果与已记录真实事件分开呈现。
- 包含当前工作区的 Agent 工作区、用量阅读、任务导航、双语界面改进，以及 Windows 应用身份和单实例启动修复。

### 验收与发布范围

本版通过 Windows 隔离程序的 **9 项真实 WebView / IPC 验收**。六条本地代理入口均返回 `channel_group_pilot_only`，上游接收器收到 0 次请求，账本记录数没有增加。全量 Rust **862 项通过、7 项忽略**，前端 **904 项通过**，辅助网络测试 **84 项通过**，打包门禁 **83 项通过**。详见[验收报告](channel-groups-acceptance-1.3.0.md)、[架构](channel-groups-architecture.md)与[接口契约](channel-groups-contract.md)。

这是预发布版本。渠道组固定为离线试点，虚拟模型请求在本地返回 `channel_group_pilot_only`；本版不提供真实上游多渠道派发。

2026 年 11 月 11 日记录的上游 525 请求，其实际用量与费用尚未核实。收费测试暂停继续生效；安装本版或新增渠道组不会解除该暂停。

本地验收针对 Windows 隔离调试程序、真实 IPC、本地代理门禁与新构建安装包的静态核验。其他平台、安装升级流程、真实供应商请求的验收结果须由对应环境的独立证据确认。Windows 安装包未做商业代码签名。

本次新构建的 Windows x64 NSIS / MSI 均通过版本、SHA-256、包内主程序及附带程序比对，MSI 应用标识和控件关联检查通过。发布附件提供校验和、构建清单、原生验收摘要及静态核验报告。

## English

This preview adds virtual channel groups under Providers, with editable channel bindings, weights, priorities, limits, budgets and cooldowns. The offline engine supports round robin, weighted round robin, least load, latency preference and quota awareness, including shared quota pools, affinity and response-driven health decisions.

Existing provider references and usage records remain the source of truth. Unknown usage and cost remain unknown. This release also includes the current Agent workspace, usage-reading and navigation improvements, bilingual UI updates and Windows application-identity/single-instance fixes.

Nine real Windows WebView/IPC checks pass in isolation. Six loopback proxy routes reject pilot models with zero upstream requests and no added billing records. Validation records **862 Rust passes (7 ignored), 904 frontend passes, 84 browser-helper passes and 83 packaging-gate passes**. See the [acceptance report](channel-groups-acceptance-1.3.0.md).

**Channel groups remain an offline pilot.** Virtual-model requests fail locally with `channel_group_pilot_only`; live multi-channel upstream dispatch is not implemented. The billing hold for the HTTP 525 event recorded on November 11, 2026 remains in force because actual usage and charges have not been verified.

Local acceptance targets the isolated Windows debug application, real IPC, the local proxy gate and static inspection of newly built Windows packages. It does not establish other-platform, installation/upgrade or live-provider acceptance. Windows packages are not commercially code-signed.

Both newly built Windows x64 packages pass version, SHA-256 and extracted-payload checks, including the companion executable, MSI application identity and control references. Release assets include checksums, the build manifest, a native acceptance summary and the static verification report.
