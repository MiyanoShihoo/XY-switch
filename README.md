# XY Switch

English | [简体中文](README.zh-CN.md)

A local AI configuration and protocol-routing workbench built with Tauri 2, Rust, React, and TypeScript. Manage providers, model mappings, MCP servers, prompts, and local Skills; inspect proxy usage and model-response evidence without repeatedly editing client configuration.

**Version 1.1.0 adds English and Simplified Chinese interfaces.** Choose **System / 简体中文 / English** in **Settings → General**. Native preferences are persisted locally and update the desktop tray; browser previews use local storage. User names, prompts, Skills, conversation content, model IDs, and raw diagnostics are not translated.

## Download and platform scope

Get packages, checksums, native screenshots, and the verification report from [GitHub Releases](https://github.com/MiyanoShihoo/XY-switch/releases). Source archives alone are not installers.

| Platform | Distribution | Credential storage | Important scope |
| --- | --- | --- | --- |
| Windows x64 | NSIS `.exe` / `.msi` | Windows Credential Manager | Unsigned installers; WebView2 required; no portable ZIP in this release |
| Linux x64 | `.deb` / `.AppImage` | Secret Service | Built on Ubuntu 22.04; needs compatible libraries and an unlocked keyring; tray availability depends on the desktop |
| macOS Intel + Apple Silicon | Universal `.app.zip` / `.dmg` | Apple Keychain | Ad-hoc signed, not Developer ID signed or notarized |
| Android 7.0+ / API 24+ | ARM64 + x86_64 universal `.apk` | Android Keystore + private AES-GCM records | Development-signed distribution; manages only its own sandbox; background proxy lifetime is not guaranteed |
| iOS | ARM64 simulator `.zip`; separate signed-device build entry | Apple Keychain | **The simulator ZIP is not an iPhone IPA.** Device distribution requires Apple signing and a matching provisioning profile |

The Android CI runs an isolated x86_64 emulator; static ARM64 package checks do not establish real-device testing. Minimum build targets are not a claim that every OS/device combination has been tested. Development certificates can differ between Android builds, so in-place upgrades are not guaranteed. See [packaging and native verification](docs/packaging-en.md) for exact commands and signing limits.

## What is implemented

| Area | Delivered behavior | Boundary |
| --- | --- | --- |
| Providers and models | Endpoint/credential management, active-provider selection, discovery, mapping, header/body overrides | Saving, activating, connection testing, and successful upstream requests are different states |
| Protocol routing | Local proxy adapters for OpenAI Chat Completions, Responses, Anthropic Messages, and Gemini | Protocol/tool compatibility and declared model names are not proof of model-weight identity |
| Desktop client takeover | Explicit configuration takeover and restoration for **Claude Code, Codex, and Gemini CLI** | Other displayed client groups do not imply takeover support; mobile cannot edit other apps’ CLI configuration |
| MCP | Server management and supported client import/synchronization workflows | Platform permissions and client support still apply |
| Prompts | Local create/edit/delete and enable/disable operations | Does **not** write prompts into `CLAUDE.md`, `AGENTS.md`, or `GEMINI.md` |
| Skills | Opt-in desktop source scanning and manual references | Sources start disabled; removing a manual reference does not delete its source files; plugin caches are excluded |
| Usage and evidence | Proxy request/token/cache/cost records, response/model evidence, Codex and DeepSeek Harness local-usage reading | Missing price/usage stays unknown; imported session usage cannot provide real HTTP latency or billing evidence |
| Language and settings | English/Simplified Chinese, system-language mode, themes, configuration import/export, actual data paths | Ordinary exports are not full credential backups |

**Full native conversation management is not delivered:** session-list and message-query commands currently return empty lists, and deletion returns an explicit not-implemented error. Synthetic UI fixtures are used for renderer testing, not evidence of native session support.

## Get started

1. Install the package for your platform and open the native app. Browser preview is not a replacement for native credential, filesystem, or proxy operations. The proxy starts disabled.
2. In **Settings → General**, choose your language. System mode uses the first preferred system/browser language: Chinese if it starts with `zh`, English otherwise. A failed native preference write keeps the previous language and offers retry.
3. Add a provider, enter its endpoint and credential, save, then run the connection/model checks you need. Clicking an enabled non-current provider card inspects **and activates** it; the details icon only inspects. Use the active-state indicator rather than assuming a click is read-only.
4. Enable the local proxy and desktop client takeover only when needed. Check the selected upstream, running proxy, client configuration, and actual request result separately. Some clients must restart before loading changed configuration.
5. For desktop Skills, explicitly enable the desired source in settings, preview the scan, then confirm import. Recursive scanning includes `.system`; it excludes plugin cache directories.

See the [English user guide](docs/user-guide-en.md) / [中文使用指南](docs/user-guide-zh.md) for resource management, model verification, backups, and troubleshooting.

## Data, credentials, and model verification

Configuration is local. Provider API keys use native credential storage, not the ordinary configuration JSON. Android stores encrypted records in `noBackupFilesDir`; damaged records, missing keys, and unavailable keyrings fail rather than silently falling back to plaintext. Hardware-backed storage is not guaranteed on every device.

Inspect actual paths in **Settings → Data**. Exported configuration excludes provider keys and sensitive MCP headers; re-enter credentials after migration. Takeover restoration affects the configuration managed by that operation, not all application data.

Model-response evidence describes observable fields and protocol behavior, not cryptographic verification of a model’s weights. Unknown cost is not zero. The active-verification token budget is a **soft budget**, default 100 and range 0–10,000; it applies to that mechanism, not a global hard billing limit.

## Run from source

Use an existing compatible Node.js installation (22.12+ recommended), pnpm 10, Rust, and your platform’s SDK prerequisites. CI pins Rust **1.98.1** and Node **24**; use the lockfiles. Windows needs MSVC/Windows SDK/WebView2, Linux needs WebKitGTK 4.1/AppIndicator/D-Bus/Secret Service, and Apple/mobile builds need their corresponding SDKs.

```bash
pnpm install --frozen-lockfile
pnpm dev:renderer           # browser renderer preview only
pnpm dev                    # native Tauri development
pnpm lint
pnpm test
pnpm test:packaging
pnpm build:renderer
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

Interactive credential-store and external Codex CLI tests can be ignored by default. Run the relevant isolated credential test separately; skipped tests are not passed tests. The [packaging guide](docs/packaging-en.md) documents these gates.

```bash
pnpm build:windows
pnpm build:linux
pnpm build:macos:universal
pnpm build:android
pnpm build:ios:simulator    # macOS with Xcode and Simulator runtime
```

Outputs go to `release/<version>/<platform>-<architecture>/`. `--tools-dir` or `XY_SWITCH_TOOLS_DIR` selects the packaging-tools cache, not the Node/Rust/system SDK installation. Signed iOS device builds use a separate workflow and are not substitutes for simulator archives.

## Repository map and documentation

- `src/`: React workbench, shared UI, feature modules, and `i18n/`.
- `src-tauri/`: Rust commands, adapters, proxy, storage, native plugins, and regression tests.
- `scripts/`: packaging, isolated native checks, and synthetic renderer automation.
- `tests/`: additional frontend/catalog regression coverage.
- `website/`: self-contained bilingual static project page; no external CDN runtime.
- `.github/workflows/`: multi-platform verification and signed-device build entry.

[User guide](docs/user-guide-en.md) · [Packaging](docs/packaging-en.md) · [1.1.0 release notes](docs/release-notes-1.1.0.md) · [Security](SECURITY.md) · [Changelog](CHANGELOG.md) · [Repository metadata](docs/github-description.md)

Dated acceptance reports describe their own historical snapshots, not the verification state of every later version. Source belongs in Git; installers and verification evidence belong in Releases. Do not publish local credentials, databases, personal configuration, signing material, logs, or build caches.

## License and provenance

[MIT](LICENSE). XY Switch is an independent project, not an official OpenAI, Anthropic, Google, or upstream-model client. Provider icons retain their respective owners’ rights; see [asset notice](public/providers/NOTICE.md).
