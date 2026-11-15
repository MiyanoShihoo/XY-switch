# Agent teams guide

[简体中文](agent-fleet-zh.md) | English · [User guide](user-guide-en.md) · [README](../README.md)

Agent teams reads structured local client logs to observe agents, tasks, recorded collaboration, and usage. It provides four views: Topology, Tasks, Timeline, and Resources.

**Delivery status:** The Windows debug native build includes R7–R10. Main-thread real-ledger, lifecycle, recovery, and 10 native UI checks pass, as do 94 Agent-focused Rust tests and 19 frontend tests. All 118 assertions in the independent R10 native check pass; see “Current acceptance record”. Installers and cross-platform releases are not part of this build.

## Open the workspace

1. In the XY Switch desktop app, open the workspace menu beside the brand and choose **Agent teams**. The in-app route is `#agents`.
2. Local collection starts with the first snapshot request. Check the actual sources at the top, or open **Resources**, to inspect their connections, collection modes, and last check times.
3. Use **Team** to select a team, or keep **All teams**. **Topology** opens by default; select an agent, task, event, or usage group to inspect its details.
4. Choose **Back to configuration** to return to the configuration page you left.

Browser preview shows **Native desktop app required / Real data is not connected**. It has no local log connection or simulated agents. A large initial history import appears in batches.

## Choose a view

| View | Actions | What you can inspect |
| --- | --- | --- |
| Topology | Find and select an agent, zoom or pan the canvas, and collapse branches | Parent-child relationships, spawning, and assignments or handoffs with explicit recipients; agent, team, parent, and evidence event IDs in details |
| Tasks | Filter by status or search, open details, and locate the agent | Turns or delegations, the executing agent, requester, start/end times, short result summaries, and attributed usage |
| Timeline | Filter by agent, task, and local start/end time; load events in batches | Structured lifecycle events, tool-call status, short summaries of logged public messages, usage, and context; source and collection timestamps, relative source paths, line numbers, and correlation IDs |
| Resources | Inspect each source's capabilities; change the usage dimension and open a group | Source connections and coverage, cumulative tokens, usage record counts, costs, and accounting basis |

**Replay at this time** in event details opens a historical snapshot. **View agent events**, **View task events**, and **Locate agent** connect the inspectors, timeline, and topology.

Agent states `running`, `waiting`, `completed`, `failed`, `interrupted`, and `unknown` require explicit lifecycle evidence. A tool failure describes that invocation. When DSH reports `completed` in `turn/end`, the turn completes and the agent becomes waiting; this does not establish permanent agent completion.

Source connections `connected`, `disconnected`, and `unavailable` separately describe log readability. A disconnected source preserves the last observed execution state. Silent logs do not imply completion, failure, or disconnection. Topology requires structured relationship evidence; hidden communication and unrecorded internal collaboration are not observable.

## Read usage in six dimensions

Select **Resources → Dimension**, then open a row for input, output, cache, reasoning, and cost details.

| Dimension | Grouping | How to read it |
| --- | --- | --- |
| Agent (`agent`) | Usage records owned by one agent | Child agents retain ownership of their own usage |
| Task (`task`) | Records with explicit turn, request, or delegation attribution | Unattributed records are not allocated by name or duration |
| Session (`session`) | Native session ID | Compare with the client's session records |
| Team (`team`) | Agents connected by explicit relationships | Inspect accumulated usage for the selected team |
| Client (`client`) | Source client | Codex Desktop and CLI logs with the same schema share the `codex` adapter group; this grouping cannot separate their process consumption |
| Model (`model`) | The reported model ID | Inspect reported model usage; missing models remain unknown |

These dimensions group the same records in six ways. Do not add the six summaries together. Values depend on team selection, replay time, source coverage, and retained history. Task groups may cover less than total agent usage when task attribution is missing.

### Token accounting

| Field | Meaning |
| --- | --- |
| Input | Normalized input, including cache reads and cache writes |
| Output | Includes any reasoning tokens reported by the source |
| Cache read / cache write | Subsets of input |
| Reasoning | A subset of output |
| Total | Input + output |

For example, input 100 and output 20, with cache read 60, cache write 10, and reasoning 8, still total **120**. Do not add the subsets again. No usage records displays `—`. Optional DSH subset counters may default to 0 under the adapter's documented accounting basis; inspect the event's **Usage basis**.

