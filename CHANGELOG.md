# 更新日志

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
