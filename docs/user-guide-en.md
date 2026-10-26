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

**Sessions:** full native conversation management is not implemented. Native list/message commands return empty lists and deletion returns a not-implemented error. Do not interpret synthetic renderer test fixtures as native session support. Local usage import, described below, is a separate feature.

## Read usage and model-response evidence

The usage page combines proxy request records with supported local usage imports, including Codex and DeepSeek Harness. Request records can expose token, cache, latency, status, model, and cost evidence when that information actually exists. Local session usage does not manufacture HTTP status, request latency, or real billing evidence.

Missing price or usage information is **unknown**, not a zero-cost request. Declared model names and protocol/tool behavior do not authenticate model weights. Active verification may send additional requests and incur cost. Its confirmation budget is a soft control for that verification mechanism (default 100, range 0–10,000), **not a hard cap on every upstream bill**.

## Back up configuration and find data

In **Settings → Data**, inspect paths returned by the running native process. Browser preview does not provide native data paths. Export ordinary configuration before an import; review the confirmation because matching IDs can update existing records and routing configuration can be imported.

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
