# XY Switch user guide

English | [简体中文](user-guide-zh.md) · [README](../README.md) · [Build and package](packaging-en.md)

This guide describes **1.2.0** desktop features; unreleased source updates are labeled separately. Current support is **Windows, macOS, and Linux only**. Native and browser-preview capabilities differ. Windows installers are unsigned; macOS packages are ad-hoc signed without notarization. Earlier mobile assets remain historical and no longer have a current build entry point. See the matching release verification report for actual test coverage.

## Navigation in current source (2026-11-10)

The app opens on the client directory. The top bar provides **Client / Providers / Sessions / Tools / Usage**, and the brand button returns to Overview. **All features** at the top right opens a right drawer containing every destination. At widths of 800px and below, the main navigation collapses; use this menu to switch pages, and it closes after selection. These paths describe current source; older installers retain their original layout.

| Entry | Main tasks |
| --- | --- |
| Client | Client directory, shared configuration, dedicated details, account and diagnostic actions |
| Providers | Connection settings, current-provider selection, and focused default-model editing |
| Sessions | Search, preview, and dedicated full-conversation reading |
| Tools | MCP servers, Prompts, Skills, Local plugins, and Plugin marketplace |
| Usage | Overview, Requests, Audit analysis, and Audit setup |
| Settings / All features at the top right | Settings, Account pool, Connection diagnostics, Safety & Acceptance, and all existing pages |
| Workspace menu beside the brand | Configuration workspace and the read-only Agent teams workspace (`#agents`); see the [Agent teams guide](agent-fleet-en.md) |

Expand runtime status at the top right to inspect routing, the listener, target clients, and the configured provider; native start/stop actions are available there. Configuration selection, listener runtime, and historical connection results remain separate: cached success does not establish current connectivity. Change the theme at the bottom of **All features**, or in **Settings → General**. Returning to the configuration workspace restores the previous configuration page. Agent teams provides read-only topology, tasks, timeline, and resources; DSH native reruns and recovery acceptance are still in progress.

### Page layouts

| Page | Workspace layout |
| --- | --- |
| Overview | Client → provider → model configuration chain, provider choices, historical diagnostic evidence, and shortcuts to continue working |
| Providers | Searchable directory → addressable configuration detail. Names and **Configuration** open details; **Set as current** switches providers; model names open focused editing |
| Client connections | Compact directory → dedicated detail, preserving filters, position, and focus on return. Shared groups, login, account binding, and path settings have explicit entry points |
| Account pool | Saved policy, participation, authorization, and quota grouped by provider; gateway accounts and native references remain separate, with dedicated detail and policy dialogs |
| Connection diagnostics | Provider selection and separate connection-test/model-review views; opening the page does not send an upstream request |
| Tools | Each tool starts with its own directory and explicit detail, target-client, or source-management task. Local plugins and Plugin marketplace remain separate, with installation and permission confirmation |
| Sessions | Directory → preview → full reader, with an optional **Expand reader** action; returning restores filters, position, and focus |
| Usage & audit / Safety & Acceptance | Requests, success rate, and proxy latency appear first; failure counts open filtered requests, missing cost is labeled unknown, and safety controls remain separate |

### Grouped settings

Open **Settings** from the top right or **All features**. Its task directory groups categories by purpose and shows their current state and when changes take effect. Select a task to open its settings:

| Settings group | Categories |
| --- | --- |
| Local preferences | General, Privacy |
| Connections and credentials | Local routing, Credentials |
| Configuration and recovery | Data, Encrypted Sync |
| Application information | About |

Use **Back to settings tasks** to return to the directory and restore focus to its entry. Each form keeps its own save flow; unsaved changes and pending operations have navigation protection, and imports, permission changes, and deletions retain their previews and confirmations.

## Install and choose a language

