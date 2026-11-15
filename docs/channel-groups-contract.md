# XY Switch 渠道组：本地接口契约 v1

日期：2026-11-14。状态：第一阶段离线试点。

规范来源：[Rust 数据结构](<../src-tauri/src/channel_groups/types.rs>)、[Rust 校验](<../src-tauri/src/channel_groups/validation.rs>)、[TypeScript 类型](<../src/types/channel-group.ts>)、[前端服务](<../src/lib/channel-group-service.ts>)。

## 通用约定

- 通过 Tauri IPC 调用，不新增公网 HTTP 管理接口。方法参数、对象字段和枚举使用 camelCase。
- 时间均为 Unix 毫秒；策略 TTL / 冷却配置为秒。金额均为整数 μUSD。计数/时间/金额需能由 JavaScript 安全整数表示；错误率和延迟允许有限小数。
- `null` 表示没有观测或未配置；不等价于 0。必需字段和可空字段见表；输入结构拒绝额外字段。
- 所有操作沿用既有本地应用访问边界；渠道配置只允许 provider/account 引用，不接受密钥或端点副本。
- 后端返回 `Result<T, String>`。以下错误码用于可恢复状态和验证失败，底层存储错误仍可能返回字符串；前端不展示未经筛选的任意错误正文。

## 命令

| 命令 | 参数对象 | 成功结果 | 写入 |
|---|---|---|---|
| `channel_groups_list` | 无参数 | `Snapshot` | 无 |
| `channel_groups_save` | `{ input: SaveGroup }` | 更新后的 `Snapshot` | 仅渠道组设置项，事务保存 |
| `channel_groups_set_enabled` | `{ groupId, enabled, expectedRevision }` | 更新后的 `Snapshot` | 仅组 enabled 与 revision |
| `channel_groups_recorded_events` | `{ groupId, limit }` | `RecordedEvent[]`，较新事件在前 | 无；limit 为 1–200 |
| `channel_groups_simulate` | `{ input: TrialInput }` | `TrialResult` | 仅本次调用的内存 |

前端另调用现有 `list_accounts` 获取账号元数据，不取账号凭据。正常浏览器预览没有原生引擎时，操作明确报 `channel_groups_native_required`；不会伪造可保存的真实结果。

## 配置对象

`SaveGroup = { group: ChannelGroup, project: ProjectBudget | null }`。`Configuration = { groups: ChannelGroup[], projects: ProjectBudget[] }`。

### ChannelGroup

| 字段 | 类型 | 规则 |
|---|---|---|
| `id` | string | 1–128 个 ASCII 字符，仅字母、数字、`-_.`；全配置唯一 |
| `name` | string | 非空，最多 256 字节，无控制字符或首尾空白 |
| `virtualModel` | string | `xy-group/` 加上述 ID 格式的后缀；全配置唯一 |
| `enabled` | boolean | 只控制离线选择资格 |
| `strategy` | string enum | `roundRobin` / `weightedRoundRobin` / `leastLoad` / `latencyFirst` / `quotaAware` |
| `projectId` | string 或 null | 引用已配置项目；也可随 `SaveGroup.project` 同事务建立 |
| `affinityTtlSeconds` | integer | 60–2,592,000 秒 |
| `channels` | Channel[] | 1–32 条，允许单条或全部停用 |
| `revision` | integer | 新建为 0；编辑提交读取到的版本；成功保存加 1 |

全配置最多 64 个组、128 个项目、512 条渠道。读取持久化 JSON 上限 2,000,000 字节。

### Channel

| 字段 | 类型 | 规则 |
|---|---|---|
| `id` | string | 配置 ID 格式；在所有组中唯一 |
| `providerId` | string | 引用已有供应商；非空身份字符串，最多 256 字节 |
| `accountId` | string 或 null | 引用该供应商的账号；null 表示供应商默认凭据路径 |
| `upstreamModel` | string | 非空身份字符串；不能再次引用 `xy-group/`；检查账号模型权限 |
| `enabled` | boolean | 该渠道参与离线选择 |
| `priority` | integer | 0–65,535，小值优先；亲和性优先于该字段 |
| `weight` | integer | 1–1,000；平滑加权轮询使用 |
| `quotaPoolId` | string 或 null | 显式共享额度池标识，使用配置 ID 格式 |
| `quotaIndependent` | boolean | 默认 false；独立性声明，不能与非空 quotaPoolId 同时使用 |
| `limits` | Limits | 可省略时使用所有字段 null |
| `cooldown` | CooldownPolicy | 可省略时使用 60 / 30 / 3600 秒 |

绑定账号时校验供应商归属，拒绝 `NativeManaged` 作为 HTTP 渠道；供应商已配置账号池但未指定账号时，选择结果为 `explicit_account_required`。这是为了维持渠道与账号之间稳定的对应关系。

