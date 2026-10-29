# XY Switch 1.2.0 — Safety, acceptance and local sessions

## 简体中文

### 本版内容

- 本地会话浏览、消息/工具结果、恢复命令复制、指定日志删除、收藏与快速筛选。不会自动执行恢复命令，不改写原客户端索引。
- 自定义模型定价与配置备份 v2；历史费用快照不随改价重算。
- 安全与验收工作区：代理访问令牌、只读监控权限、限流、持久化预留硬预算、六种真实合成检查与显式计划监控。
- 手动配置版本、diff、陈旧预览校验、事务备份与回滚。
- 提示词/Skills 显式挂载：原生目录选择、diff、确认、原文备份与回滚；不默认写入规则文件。
- 修复 Responses/OpenAI SSE 的 `error: null` 误判；真实 Chat、Responses 上游各 6/6 通过，详见[跟进记录](governance-live-followup-20261029.md)。

- 隔离 CI 发现并修复移动端编译误用桌面文件夹选择 API：该代码现在仅编译进桌面端，移动端仍拒绝规则挂载。

### 发布与已知限制

版本元数据统一为 1.2.0。新包必须由本版源提交重新构建，不能重命名旧包。只有对应 GitHub Release 的下载资产、SHA256、源提交及成功 CI/原生安装证据，才构成已发布与目标平台通过的证据；本文不预先声明构建结果。

预算限制的是本机预留账本，不保证供应商最终账单；隐藏输入、未知用量与未知价格不能假定为零。网关自报模型 ID 不认证官方型号。

**E2EE 跨设备同步仍在后续开发，不包含在 1.2.0。** 也不包含远程移动连接/推送、Grok/OpenCode 自动接管或恢复命令自动执行。移动端管理自己的沙箱，不承诺代理后台常驻。

若目标平台资产存在：Windows 未做商业代码签名；macOS ad-hoc 签名、未公证；Android 开发签名，不保证跨构建原地升级；iOS 模拟器 ZIP 不是 iPhone IPA。不把某一平台成功外推到其他平台。

## English

This release candidate brings local session browsing/favorites, custom model prices, and the safety/acceptance workspace into a freshly versioned build. It adds access tokens and read-only monitor scopes, rate limits, durable reservation budgets, six bounded synthetic checks, opt-in schedules, configuration diff/rollback, and explicit backed-up prompt/Skill mounts. Restoring a session still only copies a command; rule files are never written automatically.

Real proxy checks passed **6/6 with Chat upstream and 6/6 with Responses upstream** after fixing nullable error-field handling. See the [live follow-up](governance-live-followup-20261029.md), which retains earlier failures. The test model is a gateway-reported identifier, not authenticated model identity. Reservation limits are not a guarantee of the supplier's final bill.

Actual installer publication and platform coverage must be verified against the exact source commit, successful CI/native verification, Release assets, and SHA256 manifest. Existing 1.1.0 binaries must not be renamed. E2EE synchronization, remote mobile monitoring/push, Grok/OpenCode takeover, and automatic session-command execution are **not included**. Platform signing/distribution limitations above remain applicable.
