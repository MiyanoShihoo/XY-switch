# 模型审计：使用与实现说明

这次实现把真实业务响应中的模型自报、用量矛盾和流式结构接入 XY Switch，用证据与缺口计数帮助排查渠道。它不能证明真实模型身份，也没有使用真实供应商做身份验证。模型名一致、分词指纹相近和没有告警均不构成渠道背书。

实现位于当前 `modified` 工作副本，原工作区源文件保持原样；本副本的[集成交接文档](model-audit-integration.md)已更新，包含 2026-10-11 自适应回归修复验收；根工作区输入保留。审阅与运行时应使用这个副本。完整修改、基线和回滚以交付目录的 [VERIFICATION.txt](../../VERIFICATION.txt) 与 [ROLLBACK.sh](../../ROLLBACK.sh) 为准。

## 在用量统计中查看一次请求

1. 打开「用量统计」，按供应商、来源、模型和日期筛选请求。
2. 请求日志的「模型自报与审计」列显示请求模型；有配置改写时额外显示路由后模型；上游自报单独保留。
3. 展开「查看详情」核对证据、数值和未完成的检查；单次流式统计需再点「分析此响应流式统计」。
4. 点「分析当前筛选范围」计算跨渠道与跨时间结果。自动刷新日志不会触发这些统计分析。

| 展示 | 判定 | 可以说明什么 |
| --- | --- | --- |
| 自报一致 | 自报名称匹配请求名称或已配置的路由模型 | 仅名称符合配置；上游可改写该字段 |
| 自报不一致 | 自报名称与两者均不匹配 | 存在名称差异，需要核查配置与渠道 |
| 无法判断 | 请求名称或上游自报缺失，包括旧记录 | 没有足够观测；这不等于没问题 |

「硬证据」「自报不一致」「未采集」「有缺口」分别计数，缺口可与证据共存。当前页计数与整个筛选范围的分析计数分别标明；页面不提供渠道可信度分数。历史计费模型字段不会被反推为上游自报。

## 已实现的阶段与判断边界

| 阶段 | 已实现行为 | 主要边界 |
| --- | --- | --- |
| P0- | 保存请求、路由后和上游自报模型；显示三态 | 自报名称不是身份认证 |
| P0 | 字节上界、用量总数、自报差异、请求 token 上限检查；记录结构化证据和缺口 | 自报差异单独列为警示，不计入三类硬证据 |
| P1 | 记录文本帧数、字符分布、最长数字/汉字/空白串、帧长中位数、首字与有界到达偏移；检查批处理和分轴可比性 | SSE 边界不保证等于 token 边界，轴矛盾只作为条件性统计线索 |
| P1.5 | 默认关闭的被动影子差分、相邻轮本地 BPE 回归；用户触发跨渠道/日期分组比较 | 斜率与 2 倍变化均为统计线索，无法排除模板、参数、负载和重切帧 |
| P2 | 本地 byte BPE、固定语料差分指纹、导入/导出 JSON、带库外档的概率排名 | 定位能力止于词表等价类；没有自动主动探针 |

字节上界判据为：`outputTokens - reasoningTokens > visibleOutputBytes + toolPayloadBytes + 4`。详情中明确展示推理扣除、工具载荷字节和特殊 token 余量。缺少推理 token、载荷未完整测量、响应中断、非文本输出或推理数无效时，跳过该检查并记录缺口，绝不把缺失推理数默认为零。

用量自洽检查要求输入、输出、总数齐全；上限检查要求取得请求 token 上限。缺字段时分别记为未测。流式与非流式回填使用同一审计纯函数，原生响应透传、协议适配和用量回填仍保留各自路径。

P1 至少需要 8 个文本增量；批处理会使切分结论不可用。分轴计费交叉检查还要求完整的纯文本、无工具载荷、已知推理数和同类字符输出；混合文本不能据总用量构造某一字符轴的计费斜率。到达偏移最多保留 256 项，采样截断会记录状态。

