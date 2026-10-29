# 维护说明 — 2026-10-27（公开摘要）

本次源码包含会话管理、代理请求校验、模型定价及依赖维护改动。源码发布不等于安装包发布，也不等于业务、原生或全平台验收完成。

## 主要实现

- 会话：读取 Codex、Claude Code、Gemini CLI 的受支持本地日志；处理自定义根目录、重命名标题、消息与工具结果；对扫描、详情、重复 ID、目录边界和指定日志删除进行限制。
- 代理：校验 HTTP Host 与浏览器来源；同步前端 CSP；区分网页来源拒绝与正常原生 IPC/CLI 请求。
- 配置：分段预检与数据库事务；外部 MCP/代理阶段失败时明确报告部分完成，不声称跨资源全局原子性。
- 模型定价：持久化自定义费率、公开参考价格同步和不可变的历史费用快照；未知价格保持未知，不把缺失值转换为免费。
- 依赖：升级相关前端依赖；保留第三方许可证；对 GLib 0.18.5 应用兼容性回移，不更改其上游版本号来隐藏告警。

## 已记录的验证与边界

以下数字来自维护记录，不代表本次发布另行重跑：

| 项目 | 已记录状态 |
| --- | --- |
| 前端测试 | 402 个不同测试分两次通过；不是一次完整全量运行通过 |
| Rust 测试 | 323 passed，3 ignored；忽略项不计为通过 |
| 维护辅助工具 | 76/76 通过 |
| 依赖告警 | 保留 9 条维护告警与 2 条安全版本匹配记录；不能据此宣称零漏洞 |

仍待完成：主完整前端运行、新原生 IPC/UI/重启验收、Linux GLib optimized 验证、真实专用供应商账户联调。GitHub 推送认证不是供应商验收账户。

`goalComplete=false`，`localPhaseComplete=false`。本公开稿不包含原始日志、私有本机路径、账户信息或内部执行记录，也不链接未发布的本地验收工件。

## 公开代码与说明

- [会话存储](../src-tauri/src/sessions/mod.rs)、[会话测试](../src-tauri/src/sessions/tests.rs)、[会话页面](../src/features/sessions/SessionsPage.tsx)。
- [代理请求校验](../src-tauri/src/proxy_request_guard.rs)、[代理实现](../src-tauri/src/proxy.rs)。
- [模型定价](../src-tauri/src/model_pricing.rs)、[定价测试](../src-tauri/src/model_pricing/tests.rs)。
- [打包说明](packaging.md)、[English packaging guide](packaging-en.md)、[GLib 回移说明](../src-tauri/vendor/glib/XY-SWITCH-PATCH.md)。
