# 已批准的程序完善：验收报告

验收日期：2026-11-02，Asia/Shanghai。工作区：E:/XY switch/workspaces/session-favorites-20261029，当前未提交工作树。

## 结论

两项已批准的完善均完成，并通过离线、本地回环和已保存会话读回验证：

1. MCP 高级配置无损保存，并统一解析后的 `CODEX_HOME`。
2. 应用转发失败诊断，以及会话工具搜索、调用、审批和结果追踪。

此前真实 API 的 Chat 桥接、原生 Responses、工具执行与会话恢复结果见[真实联合验收报告](<E:/XY switch/workspaces/session-favorites-20261029/docs/joint-live-forwarding-acceptance-20261101.md>)。本轮新增真实 API 调用为 **0**；下面的结果属于本轮改进的离线及本地验收。

## MCP 配置可靠性

| 完成标准 | 实现与验证 |
| --- | --- |
| 保存、编辑后高级设置保留 | 在现有 Codex MCP 表上更新应用管理的字段；保留逐工具审批、启动超时、工具超时、启用与禁用工具列表、扩展字段及其注释 |
| 取消挂载、重启、重新挂载后设置仍在 | Codex 取消挂载写入 `enabled = false`；重启导入保留禁用状态；重新挂载恢复启用并保留高级字段 |
| 导入兼容已有写法 | 支持内联 TOML 表和已禁用的条目；导入保留文件内容，敏感值继续由凭据存储管理 |
| 切换传输方式后配置一致 | 保留共用高级设置，移除与新传输方式不兼容的 HTTP 或 stdio 字段 |
| 删除行为范围准确 | 删除对应 MCP 条目；未挂载服务器的删除可在无关客户端配置损坏时完成 |
| 自定义客户端目录一致 | 启动时解析同一个 `CODEX_HOME` 并传给 MCP 与接管管理器；已有模型目录一致性回归和新增 MCP 路径回归均通过 |

实现：[MCP 写入](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/mcp.rs:931>)、[启动目录解析](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/lib.rs:183>)。

新增配置回归：[mcp_regression_tests.rs](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/mcp_regression_tests.rs:18>)，覆盖完整编辑与挂载生命周期、内联禁用配置、传输切换、自定义目录和未挂载删除；均包含在最终 Rust 全量通过结果中。

## 转发失败诊断

请求记录与用量详情现在包含失败阶段、原因、实际发送尝试次数、是否收到过上游 HTTP 响应，以及每次发送的状态或错误。

| 完成标准 | 实现与验证 |
| --- | --- |
| 本地拒绝也有记录 | 覆盖供应商选择与配置、凭据、请求构建与转换、工具协议、预算和熔断阶段；未分配供应商的失败也进入用量查询 |
| 发送与响应证据准确 | 保留重试、故障转移中的发送结果；发生后续本地拒绝时，已有上游 HTTP 或超时证据仍可追踪 |
| HTTP 200 后的失败可定位 | 响应读取、JSON 解析、协议转换和流中断分别记录；收到的上游 HTTP 状态保留，未完成状态在界面显示 |
| 流式结果正确落库 | 消费完成后更新最终结果；识别不完整 SSE、传输错误、客户端取消和完整终止帧；较早流失败不会覆盖较新的健康结果 |
| 用量和健康含义准确 | 确定零发送的本地拒绝记录 token 为 0、费用为 0；本地拒绝、客户端取消和本地响应转换失败保留供应商健康；没有诊断的历史记录继续保留未知证据与未知用量 |
| 界面可读 | 用量列表及详情显示链路信息，提供中文和英文文案；会话导入用量保留其原有证据范围 |

实现：[诊断记录](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/request_diagnostics.rs:1>)、[请求入库](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/database.rs:817>)、[请求详情界面](<E:/XY switch/workspaces/session-favorites-20261029/src/features/usage/RequestDiagnosticsPanel.tsx:1>)。