### Limits / CooldownPolicy / ProjectBudget

| 对象.字段 | 类型 | 规则 |
|---|---|---|
| `Limits.rpm` | integer 或 null | 1–10^12，滚动 60 秒请求上限 |
| `Limits.tpm` | integer 或 null | 1–10^12，滚动 60 秒 Token 上限 |
| `Limits.concurrency` | integer 或 null | 1–100,000 |
| `Limits.dailyMicrousd` | integer 或 null | 0–10^12，UTC 日预算；null 为未配置 |
| `CooldownPolicy.rateLimitSeconds` | integer | 1–86,400，缺上游时间证据时的初始 429 等待 |
| `CooldownPolicy.failureSeconds` | integer | 1–86,400，未知结果冷却 |
| `CooldownPolicy.maxBackoffSeconds` | integer | 不小于 rateLimitSeconds，最多 604,800；只截断本地退避 |
| `ProjectBudget.id` | string | 配置 ID 格式 |
| `ProjectBudget.dailyMicrousd` | integer 或 null | 0–10^12，所有引用组共享 |
| `ProjectBudget.revision` | integer | 新建 0，更新须匹配当前版本 |

### 保存示例

以下为合成配置，`provider-alpha` / `provider-beta` 必须先作为本地试验供应商存在。示例不包含或获取任何凭据；发送的是本地 IPC，不是模型请求。

```json
{
  "input": {
    "group": {
      "id": "research",
      "name": "研发助手",
      "virtualModel": "xy-group/research",
      "enabled": true,
      "strategy": "roundRobin",
      "projectId": "project-a",
      "affinityTtlSeconds": 1800,
      "revision": 0,
      "channels": [
        {
          "id": "research-a",
          "providerId": "provider-alpha",
          "accountId": null,
          "upstreamModel": "model-a",
          "enabled": true,
          "priority": 0,
          "weight": 1,
          "quotaPoolId": null,
          "quotaIndependent": false,
          "limits": { "rpm": 20, "tpm": 20000, "concurrency": 4, "dailyMicrousd": 500000 },
          "cooldown": { "rateLimitSeconds": 60, "failureSeconds": 30, "maxBackoffSeconds": 3600 }
        },
        {
          "id": "research-b",
          "providerId": "provider-beta",
          "accountId": null,
          "upstreamModel": "model-b",
          "enabled": true,
          "priority": 0,
          "weight": 1,
          "quotaPoolId": null,
          "quotaIndependent": false,
          "limits": { "rpm": 20, "tpm": 20000, "concurrency": 4, "dailyMicrousd": 500000 },
          "cooldown": { "rateLimitSeconds": 60, "failureSeconds": 30, "maxBackoffSeconds": 3600 }
        }
      ]
    },
    "project": { "id": "project-a", "dailyMicrousd": 1000000, "revision": 0 }
  }
}
```

两个渠道的额度关系未知，默认保守合池；示例池 RPM 上限是 20，不是 40。

组和项目都使用乐观锁。修改共享项目预算时同时提交其 revision，任一冲突则整个事务回滚。`project=null` 表示不改项目记录，不会删除原项目。停用示例：`{ "groupId": "research", "enabled": false, "expectedRevision": 1 }`。成功后读取新快照中的 revision；重用旧 revision 返回冲突，不静默覆盖。

## 模拟输入

`TrialInput = { groupId, replayEventIds: number[], steps: TrialStep[] }`。最多 200 个唯一历史事件 ID，最多 4096 个步骤。目标 groupId 必须存在；历史事件必须属于该组引用的供应商。模拟引擎装入所有已配置组；步骤内的 request.groupId 可指定其中的组，以验证跨组项目预算和共享池。页面生成的步骤仅针对当前组。

历史事件先按 `(atMs, id)` 回放，再执行 steps。所有时间须单调不减，范围 0–253,402,300,799,000。有历史回放时，首个步骤不得早于最后一个已回放事件。

### RouteRequest

| 字段 | 类型 | 规则 |
|---|---|---|
| `id` | string | 非空身份字符串；一次试验内唯一逻辑请求 ID |
| `groupId` | string | 使用配置 ID 格式 |
| `reserveTokens` | integer | 1–10^12，选择时即时预留 |
| `reserveMicrousd` | integer 或 null | 0–10^12，预算存在时必须明确提供 |
| `stateful` | boolean | 默认 false；true 时至少提供一个状态身份 |
| `sessionKey` | string 或 null | 已知时绑定；新会话可创建临时亲和性 |
| `previousResponseId` | string 或 null | 必须已由该组之前的成功结果建立绑定 |
| `conversationId` | string 或 null | 必须已由该组之前的成功结果建立绑定 |

三个身份字段都按非空、无首尾空白、无控制字符、最多 256 字节校验。未知/过期服务端身份返回拒绝决策，不能换渠道补发。

