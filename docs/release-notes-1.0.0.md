# XY Switch 1.0.0

本次发布将本地 AI 配置管理、协议代理和模型响应核验整理为可构建、可验证的开源项目。源码仓库为 [MiyanoShihoo/XY-switch](https://github.com/MiyanoShihoo/XY-switch)。具体附件、构建系统和测试结果以 GitHub Release 与对应提交的 Actions 为准。

## 功能与修复

- 供应商、API 地址、协议、模型映射和独立凭据管理。
- Chat Completions、Responses、Messages 等协议路由与工具调用处理。
- Skills 父目录/嵌套目录识别及导入结果反馈；移除误导性的只读标记。
- 用量与模型核验分开记录接口能力、型号声明、计量异常和身份凭据；不把缺少独立证明直接判断为假模型。
- 低 Token 主动复核软预算支持 0–10000，默认 100；0 禁止该机制执行收费复核。
- 关于页展示真实仓库，支持打开、复制和错误反馈。

## 多平台

- Windows：NSIS 当前用户安装器、MSI，保留 Windows Credential Manager。
- Linux：Debian 包、AppImage，新增 Secret Service 凭据存储；没有托盘时正常退出。
- macOS：Apple Silicon/Intel Universal App ZIP 与 DMG、Apple Keychain；默认 ad-hoc 签名，未配置公证。
- Android：API 24+ 的 ARM64/x86_64 APK、Android Keystore 与私有 AES-GCM 记录，独立签名/ELF 对齐检查和真实 release APK instrumentation。
- iOS：移动界面、Apple Keychain、模拟器构建和独立的签名 IPA 工作流。

各平台密钥库失败时不回退到明文。Linux/macOS 暂无开机自启配置；Android/iOS 不支持桌面客户端接管或跨 App 读取 Skills/会话目录，也不保证本地代理在后台持续运行。Android 凭据同步 IPC 显式移出 UI 线程，私有插件入口拒绝直接 JS 调用。

## 产物与验证边界

桌面/Android 构建脚本构建前后端，输出 SHA-256 和产物清单，检查实际打包输入，不把不匹配的历史包重新命名后发布。未配置商业代码签名的 Windows 包为未签名产物。macOS 默认 ad-hoc 签名只能检查完整性，不代表 Developer ID 或 Gatekeeper 放行。Android 默认使用开发证书，不是 Google Play 正式发行；不同 CI 构建证书可能不同，卸载旧包会清除其凭据与数据。

iOS 模拟器 ZIP 不能安装到 iPhone。真机 IPA 必须提供 Apple 证书、描述文件和 Team ID，并通过独立签名流程；有构建脚本不等于真机包已产出或已在真机验证。

历史日期性文档仅记录当时状态，不代表本次发布已重复执行其中全部测试。安装包、实际通过的验证、未覆盖的系统版本及 SHA-256 详见该版本 Release 正文。

## 升级

先退出旧程序，保留应用数据与系统密钥库。普通 JSON 导出不携带完整凭据，迁移到新系统后需要重新录入密钥。不要用旧 `website/downloads` 或历史便携包替代本次发布附件。
