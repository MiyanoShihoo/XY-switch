# 单任务总尝试与费用控制验收

更新日期：2026-11-02（Asia/Shanghai）

开发分支：`codex/e2ee-sync-20261029`

交付状态：实现、本地回归、隔离 renderer CI 及源码推送均已完成；最终文档补充按同一已验收代码树交付。

## 1. 范围与配置

本轮完成同一逻辑任务的持久化总尝试/费用上限。每次应用重试、供应商故障转移、客户端重试和模型复核均经过统一准入。默认策略字段 `taskMaxAttempts=12`、`taskMaxMicrousd=250000`（USD 0.25）；旧策略加载获得相同默认值。0 表示阻止新尝试，不表示关闭限额。

硬预算关闭时新任务不设限；已有限额继续生效。启用或调低策略会收紧同一任务，随后放宽或关闭策略不会清账或提高已有任务上限。跨 UTC 日、同进程重开数据库及进程重启保留累计。历史无费用预留的任务启用费用限额后以 `task_unreserved_cost` 阻断。

任务、逐次发送和每日预留分别保存在 `proxy_tasks`、`proxy_attempts` 和既有预算账本中。真实网络派发在同一 SQLite immediate transaction 提交任务序号与费用预留之后启动；连接取消闸与任务取消闸覆盖准入及启动。模型复核的专属预留与任务/每日账本共用事务，任一失败全部回滚。

## 2. 身份、取消与费用契约

| 项目 | 当前行为与证据 |
| --- | --- |
| 稳定身份 | `X-XY-Task-ID` 或 `Idempotency-Key`；两者同时提供必须相同。没有显式头时，从 Codex 的 `client_metadata` 或 `x-codex-turn-metadata` 读取会话与根轮次，头/体结果冲突会拒绝。身份连同访问 scope 生成摘要，避免把原标识写入任务表。 |
| 不稳定请求 | 硬预算开启时拒绝身份缺失、不合法、重复或冲突；硬预算关闭的普通请求仅按单次客户端请求归属，无法合并独立客户端重试。合成监控请求拥有独立任务，模型复核继承原任务。 |
| 每次派发 | 在发送前建立唯一发送标识、任务内序号、客户端请求标识、供应商、模型、路由及预留，实际用量与请求事件随后关联；并发准入使用原子事务。 |
| 取消 | 确认后持久化取消状态并唤醒同任务等待、退避及流式请求；取消后新增请求/模型复核拒绝派发。迟到结算可以补充已知费用，不能撤销取消。 |
| 连接 EOF | 实际 TCP 断开回归通过；当前 HTTP/1 读取 EOF，包括合法写半关闭，也按取消处理。未实现写半关闭后继续接收响应，不声明支持该契约。 |
| 进程恢复 | 独占进程文件锁限制同一数据库的跨进程派发，同进程多句柄共享取消信号；重启将未完成发送标成 interrupted/process_restart，保留次数、预留及任务上限。 |
| 未知费用 | 缺失/不完整 usage、超时、失败、取消或无法确定价格继续占用完整预留，不显示为零费用退款。已知费用低于预留也不退款。 |
| 实际超额 | 累计占用 = 累计预留 + 各次已知实际费用超过各自预留的部分。新发送的准入占用不得越限；已发送请求若返回超额，则增加占用并阻断后续不符合上限的发送。最终账单仍由供应商决定。 |
| 本地拒绝 | 尝试/费用耗尽、取消、请求构建或其他准入失败不增加物理发送、费用预留或供应商失败计数；保留明确的诊断原因及任务快照。 |

实现入口：`src-tauri/src/governance/tasks.rs`、`proxy.rs`、`proxy_connection.rs`、`database.rs`、`response_model_probe.rs` 及 `model_verification/runtime.rs`。

## 3. 配置、任务详情与诊断

- 「安全与验收 → 硬预算」保存单任务尝试和六位小数 USD 费用上限，明确身份、历史策略和费用占用语义。
- 「最近任务」显示最近 50 个任务，每 5 秒刷新，支持完整逐次发送记录、空列表、零发送记录、查询/取消失败重试及确认后取消；取消提交中禁止重复提交，切换任务重置确认。
- 详情显示累计占用、累计预留、已知实际费用、未结费用次数、历史无预留次数及进行中的尝试；中文/英文、暗色、390px 窄屏验证均覆盖。
- 请求诊断展示任务标识、客户端请求标识、逐次发送标识与序号，并明确累计是记录产生时的快照，最新状态查看最近任务。
- 浏览器预览明确显示原生任务操作不可用，没有伪造任务记录或假装保存/取消成功。

## 4. 回归证据

| 验证 | 本轮结果 | 覆盖 |
| --- | --- | --- |
| 任务 Rust 专项 | 25 项通过 | 身份隔离/冲突、精确次数与费用边界、原子准入、重试/故障转移/客户端重试、生产 HTTP 并发、取消等待/退避/流式/TCP EOF、超时、缺失/部分 usage、实际超额、策略变化、重启及事务回滚。 |
| 模型复核专项 | 13 项通过 | 继承原任务、原任务上限耗尽时不增加网络及两类预留、复核等待/流式取消，连同既有模型复核回归。 |
| 新前端专项 | 3 文件、29 项通过 | `src/governance.test.ts`、`src/task-budget.test.tsx`、`src/request-diagnostics.test.tsx`，覆盖参数 IPC、六位费用、任务详情、语言及快照。 |
| 完整前端 | 47 文件、531 项通过 | 现有完整 Vitest 回归。 |
| 完整 Rust library | 455 passed、7 ignored、0 failed | 默认 library 全量 462 项；7 项 opt-in 未计作本轮执行成功。 |
| 类型与生产 renderer | 均通过 | `pnpm lint`、`pnpm run build:renderer`。 |
| 隔离 renderer CI | 6 套、75 项交互全部通过 | 同一干净源码提交完成 47 文件/531 项前端、81 项界面辅助检查、类型和 renderer 构建；严格本地网络断言通过，浏览器与 server 均关闭。 |
| 格式与 diff | 新增四个 Rust 模块格式检查、`git diff --check` 通过 | 全仓存在历史 Rust 格式差异；HEAD 未改动 xai_responses.rs 的独立基线检查退出 1，本轮不扩大格式化范围。 |

