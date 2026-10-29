# XY Switch 1.0.0 发布验收记录

## 已有证据

- 前端：9 个测试文件，34 项测试通过。
- Rust：45 项通过，0 项失败，1 项因需要交互式 Windows 凭据环境而忽略。
- Release EXE 已成功构建。
- 2026-09-10 对当前 Release EXE 完成冷启动验收：进程正常保持运行，项目进程没有创建监听端口，证明本地代理未随应用启动而自动开启；验收结束后仅关闭了本项目进程。
- 稳定状态共采集 5 次资源样本：工作集保持约 26.52 MiB，私有内存最高约 5.80 MiB，句柄数最高 335，采样期间累计 CPU 时间保持约 0.047 秒。
- 旧候选便携版 ZIP 曾完成解压和启动验证，但其内含 EXE 与当前 Release EXE 的 SHA-256 不一致，须在最终 UI 完成后使用最新 EXE 重新生成并验收。
- 冷启动检查未发现本地代理自动监听端口。
- Provider 凭据隔离、Grok/xAI、Chat、Responses、Anthropic、Gemini、重试、故障转移和熔断器存在自动测试覆盖。

## 发布产物记录

候选 MSI 和 ZIP 位于 `release`。正式发布前须根据最终 UI 构建重新生成，不应将当前候选文件视为 UI 优化后的最终产物。

### 当前候选产物校验

- 当前 Release EXE：`src-tauri/target/release/xy-switch.exe`
  - 大小：`13,348,352` 字节
  - SHA-256：`FB5DCB109750FC0A1776AD651D537A1C2F14F8B64D5AC5EF0DE1CE186169848D`
  - 签名状态：`NotSigned`
- MSI 候选：`release/XY-Switch-1.0.0-x64-model-selection-fix.msi`
  - 大小：`6,692,864` 字节
  - SHA-256：`F129642D14B9009D7F9C6C588174D0B87DF9D8679E65D0B0774452198E5EF961`
  - 签名状态：`NotSigned`
- 旧便携版 ZIP 候选：`release/XY-Switch-1.0.0-portable-model-selection-fix.zip`
  - 大小：`5,543,937` 字节
  - SHA-256：`93DD1AF93F9226738F78277943B56183292A0A63186EA580F5CD5B40BDD46CCF`
  - 解压目录：`artifacts/portable-validation-20260911-001258`
  - ZIP 内 EXE SHA-256：`044F7DB08EE0B358D368F9060AC82452F24BF37F11EA4307CFD27455291A5312`
  - 当前 Release EXE SHA-256：`FB5DCB109750FC0A1776AD651D537A1C2F14F8B64D5AC5EF0DE1CE186169848D`
  - `HASH_MATCH=False`

旧便携版 ZIP 内的 EXE 与当前 Release EXE 不一致，因此该 ZIP 是旧构建，不得作为最终发布包。须在 UI 优化完成后使用最新 EXE 重新生成 ZIP，并重新执行解压、启动、哈希一致性和敏感信息验收。

## 待完成验收

- MSI 实际安装、安装版启动、卸载及残留检查
- 应用内 MCP 单次确认
- 托盘切换与主界面同步
- 模型菜单和非首项模型人工调用
- 真实 Chat 与 Responses 单次低速请求
- UI 优化后重新执行 EXE 冷启动、代理默认关闭、资源占用、全量测试、构建、哈希和敏感信息扫描

## 签名状态

当前没有商业代码签名证书。发布页面和安装说明必须如实标明未签名状态，不得宣称已通过受信任发布者签名。

## 敏感信息

公开发布不得包含真实 API Key、Bearer Token、本地数据库、用户配置、诊断日志或 `artifacts` 中的恢复材料。
