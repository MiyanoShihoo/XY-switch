# Changelog / 更新日志

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