Download your platform's package and `SHA256SUMS.txt` from [Releases](https://github.com/MiyanoShihoo/XY-switch/releases). Compare the downloaded file's SHA-256 before installing. Windows offers NSIS EXE/MSI, Linux DEB/AppImage, macOS Universal App ZIP/DMG. There is no Windows portable ZIP in this release.

Open **Settings → General → Interface language** and choose **System**, **简体中文**, or **English**. System mode uses the first preferred system/browser language: a `zh` prefix selects Chinese; anything else selects English. Native preferences are stored in the local database and refresh the desktop tray. Browser previews use local storage. A failed native save keeps the previous language; retry after resolving the displayed error. Changing language does not discard the current page's unsaved form values.

Only application-owned labels and messages are translated. Provider names, prompt/Skill content, conversation text, model IDs, API payloads, and raw diagnostics keep their original values. Language preference changes do not alter upstream requests or model selection.

## Connect an upstream provider

1. Open **Providers** from the top bar or **All features**, then add a preset or custom provider. Use search or the All / Enabled / Disabled filters to find existing providers.
2. Enter the endpoint, API credential, and model settings expected by that upstream. Credentials are stored separately from normal configuration.
3. Save. Run connection testing or model discovery as appropriate for the endpoint; a successful save alone does not prove the upstream works.
4. Use **Set as current** to switch to an enabled provider. Its name or **Configuration** opens details; the model name opens the default-model editor. Check the current-provider indicator before sending requests. Open connection checks from client details or **All features → Connection diagnostics**.
5. Use model mappings and advanced request header/body overrides only when needed. Keep model identifiers exact; discovery lists what the upstream returned, not every possible model.

A provider can be saved, enabled, current, and reachable in different combinations. Treat each indicator independently. Error details may contain upstream text and are not translated or rewritten as proof of success.

## Route requests and manage desktop clients

The local proxy starts disabled. Configure the local listener and routing, then explicitly enable the proxy when you want to use adapters for OpenAI Chat Completions, Responses, Anthropic Messages, or Gemini. Point your own client at the displayed local endpoint, or use supported desktop takeover.

Takeover/restoration is implemented for **Claude Code, Codex, and Gemini CLI**. Other client groups shown in the UI do not imply they have a takeover adapter. Review the target configuration before enabling takeover and explicitly restore it when finished. The desktop tray provides **Show main window**, provider selection, and **Quit**; it is not a proxy-toggle menu.

Protocol conversion, accepted tool calls, and a matching model name establish compatibility evidence, not the identity of the upstream model's weights. Retry/failover can affect the upstream actually used; inspect request evidence rather than relying only on the saved provider.

## Manage MCP, prompts, and Skills

Open these pages from the **Tools** menu or **All features**, alongside **Local plugins** and **Plugin marketplace**. Each directory leads to configuration, document, or resource details. MCP provides target-client controls, and Skills keeps folder imports and platform-source authorization under **Add and manage sources**. Installation and permission changes retain their review and confirmation steps.

**MCP:** create or edit server definitions, choose supported target clients, and use the explicit import/synchronization actions. Desktop client synchronization writes client configuration. A server definition being saved does not establish that its executable, transport, or external service is available.

**Prompts:** create, edit, delete, and enable/disable local Markdown prompts. Version 1.1.0 does not synchronize rule files; 1.2.0 adds the explicit-mount workflow below. Saving or enabling a prompt still **never automatically** writes `CLAUDE.md`, `AGENTS.md`, or `GEMINI.md`.

**Skills:** desktop source switches start **off**. Enable only the sources you want read, or add a manual reference. Scanning is recursive, includes `.system`, and excludes plugin caches. Removing a manual reference removes its registration, not the referenced source files. Application-owned controls are translated; Skill names and source content are not.

## Use safety and acceptance (1.2.0)

This section describes current desktop source, not the published 1.1.0 installers. Open **All features → Safety & Acceptance**:

