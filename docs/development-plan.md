# XY Switch 最新开发计划

更新日期：2026-09-04

## 1. 当前状态

项目当前处于 **Windows 核心能力完成，业务管理模块继续补齐** 阶段。

已完成并通过验证：

- 7 个内置供应商与自定义供应商 CRUD，API Key 使用 Windows Credential Manager。
- 供应商高级配置：上游格式、模型探索与映射、User-Agent、Header/Body 覆盖。
- OpenAI Chat / Responses、Anthropic、Gemini 请求响应及 SSE 流式协议转换。
- 本地代理手动启停、启动强制关闭、重试、故障转移、熔断和健康测速。
- Claude Code、Codex、Gemini CLI 应用接管及原子备份恢复。
- Codex `auth.json`、`config.toml`、远程压缩、通用配置、1M 上下文和压缩阈值。
- MCP 模板、CRUD、启停、CLI 导入、敏感字段安全存储和失败回滚。
- 供应商卡片不展示具体模型名。
- 桌面与 480px 窄窗口高级配置界面无水平溢出。

验证基线：前端 22 项测试通过；Rust 38 项通过、1 项需交互式 Credential Manager 的测试按设计忽略；`cargo fmt` 和严格 Clippy 通过。

## 2. 下一阶段任务

按以下顺序实施，每一阶段均需完成代码、测试、生产构建和界面验证。

### P0：供应商管理收尾

1. 增加供应商拖拽排序，并持久化 `sort_order`。
2. 增加复制自定义供应商配置和删除二次确认。
3. 增加供应商额度、套餐和 Token 用量查询适配层；不支持查询的供应商显示明确状态。
4. 完善托盘按 Claude、Codex、Gemini 分组切换，并验证与主界面状态同步。

### P0：业务模块

5. Prompts 管理：Markdown 编辑、模板、激活切换、智能回填，并原子同步到 `CLAUDE.md`、`AGENTS.md`、`GEMINI.md`。
6. 会话管理：扫描 Claude Code、Codex、Gemini CLI 历史，支持搜索、过滤、恢复和删除确认。
7. 完整用量统计：Token、成本规则、时间趋势、供应商/模型筛选和数据清理。

### P1：配置与体验

8. JSON/YAML 配置导入导出、备份恢复；默认永久排除 API Key、Token 和密码。
9. 中英文切换和真实亮色/暗色主题，修复当前主题按钮尚未生效的问题并保证启动无闪烁。
10. WebDAV 云同步；先完成冲突策略、原子落盘和敏感字段排除，再开放界面。
11. 工作区文件与每日记忆，按需实现，不阻塞核心 Windows 版本。

### P0：发布验收

12. 增加隔离目录下的真实 Tauri IPC CRUD、接管和启动默认关闭回归测试。
13. 完成 README、中文用户手册、架构与数据流更新、故障排查和 MIT LICENSE。
14. 修复本机 Windows Installer 服务后重新生成并验证 MSI；同时生成便携 ZIP。
15. 在干净 Windows 环境执行安装、升级、卸载、托盘、代理端口、配置恢复和敏感信息检查。

## 3. 暂缓任务

以下任务保留在后续计划，不进入当前版本验收：

- Skills 管理：GitHub 仓库浏览、skills.sh、安装/卸载和仓库管理。
- 认证板块：CC Switch 中的 Codex OAuth、GitHub Copilot、xAI OAuth 等账号认证管理。
- macOS：签名、公证、DMG 和 ZIP。
- Linux：deb、AppImage 和桌面集成。

项目仍不实现 XY Switch 自身的用户登录或账号系统。

## 4. 当前阻塞

- Release EXE 已成功生成：`src-tauri/target/release/xy-switch.exe`。
- 本次 MSI 未生成。WiX 报错 `LGHT0217/LGHT0216`，根因是本机 Windows Installer 服务不可访问；目录中现有 MSI 是旧构建，不能作为本次交付物。
- 处理 Windows Installer 服务后重新执行 `pnpm build:windows`，无需修改应用代码。
