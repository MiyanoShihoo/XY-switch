# 原生 Responses 硬预算兼容验收

日期：2026-11-02

## 交付范围

完成原生 Responses 在硬预算开启时的本地工具和完整会话输入准入。合法、可计量且预算充足的请求经生产 HTTP 代理发送；超出预留、每日配额不足或携带不可计量上下文的请求在发送前返回明确拒绝原因。

此前真实收费 API 的联合验收见 [真实转发报告](joint-live-forwarding-acceptance-20261101.md)。本轮新增收费 API 请求 **0**。下面的 HTTP 和实际 Codex CLI 验证均使用本地回环模拟上游；不能将模拟响应的 usage 或预留费用当作实际账单。

## 输入覆盖与边界

| 输入 | 硬预算准入 |
| --- | --- |
| 本地 function、custom 文本工具、Lark/regex grammar | 支持，完整描述和 schema 纳入序列化计量 |
| namespace 及其中的本地工具 | 支持，名称、命名空间和定义保留 |
| 明确 execution=client 的 tool_search | 支持，搜索调用和返回的内联定义均计量 |
| additional_tools 动态定义 | 支持，本地工具定义与完整恢复历史保留 |
| function_call/custom_tool_call 及文本工具结果 | 支持，原始参数、结果和非 ASCII 文本均计量 |
| 完整 user/developer/system/assistant 文本历史、透明 reasoning | 支持，消息和工具历史全部计量 |
| metadata、结构化输出 JSON schema、客户端元数据 | 支持，合法类型及完整内容计量 |
| 远端会话/响应/提示引用，多模态输入，非空加密历史 | 发送前拒绝，避免未包含在请求中的上下文 |
| 服务端工具、未知协议字段、非法类型 | 发送前拒绝，保存具体原因码 |

预算关闭时继续沿用原有转发策略。硬预算只收紧请求输入准入；本地或上游返回的原生 Responses 内容继续保留，包括响应中的不透明 reasoning 字段。后续若将该加密内容作为输入重放，会受到输入准入限制。

## 计量与账本

1. 对实际派发模型要求精确自定义价格；参考价、模糊匹配和未知服务等级价格不能授权发送。
2. 先将输出上限收紧到请求限制与预算策略限制的较小值。
3. 对将要发送的完整 JSON 序列化，要求 **UTF-8 字节数 + 256 ≤ 每次输入预留 Token**。工具描述、grammar、schema、结果、元数据和恢复历史均包含在内。
4. 每次发送前在事务中预留输入和输出额度，费用按最高输入价及输出价向上取整到微美元。
5. 每日 Token 和费用上限并发检查；失败、超时或取消不自动退回预留，账本跨应用重启保存。

这个字节准入规则是保守的本地预留策略，不是模型 tokenizer 的精确结果，也不保证上游账单或隐藏处理费用。上游模型的实际 token 定义及计费仍由供应商决定。

## 验证结果

| 验证 | 结果 | 本地日志 |
| --- | --- | --- |
| 硬预算单元回归 | 11 项通过 | artifacts/responses-budget-20261102/budget-unit.log |
| 生产原生 Responses HTTP 回归 | 3 项通过 | artifacts/responses-budget-20261102/production-responses.log |
| Rust 全量 library 回归 | 427 项通过、0 失败、7 项 opt-in 测试忽略 | artifacts/responses-budget-20261102/rust-library.log |
| 实际 Codex CLI 三种接入方式 | Chat 桥接、原生直连、原生预算代理共 3 项通过 | artifacts/responses-budget-20261102/codex-client-modes.log |
| 前端相关回归 | 2 文件、36 项通过 | artifacts/responses-budget-20261102/frontend-related.log |
| 前端完整回归 | 46 文件、524 项通过 | artifacts/responses-budget-20261102/frontend-full.log |
| TypeScript 与 renderer 生产构建 | tsc -b 和 Vite 构建退出码 0 | artifacts/responses-budget-20261102/renderer-build.log |

默认 Rust 测试的 7 项忽略中，3 项 CLI 测试已使用已安装的 Codex CLI 0.160.0 显式执行并通过；其余 opt-in 测试沿用各自的环境要求。既有 ShadowOutcome 未使用导入及 Vite 分块提示不影响本轮通过结果。 新预算模块和新 HTTP 回归已按项目 Rust 版本格式化；全仓 cargo fmt --check 仍报告未修改文件中的历史格式差异，记录为 rust-format.log，本轮保留原有范围。

### 生产 HTTP 证据