P1.5 最多读取筛选范围内最近 10,000 条业务记录，排除健康检查；超过上限会提示截断。每个比较组至少需要 20 条完整、纯文本、未检测到批处理且未截断的可比流式样本，首字均值也要求足够有效时延样本。没有可比组时显示未测到；相近不代表共用上游，变化不证明换模型。

## 被动影子差分

在用量统计中展开“被动影子差分（默认关闭）”，按渠道 ID 与实际转发模型配置本地分词器，启用后只采集新业务。后台严格验证相邻历史前缀、实际回复、缓存为零、完整纯文本和配置分区；正文不落库。3 对仅是计算下限；判读还需至少 5 种增量和 32 token 跨度，按残差对应的斜率区间与时间稳定性决定是否继续采样，不固定等待 20 对。启停步骤、资源上限和完整验证边界见[本轮交接](model-audit-integration.md)。

## 使用本地参照库

在「离线参照库分析」中填写观测指纹 JSON 的绝对路径，以及每行一个的候选指纹 JSON 绝对路径，然后点「分析参照库」。这些文件在本机读取，不上传给供应商。

需要建立候选时，展开「从本地分词器生成候选指纹」，填文件绝对路径和候选名称，默认使用「自动识别文件规则」。生成后可参与本次分析，也可点「导出指纹 JSON」。名称由用户提供，不是程序发现的模型身份。实际渠道定位需要另外提供按同一方案采集的观测指纹；被动用量统计不能自动替代这些观测。

| 分词器输入 | 支持条件 | 不满足时 |
| --- | --- | --- |
| `.tiktoken` | base64 字节 token 与唯一整数 rank；包含全部 256 个基础字节 | 拒绝空项、重复项、无效编码与缺失字节 |
| 已知 `.tiktoken` 文件名 | `cl100k_base`、`o200k_base`、`r50k_base`、`p50k_base` 对应规则自动识别 | 其他文件名必须显式选择规则 |
| Hugging Face `tokenizer.json` | byte-level BPE；无 normalizer、无 prefix space；精确支持 GPT-2/cl100k/o200k 预分词结构 | 不支持的结构直接报错，不猜测分词结果 |

非零 dropout、byte fallback、fuse-unk、ignore-merges、非空词内/词尾附加串等选项不受支持。测试语料碰到 added token 时也拒绝生成，以免错误模拟匹配。分词器文件上限 128 MiB，指纹 JSON 上限 2 MiB；离线计算不进入代理转发热路径。

## 交换指纹 JSON

文件采用 camelCase 字段，固定语料与完整字符串以 [reference.rs](../src-tauri/src/model_audit/reference.rs) 的 `CORPUS` 为准。它不是任意文本的 token 计数格式。

| 字段 | 格式或约束 |
| --- | --- |
| `schemaVersion` | 整数 `1` |
| `corpusId` | `xy-local-differential-v1` |
| `model` | 非空名称，最多 256 UTF-8 字节 |
| `sourceSha256` | 64 位十六进制文件/采样证据摘要；仅标识内容，不认证来源 |
| `tokenizerKind`、`pretokenizer` | 记录分词器种类与所用预分词规则 |
| `authority` | 本地生成用 `tokenizer_file`；独立测量导入用 `measured` |
| `groups` | 分词组数组；每项为 `id`、`chars`、`d1`、`d2`、`stable` |
| `limitations` | 字符串数组，保存该份指纹的限制 |

分组 ID 为 `digits`、`han_common`、`han_rare`、`spaces`、`latin`、`code`、`cyrillic`、`arabic`、`thai`、`emoji`、`mixed_case`。`chars` 是对应固定字符串的 Unicode 字符数，按实际字符串计算；重复/未知分组或错误字符数会被拒绝。

每组共用填充串 `Local offline tokenizer reference. Same prefix for all three rungs.`。第一阶输入只有填充串；第二阶追加 `\n` 与该组文本；第三阶再追加完全相同的片段。`d1 = t1 - t0`、`d2 = t2 - t1`，仅两次差分均为正且相等时才有 `stable: true`。未采齐的组应省略，不能把缺失阶数当作稳定观测。

