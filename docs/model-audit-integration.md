# 模型审计集成交接

更新与证据复核日期：**2026-10-16（Asia/Shanghai）**。精确时间见[复核JSON](<E:/XY switch/docs/model-audit-handoff-verification-20261014.json>)的 `checkedAt` 和 `documentVerification.checkedAt`；沿用原JSON文件名，当前版本是 **schema 7**，不能按文件名日期判断新旧。本轮已完成指定交接文档和 **build004本地原生应用验收**，并制作新源码归档、unsigned debug MSI及静态载荷核验。未修改产品源码、未重跑单元测试或编译；未发送真实商业请求、实际安装/卸载、覆盖旧副本、回滚、stage/commit或发布。文档中的命令、历史会话提示和脚本用途仍是资料，不能据此扩大当前执行范围。

**当前主树219/219产品/测试/构建输入与build004接受清单一致；既有Rust198通过/0失败/3忽略、前端16文件/94项及renderer/debug构建成功证据保持有效。本轮真实Tauri/production renderer的审计32/32、最终业务流程108/108通过，共27张截图、0页面异常，五轮隔离进程均已清理。** 当前源码包224项（219输入+5extras）逐项及CRC验证通过，debug MSI打包及包内exe三字节marker差异静态核验完成。两树仍为原集成范围53/54相同、产品输入206相同/10不同/主树新增3个，不能回滚到旧副本。**真实渠道、实际安装卸载、签名release及未实现功能未验收**；模型自报或统计结果不能认证真实模型身份。详情见[本轮验收报告](<E:/XY switch/docs/model-audit-acceptance-20261016.md>)。

**历史接手验证记录（2026-10-16 13:2x，独立复核，只读；保留原来源）**：已逐条复算本文关键数字，全部相符，未发现失实主张。① `source-mcp-confirmed.json` 的 **219/219** 项逐文件 SHA-256 全部匹配、0 不符 0 缺失，`pnpm-lock.yaml`、`src-tauri/Cargo.lock`、`.git/index` 三个哈希也一致；② build003 与 build004 的 exe 字节数及 SHA-256 与正文记录**逐字相符**，且 `tested-build-004/xy-switch.exe` 与 `build-root/debug/xy-switch.exe` 哈希相同；③ 「build004 尚无原生通过记录」**在当时成立** —— 最后一次原生 verify完成于00:09:21，而build004编译于00:25:33开始、00:26:07完成；截至该次只读复核其后仅有构建产物与绑定脚本。该结论现已被第5.5节13:55之后的本轮原生证据更新；④ 「53/54 相同」与「219 输入 / 副本 216 / 206 相同 / 10 不同 / 新增 3」两组口径**均已按同 scope 实算复现**，差异项与正文列举完全一致；⑤ 测试集合核对：主树 `src-tauri` 下 **175 个 `#[test]` + 26 个 `#[tokio::test]` = 201**，与运行日志的 `running 201 tests`／`198 passed + 3 ignored` **精确闭合**；副本为 189，差值 12 个属性对应正文“新增 10 项”（同一函数上的多属性会各计一次）。前端主树 16 个测试文件、副本 14，与“16 文件”相符；⑥ 32 项原生审计经 `run-005` 绑定 **build003**，`run-006` 的 MCP 失败同样绑定 build003，与正文一致；⑦ run-001 至 run-006 均有 `completed=true` 清理记录，run-006 清理另行核对了路径、PID 与 exe 哈希。唯一未能在本沙盒复核的是 10 月 14 日遗留 **PID 28592** 的存活状态（Windows 进程在 Linux 沙盒不可见），正文已如实登记为未核实。

**2026-10-16续接维护状态：已批准并部分执行，整体清理尚未完成。** 已按用户指定读取上一会话本机JSONL，来源与哈希见[会话读取来源](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/continuation-20261016/session-read-summary.json>)；[批准前四次未执行状态](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/continuation-20261016/cleanup-status.json>)仍保留其当时实际释放0字节的历史事实。本次用户明确批准后，21:53:42–21:55:29按原白名单删除 **24/35项目标、17,130个文件**；E:空闲空间实测由185,645,019,136增加到201,744,273,408字节，增量 **16,099,254,272字节（14.994 GiB）**。删除文件逻辑大小为21,522,055,050字节，不将原计划36.012 GiB或硬链接重复计数当成物理释放量；实测磁盘增量也可能包含同期E:活动。**5,915个受保护文件清理前后SHA-256及字节数一致，其中release文件5,038个**；源码、锁文件、Git index、debug根EXE/DLL、接受的exe/MSI、源码ZIP和全部原生证据保留。另 **11项目标、12,356个文件、17,145,855,485逻辑字节（15.968 GiB）**仍在src-tauri/target/debug，因沙盒访问权限未删除；沙盒外审核两次在进程启动前超时，没有改ACL/所有权或改用其他工具绕过。[批准后状态汇总](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/approved-execution-20261016/maintenance-status.json>)、[实际删除结果（completed=false）](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/cleanup-result.json>)、[保护文件清理前后记录](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup>)和[沙盒外审核超时记录](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/approved-execution-20261016/resume-permission-review-timeouts.json>)分别记账。原脚本首次因JSON时间字段类型转换而在删除前误报清单差异；仅修正ISO时间字符串读取后重新通过原路径/计数/字节/mtime保护，不扩大范围；失败与修正证据都保留。[部分清理后只读复核](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/documentation-check-only-post-cleanup-partial-approved.result.json>)为exit0、869个存量文件零变更；这只能证明**已执行部分的清理后复核通过**，不是全部清理完成。

接手优先事项：

1. 本地build004验收已闭合。移交时核对219输入、exe/source/renderer绑定与本轮32+108项原始结果，携带源码ZIP、debug MSI、正文/JSON和完整证据，不能拿build003结果代替。
2. 单元测试与编译复用相同输入的既有通过证据，不因文档更新或缓存清理重复安装、编译；源码/依赖/工具链变化时按6.3节新建运行目录重新取证。
3. 后续重点是隔离Windows环境实际安装/卸载/升级、签名release和明确授权的真实供应商观测。保留当前主树增量、旧副本、所有失败日志与备份；缓存维护单独记录，不能用旧脚本直接回滚工作树。

## 1. 入口、完成状态与来源

| 对象 | 位置与当前状态 | 接手用途 |
| --- | --- | --- |
| 主工作区 | `E:\XY switch`；219个接受输入一致，既有单元测试/编译通过，本轮原生32+108项通过 | 当前已验收源码与debug对象；保留后续13项差异 |
| 审计实现副本 | [modified](<E:/XY switch/artifacts/model-audit-20261009/modified>)；保留 10 月 14 日审计实现与成功日志，不含主树后续修复 | 审计算法/样式及历史回归参考，不能全量覆盖主树 |
| 历史主树单测/编译证据 | [application-acceptance-20261015](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015>)、[source-mcp-confirmed.json](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/source-mcp-confirmed.json>)、[build-binding-004.json](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/native-desktop/build-binding-004.json>) | 目录名是启动日期；最终 Rust/构建于 10 月 16 日凌晨完成，清单与绑定于 10:32 保存 |
| 副本历史证据 | [handoff-execution-20261014](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014>) | Rust 188、前端 77、浏览器 UI 87 项通过；该次原生启动未完成 |
| 当前验收与文档复核 | [复核JSON](<E:/XY switch/docs/model-audit-handoff-verification-20261014.json>)、[验收报告](<E:/XY switch/docs/model-audit-acceptance-20261016.md>)、[application-acceptance-20261016](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016>) | schema7；build004原生通过、debug包静态核验、源码归档和真实未验边界分别登记 |
| 使用说明 | [实现说明](<E:/XY switch/docs/model-audit-implementation.md>)、[副本实现说明](<E:/XY switch/artifacts/model-audit-20261009/modified/docs/model-audit-implementation.md>)、[副本历史交接](<E:/XY switch/artifacts/model-audit-20261009/modified/docs/model-audit-integration.md>) | 用法、分词器和指纹格式；旧测试数量及“主树保持原样”不是当前状态 |
| 设计与集成历史 | [20261012 设计快照](<E:/XY switch/docs/model-audit-design-20261012.md>)、[初次集成记录](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/integration-execution.json>)、[续接完成记录](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/integration-completion-20261014.json>)、[MAIN_BEFORE.zip](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/integration-backup/MAIN_BEFORE.zip>) | 保留需求与当时 54 项同步事实；223 项备份不是运行数据库或当前新增修复备份 |

