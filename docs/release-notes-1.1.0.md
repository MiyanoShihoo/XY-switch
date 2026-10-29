# XY Switch 1.1.0 — English and Simplified Chinese

[English README](../README.md) · [中文说明](../README.zh-CN.md)

## English

### Changes

- English and Simplified Chinese interfaces, with **System / 简体中文 / English** preferences in Settings → General.
- Native preference persistence and localized desktop tray; browser-preview persistence. Save failures keep the previous language and expose retry instead of reporting a false success.
- Language changes preserve page state and unsaved form values. Provider names, prompts, Skills, conversation content, model identifiers and raw diagnostics are not translated.
- English-first repository README, Chinese companion README, bilingual user/build guides, and a self-contained bilingual project page without external CDN scripts.
- Localization regression tests, 13 workspace UI suites and 16 bilingual UI suites. These browser suites use synthetic fixtures and do not establish native session support or mobile real-device coverage.
- Corrected documentation and About text: prompts are local CRUD, native session management is not delivered, desktop takeover is limited to Claude Code/Codex/Gemini CLI, and the tray does not expose a proxy toggle.

### Distribution and verification

This source defines the 1.1.0 release candidate. The published Release's `release-verification.json`, `release-verification.md` and `SHA256SUMS.txt` are the authority for the exact source commit, successful Actions run, packages, native screenshots and verification results. The presence of these release notes alone is not proof that an installer has been built or published.

Expected distribution: Windows x64 EXE/MSI; Linux x64 DEB/AppImage; macOS Universal App ZIP/DMG; Android ARM64+x86_64 APK; ARM64 iOS simulator ZIP. Windows is unsigned; macOS is ad-hoc signed without notarization; Android uses development signing (cross-build in-place upgrades are not guaranteed); the iOS simulator ZIP is **not an iPhone IPA**. Signed physical-device iOS distribution requires separately configured Apple credentials/provisioning.

Missing cost is unknown, not zero. Model/protocol evidence does not authenticate model weights. Mobile manages its own sandbox and cannot take over other apps' CLI configuration; background proxy lifetime is not guaranteed. Normal configuration exports do not include a complete credential backup.

## 简体中文

### 更新

- 新增完整中英文界面，在设置 → 通用选择跟随系统、简体中文或 English。
- 原生语言偏好持久化并同步桌面托盘；浏览器预览独立保存。保存失败保留旧语言并提供重试，不假报成功。
- 切换语言保留页面状态及未保存表单。不翻译供应商名、提示词、Skills、会话正文、模型 ID 与原始诊断。
- 仓库使用英文首页并提供中文说明，补齐双语用户/构建指南，以及不依赖外部 CDN 脚本的双语项目页。
- 增加本地化回归、13 组工作台 UI 验收及 16 组双语 UI 验收。浏览器测试使用合成数据，不等于原生会话支持或移动真机测试。
- 修正文档及“关于”页的能力说明：提示词为本地管理，完整原生会话管理未实现；桌面接管限 Claude Code/Codex/Gemini CLI；托盘没有代理开关。

### 分发与验证

此源码定义 1.1.0 发布候选。实际源码提交、同一次成功 Actions、安装包、原生截图与结果以公开 Release 中的 `release-verification.json`、`release-verification.md`、`SHA256SUMS.txt` 为准；存在发布说明不代表已构建或上传成功。

分发范围为 Windows x64 EXE/MSI、Linux x64 DEB/AppImage、macOS Universal App ZIP/DMG、Android ARM64+x86_64 APK，以及 ARM64 iOS 模拟器 ZIP。Windows 未签名，macOS ad-hoc 签名且未公证，Android 开发签名且不保证跨构建覆盖升级。iOS 模拟器 ZIP **不是 iPhone IPA**；真机包需要独立配置 Apple 签名及描述文件。

未知成本不等于零，型号/协议证据不认证模型权重。移动端只管理自身沙盒，不能接管其他应用的 CLI，不能保证后台代理常驻。普通配置导出不包含完整凭据备份。
