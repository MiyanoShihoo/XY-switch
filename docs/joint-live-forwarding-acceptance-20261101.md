# 真实 API 经 XY Switch 转发：联合验收与完善建议

## 验收结论

2026-11-01，真实 Codex CLI 经当前源码的 XY Switch 生产代理访问此前提供的真实 API，Chat 桥接和原生 Responses 两条链路均通过。

完整过程为：客户端发起请求 → 应用转发真实 HTTPS 请求 → 模型提出工具调用 → 客户端执行本地工具 → 工具结果再次经应用发往 API → 返回准确收据 → 恢复同一会话并再次返回历史收据 → 应用会话模块读回结果。

- 客户端：Codex CLI 0.160.0。
- API：[superapi.buzz](https://superapi.buzz)，模型 gpt-6.1-sol。
- 最新真实验收时间：2026-11-01 22:50:36–22:51:42，Asia/Shanghai。
- 工作区：E:/XY switch/workspaces/session-favorites-20261029。
- 分支：codex/e2ee-sync-20261029；基准提交：fe95d1a2115186f0cb7ecb8726ff8ecb2335e544；修复在当前未提交工作树中。

## 最新真实结果

| 项目 | Chat 桥接 | 原生 Responses |
| --- | --- | --- |
| 客户端请求入口 | /v1/responses | /v1/responses |
| 真实上游入口 | /v1/chat/completions | /v1/responses |
| 实际上游请求数 | 3 | 4 |
| HTTP 状态 | 全部 200 | 全部 200 |
| 重试 | 0 | 0 |
| get_goal 实际执行 | 1 次 | 1 次 |
| receipt MCP 实际执行 | 1 次 | 1 次 |
| 工具搜索实际执行 | 本路径未调用 | tool_search_call 与 tool_search_output 各 1 条 |
| 工具参数 | a=17、b=25；随机 nonce 匹配 | a=17、b=25；随机 nonce 匹配 |
| 运算及结果回传 | 17+25=42；随机收据原样返回 | 17+25=42；随机收据原样返回 |
| 会话恢复 | 同一 session ID，再次原样返回历史收据 | 同一 session ID，再次原样返回历史收据 |
| 应用会话读回 | 工具结果和两条最终回复均存在 | 工具结果和两条最终回复均存在 |
| 服务端上报输入 token | 27,374 | 13,151 |
| 服务端上报输出 token | 225 | 198 |
| 服务端上报合计 token | 27,599 | 13,349 |

最新一轮共 7 次真实上游调用，服务端上报 40,948 token。客户端的会话累计 usage 没有被重复计入本报告；这些数值来自应用逐请求记录，不代表已独立核对的账单金额。

两条路径的会话与收据：

| 路径 | session ID | 两次最终回复均为 |
| --- | --- | --- |
| Chat | 01a19797-4af9-73d1-929f-6b021746d4d2 | XY_RECEIPT_b7f80808234b63386bf449d1ac1dc736 |
| Responses | 01a19797-cb7a-7c81-8d39-ff27abfc2347 | XY_RECEIPT_4216323b8ef984f25e204d09351c5af2 |

收据由本地 MCP 在调用时随机生成。核对原始会话中的 call_id、工具结果、上游请求中的结果和最终回复，证明实际工具执行及结果回传；恢复时 MCP 调用总数仍为一次。

## 排查过程与实际消耗

| 轮次 | 结果 | 真实上游请求 | 服务端上报 token |
| --- | --- | --- | --- |
| 14:31 UTC | MCP 因验收配置缺少逐工具授权而被 CLI 拒绝，完整链路失败 | 3 | 94,317 |
| 14:37 UTC | Chat 完整通过；原生 Responses 在应用内被错误拦截，上游请求为 0 | 4 | 126,065 |
| 14:50 UTC | 修复后的 Chat 与原生 Responses 完整通过 | 7 | 40,948 |
| 合计 | 三轮联合验收 | 14 | 261,330 |

本地配置预检未访问上游。上述合计不包含更早的 API 直连验收所上报的 18,422 token。

## 本次修复

### 原生 Responses 被错误交给 Chat 工具桥

此前所有非 None 的 Responses 转换都建立 ToolBridge，原生 ResponsesToResponses 也会因此进入只适用于跨协议转换的工具验证。真实 CLI 的 tool_search 被错误拒绝，客户端得到本地 400，API 没有收到请求。

现在只对 Chat、Anthropic、Gemini 转为 Responses 的路径建立工具映射；原生 Responses 保留原始请求，包括工具搜索、自定义工具、命名空间和动态工具定义。

源码：[proxy.rs](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/proxy.rs:1367>)。

### 原生 SSE 重组丢失工具及元数据

原生 Responses 流此前也经过通用 Delta 转换器再生成 SSE，转换器无法表示所有原生工具事件，导致 custom_tool_call、tool_search_call 等内容丢失。

现在原生 SSE 直接保留上游字节，仍经过现有用量与审计观察器。跨协议转换继续使用转换器。

新增生产路由回归覆盖：完整请求保留、非流式响应保留、完整 SSE 逐字节保留、7 字节拆分传输、中文和换行、自定义工具增量、工具搜索、加密推理、扩展事件、请求 ID，以及输入/输出/缓存 token 入库。

源码：[proxy.rs](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/proxy.rs:829>)；回归：[codex_client_regression_tests.rs](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/codex_client_regression_tests.rs:2>)。

### 验收 MCP 的逐工具授权

receipt MCP 的基础配置由应用 McpManager 保存。本次只在隔离的验收配置中，为 receipt 补写 tools.receipt.approval_mode="approve"，并设置启动及工具超时。CLI 接受该配置，真实工具调用通过。

这项验收配置调整没有实现产品 MCP 高级配置的无损保存，相关完善事项见下一节。

## 本地回归与证据复核

- Rust 默认 lib 套件：377 通过、0 失败、5 默认忽略。
- 新增原生生产路由回归单独通过，完整 lib 套件中也通过。
- 真实联合用例默认 ignored，本次显式执行后通过两条真实路径。
- 既有 Chat hosted-tool 拒绝、工具映射、会话、用量、代理和预算回归通过。
- git diff --check 通过；已有 ShadowOutcome 未使用导入警告仍存在。
- 对最新一轮的 CLI 事件、原始会话工具 call_id、MCP 单次调用记录、准确收据、代理请求、request_events 与 SessionStore 读回做了独立复核，结果通过。
- 三轮真实验收输出均通过真实 API Key 扫描。真实 Key 经一次性 Windows 管道进入内存，没有写入验收配置、日志或报告。

## 验收范围

- 本次证明当前源码的生产路由、配置生成器、MCP 保存器和会话读取实现能够完成上述真实链路。
- 使用隔离的数据库和客户端配置；真实 Key 从管道注入，用户实际凭据库的读取路径没有纳入本轮。
- 产品硬预算功能本轮保持关闭；客户端重试为 0，输出上限为 256 token。不能据此确认开启硬预算后相同请求也兼容。
- 真实执行覆盖 get_goal、receipt MCP 和原生工具搜索。自定义工具事件由本地生产路由回归验证。
- 安装包 GUI、其他工具实际操作及文档、表格、幻灯片等插件业务需要各自的验收证据。

## 程序优先完善事项

| 优先级 | 完善内容 | 当前依据 | 完成标准 |
| --- | --- | --- | --- |
| P1 | MCP 配置无损保存，并统一 CODEX_HOME | write_codex_entry 重建整张服务器表，会覆盖 tools.*.approval_mode、超时、工具白名单等高级字段；MCP 配置路径固定在 home/.codex，接管模块单独解析 CODEX_HOME | 保存/编辑/重新挂载后高级设置仍在；自定义 CODEX_HOME 下，配置、模型目录与 MCP 指向同一客户端目录 |
| P1 | 完整链路失败定位和会话工具记录 | 本次原生本地 400 没有对应 request_events；MCP 页面展示配置和挂载，无法证明实际连接、发现、审批和执行；原始会话存在工具搜索事件，而 SessionStore detail 没有显示这些事件 | 显示失败阶段、原因、是否访问上游和实际尝试数；工具搜索、工具执行、审批失败及结果回传可在会话中追踪 |
| P1 | 硬预算与当前 Codex 工具协议兼容 | budget::bound_payload 默认只允许 function 工具，并限制 encrypted_content 等输入；真实 CLI 原生请求含 custom、tool_search 及动态工具 | 为可计价的本地工具和动态 schema 提供完整输入计量；无法界定费用的请求明确显示限制；开启预算后重新完成工具与会话恢复验收 |
| P2 | 客户端与应用之间的总尝试次数及费用控制 | 应用重试、故障转移和客户端重试可以叠加；当前请求记录以最终派发结果为主 | 每次上游尝试可追踪；单任务有明确总次数/费用上限；取消后停止继续派发；未知用量保持未知 |
| P2 | 发版程序和插件实际业务验收 | 本轮验证生产源码路由，尚缺这次修复对应的安装包和其他插件业务证据 | 用相同源码构建安装包，完成 GUI 接管、凭据读取、冷启动和会话恢复；对实际使用的插件各完成一项代表性业务并检查生成结果 |

建议先完成 MCP 配置可靠性和失败定位，再完善预算兼容及发版验收。已有核心转发链路通过，下一阶段优先提高设置保存的可靠性和问题定位能力。

依据：[MCP 写入](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/mcp.rs:904>)、[MCP 配置路径](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/mcp.rs:598>)、[客户端目录初始化](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/lib.rs:175>)、[预算输入验证](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/governance/budget.rs:47>)、[MCP 页面](<E:/XY switch/workspaces/session-favorites-20261029/src/features/mcp/McpPage.tsx:18>)。

## 可复核材料

- [最终真实验收结果](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/joint-live-20261101/run-20261101T145036774Z-a0165e05/runner-result.json>)。
- [独立证据复核、三轮消耗及源码/执行文件 SHA-256](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/joint-live-20261101/run-20261101T145036774Z-a0165e05/evidence-audit.json>)。
- [Chat 真实请求记录](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/joint-live-20261101/run-20261101T145036774Z-a0165e05/chat/proxy-requests.json>)、[Responses 真实请求记录](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/joint-live-20261101/run-20261101T145036774Z-a0165e05/responses/proxy-requests.json>)。
- [Chat 会话读回](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/joint-live-20261101/run-20261101T145036774Z-a0165e05/chat/session-readback.json>)、[Responses 会话读回](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/joint-live-20261101/run-20261101T145036774Z-a0165e05/responses/session-readback.json>)。
- [Rust 本地回归日志](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/joint-live-20261101/local-lib-regressions.log>)、[原生 SSE 回归日志](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/joint-live-20261101/native-routing-preservation-fix.log>)。
- [真实联合用例](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/governance/live_client_tests.rs:467>)、[本地 MCP 收据工具](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/governance/live_receipt_mcp.mjs:1>)。