- Codex prefers `token_usage_record`, deduplicates globally by `response_id`, and checks the `thread_id` owner. Where per-response records exist, `token_count` is not added again. Legacy logs use cumulative differences with a baseline and expose coverage or attribution gaps. Copied parent history is not new child consumption.
- Native DSH v4 `inputTokens` counts uncached input. The adapter adds `cacheReadTokens` and `cacheWriteTokens` to normalize input. Revisions to the same step/attempt settlement slot replace earlier values by native `seq`; an explicit new attempt creates a new slot. Seeded sessions require an inherited-history boundary so inherited usage is not charged again. No real seeded-run sample was available in this acceptance run.

### actual / estimated / unknown costs

| Basis | Display | Meaning |
| --- | --- | --- |
| `actual` | Actual cost | Only a cost explicitly reported by the source; the Codex / DSH logs in this acceptance run do not provide invoices, so actual cost is unknown |
| `estimated` | Estimated cost | Uses an exact full model ID in XY Switch's saved prices and all rates required by that record; event details retain the price source and version |
| `unknown` | `—`, with a count of records with unknown cost | Model, exact price, or a required rate is missing; for example, cache writes occurred but no cache-write rate is available |

Estimates are displayed in USD. A known estimate subtotal can coexist with unknown-cost records and does not represent the full cost. Prices are not inferred from model-name fragments, and unknown costs are not replaced with zero. Actual and estimated costs are shown separately.

In the native ledger report, all 995 Codex and 362 DSH usage records have unknown costs; both `actualCostUsd` and `estimatedCostUsd` are `null`. The native UI checks cover cost categories and missing-value display. These results provide no reconciliation evidence for actual invoices or known estimated amounts.

## Understand the latest request context

**Context** in the agent inspector shows the latest source-reported request input footprint, a reported window size, **Measured**, and **Measurement basis**. Codex uses the latest request input; DSH uses the latest settled request input. Replay uses the latest measurement within its cutoff, preserving its `measuredAt` and `basis`. This is separate from cumulative token usage.

This is not live memory, CPU/RAM sampling, or hidden model state. Until the client writes a new measurement, the previous measurement keeps its original timestamp. Missing windows display **Window not reported**; missing measurements display **No context measurement**. The workspace does not infer a window or percentage from a model name.

## Sources and evidence coverage

The collector reads `sessions` and `archived_sessions` under the resolved Codex home and `sessions` under the resolved DSH home. These are usually `~/.codex` and `~/.dsh`; effective `CODEX_HOME` / `DSH_HOME` values affect their locations.

| Source | Current implementation | Evidence in this acceptance run |
| --- | --- | --- |
| Local Codex Desktop `0.159.0-alpha.12.1` | Bounded incremental JSONL collection; explicit parent threads, collaboration recipients, turns, and per-response records | This native build reconciled the selected historical snapshot: 44 agents, 995 responses, and 78,811,720 tokens; revalidation of the recorded real lifecycle also passed |
| Codex CLI logs with the same schema | Uses the Codex adapter and the structured fields actually present | No independent CLI process execution evidence this time; Desktop evidence does not establish a CLI runtime pass |
| Codex `archived_sessions` | Historical reading of recorded relationships and events | Historical evidence does not establish that a process is still running |
| DSH v4 plain JSONL | Bounded incremental reading; header `parentSession`, `turn/start`, `turn/end`, and step/attempt usage | Implementation and field contract checked; no independent live plain-JSONL execution evidence this time |
| DSH v4 `session.v4.jsonl.zstd` | Bounded historical decompression with complete-frame validation; resumes after file changes | This native build reconciled a real parent-child history sample: 2 source sessions, 362 settlement slots, and 101,738,633 tokens; the earlier large-line import blocker for this sample is resolved |

Ordinary logs use `delayed` collection; archives and compressed logs use `history`. Source details list capabilities and limitations. The DSH adapter accepts the audited v4 schema; experimental Team v2 communication is not integrated. Clients without an observation adapter do not acquire agents or relationships inferred from ordinary message text.

## Refresh, replay, and retention

