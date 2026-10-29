# 真实验收跟进 — 2026-10-29

## 结果

针对用户提供的 SuperAPI 测试端点，使用其提供的凭据、网关自报模型 ID `gpt-6.1-sol`，**Chat 上游 6/6、Responses 上游修复后 6/6 通过**。这些 ID 与响应证据不认证官方模型身份或权重。模型发现返回 HTTP 200、11 个 ID，只作为发现证据。

| 客户端检查 | Chat 上游 | Responses 上游（修复后） |
| --- | --- | --- |
| chat | 通过 / 4562 ms | 通过 / 2548 ms |
| stream | 通过 / 1846 ms | 通过 / 1878 ms |
| tools | 通过 / 1811 ms | 通过 / 1849 ms |
| responses | 通过 / 2369 ms | 通过 / 1923 ms |
| anthropic | 通过 / 1967 ms | 通过 / 2295 ms |
| gemini | 通过 / 1951 ms | 通过 / 1652 ms |

每项均核对 HTTP 状态、JSON/SSE 结构、完成状态及精确文本 `OK`；工具项核对唯一函数名称和完整参数，不执行工具。流式项要求结束原因与终止事件。实际链路是回环 HTTP → 代理鉴权/预算 → 路由与协议转换 → 真实上游；不是模拟 IPC。所有上游事件为 200、重试 0。使用隔离内存数据库、临时目录和一次性命名管道，没有使用生产凭据库或用户数据库。

## 发现与修复

1. 先前 `gpt-5.5` 的两次 HTTP 500 是独立历史失败，保留在[原记录 E6/E7](governance-acceptance-20261029.md)。此次成功不能解释或抹去其根因。
2. 三项有界直连诊断（Chat、Responses、Responses SSE）均 200、精确 OK、完成；不因此跳过完整代理验收。
3. Responses 上游首轮 5/6：客户端 Responses 路径 HTTP 200，但验收器把合法的 `error: null` 当成错误。修复验收器及 OpenAI SSE 归一化的空值判定，再次完整运行得到 6/6。非空错误（包括空对象、false、空字符串）仍拒绝，缺终止或 incomplete 仍失败；增加三项针对性单测。

## 次数、预算与边界

- 本次跟进共 **21 次生成请求**：3 个直接诊断、Chat 6 项、Responses 修复前 6 项及修复后 6 项；没有自动重试。不含此前两次 500。
- 每轮完整验收 6 次预留 **49,280 Token / 49,408 微美元**；输入预留每次 8192，文本输出上限 16、工具 48，合计输出上限 128。每日账本上限 50,000 Token / 60,000 微美元。
- 测试自定义价为输入 1 / 输出 2 USD 每百万 Token，**不是真实供应商报价**。账本硬限不等于供应商真实账单硬限。
- 极短可见提示词仍收到输入 4390 Token 的用量报告；工具项报告输入 4432、输出 19，文本项输出 5。存在网关不可见输入开销，不能按提示词长度估计实际账单，也不能用模型列表证明定价或型号真实性。
- 不覆盖原生 Gemini 上游预算、所有模型、任意上下文/多模态、互联网远程管理或移动后台常驻。

## 证据

脱敏日志仅保留在开发副本的 `artifacts/governance-qa/`（Git 忽略）：

- `live-protocol-diagnostics.log`
- `live-e2e-chat-followup.log`
- `live-e2e-responses-followup.log`（5/6 失败保持原样）
- `live-e2e-responses-null-error-fix.log`

测试源码：[live_tests.rs](../src-tauri/src/governance/live_tests.rs)；修复：[monitor.rs](../src-tauri/src/governance/monitor.rs)、[streaming.rs](../src-tauri/src/streaming.rs)。没有把凭据或任意上游错误正文写入源码与日志。
