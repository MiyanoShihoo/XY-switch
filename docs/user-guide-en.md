# XY Switch user guide

English | [简体中文](user-guide-zh.md) · [README](../README.md) · [Build and package](packaging-en.md)

This guide describes **1.1.0**. Native and browser-preview capabilities are different. Windows installers are unsigned; macOS packages are ad-hoc signed without notarization; the Android APK is development-signed; the published iOS ZIP is for an ARM64 simulator, **not an iPhone IPA**. See the release's verification report for the platforms actually tested.

## Install and choose a language

Download your platform's package and `SHA256SUMS.txt` from [Releases](https://github.com/MiyanoShihoo/XY-switch/releases). Compare the downloaded file's SHA-256 before installing. Windows offers NSIS EXE/MSI, Linux DEB/AppImage, macOS Universal App ZIP/DMG, Android an APK, and iOS a simulator ZIP. There is no Windows portable ZIP in this release.

Open **Settings → General → Interface language** and choose **System**, **简体中文**, or **English**. System mode uses the first preferred system/browser language: a `zh` prefix selects Chinese; anything else selects English. Native preferences are stored in the local database and refresh the desktop tray. Browser previews use local storage. A failed native save keeps the previous language; retry after resolving the displayed error. Changing language does not discard the current page's unsaved form values.

Only application-owned labels and messages are translated. Provider names, prompt/Skill content, conversation text, model IDs, API payloads, and raw diagnostics keep their original values. Language preference changes do not alter upstream requests or model selection.

## Connect an upstream provider

1. Open **Providers**, choose the relevant client group, and add a preset or custom provider.
2. Enter the endpoint, API credential, and model settings expected by that upstream. Credentials are stored separately from normal configuration.
3. Save. Run connection testing or model discovery as appropriate for the endpoint; a successful save alone does not prove the upstream works.
4. Select an enabled provider as current. Clicking an enabled, non-current provider card both inspects **and activates** it; the details icon only inspects. Check the current-provider indicator before sending requests.
5. Use model mappings and advanced request header/body overrides only when needed. Keep model identifiers exact; discovery lists what the upstream returned, not every possible model.

A provider can be saved, enabled, current, and reachable in different combinations. Treat each indicator independently. Error details may contain upstream text and are not translated or rewritten as proof of success.

## Route requests and manage desktop clients

The local proxy starts disabled. Configure the local listener and routing, then explicitly enable the proxy when you want to use adapters for OpenAI Chat Completions, Responses, Anthropic Messages, or Gemini. Point your own client at the displayed local endpoint, or use supported desktop takeover.

Takeover/restoration is implemented for **Claude Code, Codex, and Gemini CLI**. Other client groups shown in the UI do not imply they have a takeover adapter. Review the target configuration before enabling takeover and explicitly restore it when finished. The desktop tray provides **Show main window**, provider selection, and **Quit**; it is not a proxy-toggle menu.

Protocol conversion, accepted tool calls, and a matching model name establish compatibility evidence, not the identity of the upstream model's weights. Retry/failover can affect the upstream actually used; inspect request evidence rather than relying only on the saved provider.

## Manage MCP, prompts, and Skills

**MCP:** create or edit server definitions, choose supported target clients, and use the explicit import/synchronization actions. Desktop client synchronization writes client configuration; it is unavailable on mobile. A server definition being saved does not establish that its executable, transport, or external service is available.

**Prompts:** create, edit, delete, and enable/disable local Markdown prompts. This version does **not** synchronize prompt content into `CLAUDE.md`, `AGENTS.md`, or `GEMINI.md`.

**Skills:** desktop source switches start **off**. Enable only the sources you want read, or add a manual reference. Scanning is recursive, includes `.system`, and excludes plugin caches. Removing a manual reference removes its registration, not the referenced source files. Application-owned controls are translated; Skill names and source content are not.

## Browse local sessions (unreleased maintenance, 2026-10-27)

The current desktop source supports Codex, Claude Code, and Gemini CLI transcript logs. This replaces the old empty native endpoints; it is not included in the published 1.1.0 packages.

- Codex: `CODEX_HOME/sessions` and `CODEX_HOME/archived_sessions`, defaulting to `~/.codex`. Claude: `CLAUDE_CONFIG_DIR/projects/<project>/*.jsonl`, with `CLAUDE_CONFIG_DIR` defaulting to `~/.claude`. Gemini: `GEMINI_CLI_HOME/.gemini/tmp/<project>/chats/session-*.json`; the override replaces HOME, not the `.gemini` directory.
- Search by title, ID, or working directory and filter by client. Message details include supported text and tool calls/results, not images or hidden reasoning. Codex `session_index.jsonl` can supply renamed titles for already-discovered logs, never new file paths. The index is limited to 4 MiB / 20,000 rows; malformed indexes produce warnings without blocking valid log reading.
- A resume command is copied, never executed. First switch to the recorded working directory, then run it yourself. The matching client must still be installed and its own session state must be intact.
- Deletion permanently removes only the selected log file. Stop the session first. The original client’s SQLite/database/index is not rewritten, so it may retain a stale entry. Ambiguous IDs, unsafe paths, corrupt logs, or incomplete scans prevent deletion.
- A file is limited to 32 MiB, a catalog scan to 64 MiB of reads, and a detail view to 4 MiB / 5,000 messages. Partial or damaged content is reported instead of shown as an empty success. Browser previews and mobile explicitly reject native session operations.