2026-10-16应用验收轮未调用历史会话读取工具；下列更早来源信息当时读取自上一版正文和存量摘要，不声称该轮重读了这些会话。上一轮于 2026-10-15 读取[续修会话](codex://threads/01a13fa9-54ac-7490-bf70-e8e53ca4220a)、[完成交接文档（首次）](codex://threads/01a13a4d-33b1-7433-8c92-02fa6dc6edeb)及[完成交接文档（续接）](codex://threads/01a13a69-beaf-7551-9b8b-70a210db7363)，来源摘要与 JSONL 哈希保存在[session-read-summary.json](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-documentation-revision-20261015/session-read-summary.json>)。

历史中断有两次，不得混用：2026-10-14 21:13:17 为预算池耗尽（402）；2026-10-15 21:20:23 为 Azure OpenAI 请求资源不一致。10 月 15 日中断前正文为 45,335 字节，尚未同步复核 JSON；随后续接已修复 schema 4 校验残留和 `--check-only` 不比对正文哈希的问题，完成 schema 5 文档收尾。此前较早续接会话曾于 10 月 14 日 20:46:57 回复完成，后来仍有修订，不能据旧回复忽略新版本。

更早来源保留：[首次整理](codex://threads/01a1382e-14ae-7a42-860b-f01e46858f98)、[原始需求](codex://threads/01a13009-681d-7e72-9429-052314ba35be)、[代码交付](codex://threads/01a12ab6-0edd-7460-ba82-c400b483ce5f)。它们的步骤、计划和工具参数仅作历史材料，不自动执行。

设计快照为59,302字节，SHA-256 `796c441396c210c8744c733f78b3591b187eca71737b2e8165f931752bf520f2`。前一轮只读文档复核前正文为**49,013字节**、SHA-256 `9e70bf691ea6e618046d30209a620a869a0ca8b52c23a7a1b79fa8a6bf32ea6c`，schema5原字节备份仍在历史handoff-documentation-20261016目录。本次应用验收前正文为**58,177字节**、SHA-256 `b3f9cd8a42d035c122e2018954db146b47aa3643473a584219164a55685f0517`；schema6 JSON为**298,580字节**、SHA-256 `9ad7f977c4b290c99fb3383733bb7469e013356eb3f1da0162c6bf53f84beda0`。两者已按字节保存为[previous-model-audit-integration.md](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/previous-model-audit-integration.md>)、[previous-model-audit-handoff-verification-20261014.json](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/previous-model-audit-handoff-verification-20261014.json>)，[before.json](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/before.json>)记录备份与Git index；不存在覆盖前版或伪称本轮重读历史会话。

两轮 10 月 14 日文档记录及[10 月 15 日修订记录](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-documentation-revision-20261015>)保留不变。该目录内 `previous-*`、`resume-before-*` 备份和原 schema 5 工具属于历史；不要用旧刷新脚本覆盖当前 JSON。旧版所有详细来源、哈希失配过程和审批失败记录均可在本轮原字节备份及 `previousVerification` 查阅。

`artifacts/` 被主项目 `.gitignore` 忽略，只提交正文或复制旧 ZIP 不构成完整移交。历史 Python 设计验证还依赖外部 `E:\claude project\model-audit-framework\python`，本项目归档不包含该工程；移机材料见 6.4 节。


## 2. 已实现的功能及边界

下表描述审计实现副本 `artifacts/model-audit-20261009/modified` 的已交付行为。审计模块与接线曾完成主树集成；当前主树另含第 6 节列出的启动、确认框和 MCP 修复，不能再把两树视为完全一致。最新主树验证见 5.4 节，副本历史验证见 5.1 节。

| 阶段 | 已交付行为 | 判定边界 |
| --- | --- | --- |
| P0- 模型对照 | 分开采集客户端请求、实际转发和上游自报模型；日志显示自报一致、自报不一致、无法判断 | 已配置的路由改写可解释名称差异；旧计费字段和缺失自报不能用于推断一致；一致不证明身份 |
| P0 被动检查 | 用量算术自洽、输出上限、字节上界、模型名差异；流式与非流式共用审计记录 | 模型名差异单列警示，不计入三类硬证据；缺字段、未完整输出或无法测量时保留缺口 |
| P1 流式统计 | 文本帧数、UTF-8 字节、字符分布、最长数字/汉字/空白串、帧长中位数、首字时间、有界到达偏移、批处理识别及计费交叉比较 | SSE 帧不保证对应生成 token；重切帧、缓冲和混合文本会影响结论；帧/token 矛盾只作条件性统计线索 |
| P1.5a 聚合比较 | 用户主动分析当前筛选范围，按渠道和日期分组；相近语言成分与长度才比较 | 每组至少 20 条可比记录；相近不证明共用上游，漂移不证明换模型；高并发专门分层尚未实现 |
| P1.5b 影子差分 | 默认关闭；按渠道与实际转发模型配置本地分词器；原请求连续性、转发文本、真实回复和分区检查；后台 BPE、数值落库、回归与界面 | 仅接受合格的新业务配对；缓存、工具、推理输出、多模态、失败、历史改写、配对歧义等排除；斜率偏离不能独立证明虚报 |
| P2 本地参照 | byte-BPE、支持的预分词规则、指纹生成/导入/导出、相关证据族软封顶、包含库外项的候选排名 | 仅定位词表等价类；需要独立且同语料的观测指纹；不支持的 tokenizer 结构明确拒绝；未实现自动主动探针 |

日志第三列已改为“模型自报与审计”，在同一单元格中展示请求、路由后和上游自报。**日志仍为 9 列，空状态 `colSpan={9}` 正确**；设计中改成 10 只适用于另加一整列的方案。用量列表读取已有 `get_usage_logs` 通路，离线分析、参照和影子配置另有 Tauri 命令。

审计详情允许证据与缺口共存，当前页计数与整个筛选范围计数分别展示。统计分析由按钮触发，自动刷新不会自动执行。现有交付显示计数和详情，尚未提供设计示例中的实时弹窗或系统通知。

字节上界实现使用 `outputTokens - reasoningTokens > visibleOutputBytes + toolPayloadBytes + 4`，记录推理扣除、工具载荷字节及特殊 token 余量。缺少推理数、载荷不完整、非文本或中断响应等情形跳过该检查。它依赖完整计量及对应 token/字节语义，不能推广为对任意供应商计费方式的无条件证明。`tokensPerByte` 使用 UTF-8 字节数，不能用字符数替代。

## 3. 影子差分的当前规则

### 已修复内容

10 月 11 日交付修复了缓存别名冲突、隐藏推理内容和 Gemini 工具提示计费的资格过滤，并把增量多样性要求统一为 5 种。随后移除了回归算法固定等待 20 对样本、以及固定 RMSE 上限永久阻止判读的问题，采用 HC3 残差方差和与小样本自由度相符的置信区间。

当前规则见 [shadow.rs](<E:/XY switch/artifacts/model-audit-20261009/modified/src-tauri/src/model_audit/shadow.rs>) 的 `regress_shadow`：

- 3 对是代数计算下限；正式判读还需至少 5 种不同的本地 token 增量、至少 32 token 的增量跨度，因此 3 对本身不构成证据充分。
- `x` 是上一轮实际 assistant 回复与本轮新增 user 文本分别本地计数后求和；`y` 是相邻两轮上游输入 token 数之差。角色模板开销由截距吸收。
- 使用 95% t 区间，通常取 `n-2` 自由度，超过 18 自由度保守沿用 t(18)。区间整体高于 1.05 或低于 0.95 为 `suspicious`；整体位于 `[0.95, 1.05]` 为 `consistent`；其余为 `untested`。截距时间漂移超出容差时留下缺口并保持 `untested`，不会仅按斜率区间判定。
- 高噪声通过更宽区间体现不确定性，不再由固定残差值直接拒绝全部样本。区间仍依赖样本代表性、独立性等假设，模板或上游输入处理变化尚不能完全排除。

**后台旧“至少 20 对”说明已清理，10 月 14 日的 Rust notes 断言、空/非空真实报告和浏览器回放均已通过，见第 5、8 节。** 后续主树 build003 已通过实际 Tauri IPC 的空态、设置、合成审计记录和报告检查；当前 build004 的原生复验仍待执行，不能沿用旧 exe 的通过结论。跨渠道/跨日期比较的 20 条门槛是另一项功能的有效规则，仍然保留，不能一起删除。

[20 案例同输入结果](<E:/XY switch/artifacts/model-audit-20261009/logs/adaptive-sampling-20261011/completion-summary.json>)体现了以下变化；回滚结果逐项恢复基线：

| 输入 | 自适应修复前 | 当前交付 |
| --- | --- | --- |
| 无噪声、5/6/12 对的诚实或 1.2 倍样本 | 固定 20 对门槛，均未测到 | 诚实 `consistent`，倍率 `suspicious` |
| 有噪声、6/12 对倍率样本 | 未测到 | 区间不明确，仍 `untested` |
| 相同噪声扩展到 24 对倍率样本 | 固定残差上限阻止判读 | `suspicious`，斜率约 1.17857 |
| 有噪声、6/12/24 对诚实样本 | 未测到 | 均未误报为偏离，保留 `untested` |
| 3 对、仅 4 种增量、无变化或无样本 | 未满足条件 | 仍保留数量、多样性及跨度保护 |

### 启用和采集

1. 使用实现副本的“用量统计”，展开“被动影子差分（默认关闭）”。
2. 选择现有渠道 ID、实际转发模型及支持的本地分词器绝对路径；有路由改写时使用改写后的模型。支持 `.tiktoken` 和受限 byte-level BPE `tokenizer.json`；无法从 `.tiktoken` 文件名识别预分词规则时须显式选择。完整格式见[实现说明](<E:/XY switch/artifacts/model-audit-20261009/modified/docs/model-audit-implementation.md>)。
3. 保存并启用。配置不能加载时显示不可用；停用后停止新采样。该操作不发送探针。
4. 通过渠道产生新的多轮、纯文本、完整业务响应。历史日志缺失原文，无法补算。工具、多模态、显式推理输出、缓存命中或缓存状态未知等请求不参与。
5. 点击“分析已采集差分”，查看有效配对、排除原因、区间与缺口；证据不足时继续采集，不按固定请求总数宣布通过。

连续性快照在协议转换前生成，纳入顶层 system/instructions；转换后另核对实际发送文本。配对要求历史前缀精确匹配、消息数增加 2、上轮真实回复摘要匹配、基线未被重复消费。渠道、端点、协议、请求/转发/自报模型、分词器和请求上下文变化均隔离分区。缓存已报告的正值不能被后续零值覆盖。Responses 仅接受受限的 `input` 数组，字符串 `input` 暂不采集；服务端会话状态、自动截断等也排除。

请求正文片段和配对摘要只在有界内存暂存；审计持久化保存标签、数字、统计、证据和缺口，不新增对话正文、工具参数或配对内容哈希。设置保存在 `app_settings` 的 `shadow_audit_settings` 键，样本在 `shadow_audit_samples`。实现用内存摘要代替设计示意的持久化 `prompt_*` 三列，重启或过期后重新建立基线。

资源上限为队列 32、基线 256、有效期 30 分钟、请求 1 MiB/1024 条消息、新增文本 8 KiB、约 50,000 条持久化样本。一次分析读取最近至多 10,000 条、显示样本最多的 200 组。BPE 在专用后台线程运行；队列满时丢弃并计数，转发侧不等待分词。

## 4. 代码入口

以下链接指向审计实现副本；主树同名模块仍是审计接线入口，但 `database.rs` 另有 MCP 事务修复。按符号定位，避免把易变行号当接口；当前主树完整输入清单见 6.1 节。

| 工作 | 源码入口与接线 |
| --- | --- |
| 请求身份与采集 | [proxy.rs](<E:/XY switch/artifacts/model-audit-20261009/modified/src-tauri/src/proxy.rs>)：转换前 `begin_capture`、实际转发身份、非流式 collector、原生与转换流观察包装 |
| 原始响应观测 | [streaming.rs](<E:/XY switch/artifacts/model-audit-20261009/modified/src-tauri/src/streaming.rs>)：`audit_capture`、`publish_shadow`、`shadow_plain_complete`；包括原生透传流，不能只改旧流结束回填点 |
| 纯函数、统计与分词 | [invariant.rs](<E:/XY switch/artifacts/model-audit-20261009/modified/src-tauri/src/model_audit/invariant.rs>) 的 `Observation`/`audit`；[shadow.rs](<E:/XY switch/artifacts/model-audit-20261009/modified/src-tauri/src/model_audit/shadow.rs>) 的 `regress_shadow`；[tokenizer.rs](<E:/XY switch/artifacts/model-audit-20261009/modified/src-tauri/src/model_audit/tokenizer.rs>)、[reference.rs](<E:/XY switch/artifacts/model-audit-20261009/modified/src-tauri/src/model_audit/reference.rs>) |
| 配对及后台 | [shadow_audit_runtime.rs](<E:/XY switch/artifacts/model-audit-20261009/modified/src-tauri/src/shadow_audit_runtime.rs>)：有界队列、专用线程、两层快照、模型分区与 `try_send` |
| 数据库 | [database.rs](<E:/XY switch/artifacts/model-audit-20261009/modified/src-tauri/src/database.rs>)：`Database::open` 增量迁移、影子表、`update_request_usage`；请求、自报及完整 `audit_json` 分开持久化，转发模型在 observation 内 |
| 查询与命令 | [model_audit_commands.rs](<E:/XY switch/artifacts/model-audit-20261009/modified/src-tauri/src/model_audit_commands.rs>)、[shadow_audit_commands.rs](<E:/XY switch/artifacts/model-audit-20261009/modified/src-tauri/src/shadow_audit_commands.rs>)、[lib.rs](<E:/XY switch/artifacts/model-audit-20261009/modified/src-tauri/src/lib.rs>) |
| 日志与分析 UI | [UsageView.tsx](<E:/XY switch/artifacts/model-audit-20261009/modified/src/components/UsageView.tsx>)、[ModelAuditCell.tsx](<E:/XY switch/artifacts/model-audit-20261009/modified/src/components/ModelAuditCell.tsx>)、[ModelAuditPanel.tsx](<E:/XY switch/artifacts/model-audit-20261009/modified/src/components/ModelAuditPanel.tsx>)、[ShadowAuditPanel.tsx](<E:/XY switch/artifacts/model-audit-20261009/modified/src/components/ShadowAuditPanel.tsx>)、[model-audit.css](<E:/XY switch/artifacts/model-audit-20261009/modified/src/styles/model-audit.css>) |

七个新增命令为 `analyze_model_audit`、`generate_model_reference`、`load_model_reference`、`analyze_model_reference`、`get_shadow_audit_settings`、`save_shadow_audit_setting`、`analyze_shadow_audit`。已有日志继续走 `get_usage_logs`。`Cargo.toml` 新增本地依赖 `src-tauri/vendor/unicode-general-category`，移交和集成必须包含其完整 14 文件。

## 5. 验证证据与交付完整性

### 5.1 实现副本验证：2026-10-14

下列记录均验证完整副本，各命令工作目录以对应 `command.json` 为准；浏览器脚本从副本的父目录运行。后端于 19:57:22 完成，前端汇总于 20:03:40 完成；桌面构建于 20:01:34 完成，原生报告于 20:08:17 记录未完成。10 月 15 日复核已核对这批记录，本次再次校验其 stdout/stderr 哈希及副本 47 个后端输入；没有重新执行应用测试。当前主树结论见 5.4 节。

| Evidence：原始证据 | Finding：核实结果 | Path：覆盖与边界 |
| --- | --- | --- |
| [Rust 测试](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/backend/rust-tests.stdout.txt>)、[后端汇总](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/backend/completion.json>) | 188 通过、0 失败、3 忽略，退出码 0；汇总 47 个源码/配置哈希全部匹配当前副本 | 审计纯函数、数据库迁移、配对、代理/HTTP/SSE/SQLite；忽略项为既有凭据管理器及两个 Codex CLI 交互测试 |
| [cargo check](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/backend/cargo-check.stderr.txt>) | 退出码 0；3 个既有 warning：`ShadowOutcome` 未使用、`non_stream` 和 `observe_stream` 未使用 | 静态编译检查；不是主树检查 |
| [真实影子报告](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/backend/shadow-audit-response.json>) | 空/非空报告各 6 条 notes；阈值及无旧“至少 20 对”说明断言通过 | Rust `analyze_database` 直接结果；浏览器回放文件哈希与该结果一致 |
| [前端测试](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/frontend/frontend-tests.stdout.txt>)、[构建](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/frontend/frontend-build.stdout.txt>) | 14 文件、77 项通过，生产 renderer 构建通过，退出码均为 0 | 三态、缺口、旧字段不冒充自报、服务参数、类型与打包 |
| [前端汇总](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/frontend/completion.json>)、[原 UI 结果](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/frontend/existing-ui-final/ui-result.json>)、[扩展 UI 结果](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/frontend/extended-ui-final/ui-result.json>)、[视觉复核](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/frontend/visual-review.json>) | 32 + 55 = 87 项通过，0 个意外页面错误；错误边界的 8 次预期 console 事件单独登记；扩展检查留存 19 张截图 | 明暗与 system 跟随、空态/样本不足、灰度/琥珀/红/青/紫、1180/820 布局、状态对比度、错误页正常 token 与隔离 fallback、后端 notes 回放；`tauriIpcExercised=false` |
| [debug 构建结果](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/native-desktop/desktop-build.result.json>) | `cargo build --offline --features tauri/custom-protocol` 退出码 0；新 exe 哈希及独立 tested copy 一致 | 含生产前端资源的 debug 程序；未构建或发布新安装包 |
| [原生桌面报告](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/native-desktop/completion.json>) | `nativeAcceptanceComplete=false`、`nativeChecksPassed=0`、无原生截图 | 首次进程曾存在，但 CDP 连接拒绝、隔离数据库未创建、应用日志为空；产品原因未确定 |

前端测试和构建的源码清单各 101 文件，当前 100 个相同，仅 QA 入口 `scripts/audit-qa/main.tsx` 在其后调整；各次记录中的 82 个 `src/` 产品文件均一致。最终扩展 UI 的 97 个记录文件全部匹配当前副本，19 张截图及结果 JSON 的哈希也匹配。不能把 QA 文件调整写成产品源码漂移，也不能笼统声称所有较早清单逐字不变。

原生重试记录见 [approval-failures.json](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/native-desktop/approval-failures.json>)：自动审批服务分别返回 503 和 `content-blocked`，两次外部桌面重试均未执行；记录明确属于审批服务未完成，并非认定动作不安全。受限清理返回 Access denied，报告登记测试进程 PID 28592 尚未清理。指定续修会话在 **2026-10-15 21:13:55** 曾[只读查询历史 PID](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-documentation-revision-20261015/process-recheck.json>)，Win32_Process 查询返回“拒绝访问”；10 月 15 日续接及本次文档复核均未重复查询，进程存活及清理状态仍未核实。未启动或终止任何进程；接手时须先核对可执行文件路径，不能仅凭旧 PID 终止进程。恢复脚本只有语法检查记录，不能当作原生业务验收通过证据。

### 5.2 历史验证与失败记录

10 月 11 日 [adaptive-sampling 日志](<E:/XY switch/artifacts/model-audit-20261009/logs/adaptive-sampling-20261011>)保留 Rust 188/0/3、前端 14 文件/77 项、浏览器 32 项、Python 设计 51/51，以及 renderer/check/debug 构建通过结果。Python 51 项和基线/修改/回滚 20 案例仍只引用该日期；本次未发现 10 月 14 日重跑它们的结果。不能把旧 Python 工程的“573 项”记为本项目验收数量。

[同输入汇总](<E:/XY switch/artifacts/model-audit-20261009/logs/adaptive-sampling-20261011/completion-summary.json>)、[事务清单](<E:/XY switch/artifacts/model-audit-20261009/artifact-manifest.json>)、[VERIFICATION.txt](<E:/XY switch/artifacts/model-audit-20261009/VERIFICATION.txt>)记录自适应修复的补丁重建、回滚和重应用。中间曾有 187 通过/1 失败，修正跨度不足夹具后的最终专属日志为 188 通过，不能混用中间结果。

10 月 14 日前端最初导航超时和 reload 竞态也保留在 `frontend/attempts` 与旧尝试日志中；最终使用 30 秒导航等待及新页面时间点确认后通过。前次文档记载的另一环境缺 Rust、`pathe` 解析失败是更早失败经历，当前 Windows 的成功日志已提供后续结果。

### 5.3 历史归档与副本可执行文件

旧 `MODIFIED_FILE.zip` CRC 检查通过，内含 246 文件；对当前 `modified` 逐文件比较为 **240 相同、6 不同、0 缺失**。六个差异是五个产品/后端测试文件，以及 `scripts/audit-qa/main.tsx`。源码、脚本与文档扫描另发现两个未入旧 ZIP 的文件：`scripts/error-boundary-qa/index.html`、`scripts/error-boundary-qa/main.tsx`。完整文件哈希见[复核 JSON](<E:/XY switch/docs/model-audit-handoff-verification-20261014.json>)及保留的 `previousVerification`；旧 246/246、242/4、241/5 结论仅属于前次快照。

| 文件 | 作用 | SHA-256 |
| --- | --- | --- |
| [MODIFIED_FILE.zip](<E:/XY switch/artifacts/model-audit-20261009/MODIFIED_FILE.zip>) | 10 月 11 日完整源码，未含全部 10 月 14 日改动 | `27e9eb2bf34e153dc154d141e7b5ab8b061346f5145645b80082128818350b4a` |
| [DIFF_FILE.patch](<E:/XY switch/artifacts/model-audit-20261009/DIFF_FILE.patch>) | 自适应修复增量；不是主树全量迁移补丁 | `c0938f68584b8a8730bca4bb42fefa83307ea07b88f4e28fdb15cf8a663d6808` |
| [VERIFICATION.txt](<E:/XY switch/artifacts/model-audit-20261009/VERIFICATION.txt>) | 历史事务原始证据 | `f91384284710069e27c29aa5cf4870de674296b88e5f5a3d97f55b59b13a5ff8` |
| [ROLLBACK.sh](<E:/XY switch/artifacts/model-audit-20261009/ROLLBACK.sh>) | 将指定 ZIP 副本恢复为同目录 BASELINE | `8f0be8de868a1539e7636f8965a3e285e4b968e35ad0c5d38db3dccc3323f862` |
| [BASELINE.zip](<E:/XY switch/artifacts/model-audit-20261009/BASELINE.zip>) | 自适应回归修复前源码；已包含此前审计实现 | `4b217e33f35ac2075af192a43a2b1345fcec3284b59c7315f8791e933a51288c` |
| [10 月 14 日 tested exe](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/native-desktop/tested-build/xy-switch.exe>) | 10 月 14 日 debug，34,453,504 字节；与 `build/debug/xy-switch.exe` 一致 | `038ecf1bb35c57a6690f0480e1523e394b09918a9ff71846a5ef391da9b4198c` |
| [历史 exe 副本](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/native-desktop/historical-debug/xy-switch.exe>) | 10 月 11 日 debug，34,458,112 字节；旧构建已移存此处 | `32c1d310a67c51f4e7e79169cb0c9e14f0adab45bf0cb46cb2ef5005ab490efa` |

四角色文件均与 `role-hashes.json` 匹配，两个 ZIP 的 CRC 均通过。本次再次核对角色文件、两个 ZIP 的 CRC 和副本差异，归档未重建；这些副本归档不包含当前主树后续 13 项修复。`ROLLBACK.sh` 只复制和比较 ZIP，不恢复当前源码、运行数据库或已安装程序，也不撤销 10 月 14 日文案/样式/QA 改动。

### 5.4 当前主树独立验证：2026-10-15 至 2026-10-16

此前正文遗漏了[主树验收目录](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015>)中的依赖安装、修复、测试、构建及原生尝试。**本节为此前只读复核所恢复的历史记录**；本轮继续读取和重算存量材料，但没有重跑应用单测或编译。新的原生验收另列第5.5节。最终成功命令的 `cwd` 均为主工作区；`source-mcp-confirmed.json` 于 **2026-10-16 10:32:30** 保存，`build-binding-004.json` 于 **10:32:32** 将该清单、凌晨日志、renderer 资源和 exe 绑定。10:32 是绑定记录时间，不是重新执行测试/构建的时间。

| Evidence：原始证据 | Finding：核实结果（Asia/Shanghai） | Path：覆盖与限制 |
| --- | --- | --- |
| [按锁安装结果](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/frontend/install-retry1.result.json>)、[输出](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/frontend/install-retry1.stdout.txt>) | 2026-10-15 21:55:25 完成 `pnpm install --frozen-lockfile --store-dir …`，退出码 0；本次检查旧缺失的 11 个命令/包/安装元数据入口均存在 | 旧“主树依赖安装不完整”结论已过时；移机或依赖变化仍需重新按锁安装 |
| [Rust 最终结果](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/backend/cargo-test-retry3.result.json>)、[stdout](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/backend/cargo-test-retry3.stdout.txt>) | 2026-10-16 00:25:24 完成；**198 通过、0 失败、3 忽略**，退出码 0，使用 `--offline --locked` | 相对副本新增 10 项、未删除测试：启动隔离 3、确认框 ACL 1、MCP/数据库事务 6；忽略项仍是既有凭据管理器与两项 Codex CLI 交互测试 |
| [cargo check 最终结果](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/backend/cargo-check-retry3.result.json>)、[stderr](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/backend/cargo-check-retry3.stderr.txt>) | 2026-10-16 00:25:33 完成，退出码 0；仍是 `ShadowOutcome`、`non_stream`、`observe_stream` 三个既有 warning | 不是“无 warning”；这些是编译告警，不是测试失败 |
| [前端最终结果](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/frontend/frontend-test-retry5.result.json>)、[stdout](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/frontend/frontend-test-retry5.stdout.txt>)、[renderer 构建结果](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/frontend/renderer-build-retry3.result.json>) | 2026-10-16 00:02:55 测试完成，**16 文件/94 项通过**；00:03:03 renderer 构建完成；退出码均 0 | 副本 77 + 代理启动 5 + 确认框 12 = 94；MCP 后续只改后端，build004 复用已验证相同的前端输入与 dist 字节，不声称后续重跑前端 |
| [debug 最终构建](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/native-desktop/desktop-build-retry3.result.json>)、[build004 绑定](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/native-desktop/build-binding-004.json>) | 2026-10-16 00:26:07 完成 `cargo build --offline --locked --features tauri/custom-protocol`，退出码 0；主树 219 个输入、15 个结果/输出哈希及 renderer 资源绑定均匹配 | 当前 debug exe 已含生产 renderer；不是新安装包或真实供应商验收 |
| [build003 原生审计结果](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/native-desktop/run-005/native-result.json>)、[命令结果](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/native-desktop/native-verify-retry3.result.json>) | 2026-10-16 00:09:21 完成，`complete=true`，**32/32 通过**、11 张截图、0 页面异常、退出码 0 | 真实 Tauri IPC、production renderer、隔离 SQLite 中的合成审计/影子样本，覆盖启停、空态、缺口、过滤及三档主题；仅适用于 build003，不能转记 build004 原生通过 |
| [build003 业务流程失败](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/native-desktop/run-006/workflow-001/workflow-result.json>)、[命令结果](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/native-desktop/native-workflow-retry2.result.json>) | 2026-10-16 00:14:45 结束，`complete=false`，**41 条检查中 40 通过/1 失败**，退出码 1；MCP 创建返回所有客户端选择为 false | 原生 UI/IPC、隔离配置和回环合成上游，不是商用渠道；流程在 MCP 选择/同步处停止，后续阶段不能记通过 |
| [MCP 修复清单](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/repair-mcp-selection-manifest.json>)、[当前输入清单](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/source-mcp-confirmed.json>) | 后端修复后新增 6 项事务/选择回归测试，Rust/check/debug 全部通过；本次主树与接受清单 **219/219 一致** | 截至13:2x只读复核尚无build004原生结果；当时schema6的`currentBuildNativeAcceptanceComplete=false`，现已由第5.5节新证据更新为true |
| [run-005 清理](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/native-desktop/run-005/cleanup.json>)、[run-006 清理](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/native-desktop/run-006/cleanup.json>) | 分别于 2026-10-16 00:14:19、00:14:47 完成，记录先核实路径/PID 再停止；新一轮 run-001 至 run-006 均有 `completed=true` 清理记录 | 不覆盖 10 月 14 日遗留 PID 28592 的未核实状态；该次只读复核未查询或操作进程；本轮五次新运行清理另见第5.5节 |

当前 build004 exe 位于 [tested-build-004/xy-switch.exe](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/native-desktop/tested-build-004/xy-switch.exe>)，34,470,400 字节，SHA-256 `f10a4fe59d42a87cf7f73e894a229394d2e971e1a1d4ca7bf3845c5c17e64d7f`，与 `build-root/debug/xy-switch.exe` 一致。此前原生审计/流程所测 build003 为 34,455,040 字节，SHA-256 `ff4d7bbde36ca0112625cb7b3601990c1a77a5f5b141b27ae259f5a5fdac804d`，保留于 [tested-build-003](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/native-desktop/tested-build-003>)。两者不能互相替代。

早期原生失败也须保留：run-001 暴露 HOME/session 导入隔离和启动安全状态问题；run-002 的导航定位器失败后修复脚本；run-004/workflow-002 暴露 `window.confirm` 返回 Promise 被当作布尔值、未获得原生 dialog 权限而产生的删除确认缺陷。主树后续改用显式应用 `alertdialog`，不扩大 ACL；run-006 已确认提示词删除的取消、Escape、键盘焦点和显式确认，然后发现 MCP 选择问题。所有尝试、失效标记、清理和已有修复前备份均留在验收目录；“部分检查通过”不等于整个流程完成。


### 5.5 本轮build004原生验收、debug打包与源码归档：2026-10-16

[本轮验收目录](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016>)使用同一接受清单、exe和production renderer；产品源码没有变化。[完整验收报告](<E:/XY switch/docs/model-audit-acceptance-20261016.md>)保存Evidence→Finding→Path、时间/范围、失败尝试与未验收边界。

| Evidence：本轮原始证据 | Finding：结果（Asia/Shanghai） | Path：覆盖与限制 |
| --- | --- | --- |
| [run-001原生审计](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/run-001/native-result.json>)、[审计命令结果](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/native-audit.result.json>) | 13:55:30–13:55:45，**32/32通过**，complete=true、exit=0；11张截图，0 pageErrors | 真实Tauri IPC/production renderer、隔离SQLite合成审计/配对；适用于当前build004 |
| [run-005最终业务](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/run-005/workflow-001/workflow-result.json>)、[业务命令结果](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/native-workflow-retry3.result.json>) | 19:28:30–19:29:01，**108/108通过**，complete=true、exit=0；16张截图、0 pageErrors | 设置/导航、provider/prompt CRUD、显式确认/焦点/Escape/cancel；MCP四客户端选择/同步/编辑/toggle/删除、无效配置失败后DB与文件逐字节回滚、修复后恢复 |
| 同一最终业务中的HTTP/SSE、Credential Manager、数据库核对 | 回环代理13请求/12成功/1合成HTTP400；成功SSE complete+3deltas；Responses→Chat桥接；6shadow=1baseline+5pairs，5不同增量，slope1/intercept0/xSpan80、consistent；假key已移除 | 全部为隔离路径、回环合成上游；数据库正文核对仅限随机provider的audit/shadow字段，不能扩大为所有存储；不认证真实商业模型 |
| [最终harness清单](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/harness-retry3-manifest.json>)及本轮run-002/003/004 | 75/76、94/95、100 checks true但complete=false的三次失败完整保留；修正路由夹具、SSE日志选择、provider控件定位后run-005闭合 | 无产品源码修改，不把脚本失误或局部通过改写为产品/整轮通过 |
| 本轮run-001至run-005 cleanup.json | 五轮completed=true，路径/PID/创建时间归属核对后停止程序及WebView2后代；run-005于19:29:05清理完成，workflow资源清理5/5 | 仅本轮验收对象；历史PID28592状态不因此被重新确认 |
| [MSI打包结果](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/installer/retry1/bundle.result.json>)、[静态载荷核验](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/installer/retry1/static-extraction/payload-verification.json>) | 19:44:58 debug MSI打包完成，exit=0，11,419,648字节；静态解包验证产品1.0.0/x64；包内exe仅3字节UNK→MSI marker与被测exe不同；4项WiX warning保留 | **unsigned debug**；没有包内runtime、实际安装/卸载/升级、签名release或custom actions执行，不算安装发布验收 |
| [当前源码ZIP](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/source-delivery/XY-Switch-build004-source-20261016.zip>)、[归档清单](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/source-delivery/source-archive-manifest.json>) | 5,938,894字节；**224文件=219输入+5extras**，219/219接受输入与全部224项主树字节、SHA-256、CRC一致 | 源码包不含文档/最终JSON、MSI/exe、日志、真实配置或缓存；这些为独立随附材料，避免循环hash |

schema7的 `currentBuildNativeAcceptanceComplete=true` 只依据本轮32+108真实原生检查。`realSupplierAcceptanceComplete=false`、`newInstallerAcceptanceComplete=false` 明确保持真实商业/安装未验；debug打包静态核验和新源码归档单列为完成。Session/Skills只验证空查询和未实现接口的拒绝，不算完整业务交付。旧build003通过/失败记录保持第5.4节原值。

## 6. 主树状态、未完成项与复验

### 6.1 历史集成已完成，当前主树已分化

10 月 14 日[初次执行记录](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/integration-execution.json>)开始于 20:01:12，因中断保留 `completed=false`；[续接完成记录](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/integration-completion-20261014.json>)开始于 20:26:20，记录 27 个新建、8 个覆盖、19 个已一致、0 失败及 `completed=true`，未记录结束时间。10 月 15 日文档复核为 54/54 一致。初次记录不修改，续接与当时哈希证明那轮同步完成；**当前有后续修复，不等于那轮集成未完成，也不应重新复制旧文件。**

本次[产品/构建输入清单](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-documentation-20261016/build-input-comparison.json>)使用与接受源码清单相同的显式范围：前端源码/测试/public、Rust 源码/迁移/vendor/图标/能力及两端配置、锁文件；不包含 `node_modules`、生成文件、工具链或运行数据库。核实结果：

- 原 54 项复制清单：**53 相同、1 不同、0 缺失**；唯一差异为主树 `src-tauri/src/database.rs` 的 MCP 状态事务方法和测试。
- 主树 **219** 个输入、副本 **216** 个输入：**206 相同、10 不同、主树新增 3 个、副本独有 0 个**；不是两树相同。
- 主树与[build004 接受清单](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/source-mcp-confirmed.json>)：**219/219 一致、0 差异、0 新增/缺失**；额外修复均有清单和测试/构建记录。
- 两个锁文件以及 Git index 均与接受清单及本轮修改前记录一致。当前 `master` 仍无 HEAD 提交，已有暂存/工作区变化；本次未 stage、commit 或改变 index。

| 主树后续增量 | 与副本不同的既有文件及新增文件 | 证据与影响 |
| --- | --- | --- |
| 启动安全与 session 路径 | 修改 `src/App.tsx`、`src/lib/proxy-service.ts`、`src-tauri/src/session_usage_codex.rs`；新增 `src/proxy-service.test.ts` | [启动修复清单](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/repair-manifest.json>)：遵从后端 disabled 状态，不强制启用代理；debug HOME 隔离不回退扫描真实 Codex 会话；新增 Rust 3、前端 5 项 |
| 显式破坏性操作确认 | 修改 `McpView.tsx`、`PromptsView.tsx`、`SessionsView.tsx`、`SkillsView.tsx`、`acl_regression_tests.rs`；新增 `src/components/ConfirmActionDialog.tsx`、`src/ConfirmActionDialog.test.tsx` | [确认修复清单](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/repair-confirmation-manifest.json>)：替代 Promise 值 `window.confirm`，默认焦点取消、支持 Escape 与焦点恢复，不扩大 dialog ACL；新增前端 12、Rust 1 项 |
| MCP 客户端选择与事务 | 修改 `src-tauri/src/mcp.rs`、`src-tauri/src/database.rs` | [MCP 修复清单](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/repair-mcp-selection-manifest.json>)：显式选择覆盖旧状态、未提供的键保留旧状态；写前检查已安装客户端，配置失败恢复，数据库选项一次事务提交；新增 6 项 Rust 回归测试 |

这些主树增量并未改变审计回归阈值或审计数据库迁移，但改变了应用启动、删除确认和 MCP 保存/同步行为。审计模块、vendor 完整 14 文件、主题 token 与错误边界仍保留集成版本；不能因为审计模块大多相同，就忽略 `database.rs` 的合法后续变化。主树与副本如何长期收敛需另行决定，本轮未做源码复制。

集成前[MAIN_BEFORE.zip](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/integration-backup/MAIN_BEFORE.zip>) SHA-256 为 `79353b04c2dc17998b5e4c8d2fa2241ddc90daec5f4fe7c367ab3a94f0a34157`，本次 CRC 及原记录 **223** 项内容哈希/大小均匹配。它不包含运行数据库、`.git` 或 index，也不是后续 13 项修复的完整回退包。另携带主树验收目录中已有的 `repair-before/`、`repair-confirmation-before/` 及修复清单；哈希只证明字节，恢复仍需原内容。新增文件也要单独处理，不能把解压覆盖当完整回退。

### 6.2 工作项与完成条件

| 项目 | 当前状态 | 完成条件或下一步 |
| --- | --- | --- |
| 交接文档与复核元数据 | schema7；原字节备份、正文/JSON/raw evidence一致性校验；[文档工具67项回归通过](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/documentation-tests.result.json>) | 文档及本地应用交接完成，不代表商业渠道或发布全部验收；携带6.4材料 |
| 主树文件集成及后续修复 | 历史同步完成；10个不同文件+3个新增文件有来源；219/219与build004输入一致 | 保留主树，不用旧副本覆盖；长期源码树收敛为单独版本管理问题 |
| 主树依赖、单元测试与编译 | 既有Rust198/0/3、前端16文件/94项、renderer/debug构建通过；3个check warning | 本轮未重跑；源码/依赖/工具链变化才在新目录复验 |
| 当前原生审计UI与IPC | build004本轮 **32/32、11截图、0页面异常**通过 | 本地合成审计范围完成；不拿build003历史结果代替 |
| 当前原生业务流程 | build004本轮 **108/108、16截图、0页面异常**通过，MCP四客户端、事务恢复和回环proxy/SSE/影子闭合 | 当前本地业务范围完成；Session/Skills未实现接口只验拒绝，真实上游另验 |
| 本轮进程与资源 | 五轮scoped cleanup完成；proxy/mock/假凭据恢复清理通过 | 保留cleanup原记录；历史PID28592未核实状态未改变，不能仅凭旧PID操作 |
| 当前源码归档 | 新ZIP **224项**，219/219输入及全体内容/CRC一致 | 归档完成；文档/JSON、二进制与日志作为sidecar，旧ZIP不作为当前包 |
| debug MSI构建与载荷 | 打包exit0、静态产品/架构/载荷核验通过；3字节marker差异、4WiX warning明确登记 | 此项完成；真实安装/卸载/升级、安装后runtime与签名release仍未验收 |
| 真实渠道与独立指纹 | 无真实供应商验收或官方直连观测 | 按协议记录合格/排除样本及模板/缓存混淆因素；统计或自报不能证明真实身份 |
| 后续设计与未实现功能 | 主动探针、高并发分层、实时通知、Session/Skills完整业务等未交付 | 独立定义实现与验收，不并入当前通过结论 |
| 调试缓存维护 | 已批准；24/35项目标已删、实测空闲增量14.994 GiB；原11项因权限和审核超时待处理；[批准后状态汇总](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/approved-execution-20261016/maintenance-status.json>) | 5,915项保护哈希未变，部分清理后check-only通过；保留所有证据，不把completed=false写成成功 |

### 6.3 复验入口

以下安装、单元测试和编译命令为**输入变化后的复验说明，本轮未重跑**；原生验收已在第5.5节实际完成。主树既有通过结果无需因文档更新或缓存清理重复运行。使用 Windows/MSVC 环境；[主树环境记录](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/environment.json>)包含 Node 24.15.0、pnpm 11.25.0、Rust/cargo 1.98.1 等实际版本。它是 10 月 15 日的工具记录，本次未重跑版本命令；移机按实际安装路径及项目锁文件准备。

两树均使用 `pnpm-lock.yaml`；缺依赖时在要验证的树内按锁安装，不直接复制带绝对链接的 `node_modules`，也不混用包管理器生成新锁。Vite 7.3.6 的既有包元数据要求 Node `^20.19.0 || >=22.12.0`。`--offline` 仍依赖完整 Cargo 缓存，14 文件 vendor 只覆盖一个本地包，不是全部依赖分发。每次使用新日志目录，保存工作目录、命令、版本、环境覆盖、源码哈希、stdout/stderr 和退出码，勿覆盖既有成功或失败证据。

主树在输入发生变化后复验（缺依赖时才执行首行安装；当前依赖已补齐）：

```powershell
Set-Location -LiteralPath 'E:\XY switch'
pnpm.cmd install --frozen-lockfile
if ($LASTEXITCODE -ne 0) { throw 'Root dependency preparation failed.' }
$env:CARGO_TARGET_DIR = 'E:\XY switch\artifacts\model-audit-20261009\build-root'
cargo check --manifest-path src-tauri/Cargo.toml --offline --locked
if ($LASTEXITCODE -ne 0) { throw 'Root cargo check failed.' }
cargo test --manifest-path src-tauri/Cargo.toml --lib --offline --locked
if ($LASTEXITCODE -ne 0) { throw 'Root Rust tests failed.' }
npm.cmd test
if ($LASTEXITCODE -ne 0) { throw 'Root frontend tests failed.' }
npm.cmd run build:renderer
if ($LASTEXITCODE -ne 0) { throw 'Root renderer build failed.' }
```

当前主树基线为 Rust 198/0/3、前端 16 文件/94 项、3 个 cargo check warning；副本仍是 Rust 188/0/3、前端 14 文件/77 项。两者差别源于第 6.1 节的实际源码和新增测试，已查明，不是 12 个根独有文档/发行物造成。今后数量、忽略项或 warning 变化必须说明原因；退出码 0、测试 0 失败与差异有证据才能登记通过。

副本只有源码再变动才复验，现有通过结果不因文档更新重跑：

```powershell
Set-Location -LiteralPath 'E:\XY switch\artifacts\model-audit-20261009\modified'
$env:CARGO_TARGET_DIR = 'E:\XY switch\artifacts\model-audit-20261009\build'
npm.cmd test
if ($LASTEXITCODE -ne 0) { throw 'Frontend tests failed.' }
npm.cmd run build:renderer
if ($LASTEXITCODE -ne 0) { throw 'Renderer build failed.' }
cargo test --manifest-path src-tauri/Cargo.toml --lib --offline --locked
if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed.' }
cargo check --manifest-path src-tauri/Cargo.toml --offline --locked
if ($LASTEXITCODE -ne 0) { throw 'Cargo check failed.' }
cargo build --manifest-path src-tauri/Cargo.toml --offline --locked --features tauri/custom-protocol
if ($LASTEXITCODE -ne 0) { throw 'Desktop build failed.' }
```

执行前保存需留存的旧构建。以后复验仍使用新隔离数据/配置与独立日志，不覆盖本轮成功/失败目录；本轮执行对象均为用户请求的本地验收：

| 入口 | 被测对象与限制 | 后续取证要求 |
| --- | --- | --- |
| [既有浏览器 UI](<E:/XY switch/artifacts/model-audit-20261009/verify_ui_handoff_regression.mjs>)、[扩展 UI](<E:/XY switch/artifacts/model-audit-20261009/verify_handoff_ui.mjs>) | 固定使用 artifact 下的 `modified`、Vite 1438/1439、Chrome 和合成 IPC；切换 cwd 不会改成主树 | 新 `XY_UI_LOG_DIR`，扩展回放设置 `XY_BACKEND_REPORT`；不能替代主树真实 Tauri |
| [当前原生启动](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/start_native.mjs>)、[审计验证](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/verify_native.mjs>)、[最终业务验证](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/verify_workflows.mjs>)、[当前runner](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/run_acceptance.py>) | 本轮绑定build004；已有run-001至run-005和所有命令不能直接重跑覆盖，移到全新目录并检查runner参数与manifest后才复验 | 保存source/renderer/exe、真实IPC/SQLite/配置、截图/退出码及清理；源码变化须新构建/绑定，不能沿用build004标签 |
| [旧runner（历史专用）](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/run_acceptance.py>)、[旧清理脚本](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015/native-desktop/stop_native.ps1>) | runner 的 `native-start` 仍硬编码 `run-006` 与 `build-binding-003.json`，不可直接用于新验收；清理脚本按活动记录核实路径/PID | 先改新入口参数并检查记录，不因命令叫 retry 就认为换了 exe；停止前核实目标只能属于本次隔离验收 |
| [10 月 14 日副本恢复入口](<E:/XY switch/artifacts/model-audit-20261009/logs/handoff-execution-20261014/native-desktop/resume_native.ps1>) | 固定使用旧副本 `tested-build/xy-switch.exe`；其语法通过并不证明当前主树验收 | 不用于当前主树 build004；旧历史 PID 只能经路径核对后处理 |

旧 `ROLLBACK.sh` 只处理同目录 ZIP 副本，不恢复当前源码或数据库；若复验其归档行为，另建 ZIP 副本，不能对原归档或工作源码直接执行恢复。

### 6.4 移机材料与本轮文档验收

| 材料 | 需携带内容 | 接收核对 |
| --- | --- | --- |
| 当前源码交付 | [224项源码ZIP](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/source-delivery/XY-Switch-build004-source-20261016.zip>)、[逐项归档清单](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/source-delivery/source-archive-manifest.json>)；主树与完整旧副本分别保留 | 当前ZIP对应219接受输入+5extras，全部字节/CRC一致；旧副本216输入及13差异不混用 |
| 当前文档与元数据 | 本篇、[验收报告](<E:/XY switch/docs/model-audit-acceptance-20261016.md>)、schema7 JSON、设计/实现说明、本轮tools与before/previous备份 | documentVerification为主篇的当前SHA-256/bytes/lines；JSON文件名旧日期不代表旧版本 |
| build004原生证据 | 完整 [application-acceptance-20261016](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016>)：source/绑定/tested exe、所有commands/results/stdout/stderr、harness manifest、五轮run JSON/截图/cleanup、隔离夹具 | 32+108当前通过，3次未完成harness保留；不把合成HTTP/SSE、测试凭据当真实商业观察 |
| debug MSI与静态核验 | [x64 MSI](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/installer/retry1/target-build004/debug/bundle/msi/XY Switch_1.0.0_x64_en-US.msi>)、installer全部失败/成功日志、static-extraction全载荷/product.wxs/验证JSON | MSI SHA-256 `fe57eda62c65d1b93190b9ab588aa54ccde6721e38aab2bee8db14c77b2f1d17`；三字节marker差异与4告警明确，不能称安装/签名release通过 |
| 历史单测/构建与恢复 | application-acceptance-20261015、三份修复清单及修复前备份、handoff-execution-20261014、旧归档/补丁/role-hashes、MAIN_BEFORE.zip | Rust198/前端94为主树历史有效证据；副本188/77/UI87保持原口径；223备份/54集成/219当前输入为不同范围 |
| 外部工具及缓存维护 | Node/pnpm、Rust/MSVC、WebView2、Python、WiX；历史Python设计如需重跑须另带外部工程；[维护记录](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup>) | 源码包不分发全部依赖/工具链；可重建debug中间缓存无需交付，保留绑定exe和所有evidence |

本轮文档工具 [test_verify_handoff.py](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/test_verify_handoff.py>) **67项通过、exit0**（20:26:18）；原schema6的33项及原工具保存在tools-schema6-original，34个新增用例绑定build004/原生异常/失败记录/进程归属、源码包、MSI三字节差异与告警、未验收边界及绝对链接。[测试命令/结果](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/documentation-tests.result.json>)与stdout/stderr独立保存，不混算应用测试数量。refresh_snapshot.py从存量文件汇总schema7，verify_handoff.py核对主篇及live raw evidence；围栏用PowerShell ParseFile静态解析，**不执行围栏应用命令**。未新增独立agent复核，顶部13:2x复核保留其原来源和当时结论。

在源文件与材料不变时，接收方只读复核：

```powershell
Set-Location -LiteralPath 'E:\XY switch'
$handoffDir = 'E:\XY switch\artifacts\model-audit-20261009\logs\application-acceptance-20261016'
python -B "$handoffDir\verify_handoff.py" --check-only
if ($LASTEXITCODE -ne 0) { throw 'Handoff evidence or document verification failed.' }
python -B "$handoffDir\test_verify_handoff.py"
if ($LASTEXITCODE -ne 0) { throw 'Handoff validator regression tests failed.' }
```

系统PATH须可解析Python和PowerShell；本机 [run_documentation.py](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/run_documentation.py>)已登记实际Python/pwsh路径，拒绝覆盖同名尝试。重新生成证据时先刷新同目录snapshot，再用 [attach_cleanup_status.py](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/approved-execution-20261016/attach_cleanup_status.py>)将独立维护状态及其原始记录哈希挂入cacheMaintenance/liveEvidenceFiles，然后运行不带--check-only的验证器；最后check-only复算且不改存量文件。这些步骤只处理文档并读取应用存量材料，不重跑应用。源码/构建/原生或包范围变化时先调整接受逻辑与正文，不机械沿用true/false。缓存维护为**部分完成**，数字和保护边界见顶部及[批准后状态汇总](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/approved-execution-20261016/maintenance-status.json>)；[11项残余清单及续跑准备](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/approved-execution-20261016/resume-permission-retry>)严格限定原11项，真正继续前须完成权限审核并绑定已明确修改的最新文档保护哈希，不应直接重跑35项原计划。本次已先完成部分删除后的只读核对，再同步正文和文档元数据；[本次正文及元数据修改前原字节备份](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/approved-execution-20261016/before-documentation-manifest.json>)为直接复制的原字节备份。原四次未执行记录、解析误报失败、部分执行结果及新两次审核超时分别保留、不覆盖。

## 7. 旧设计中需要纠正的假设

下表涉及“旧主树”的描述指 10 月 14 日集成前版本；那轮文件同步已完成。当前主树已发生有证据的后续修复并取得独立测试/构建结果，版本与原生未完成项按第 5、6 节判断。

| 旧表述 | 正确解释 |
| --- | --- |
| `request_events.model` 就是上游自报 | 旧列混合响应值、请求 fallback 和规范化后的 Codex 会话导入模型；副本单独保存捕获的 `audit.observation.upstreamModel`，旧记录保持未知 |
| `fallback_model` 就是客户端原始 model | 路由映射、配置与 payload override 可能改值，需分开保存请求、实际发送和上游自报 |
| 只改两个旧回填点即可覆盖所有流量 | 原生透传流可能不进入旧转换流回填；副本有独立原始观测包装，必须完整保留 |
| 转换后的 usage 可用于自洽证明 | 缺字段可能被补 0，total 可能由本机相加；需保存原字段存在性与上游原始 total |
| `database.rs:1187` 是生产建表，`affects_health` 已是表列 | 旧行号指迁移回归测试；初始建表在 `migrations/001_initial.sql`，审计升级还依赖 `Database::open` 增量迁移；查询按 `route <> 'health_check'` 排除健康检查 |
| P0 四条都是硬证据，流式指纹不可规避 | 名称差异是弱信号；三类用量检查都有前提，上游也能改写元数据、usage、缓冲与帧边界 |
| 需要新增可配置 User-Agent | 主树及副本已有 `custom_user_agent` 配置和发送逻辑，先核对现有实现 |
| 缺 Rust 或旧 exe 能代表当前编译状态 | 副本有 10 月 14 日成功记录；当前主树已有 10 月 16 日独立测试和 debug 构建结果。旧 exe 或旧失败均不能代替当前源码绑定的验收 |
| 灰度 token 不存在，或移除 `data-theme` 就能触发 fallback | `ui-refinement.css` 的选择器含无条件 `:root`，默认也有浅色变量；fallback 在不加载 token 的隔离页面验证 |
| 危险文字色需要再次硬编码 | `--xy-danger-strong` 原已存在；现在补齐 `--xy-success-strong` 并统一复用。查变量时需读完整选择器和后补别名段 |

设计原文保留用于理解需求；当前结论以源码、与其哈希对应的最终日志及本文未完成项为准。

## 8. 10 月 14 日增量及该版本已完成验收

本节描述 10 月 14 日副本增量，不代表当前主树 build004 的全部验收。五个产品/后端测试文件相对旧 ZIP 发生变化；另外改了一个 QA 入口、新增两个错误边界 QA 文件。这批副本改动没有改变审计算法、阈值、数据库结构或命令签名，但增强了既有 Rust 测试，不能再写成“全部只是文案/样式/注释”。

| 文件或范围（位于 modified） | 当前改动 | 已有验证与剩余边界 |
| --- | --- | --- |
| `src-tauri/src/shadow_audit_commands.rs` | notes 由 5 条改 6 条，解释 3 对/5 增量/32 token/95% 区间与截距稳定性；增强既有测试，检查空/非空 notes 一致并输出报告 JSON | Rust 全套与单独报告命令通过，浏览器准确回放；build003 实际 Tauri IPC 已有 32 项审计验收；build004 仍需复验 |
| `src/styles/model-audit.css` | unknown/untested 用中性文字、底色和描边；去掉深色琥珀覆盖；自报不一致单独用琥珀，硬证据仍红 | 明暗两套、空态/样本不足、五类视觉及布局浏览器检查通过；颜色类别不改变三态逻辑 |
| `src/styles/usage.css` | ok/fail 背景、描边、文字改为 success/danger 的 soft/border/strong token；移除冗余深色覆盖 | 明暗状态胶囊与 1180/820 布局通过；浅色 ok/fail 对比度约 4.76/5.91，深色约 7.49/6.74；窄屏保留原表格横向滚动 |
| `src/styles/ui-refinement.css` | 在三档作用域补 `--xy-success-strong`：浅 `#15803d`、深 `#6ee7b7`；复用既有 `--xy-danger-strong` | token 及状态文字验证通过；这是共享样式增量，集成时和 usage 一起同步 |
| `src/components/FrontendErrorBoundary.tsx` | `--xy-error` 改为 `var(--xy-danger, #dc2626)`，muted 增 fallback；注释已修正默认 `:root` 作用域说明 | 默认、light、dark 及无 token 隔离页验证通过；默认 root 有变量，移除 data-theme 不能验证 fallback |
| `scripts/audit-qa/main.tsx`，新增 `scripts/error-boundary-qa/` 两文件 | 增加实际后端报告回放、主题/颜色/错误边界 QA 场景 | 最终扩展 UI 55 项通过；QA 文件不进入正常产品入口 |

P1.5a 的 20 条门槛保留：`MIN_COMPARABLE=20`、跨组 notes 以及“两组各需至少 20 个合格计费样本”的前端断言均有效。影子报告不再固定等 20 对；全文搜索 `20 对` 还会命中新的负向测试断言，不能把命中数量当验收结论。

回退时按关联组处理：灰度规则及旧深色覆盖配套；`--xy-success-strong` 与 `.status-ok` 配套；QA 和新增 notes 断言需与对应行为一致。先保存当前字节，再与旧归档逐项比较。旧 `ROLLBACK.sh` 不恢复这些文件。当前主树的额外 13 项差异不能通过旧副本或旧 ZIP 覆盖；源码、QA、证据与新归档的交付闭合按第 6 节推进。