- **Collection cadence:** Incremental checks run about every **2 seconds**, with new-file discovery about every **15 seconds**, plus client log-flush latency. File counts, read bytes, and events per pass are bounded; backlogs progress in batches. This is not a guarantee that an event appears within two seconds.
- **UI refresh:** Native event notifications trigger snapshot refreshes, with **30-second** reconciliation polling. **Refresh** retrieves already-collected data; it does not force the client to flush its logs. **Live observation** follows the latest collected events; each source still reports its `delayed` / `history` mode.
- **Compressed history:** zstd imports validated complete frames. Incomplete frames or lines wait for more data, subject to read budgets and decoder-window limits. Inspect source details for incremental progress, incomplete coverage, and parse diagnostics.
- **History replay:** Open **History replay**, choose a local time or drag the timeline, then play or pause at 1× / 10× / 60×. The backend reconstructs each frame from retained events. Source connections and last-check times still describe current collection. Use **Return to live** to follow new events.
- **Replay and date filters (R7):** The top-level `scope.at` in an event request independently selects the team-membership cutoff: pass the original `replayAt` for replay, or `{ at: null }` for current membership. `query.since` / `query.until` filter event dates only. During replay, the event upper bound is `min(until, at)`, or `at` when `until` is absent. Changing replay time or filters restarts pagination. Legacy calls that omit the entire `scope` still use `until` as the membership cutoff; that compatibility path cannot express an independent date filter.
- **Retention:** The default is **30 days**, with a global maximum of **200000 normalized event records** in the local observation store. Pruning uses source event timestamps; newly imported old logs may already fall outside the retention period. Either limit can shorten the replayable range. The footer's **Observation notes** shows retention, available history, and truncation. Pruning affects only XY Switch's derived observation records, not original client files.

## Read-only access and credential redaction

Agent teams reads client logs and has no actions to create, stop, control agents, or send task messages. It reuses XY Switch's local SQLite database, Tauri IPC, and model price catalog.

Logs are parsed during collection. Persistence is limited to allowlisted metadata, counts, and short summaries. Summaries and displayable fields are sanitized for credentials, including tokens, passwords, authorization information, and URL credentials. Full prompts, tool arguments/results, and reasoning text are not stored as observation events. Logged public-message summaries do not expose hidden reasoning or hidden communication.

## Troubleshoot missing or inconsistent data

| Symptom | Where to check and what to do |
| --- | --- |
| Native desktop app required | Open `#agents` in the XY Switch desktop app; browser preview has no local observation source |
| Source unavailable or no agents | In **Resources**, inspect source readability, file counts, mode, and diagnostics; allow discovery and batched import, then check the selected team and replay time |
| Source disconnected but an agent keeps its status | Restore source readability and inspect the last check; later lifecycle events update the agent state |
| Last snapshot shown or notification connection interrupted | Check the last successful read and displayed range, then use **Retry / Refresh**; low-rate polling reconciles missed notifications |
| Tokens differ from another total | Align team, native session, cutoff, retention, and usage basis; check response deduplication, inherited history, settlement revisions, and unknown task attribution |
| Cost or context is `—` | Inspect exact model prices, unknown-cost records, measurement time, and whether a window was reported; missing values do not mean zero consumption |
| Parent-child or collaboration edges missing | Check for explicit parent-session or recipient IDs; names, message text, and a shared working directory do not establish a relationship |

## Acceptance and known limitations

Earlier isolated Windows native acceptance covered source-ledger reconciliation, explicit parent relationships, event/lifecycle ordering, source disconnect/reconnect, restart recovery, six resource dimensions and the responsive interface. Raw source records and operator-specific telemetry remain in the local evidence archive. See the [1.3.0 release acceptance](channel-groups-acceptance-1.3.0.md) for current suite results and the scope of the new native run.

Existing persisted lifecycle timestamps are not automatically migrated. A cursor blocked by an old `sequence_gap` does not reread an unchanged file solely because the line-size limit increased. The earlier R10 acceptance used a fresh isolated observation database; no automatic repair of an existing observation database is claimed.

Independent Codex CLI process execution, a live DSH plain-JSONL task, a real seeded-run sample, DSH Team v2, hidden state, invoice verification and cross-platform Agent execution remain unverified. Missing costs stay unknown.