Local usage import below is a separate feature.

## Read usage and model-response evidence

The usage page combines proxy request records with supported local usage imports, including Codex and DeepSeek Harness. Request records can expose token, cache, latency, status, model, and cost evidence when that information actually exists. Local session usage does not manufacture HTTP status, request latency, or real billing evidence.

Missing price or usage information is **unknown**, not a zero-cost request. Declared model names and protocol/tool behavior do not authenticate model weights. Active verification may send additional requests and incur cost. Its confirmation budget is a soft control for that verification mechanism (default 100, range 0–10,000), **not a hard cap on every upstream bill**.

### Manage model prices (unreleased maintenance)

1. Open **Usage & audit → Model pricing**. Enter the exact model ID returned by the upstream and its input/output rates in USD per million tokens. Leave cache-read/write rates empty for unknown; `0` means free. Rates must be finite numbers from 0 to 1,000,000.
2. Save to local SQLite. Custom rates match the full model ID exactly and are shared across providers using that ID; they are not provider-specific. Changes apply only to future requests and newly imported records. Historical costs and component snapshots are not recalculated.
3. Use **Sync reference prices from models.dev** for public reference data. The fixed HTTPS endpoint receives no credentials and redirects are rejected. Failure retains the previous catalog and custom rates. Only supported text-token rates are imported; media and tiered prices are not converted.
4. Reference IDs retain the `provider/model` prefix and do not automatically match a bare model name. Search/filter the catalog, choose **Use reference price**, confirm or edit the model ID, then save a custom rate. Lists are paginated at 25 rows.
5. Removing a custom price requires confirmation and affects only future records. Existing built-in family rates remain approximate fallbacks, not provider bills; unavailable rates or required usage remain unknown.

Codex usage import handles per-event and cumulative counters, including incremental cache writes. Missing, negative, malformed or indeterminate counters are skipped with a warning instead of treated as free requests. Failed writes roll back both events and scan checkpoints for retry. Browser previews explicitly reject native price writes and synchronization.

## Back up configuration and find data

In **Settings → Data**, inspect paths returned by the running native process. Browser preview does not provide native data paths. Export ordinary configuration before an import; review the confirmation because matching IDs can update existing records and routing configuration can be imported.

The unreleased source exports v2 configuration with custom prices and still accepts v1 backups; v1 cannot contain nonempty `modelPricing`. Public reference catalogs, historical usage and cost snapshots are excluded. Imports/exports are limited to 8 MiB, with 1,000 providers/prompts/MCP servers per kind and a 30,000-row pricing catalog limit. All supported sections are validated before mutation; provider, prompt and custom-price rows commit together in one SQLite transaction. MCP synchronization and proxy/listener changes are external operations, not an all-resource transaction. An I/O error reports partial progress and refreshes the displayed state. Imported MCP secret values are ignored; existing local environment/header credentials are preserved for matching server IDs. New devices require credentials to be entered again.

Normal configuration export is **not a complete credential backup**. Credentials use Windows Credential Manager, Linux Secret Service, Apple Keychain, or Android Keystore with private AES-GCM records. Keep source credentials available for re-entry when moving devices or replacing an Android build signed by a different certificate. Removing an app's data or uninstalling it can remove stored settings and credentials.

## Platform limits and troubleshooting

- **Browser preview:** UI review only. It does not replace native keyring, filesystem, listener, or takeover operations.
- **Linux:** requires compatible system libraries and an unlocked Secret Service keyring. Tray support depends on the desktop environment.
- **Mobile:** manages its own sandbox; cannot take over another app's CLI configuration or read its Skills/session directories. Background proxy availability is not guaranteed.
- **Android upgrade refused:** independently generated development certificates may differ. Do not assume one release can install over another; preserve ordinary configuration and prepare credentials before changing installations.
- **iPhone installation:** the simulator ZIP cannot be installed on a physical phone. A device build needs Apple signing and matching provisioning; see [packaging](packaging-en.md).
- **Language save failed:** the old language is intentionally retained. Retry instead of assuming the tray and persisted preference changed.
- **Upstream failure:** distinguish saved configuration, active provider, listener state, authentication, protocol support, and actual response. Preserve relevant diagnostics without publishing credentials or private request content.

For a reproducible report, include the app version, OS/architecture, native versus preview mode, steps, expected/actual behavior, and redacted errors. Historical dated reports describe their own source snapshot, not every later release.