最终全量测试中，新增 **8 项生产 HTTP 路由诊断回归**全部通过，覆盖本地拒绝、故障转移、超时证据、重试计数、无效 JSON、截断响应、不完整 SSE 和本地工具响应转换。

同时修正既有熔断用例：8 条记录中含 1 条零发送的本地熔断记录；发送次数合计 13，与本地服务收到的 9 次主供应商请求及 4 次备用请求吻合。

## 会话工具追踪

- 会话详情展示工具搜索、调用、审批和结果，保留原有工具参数、结果内容与协议标识。
- 依据记录显示等待执行、运行、等待审批、批准、拒绝、仅收到结果、完成、成功、失败、取消及未知状态。
- 工具结果与审批记录可跳转到对应调用；重复调用 ID、重复轮次和主记录与运行时记录的去重保留实际发生的事件。
- Codex 原生工具搜索与动态工具定义可读回；Claude、Gemini 工具格式、托管 MCP 发现与审批也有解析回归。
- 缺少明确结果或存在歧义时保留未知状态；元数据和显示大小限制继续生效。

实现：[工具事件解析](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/sessions/tools.rs:1>)、[会话显示及调用关联](<E:/XY switch/workspaces/session-favorites-20261029/src/features/sessions/SessionTranscript.tsx:1>)。

对此前真实验收保存的会话执行了单独离线读回，结果如下：

| 已保存路径 | 工具搜索 | 工具调用 | 准确工具结果 |
| --- | --- | --- | --- |
| Chat 桥接 | 0 | 2 | 2 |
| 原生 Responses | 1 | 2 | 2 |

读回用例：[saved_native_acceptance_tool_history_reads_back_without_new_api_requests](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/sessions/tests.rs:375>)。该用例读取已保存文件，不发起模型请求。

## 最终验证证据

| 验证 | 最终结果 | 日志 |
| --- | --- | --- |
| 前端完整测试 | 46 个文件、524 项通过 | [final-frontend.log](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/approved-improvements-20261101/final-frontend.log>) |
| TypeScript 检查 | 退出码 0 | [final-typescript.log](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/approved-improvements-20261101/final-typescript.log>) |
| Rust 默认 library 测试 | 418 项通过、0 失败、6 项忽略 | [final-native-lib-rerun.log](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/approved-improvements-20261101/final-native-lib-rerun.log>) |
| 指定的保存会话读回 | 1 项通过 | [final-saved-native-readback.log](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/approved-improvements-20261101/final-saved-native-readback.log>) |
| 改动空白检查 | 退出码 0 | [final-diff-check.log](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/approved-improvements-20261101/final-diff-check.log>) |

可复用的现有验证入口：

- 前端：`npm test`；类型检查：`npm run lint`。
- Rust：`cargo test --manifest-path src-tauri/Cargo.toml --offline --locked --lib`。
- 已保存会话：指定上述完整用例名称，并使用 `--ignored --exact --nocapture` 单独执行。

Rust 的 `--offline` 用于依赖解析；涉及生产代理的 HTTP 用例使用本地回环服务，测试数据库先清除默认真实供应商。首次全量测试发现的熔断旧计数断言已修正，最终重跑结果以表中日志为准。编译仍有既有 `ShadowOutcome` 未使用导入警告。

## 下一阶段优先顺序

1. **硬预算协议兼容**：完善 custom、tool_search、动态工具 schema 和原生 Responses 输入的计量，再验收开启预算后的工具调用及会话恢复。
2. **单任务总尝试与费用控制**：在当前逐发送证据基础上，对应用重试、故障转移与客户端重试叠加设置统一上限，完善取消后的停止派发和未知费用处理。
3. **安装包与插件业务验收**：验证相同源码构建的 GUI、接管、凭据读取、冷启动及恢复，并为实际使用的插件各完成一项代表性业务。

本轮验证覆盖源码回归与已保存真实会话的读回；新安装包 GUI、开启硬预算的真实请求和其他插件业务由下一阶段验收记录证明。
