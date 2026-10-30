# Changelog / 更新日志

## Unreleased / 未发布 — 2026-10-30

- Limit current build, CI, documentation, and website distribution entries to **Windows, macOS, and Linux**. Remove iOS/Android packaging and simulator workflows, including signed iOS device builds.
- 当前仅保留 Windows、macOS、Linux；停止移动端原生验收。已发布的版本、安装包与历史验证记录不追溯修改。
- Keep Windows encrypted-sync native acceptance isolated from daily user data and from the published 1.2.0 packages. [Current native acceptance scope](docs/native-acceptance-20261030.md).

## 1.2.0 — 2026-10-29

- Promote maintenance, local-session favorites, pricing, and the safety/acceptance workspace into a new release candidate; never relabel existing 1.1.0 binaries.
- Real proxy acceptance passed 6/6 using Chat upstream and 6/6 using Responses upstream with the gateway-reported test model. Preserve earlier HTTP 500 and 5/6 failure evidence.
- Treat nullable OpenAI/Responses error fields correctly without accepting real errors or incomplete streams; add focused regression tests and bounded redacted live diagnostics.
- [Release notes](docs/release-notes-1.2.0.md) and [live follow-up](docs/governance-live-followup-20261029.md). Package publication/native verification requires the exact Release evidence. E2EE sync is not included in this snapshot.

## Unreleased / 未发布 — 2026-10-29

- Add the bilingual **Safety & Acceptance** workspace: overview, access control, hard-budget admission, synthetic monitoring, configuration versions, explicit mounts, and governance audit.
- Add expiring/revocable, digest-only local tokens with proxy/monitor scopes. Status reads always require authentication; model authentication is opt-in for compatibility. Share rate limits across valid and rejected credentials and hold concurrency permits until streaming bodies finish.
- Add SQLite reservation admission before every upstream attempt, retry, failover, and model probe. Require exact custom prices and bounded supported payloads; preserve reservations on cancellation/failure and across restarts. Disable implicit HTTP retries. Reservations are not an upstream invoice guarantee.
- Add six bounded loopback-to-upstream synthetic checks, explicit paid confirmation, opt-in schedules, persistent daily limits, redacted receipts, and failure/recovery audit transitions. Live acceptance remains **not passed** after HTTP 500; model discovery alone is not E2E evidence.
- Add manual configuration snapshots and hash-guarded transactional rollback with a pre-restore backup; do not restore credential stores or security ledgers/policies.
- Add native-folder-granted prompt/Skill mounts for `CLAUDE.md`, `AGENTS.md`, and `GEMINI.md`: diff preview, five-minute single-use confirmation, original-content backups, atomic replacement, and guarded rollback. Never write automatically or execute Skill scripts.
- Keep mobile governance read-only and browser privileged operations fail-closed. Add real-socket/file regression tests, an ignored Windows one-shot-pipe live test, and 11 isolated browser interaction checks.
- 新增安全与验收工作区、访问令牌与共享限流、费用/Token 预留准入、六类收费验收与显式计划、配置版本回滚、提示词/Skills 显式挂载及本机只读监控。会话恢复仍只复制命令；跨设备加密同步、远程移动连接与更多接管适配未实现。
- These are **unreleased source changes**. Version 1.1.0 is unchanged; no new installers were built or published. See [治理与验收记录](docs/governance-acceptance-20261029.md).

## Unreleased / 未发布 — 2026-10-27

- Implement guarded desktop session-log browsing for Codex, Claude Code and Gemini CLI, including message/tool display, explicit read warnings and selected-log deletion. Client indexes are not rewritten.
- Protect the loopback proxy against browser-origin spending and DNS rebinding; reject unsupported Gemini operations, preserve empty model catalogs, fix IPv6 status URLs, and restrict the production WebView CSP.
- Validate configuration imports before mutation; commit provider/prompt/custom-price rows transactionally and preserve existing local MCP credentials. Report external-resource partial failures and refresh the UI.
- Add persistent custom model pricing, guarded models.dev reference synchronization, exact-ID matching, historical cost snapshots, and bilingual editing/search/removal UI. Export custom prices in v2 configuration while keeping v1 imports compatible.
- Read Codex renamed-title indexes and Claude/Gemini custom roots without allowing index-only path discovery. Preserve debug test-home isolation.
- Fix mixed per-event/cumulative Codex token accounting, cache-write double counting, malformed/free-request confusion, and swallowed database/checkpoint failures.
- Harden timestamp parsing and exclude locked/generated test artifacts from Vite watching.
- Add session/IPC, import rollback/credential, proxy HTTP, renderer interaction and isolated Codex tool/resume regressions.
- 新增三客户端会话日志管理、Codex 重命名标题、自定义目录、自定义/公开参考定价与历史费用快照；修补本地代理访问边界、配置导入回滚与凭据保留、Codex 用量重复计费和开发服务文件监视问题。仍需测试账户验证真实上游。
- These are working-tree changes, not rebuilt or republished 1.1.0 installers. See [检修报告](docs/maintenance-20261027.md).

## 1.1.0 — 2026-10-20

- Add English/Simplified Chinese interfaces, system-language preference, native persistence and localized desktop tray.
- Preserve unsaved page state during language changes; keep user content and raw diagnostics unchanged; retain the old language if persistence fails.
- Add English/Chinese repository documentation and a self-contained bilingual website; correct overstated prompt, session, tray and client-takeover claims.
- Add localization catalog/runtime tests and bilingual UI regression coverage; align package/native versions.
- 新增中英文界面、系统语言偏好、原生持久化与托盘翻译；完善双语仓库文档和项目页，明确实际能力与签名边界。
- Release evidence and distribution limits: [1.1.0 notes](docs/release-notes-1.1.0.md). Historical entries below describe their own snapshots.

## 1.0.0 多平台交付 — 2026-10-18

- 新增 Linux、macOS Universal、Android 与 iOS 模拟器构建/原生验收流程。
- Android 使用 Keystore 和私有 AES-GCM 记录，覆盖原生存储与最终 release APK 的集成测试；修复 Windows 工具路径大小写和 CRLF 生成工程兼容问题。
- 新增 macOS App/DMG 完整性、临时 Keychain、真实窗口检查；桌面与移动能力按实际平台区分。
- 完整文档、签名限制及当次实际验证结果见 `docs/release-notes-1.0.0.md` 和 GitHub Release。
- 以下 2026-09-10 内容是早期历史快照，其中“尚未交付”的状态不代表当前发布。

## 1.0.0 - 2026-09-10

### 新增

- Windows 本地供应商、模型、MCP 和提示词管理
- OpenAI Chat、OpenAI Responses、Anthropic 与 Gemini 协议适配
- 本地代理、健康检查、基础请求指标、重试、故障转移和熔断器
- 供应商模型探索、模型选择和模型映射导入
- 系统托盘、配置接管与恢复

### 安全

- Provider 凭据隔离
- Grok/xAI Bearer Token 链路隔离
- 禁止自定义认证头覆盖当前供应商凭据
- 应用启动时强制将代理恢复为关闭状态

### 已知限制

- Skills、完整用量统计和云同步尚未正式交付
- macOS 与 Linux 安装包暂不提供
- 商业代码签名尚未配置
- MSI 仍需完成最终安装和卸载验收