- 从实际 CLI 的本地验收保存的 4 条完整原生请求，连同包含命名空间、动态 schema、客户端搜索、工具结果和恢复历史的请求，分别进行普通及流式发送，共 10 次准入转发；输出 cap 收紧为 64，其余请求字段及原生响应保持完整。
- 中文工具结果在 UTF-8 字节精确边界准入；预留少 1 字节时发送前拒绝。
- 服务端工具、远端/加密/多模态输入、非法 schema、未知字段、每日 Token 或费用不足时返回 HTTP 402；上游次数及预留账本不增加。
- 被拒绝请求的诊断阶段为 budget，send_attempts=0，无上游响应。本地 usage 和费用为 0；健康状态不因本地拒绝变化，治理事件保存具体静态原因码。
- 请求 fixture 保存在 src-tauri/src/governance/fixtures/codex-native-requests.json；这是 CLI 访问本地模拟上游的请求样本。

### 实际 CLI 证据

使用生产 sync_codex_provider 配置生成器，将隔离 CODEX_HOME 的原生 Responses provider 指向应用代理 /v1，保持生产代理运行。模型侧响应为确定性的本地 fixture。

- 实际客户端 tool_search 搜索并返回 spawn_agent 的 namespace/schema；仅完成发现。
- 实际执行 get_goal 和故意非法的 custom apply_patch，验证工具执行及解析器结果回传。
- 第一轮工具链和第二轮恢复使用同一会话：01a19852-4280-7203-8721-b6be550a9517。
- 上游共收到 5 次请求。每次输出 cap=512，输入预留=128,000；账本共 5 次、642,560 Token、645,120 微美元预留。
- 恢复请求保留 11 个输入项，包括搜索返回的定义和完整工具结果；应用读回包含 1 个搜索事件。
- 每次应用请求日志均成功，send_attempts=1，有上游响应，无失败阶段。

最终 CLI 证据目录：artifacts/responses-budget-20261102/codex-e2e-native-budget-66c404d5-7e4c-4b9a-9425-6d52fadca9f3。包含 budget-proxy-evidence.json、mock-requests.json、session-readback.json 和两轮 CLI 输出。

## 截图中的构建失败