本轮所有新 HTTP 回归使用生产代理路由和本地模拟上游；usage/价格用于验证记账，不是供应商实际账单。新增收费 API 调用 **0**。完整 Rust 结果仅有既有 `ShadowOutcome` 未使用导入警告。新增 Rust 模块完成格式检查，取消测试同步修复后再次完成全量 library 回归。

任务测试入口：`src-tauri/src/governance/task_tests.rs`、`task_proxy_tests.rs` 和 `src-tauri/src/model_verification/probe_tests.rs`。格式化后复验发现退避取消测试存在时序竞争：测试每 5ms 轮询，但测试构建的退避仅 1ms，取消可能晚于第二次发送。已改用数据库专属、仅测试编译的同步屏障，明确等待进入重试阶段后取消，生产构建不包含该屏障。修复后再次完成全量 Rust library 验证：455 项通过、7 项 opt-in 忽略、0 失败，最终证据为 `artifacts/task-budget-qa/checks/rust-library-final.log`。前端专项最终证据为 `artifacts/task-budget-qa/checks/frontend-task-final.log`；旧 `task-regression-final.log` 保留复验历史，其中包含修复前失败及一次未匹配测试的过滤器执行，不作为最终通过证据。

## 5. 浏览器与隔离环境验收

本机 Edge 与 Chrome 两份 `governance/results.json` 均记录 **17 组治理交互、17 PASS**，新增场景覆盖任务限额保存、历史限额保留、未知/超额费用、取消确认、切换/重复提交、错误恢复、空/零发送记录及英文暗色窄屏。最终 `assertLocalNetwork()` 均失败，分别检测到安全软件注入的 `me.kis.v2.scr.kaspersky-labs.com`、`gc.kis.v2.scr.kaspersky-labs.com` 外部来源，故本机浏览器整组验收**未通过**。网络断言保持原标准，浏览器和 renderer server 均已关闭。

本机证据位于 `artifacts/task-budget-qa/20261102/`、`artifacts/task-budget-qa/20261102-chrome/` 中的 `headless-acceptance.json`、`governance/results.json`、`verification.log` 及截图。已视觉检查中文桌面任务详情和英文暗色 390px 详情；费用与状态显示一致。此机器先前已有相同注入记录，见 [原生验收](native-acceptance-20261030.md) 和 [界面重建验收](workspace-ui-reconstruction-20261030.md)。

隔离 `Build and verify` 运行 [41413538674](https://github.com/MiyanoShihoo/XY-switch/actions/runs/41413538674) 已完成，最终状态 `success`，`renderer-ui` 成功；输入为 `renderer_acceptance_only=true`。源码 SHA 严格绑定 `0eaedf958c6c0271f47bcc80cc31859597263aa9`，运行平台为 GitHub 托管 Linux。类型、完整前端 47 文件/531 项、81 项界面辅助检查、生产 renderer 构建及六套 headless UI 全部成功。安装包及其他平台任务本次按工作流设计跳过，未计入本轮验收成功。

| 隔离交互套件 | 成功 / 总数 |
| --- | --- |
| navigation | 8 / 8 |
| workspace | 15 / 15 |
| i18n | 16 / 16 |
| sessions | 8 / 8 |
| governance | 17 / 17 |
| sync | 11 / 11 |
| 合计 | 75 / 75 |

产物 `workspace-renderer-ui` 的不可变 ID 为 `13126248750`；GitHub 返回摘要与本地 ZIP 实算摘要一致：`sha256:9c04fc28087d75a072830218bb4f9b266d8f9f01e0a2c8ce6cfa31e0e9a6f7fc`。下载后逐份检查六套结果文件，全为 PASS；`headless-acceptance.json` 确认同一源码、仅精确 renderer HTTP/WebSocket 来源、`passed=true`、`browserClosed=true`、`serverClosed=true`。已视觉检查 CI 中文桌面任务详情及英文暗色 390px 详情，累计预留、已知超额、未结费用与状态显示一致。

本地下载证据位于 `artifacts/task-budget-qa/ci-41413538674/`，步骤日志为 `ci-renderer-41413538674.log`；同目录上级的 `ci-status.json`、`ci-artifacts.json`、`ci-download-receipt.json` 和 `acceptance-evidence.json` 记录来源、摘要及计数，均为本机验收产物。安装包与插件业务验收是开发计划中独立的下一项 P0。

## 6. 交付核对

2026-11-02 本轮源码提交 `0eaedf958c6c0271f47bcc80cc31859597263aa9` 已正常推送至 `codex/e2ee-sync-20261029`，推送后核对远端 SHA 与本地一致；其父提交为上一轮文档 `f4bd5268b9639c018dffc0d56290df0a6428556f`。源码以该 SHA 完成隔离 CI 验收。

验收完成后的补充提交仅更新本文与 `docs/development-plan.md`；交付时再次检查该文档提交与已验收源码之间没有代码、测试、工作流、脚本或依赖差异，并核对当前开发分支远端 SHA。具体文档提交及远端结果保存在本机 `artifacts/task-budget-qa/delivery-receipt.json`，源码证据与最终文档提交分别记录。