### TrialStep 与 Completion

```text
{ kind: "request", atMs, request: RouteRequest }
{ kind: "streamStarted", atMs, requestId }
{ kind: "complete", atMs, requestId, result: Completion }
```

`Completion` 字段如下：

| 字段 | 类型 | 规则 |
|---|---|---|
| `outcome` | enum | `http` / `timeout` / `transportUnknown` / `cancelled` / `notSent` |
| `status` | integer 或 null | http 必须为 200–599；其他 outcome 必须 null |
| `usage` | object | `{ tokens: integer|null, microusd: integer|null }`；可省略，代表均未知；数值最多 10^12 |
| `headers` | string map | 可省略；最多 32 个头，键最多 128 字节，值最多 512 字节 |
| `responseId` | string 或 null | 仅成功 2xx 可建立，需校验身份和绑定冲突 |
| `conversationId` | string 或 null | 同 responseId |

明确未发送只允许没有流式输出、usage 和 headers 证据的结果。已结束请求拒绝重复完成；被拒绝选择的请求没有 attempt，不能提交 streamStarted / complete。

### 525 未知结果示例

这是传给 `channel_groups_simulate` 的合成时间线，不会发送 HTTP 请求。所用日期仅为可重放输入；用于当前预算验收时应选择与目标账本日一致的时间。

```json
{
  "input": {
    "groupId": "research",
    "replayEventIds": [],
    "steps": [
      {
        "kind": "request",
        "atMs": 1794643200000,
        "request": {
          "id": "offline-1", "groupId": "research", "reserveTokens": 128,
          "reserveMicrousd": 1000, "stateful": true, "sessionKey": "session-a",
          "previousResponseId": null, "conversationId": null
        }
      },
      { "kind": "streamStarted", "atMs": 1794643200100, "requestId": "offline-1" },
      {
        "kind": "complete", "atMs": 1794643200200, "requestId": "offline-1",
        "result": {
          "outcome": "http", "status": 525,
          "usage": { "tokens": null, "microusd": null }, "headers": {},
          "responseId": null, "conversationId": null
        }
      }
    ]
  }
}
```

预算和资格满足时仅产生一个选择；完成后并发和金额预留继续保留，实际用量未知，绝不产生第二次派发。已有预算未知记录也可能使首个选择被拒绝，这时应检查拒绝决策，不要继续提交不存在 attempt 的完成步骤。

## 返回对象

### Snapshot / TrialResult

| 对象 | 字段 |
|---|---|
| `Snapshot` | `phase: "offlinePilot"`, `liveEnabled: false`, `billingHold: true`, `configuration: Configuration`, `channels: ChannelView[]`, `warnings: string[]`, `generatedAtMs: integer` |
| `TrialResult` | `source: "offlineSimulation"`, `liveDispatches: 0`, `ledgerWrites: 0`, `decisions: Decision[]`, `replayedEventIds: integer[]`, `channels: ChannelView[]`, `warnings: string[]` |

前端校验这些离线标记及完整字段形态；有真实派发、账本写入或不兼容数据时拒绝接受结果。

### Decision / Exclusion

`Decision = { requestId, channelId: string|null, upstreamModel: string|null, reason, excluded: Exclusion[], dispatchAllowed: false }`。

`Exclusion = { channelId, reason, retryAtMs: integer|null }`。`excluded` 列出因资格、容量等被排除的渠道，不把所有未选中的正常候选都当作失败项。

| 原因类别 | 典型值 |
|---|---|
| 已选择 | `selected_for_simulation`, `session_affinity` |
| 拒绝选择 | `request_already_considered`, `group_missing`, `group_disabled`, `no_eligible_channel` |
| 会话 | `stateful_identity_required`, `affinity_unknown`, `affinity_expired`, `affinity_conflict`, `affinity_unavailable` |
| 元数据与权限 | `channel_disabled`, `provider_missing`, `provider_disabled`, `account_missing`, `account_unavailable`, `explicit_account_required`, `model_not_available_to_account`, `authentication_disabled` |
| 容量与冷却 | `cooling_down`, `rpm_limit`, `tpm_limit`, `token_window_unknown`, `concurrency_limit`, `request_quota_exhausted`, `token_quota_exhausted` |
| 预算 | `cost_reservation_required`, `unresolved_ledger_cost`, `channel_budget_exhausted`, `project_budget_exhausted` |

这些是成功返回的决策原因。输入格式错误、重复完成、存储错误等则使整个命令返回错误；没有部分试验落盘。

### ChannelView