已从 GitHub API 和有权限的任务日志确认：截图红色检查关联 **v1.0.0 标签**的运行 [39224490704](https://github.com/MiyanoShihoo/XY-switch/actions/runs/39224490704)，提交 1f0c8c197b9aab301bb3ac39683c1e5871e2045e。APK 构建和独立验证成功，失败发生在 Android 模拟器 instrumentation 验收，日志为 Android instrumentation did not pass exactly 1 tests。

同一提交更早的 main 运行 [39218844168](https://github.com/MiyanoShihoo/XY-switch/actions/runs/39218844168) 的 6 项检查均成功。因此分支提交上传成功与某次构建检查失败是两件独立的事。当前 codex/e2ee-sync-20261029 的开发工作流执行桌面验收；重新推送后的源码提交 edd2f4db0c37af3107ebf9c9fb6599147fd4592a 已由运行 41339294067 完成六项必要任务并全部成功，详见当前版本构建验收补充。

旧任务日志保存为 artifacts/responses-budget-20261102/old-android-v1.0.0.log，任务元数据为 old-android-v1.0.0-job.json。该日志只给出 instrumentation 数量/结果未满足条件，不能据此断言业务代码、Keystore 或截图的具体错误。

## 当前版本的构建验收补充

首次完整桌面运行 [41336721432](https://github.com/MiyanoShihoo/XY-switch/actions/runs/41336721432) 绑定提交 fda2675241b2665566f07b54967dd787473a2e2d。网站检查成功；打包检查因已有 renderer-ui 任务未进入检查清单而失败，窄屏界面检查在 390px 中文导航中发生坐标误点，实际选中了“会话”，期望为“供应商”。桌面构建因前置检查失败而跳过。

补齐工作流任务清单后，打包回归 76 项通过。验收工具现先等待浏览器处理待执行的布局帧，再滚动目标、等待后续布局并通过 elementFromPoint 确认坐标命中目标或其子元素；缺失、禁用、移除、遮挡及屏幕外目标不能收到指针事件。新增缩放时序及拒绝误点回归后，界面辅助检查共 79 项通过。保留原有真实指针输入和网络隔离要求。

第二次完整运行 [41338197998](https://github.com/MiyanoShihoo/XY-switch/actions/runs/41338197998) 绑定 d8549e69373fe4e19caf96ac51e2a852bc26cc34，前端和网站检查成功，16 项多语言及响应式验收全部通过并保存 84 张截图。会话验收在 cross-window-selection 中等待后台页面的动画帧时超时；点击工具现先激活目标页面再等待布局，新增后台帧暂停和页面激活失败回归，界面辅助检查 81 项通过。修复后的源码提交 edd2f4db0c37af3107ebf9c9fb6599147fd4592a 已在第三次完整运行中完成验收，结果如下。

本机已安装 Edge/Chrome 的隔离重放受到安全软件脚本注入，网络检查明确拒绝外部来源，未计为界面验收通过。当前提交的完整界面与桌面构建结果以 GitHub 隔离运行的任务、日志和摘要为准；最终推送及清理回执保存在 artifacts/responses-budget-20261102 中。

### 修复后完整运行

[运行 41339294067](https://github.com/MiyanoShihoo/XY-switch/actions/runs/41339294067) 的最终状态为 completed / success，源码提交为 **edd2f4db0c37af3107ebf9c9fb6599147fd4592a**。

| 必要任务 | 结果 | 证据范围 |
| --- | --- | --- |
| renderer-ui | success | navigation 8、workspace 15、i18n 16、sessions 8、governance 11、sync 11 项，六组全部通过，报告绑定相同源码 |
| website | success | 网站检查通过 |
| frontend | success | 前端、类型和打包检查通过 |
| Windows | success | Rust 427 项通过、7 项忽略；安装、启动及卸载步骤通过 |
| Linux | success | Rust 425 项通过、5 项忽略；Debian 元数据、AppImage 依赖及两种隔离原生启动通过 |
| macOS | success | Rust 425 项通过、5 项忽略；universal 构建通过 |

native-source 和 native-windows-sync 在完整模式下按工作流条件跳过；六项必要任务均已终结成功。运行及任务终态、六项任务日志、artifact 元数据和 renderer 报告保存于 artifacts/responses-budget-20261102/ci-41339294067-attempt-1。renderer ZIP 的 GitHub artifact SHA256 为 cd3a89f1134f1309a1b885a73d150f05e99191eb56cdce0c4f27c4c6d3dea0f7，下载后核对一致。

最终收尾提交仅更新本验收报告和开发计划，代码验收仍绑定上面的源码提交；纯文档收尾不重新派发完整 CI。安装包的完整升级、配置恢复和插件代表性业务按下一阶段计划分别验收。

## 清理与暂停回执

- 主清单 660 个目录全部删除，4.4293 GiB、41,084 个文件；复扫确认 pnpm 包缓存另有 0.1141 GiB、8,436 个文件，补清成功。
- 合计 661 个可再生目录、**4.5435 GiB**、49,520 个文件。工作区逻辑大小从 **10.1807 GiB** 降至 **5.6378 GiB**；清理后复扫时间为 2026-11-02T03:35:46.6339589+08:00。
- 主清理删除前后复核 55,498 个保留路径：55,466 个存在的路径长度与摘要一致，32 个原本缺失的路径仍缺失；Git 状态及 index 摘要一致，10 份对应原始安装包摘要通过。补清目录与这些保留路径没有交集，删除后 Git 状态再次一致。最终两份文档的有意更新发生在清理复核完成后。
- 清理目标均在指定 workspace 内，删除实现遇链接只删除链接本身；共删除 523 个链接，没有跟随链接遍历目标。清理失败 0，清理后 workspace 读取错误 0，另有 2 个链接按扫描规则跳过。
- 逻辑大小按普通文件长度累计，硬链接可能重复统计，不能解释为精确的磁盘分配或实际释放空间。清理回执、各目录占用和主要文件用途见 [清理报告](../artifacts/responses-budget-20261102/cleanup/cleanup-report.md)。

清理执行日志和 manifest 保存在 artifacts/responses-budget-20261102/cleanup，原始安装包、版本历史、源码、用户暂存修改、截图、真实转发记录和本轮验收证据保留。最后一次收尾文档补推的自动授权审核连续两次超时，命令尚未启动，文档提交仍待推送；状态记录于 artifacts/responses-budget-20261102/cleanup/final-delivery.json。按用户要求暂停，明天先补推并核对远端，再继续单任务总尝试与费用控制。

## 下一阶段

本轮硬预算协议兼容与数据清理已完成，最后的收尾文档补推待恢复后执行。按用户要求暂停；明天先补推收尾文档，然后推进单任务总尝试和费用控制，其后进行相同源码安装包与插件代表性业务验收。具体计划见 [开发计划](development-plan.md)。