1. **Access control:** Stop the proxy and restore active takeovers before changing policy. Legacy mode does not require authentication by default. After enabling it, issue `proxy` tokens for model clients or `monitor` tokens for read-only status. Tokens are shown once and support expiry/revocation. Failed authentication counts toward the shared request limit; the minimal `/health` liveness check is exempt.
2. **Hard budget:** First save an exact custom price for the dispatched model ID in **Usage & audit → Model pricing**. Confirm daily token/cost reservations, per-attempt input/output limits, and per-task attempt/cost limits. Retries, failover, and model probes reserve too; failures, cancellation, and missing usage do not refund. Daily limits use UTC days; task totals survive day changes, restarts, and policy edits. This is local admission accounting, not an invoice guarantee; unsupported multimodal/remote-history/built-in-tool payloads and native Gemini upstreams fail closed.
3. **Acceptance monitoring:** Enable the budget, run the proxy, choose a provider/model/mode, and type `ALLOW PAID CHECKS` before a paid manual check or enabled schedule. Modes cover Chat, SSE, tools, Responses, Anthropic, and Gemini client paths. Scheduling defaults off, with a 30-minute interval and 10 checks per UTC day shared by manual/scheduled runs. Alerts are recorded on failure/recovery transitions, not repeatedly for an unchanged state. Execution is not guaranteed while the app is closed or suspended.
4. **Config versions:** Create snapshots manually. To restore, stop the proxy, restore takeovers, preview the diff, acknowledge it, and type `ROLLBACK`. The transaction backs up the current configuration first; stale previews are rejected. Snapshots exclude the credential vault, MCP, favorites, external files, and security policies; they are not complete backups.
5. **Explicit mounts:** Grant a project through the native folder picker, choose saved prompts/authorized local Skills and one of the three root rule files, then preview without writing. Review the diff and type `WRITE FILE`. Previews expire in five minutes and are single-use; complete original content is backed up before atomic replacement. Intervening file changes block stale writes or restores. Only `SKILL.md` text is mounted; scripts and dependencies are not copied, executed, or installed.

### Task budgets and cancellation (source update, 2026-11-02)

- Each task defaults to **12 upstream attempts and USD 0.25 committed cost**. Zero blocks new attempts. New tasks created with the hard budget disabled have no limits; protected existing tasks retain theirs. Lowering policy tightens existing tasks, and raising or disabling policy does not reset totals or raise their retained limits.
- Application retries, failover, client retries, and model rechecks share the task ledger. Codex supplies conversation/root-turn metadata. Other clients should reuse the same `X-XY-Task-ID` or `Idempotency-Key` for every request in one task; the two headers must agree when both are supplied. Missing, invalid, or conflicting identity is rejected before dispatch with the hard budget enabled. Access-token scopes remain separate, and identifiers are stored as digests.
- **Hard budget → Recent tasks** shows the latest 50 tasks and refreshes every five seconds. Details include totals, limits, each send/client request ID, provider, model, route, status, reservation, known cost, and usage completeness. Confirm cancellation to stop the task's concurrent requests and future retries. Late responses and restarts do not revive a cancelled task.
- Unknown or incomplete cost keeps its full reservation, and known cost below the reservation does not refund it. Known overages increase committed cost and block subsequent attempts that would exceed the limit. Providers settle the final bill for requests already dispatched.
- The task totals in **Usage & audit → Requests → request details** are snapshots taken when each record was produced; use Recent tasks for current state. HTTP/1 TCP read EOF, including a client write half-close, currently cancels the task. Continuing to receive a response after write half-close is not implemented.

See the [task budget acceptance report](task-budget-acceptance-20261102.md) for scope and evidence. Remote-device connections are not implemented. Browser preview never simulates successful privileged writes. Session recovery still only copies commands. See the [governance and acceptance record](governance-acceptance-20261029.md), including the live HTTP failure and remaining limits.

## Browse local sessions (1.2.0)