| 字段 | 类型 / 含义 |
|---|---|
| `groupId`, `channelId`, `providerId` | string，引用身份 |
| `poolId` | string，本次配置推导的共享池标识；不作为跨配置稳定的外部账号 ID |
| `poolDeclared` | boolean，成员有声明，不表示独立性已验证 |
| `health` | string：资格拒绝原因，或 `group_disabled` / `cooling_down` / `unknown_result` / `quota_exhausted` / `unobserved` / `degraded` / `ready` |
| `cooldownReason` | string 或 null：`rate_limited` / `upstream_result_unknown` / `account_cooling_down` |
| `recoveryAtMs` | integer 或 null，仅已记录冷却的结束时间，不承诺届时必定可用 |
| `rpm` | 最近 60 秒该运行时记录的请求数 |
| `tpmReserved` | 同窗口已知/预留 Token 合计；不表示实际账单 Token |
| `tpmUnknownRequests` | 同窗口没有 Token 证据的记录数 |
| `concurrent` | 尚未完成或结果未知而保留的并发数 |
| `errorRate` | number 或 null，本内存实例观测错误数 / 观测总数 |
| `latencyMs` | number 或 null，回放最新延迟或模拟指数平滑延迟 |
| `quota` | QuotaView |
| `recordedUsage` | UsageTotals；供应商历史总量，可被多个渠道重复引用，不能相加 |
| `simulatedUsage` | UsageTotals；本次试验该渠道的请求与已知用量 |
| `unknownResults` | integer，观测到的未知结果计数 |

`QuotaView = { remainingRequests, remainingTokens, requestsResetAtMs, tokensResetAtMs, source }`，四个数值字段均为 integer 或 null；source 为 string 或 null，当前可为 `recordedAccountSnapshot` / `simulatedResponse`。返回的是多窗口的保守汇总，source 为最近观测来源。

`UsageTotals = { requests, knownTokens, tokenKnownRequests, knownMicrousd, costKnownRequests }`，均为非负整数。已知条数小于请求数时仍有未知记录；已知小计 0 不能证明所有请求免费。

### RecordedEvent

`{ id, providerId, atMs, status, latencyMs, tokens, costMicrousd, errorKind }`。id 与 atMs 为整数；providerId 为字符串；status、latencyMs、tokens、costMicrousd 可为 null，errorKind 为 string 或 null。事件没有渠道、账号、原始限流头或凭据代次字段，回放必须保留这些归因限制。

## 错误与说明

| 场景 | 错误码 / 行为 |
|---|---|
| 并发编辑 | `group_revision_conflict`, `project_revision_conflict`；保留草稿，刷新再编辑 |
| 缺失引用 | `provider_missing`, `account_missing_or_wrong_provider`, `project_not_configured`, `group_missing` |
| 账号类型/模型 | `native_account_not_http_channel`, `model_not_available_to_account` |
| 配置冲突 | `duplicate_group_or_virtual_model`, `duplicate_channel_id`, `conflicting_quota_relationship`, `nested_virtual_model_not_allowed` |
| 格式/范围 | `invalid_identity`, `invalid_configuration_id`, `invalid_rate_limit`, `invalid_channel_weight`, `invalid_cooldown_policy`, `amount_out_of_range` 等 |
| 历史输入 | `invalid_event_limit`, `recorded_event_missing`, `duplicate_recorded_event`, `recorded_event_outside_group` |
| 试验输入 | `trial_too_large`, `trial_time_must_be_monotonic`, `request_not_selected`, `request_already_completed`, `not_sent_evidence_conflict` |
| 响应输入 | `http_status_required`, `unexpected_http_status`, `state_id_requires_success`, `duplicate_response_header`, `response_headers_too_large` |
| 本地能力 | `channel_group_account_metadata_unavailable`, `channel_group_store_unavailable`, `invalid_saved_channel_groups` |
| HTTP 入口 | HTTP 400 / `channel_group_pilot_only`；不取凭据、不记账、不派发 |

重要 warnings：`quota_relationship_unknown_shared_conservatively`、`same_origin_shared_conservatively`、`account_usage_attributed_to_provider_conservatively`、`provider_history_is_not_group_attributed`、`routing_metrics_are_replay_and_simulation_not_live_counters`、`superapi_525_billing_unresolved_live_dispatch_disabled`、`health_history_limited_to_latest_2048_events`、`recorded_headers_unavailable_configured_cooldown_used`。

## 协议对接参考

- [RFC 9110 Retry-After](https://www.rfc-editor.org/rfc/rfc9110.html#section-10.2.3)：秒数或 HTTP 日期。
- [OpenAI rate limits](https://developers.openai.com/api/docs/guides/rate-limits/)：OpenAI 风格 remaining/reset 头的后续对接依据。
- [OpenAI Responses create](https://platform.openai.com/docs/api-reference/responses/create)：`previous_response_id` 与 conversation 的协议对接依据。

以上是对接参考，不代表已访问供应商模型接口或完成真实接入。当前实际支持范围由此文档、类型定义、解析器和离线测试共同界定。