排名仅使用观测和所有候选共同具有的稳定分组，默认至少 3 组、最多 128 个候选；不足时不输出概率。相关分组按同一证据族使用 `c*tanh(sum/c)` 软封顶，默认 `pHit=0.97`、`outsideEdge=0.1`、`familyCap=3`。参数校验包含封顶后的单次失配下界，确保最佳库内候选有失配时库外项可以领先。概率依赖候选库与等权先验；全组吻合仍只能归入词表等价类。

## 数据与来源限制

审计持久化内容是模型标签、usage 数字、字节数、帧统计、到达偏移、证据 ID 和缺口；不新增请求正文、响应正文、响应头值或工具参数正文的持久化。旧记录不能补算已经丢失的正文和原请求模型，中断响应也不能冒充完整样本。

Unicode 类别依赖使用本机已有的 `unicode-general-category 1.1.0` 源码及 Apache-2.0 许可证，置于 [vendor 目录](../src-tauri/vendor/unicode-general-category)。本地文件哈希已记录，但没有原始 Cargo 下载包/注册表校验文件，不能称为已通过注册表来源认证。见[来源说明](../../logs/unicode-provenance.txt)与[本地哈希清单](../../logs/unicode-source-hashes.json)。

## 复核实现和证据

在 `modified` 根目录执行以下命令；前端依赖与 Rust 工具链需已就绪。

```powershell
npm test
npm run build:renderer
cargo test --manifest-path src-tauri/Cargo.toml model_audit
```

2026-10-10 前端已验证 77 项测试通过；Rust 181 项通过、3 个既有交互测试忽略，生产前端和桌面构建通过。本轮原始日志位于 [completion-20261010](../../logs/completion-20261010)，下方首轮日志保留为历史记录。首次依赖不完整的失败与修复保存在[环境记录](../../logs/frontend-setup-notes.txt)；最终输出见[前端测试](../../logs/frontend-tests.log)、[前端构建](../../logs/frontend-build.log)。Rust 与整个事务的最终结果以交付 [VERIFICATION.txt](../../VERIFICATION.txt) 为准，不能以代码中的测试名称代替执行结果。

| Evidence：可核查证据 | Finding：覆盖的行为 | Path：实现入口 |
| --- | --- | --- |
| [model-audit.test.ts](../src/model-audit.test.ts) 与前端测试日志 | 三态、旧记录未知、缺口独立显示、库外概率完整性、命令参数 | [ModelAuditCell](../src/components/ModelAuditCell.tsx)、[ModelAuditPanel](../src/components/ModelAuditPanel.tsx)、[前端服务](../src/lib/model-audit-service.ts) |
| [代理回归测试](../src-tauri/src/proxy_regression_tests.rs) 中真实本地 HTTP 故障/诚实样本、取消请求用例 | 透传字节、流式/非流式采集和部分观测回填 | [proxy.rs](../src-tauri/src/proxy.rs)、[streaming.rs](../src-tauri/src/streaming.rs)、[invariant.rs](../src-tauri/src/model_audit/invariant.rs) |
| [SQL/聚合测试](../src-tauri/src/model_audit_commands.rs) | 幂等旧库迁移、健康检查排除、缺口/自报计数、样本门槛 | [database.rs](../src-tauri/src/database.rs)、`model_audit_commands.rs` |
| [99 条独立 Unicode regex 样例](../src-tauri/src/model_audit/pretokenizer-oracle.json)及 tokenizer 回归测试 | 组合音标可选前缀回溯、长 s 缩写折叠、大小写/数字/空白边界 | [tokenizer.rs](../src-tauri/src/model_audit/tokenizer.rs) |
| [reference.rs](../src-tauri/src/model_audit/reference.rs) 的指纹与排名测试 | 同一追加片段、哈希往返、词表等价、库外领先和退化配置拒绝 | `generate_reference`、`load_reference`、`rank_references` |

这些测试使用本地夹具和可重复输入，未对真实供应商作身份断言。遇到未知或缺口时，先查具体缺失字段；需要跨渠道比较时积累足够同类业务样本；需要参照定位时导入独立采集且同语料的观测指纹。