Open **Sessions** from the top bar or **All features**. Select a directory row to preview it, then use **Read from start** to enter the dedicated reader. Search, client, project, and favorite filters are in the directory toolbar; **Refresh index** is in the directory header. The reader keeps the title, favorite action, and resume command near the conversation, with source information available on demand. **Expand reader** opens a larger dialog and returns to the same reading position when closed. **Back to directory** restores filters, scroll position, and entry focus; narrow windows use the same flow. **Session information and actions** expands source details and deletion separately.

The current desktop source supports Codex, Claude Code, and Gemini CLI transcript logs. This replaces the old empty native endpoints; it is not included in the published 1.1.0 packages.

- Codex: `CODEX_HOME/sessions` and `CODEX_HOME/archived_sessions`, defaulting to `~/.codex`. Claude: `CLAUDE_CONFIG_DIR/projects/<project>/*.jsonl`, with `CLAUDE_CONFIG_DIR` defaulting to `~/.claude`. Gemini: `GEMINI_CLI_HOME/.gemini/tmp/<project>/chats/session-*.json`; the override replaces HOME, not the `.gemini` directory.
- Search by title, ID, or working directory and filter by client. Message details include supported text and tool calls/results, not images or hidden reasoning. Codex `session_index.jsonl` can supply renamed titles for already-discovered logs, never new file paths. The index is limited to 4 MiB / 20,000 rows; malformed indexes produce warnings without blocking valid log reading.
- A resume command is copied, never executed. First switch to the recorded working directory, then run it yourself. The matching client must still be installed and its own session state must be intact.
- Deletion permanently removes only the selected log file. Stop the session first. The original client’s SQLite/database/index is not rewritten, so it may retain a stale entry. Ambiguous IDs, unsafe paths, corrupt logs, or incomplete scans prevent deletion.
- A file is limited to 32 MiB, a catalog scan to 64 MiB of reads, and a detail view to 4 MiB / 5,000 messages. Partial or damaged content is reported instead of shown as an empty success. Browser previews explicitly reject native session operations.

Local usage import below is a separate feature.

### Favorites and quick filters (current source)

- Open a session and use **Favorite session** in the reading header; the expanded reader provides the same action. Favorites appear first and show a star in the list, with recent activity ordering inside each group. Favorites use the client and session ID together, so identical IDs from different clients stay independent.
- Use the star filter in the directory toolbar to show **Favorites only**, and combine it with client and title/summary/ID/directory search. Its count includes only favorites present in the current catalog. Press `Esc` in the search box to clear just the text; **Clear filters** resets all filters and returns focus to search.
- Favorites are stored locally in the app's WebView storage. Only client/session identifiers are added, never titles, directories, source paths, or message bodies; original session logs are unchanged. Windows sharing the same storage origin synchronize changes. There is no cross-device sync.
- Storage failures or an unrecognized format show a retryable error. Unsaved changes last only on the current page; retry before navigating away or reloading. Unrecognized saved data is not overwritten.
- A missing catalog entry does not erase its favorite. Only confirmed successful deletion removes it; errors and zero-count deletions preserve it. Removing the selected session from favorites while **Favorites only** is active clears the selection, exits reading, closes its expanded dialog if open, and returns focus to the filter button.

This is a source-branch feature, not an update to existing 1.1.0 installers. See the [favorites verification notes](session-favorites-20261029.md) for reproducible checks.

## Read usage and model-response evidence

Open **Usage** from the top bar or **All features → Usage & audit**, then choose **Overview**, **Requests**, **Audit analysis**, or **Audit setup**. Requests, success rate, and proxy latency appear first; selecting the failure count opens filtered failed requests. Open a request for detailed evidence, preserving filters and position on return. Use the page-header **Model pricing** action for price management. Costs and tokens are separate, missing cost evidence is labeled unknown, and import controls and longer explanations expand on demand.

