# XY Switch 模型来源修复与回归记录

日期：2026-09-21。工作目录：`E:\XY switch`。

## 结论与边界

模型来源相关代码缺陷已修复；隔离环境中，实际安装的 Codex app-server 正确读取应用导出的 XY 模型目录，并以 `xy_switch` 创建新会话。前端 46 项、后端 87 项通过。**不等同于用户当前桌面配置或真实 Grok/Kimi 云端调用已经通过验收。**

未读取真实 Codex/CC Switch/XY Switch 配置、供应商数据库或凭据。未查看用户截图，以免显示敏感信息。测试使用合成配置、占位凭据和本机回环模型服务。

## 原因与修复

1. 原导出器同时写 XY 和 CC 命名的模型目录：现在只生成 `xy-switch-model-catalog.json`，保留现有 CC 文件不动。
2. 原导出器可能复用外部 `custom` provider：现在固定选择并只维护 `model_providers.xy_switch`，外部 provider 表保持原样。
3. 原目录路径与配置目录不一致风险：模型目录改为绝对路径，导出/接管/状态读取统一遵守 `CODEX_HOME`；空值使用默认目录，非绝对覆盖路径明确报错。
4. 主模型先插入原始名称、默认上下文，导致同名映射失效：现在主模型应用映射名称和上下文参数。
5. 实际客户端测试发现旧根级 `profile = "..."` 会触发配置无效、回退默认配置：现在合并该旧 profile 的生效设置至根级并移除旧选择器，保留 profiles 表。无效 profile 或损坏的自有 provider 表不再报告成功。

第 5 项由合成配置复现，不代表检查过用户真实配置或确认其具有同一字段。

## 主要改动文件

- `E:\XY switch\src-tauri\src\takeover.rs`
- `E:\XY switch\src-tauri\src\lib.rs`
- `E:\XY switch\src-tauri\src\takeover_regression_tests.rs`
- `E:\XY switch\src-tauri\src\codex_client_regression_tests.rs`
- `E:\XY switch\src\components\ProviderAdvancedFields.tsx`

## 验证结果

| 项目 | 结果 | 范围 |
| --- | --- | --- |
| 修复前复现 | 3 项失败 | CC 目录被覆盖、provider 混用、主模型映射被忽略 |
| 前端单元测试 | 46/46 通过，11 个文件 | 应用前端回归 |
| 后端测试 | 87 通过，0 失败，2 项过滤 | 包含实际 Codex 的 2 项协议端到端测试 |
| 凭据存储测试 | 未运行 | 避免接触 Windows 凭据库和 DPAPI 存储 |
| 实际 app-server 模型列表 | 通过 | 隔离目录中的应用生成配置；返回 XY 独立模型 A/B |
| 实际 app-server 新会话 | 通过 | model=xy-model-a，modelProvider=xy_switch |
| Responses 原生协议 | 通过本机模拟测试 | 实际 CLI、工具往返、会话创建及恢复 |
| Chat Completions 桥接协议 | 通过本机模拟测试 | 实际 CLI、工具往返、会话创建及恢复 |
| 桌面指定文件工具读取 | **未通过** | 工具请求已发起，但执行策略返回 blocked by policy |
| 电脑操控/桌面 GUI 验收 | **未完成** | Computer Use native pipe is unavailable；未采用其他 UI 自动化绕过 |
| Grok/Kimi 真实上游 | **未测试** | 没有使用真实凭据或向云端发起模型调用 |

协议测试包含 `get_goal` 调用，以及故意无效的 `apply_patch` 输入产生的错误回传，验证自定义工具适配及历史恢复；不把该测试描述为成功的文件写入测试。

程序具有 Responses 原生路径和 Chat Completions 桥接路径。上游不支持 Responses 时，应选择 Chat Completions 格式或对应端点；本报告不承诺所有上游错误都能自动降级，也不承诺供应商所有模型功能兼容。

### 桌面文件的隐私边界

仅对明确授权的 `C:\Users\xxy\Desktop\新建 文本文档 (4).txt` 做本地有界敏感模式筛查及内存哈希比较准备；未显示、保存副本或向模型返回正文。模拟模型要求实际 Codex 调用工具计算该文件哈希，命令被执行策略拒绝，因此没有成功读取返回。未绕过限制、未修改文件。工具错误往返成功不算文件读取验收通过。

Computer Use 工具说明还禁止通过其 Windows UI 接口自动操作 Codex；最终 Codex 桌面发送消息步骤需由用户手动执行。

## 证据路径

所有下列文件均在 `E:\XY switch\artifacts\2026-09-21-model-source-fix`：

- `before.log`：修复前失败记录。
- `frontend-tests.log`、`rust-tests.log`：回归结果。
- `app-server-model-source.json`：真实 app-server 的合成模型目录及新会话结果。
- `verify-model-source.py`：隔离 app-server 验证脚本。
- `desktop-file-tool-check.json`：文件工具受阻记录，passed=false。
- `windows-build.log`：正式构建记录。
- `verify-release.ps1`、`release-integrity.json`：发布包结构与哈希验证（已通过）。

## 用户侧生效步骤

1. 退出旧 XY Switch，包括托盘进程，运行此次模型来源修复版。
2. 避免同时让 CC Switch 写入相同 Codex 配置。
3. 在 XY Switch 中保存并重新点击所需供应商，触发配置导出。
4. 完全退出并重启 Codex，然后创建新任务检查模型列表和供应商。
5. 两个程序应使用相同 CODEX_HOME。显式启动参数 `--profile`、`-c`、独立 profile 文件和项目覆盖配置可能改变最终选择；本次未检查这些真实用户数据，也不会强制清除它们。

未自动启动发布版，未替换用户正在运行的程序，未擅自激活真实供应商。若仍异常，只需提供脱敏后的模型显示名称、错误类型与新旧版本信息，不要提交密钥、完整配置或原始敏感日志。

## 正式构建与完整性检查

`pnpm tauri build --no-bundle` 成功（release 优化构建）。链接器仅报告创建导入库/对象文件的提示，无构建错误。

- EXE：`E:\XY switch\release\XY-Switch-1.0.0-model-source-fix-20260921.exe`
- ZIP：`E:\XY switch\release\XY-Switch-1.0.0-model-source-fix-20260921.zip`
- EXE 大小：13812224 字节。
- EXE SHA256：`0F792C92A90209F6C853F7FB0083B1F70ACE2436EE0F65E650A24CF0DEB34D08`
- ZIP SHA256：`23E9D82D54509A4624A0D7FF2956D2B652A581FAC27EB4A63E5C80100EF5A9CE`
- 已验证：PE32+ x64、GUI 子系统、节区边界、前端 JS/CSS 引用、图标资源、构建时间与源码一致、编译后的本地窗口事件权限、EXE 复制哈希、ZIP 内 EXE 哈希一致。
- ZIP 仅包含 EXE 与说明文件，不包含用户配置、测试配置或数据。

完整性检查属于构建产物结构与一致性检查，不等同于 GUI 运行验收或安全审计。桌面文件工具读取与 GUI 验收仍未通过/未完成，未因打包成功改为通过。