The usage page combines proxy request records with supported local usage imports, including Codex and DeepSeek Harness. Request records can expose token, cache, latency, status, model, and cost evidence when that information actually exists. Local session usage does not manufacture HTTP status, request latency, or real billing evidence.

Missing price or usage information is **unknown**, not a zero-cost request. Declared model names and protocol/tool behavior do not authenticate model weights. Active verification may send additional requests and incur cost. Its confirmation budget is a soft control for that verification mechanism (default 100, range 0–10,000), **not a hard cap on every upstream bill**.

### Manage model prices (1.2.0)

1. Open **Usage & audit → Model pricing**. Enter the exact model ID returned by the upstream and its input/output rates in USD per million tokens. Leave cache-read/write rates empty for unknown; `0` means free. Rates must be finite numbers from 0 to 1,000,000.
2. Save to local SQLite. Custom rates match the full model ID exactly and are shared across providers using that ID; they are not provider-specific. Changes apply only to future requests and newly imported records. Historical costs and component snapshots are not recalculated.
3. Use **Sync reference prices from models.dev** for public reference data. The fixed HTTPS endpoint receives no credentials and redirects are rejected. Failure retains the previous catalog and custom rates. Only supported text-token rates are imported; media and tiered prices are not converted.
4. Reference IDs retain the `provider/model` prefix and do not automatically match a bare model name. Search/filter the catalog, choose **Use reference price**, confirm or edit the model ID, then save a custom rate. Lists are paginated at 25 rows.
5. Removing a custom price requires confirmation and affects only future records. Existing built-in family rates remain approximate fallbacks, not provider bills; unavailable rates or required usage remain unknown.

Codex usage import handles per-event and cumulative counters, including incremental cache writes. Missing, negative, malformed or indeterminate counters are skipped with a warning instead of treated as free requests. Failed writes roll back both events and scan checkpoints for retry. Browser previews explicitly reject native price writes and synchronization.

## Back up configuration and find data

In **Settings → Data**, inspect paths returned by the running native process. Browser preview does not provide native data paths. Export ordinary configuration before an import; review the confirmation because matching IDs can update existing records and routing configuration can be imported.

Version 1.2.0 exports v2 configuration with custom prices and still accepts v1 backups; v1 cannot contain nonempty `modelPricing`. Public reference catalogs, historical usage and cost snapshots are excluded. Imports/exports are limited to 8 MiB, with 1,000 providers/prompts/MCP servers per kind and a 30,000-row pricing catalog limit. All supported sections are validated before mutation; provider, prompt and custom-price rows commit together in one SQLite transaction. MCP synchronization and proxy/listener changes are external operations, not an all-resource transaction. An I/O error reports partial progress and refreshes the displayed state. Imported MCP secret values are ignored; existing local environment/header credentials are preserved for matching server IDs. New devices require credentials to be entered again.

Normal configuration export is **not a complete credential backup**. Credentials use Windows Credential Manager, Linux Secret Service, or macOS Keychain. Keep source credentials available for re-entry when moving devices. Removing an app's data or uninstalling it can remove stored settings and credentials.

## Platform limits and troubleshooting

- **Browser preview:** UI review only. It does not replace native keyring, filesystem, listener, or takeover operations.
- **Linux:** requires compatible system libraries and an unlocked Secret Service keyring. Tray support depends on the desktop environment.
- **iPhone installation:** the simulator ZIP cannot be installed on a physical phone. A device build needs Apple signing and matching provisioning; see [packaging](packaging-en.md).
- **Language save failed:** the old language is intentionally retained. Retry instead of assuming the tray and persisted preference changed.
- **Upstream failure:** distinguish saved configuration, active provider, listener state, authentication, protocol support, and actual response. Preserve relevant diagnostics without publishing credentials or private request content.

For a reproducible report, include the app version, OS/architecture, native versus preview mode, steps, expected/actual behavior, and redacted errors. Historical dated reports describe their own source snapshot, not every later release.
