# XY Switch：Codex 兼容性修复与回归报告

日期：2026-09-21  
工作目录：`E:\XY switch`  
版本：1.0.0 / codex-compat-fix-20260921

## 1. 结论与范围

本次已完成保存供应商故障修复、Codex 原生 Responses / Chat Completions 桥接链路修复、自动化回归、Windows release 构建与交付产物完整性校验。

- 前端：**46 项通过，11 个测试文件**。
- 后端最终完整运行：**76 项通过，0 失败，0 忽略，0 过滤**。
- 其中包含真实安装的 Codex CLI 两项端到端测试：原生 Responses、Chat 桥接的创建会话、工具往返与恢复同一会话。
- 前端生产构建和 `pnpm tauri build --no-bundle` 成功。
- Windows EXE 结构、资源存在性、源文件/构建时间、复制前后 SHA256、ZIP 内 EXE SHA256 校验通过。

**以上不等于整个产品所有功能验收通过。** 未执行正式桌面 GUI 操作验收、安装器测试、真实供应商云端请求、成功文件写入或全产品安全审计。发布的是独立 EXE 与 ZIP，不是 MSI 安装包。

## 2. 根因和主要修复

### 2.1 保存供应商失败：duplicate field appGroup

前端将 UI Provider 对象展开到 IPC 请求，同时补充 `appGroup`，导致同时发送 `group` 和 `appGroup`。后端反序列化将两者视为同一字段及其别名，因而拒绝请求。

修复：显式构造规范的九字段后端 payload，不透传 UI 对象；统一加载结果转换。供应商排序参数同步修正为后端要求的 `providerIds`。新增 Grok、Kimi、OpenAI 保存契约回归。

相关文件：
- `E:\XY switch\src\lib\provider-service.ts`
- `E:\XY switch\src\provider-service.test.ts`

### 2.2 激活、保存和配置同步顺序

- 后端切换成功后才更新前端选中状态，避免失败后 UI 显示已切换。
- 启动加载不再顺便重写 Codex 配置。
- 保存顺序调整为元数据、凭证、刷新已激活 Codex 供应商，再刷新代理状态。
- 新增刷新命令，不通过切换全局 UI 分组来刷新 Codex。
- 托盘使用相同切换流程并传递失败事件。
- 配置覆盖、接管恢复、外部修改保护及部分失败回滚有隔离测试覆盖。

相关文件：
- `E:\XY switch\src\App.tsx`
- `E:\XY switch\src-tauri\src\commands\providers.rs`
- `E:\XY switch\src-tauri\src\commands\proxy.rs`
- `E:\XY switch\src-tauri\src\takeover.rs`
- `E:\XY switch\src-tauri\src\tray.rs`
- `E:\XY switch\src-tauri\src\lib.rs`

配置、认证文件与模型目录的写入不属于完全原子的多文件事务，不作该保证。

### 2.3 Responses 直连 / Chat 桥接

- 统一有效上游协议判断和 URL 拼接，修复自定义版本路径被再次拼接 `/v1` 等问题。
- 原生 Responses 正常情况下直连上游，不因其他用途的本地代理运行而被强制绕行；显式接管模式除外。
- Chat Completions 上游通过本地 Responses 入口适配，激活时自动启动需要的本地路由。
- Codex 导出配置使用 `wire_api="responses"`，去除冲突的旧认证覆盖参数。
- 真实 Codex 校验暴露的工具目录兼容问题已修复：`apply_patch_tool_type` 使用客户端接受的 `freeform`。
- 代理不可用、凭证缺失及不可支持的请求不再被伪装成成功。

**没有新增主动协议能力探测或失败后自动切换接口。** 使用者应根据上游实际能力选择 Responses 或 Chat Completions；已有供应商启发式判断不等同于能力验证。

### 2.4 工具、多轮对话与 SSE

新增 `E:\XY switch\src-tauri\src\tool_bridge.rs`：
- Codex namespace 工具转为稳定、无冲突的 Chat 函数别名，再恢复原始名称与命名空间。
- 自定义/freeform 工具转为字符串 `input` 的函数封装，保留原始输入并转换调用结果历史。
- 严格处理畸形工具参数、重复工具、不可支持的托管工具，避免静默丢失或 panic。

协议及流处理修复：
- developer/system、图片、tool_choice、parallel_tool_calls 等参数转换。
- Kimi 风格 reasoning_content 的工具结果重放。
- SSE 输出项生命周期、并行工具索引、参数完成事件、最终 output、晚到 usage 与序号。
- 混合换行、字节分片、提前断流、截断及过滤，不再错误报告 completed。

相关文件：
- `E:\XY switch\src-tauri\src\adapters.rs`
- `E:\XY switch\src-tauri\src\streaming.rs`
- `E:\XY switch\src-tauri\src\proxy.rs`

## 3. 测试证据

| 项目 | 结果 | 证据 |
| --- | --- | --- |
| 前端单元/契约测试 | 46 passed / 11 files | `E:\XY switch\artifacts\2026-09-21-regression\frontend-tests.log` |
| 后端完整测试 | 76 passed / 0 failed / 0 ignored | `E:\XY switch\artifacts\2026-09-21-regression\rust-full-tests.log` |
| 真实 Codex 两条协议测试 | 2 passed，已计入 76 项 | `E:\XY switch\artifacts\2026-09-21-regression\codex-client-test.log` |
| 生产前端与 Windows release | 成功 | `E:\XY switch\artifacts\2026-09-21-regression\windows-build.log` |
| 发布包完整性 | 通过 | `E:\XY switch\artifacts\2026-09-21-regression\release-integrity.json` |

实际客户端版本：`0.154.0-alpha.6.2`。

实际客户端测试使用隔离的 HOME、USERPROFILE、CODEX_HOME、工作目录及测试数据库，使用虚构凭证和本地模型模拟服务；最终测试禁用了种子供应商，避免测试失败后转发到真实模型服务。原生测试停止本地桥接，验证直连本地 Responses 模拟上游。Chat 测试经过真实应用路由。

测试检查：
1. 创建会话并获得会话 ID。
2. 普通 `get_goal` 工具实际执行及结果回传。
3. 自定义 `apply_patch` 调用进入客户端验证器，预期非法输入返回验证错误，并正常重放给上游。
4. 获取测试完成标记。
5. `codex exec resume` 恢复，确认同一会话 ID。

**自定义工具验证错误的往返测试不等于成功创建/修改文件。** 先前有效补丁尝试受客户端只读沙箱限制，未将其作为成功文件写入验收。

Grok/Kimi 命名的本地 HTTP 测试覆盖流式和非流式两轮工具对话；这证明测试协议形态可用，不证明真实供应商账户、付费接口、任意模型工具能力或所有云端扩展兼容。

隔离 DPAPI 和 Windows Credential Manager 测试使用临时测试位置、随机测试 ID 及合成字符串，已在 Windows 用户上下文通过；本次重试未再次访问凭证存储。

## 4. 交付和完整性

发布包只含本次 EXE 和使用说明，不打包配置、测试日志或用户数据。

### EXE

`E:\XY switch\release\XY-Switch-1.0.0-codex-compat-fix-20260921.exe`

- 大小：13,793,280 字节
- PE32+ / x64 / Windows GUI / 6 sections
- SHA256：`09E4AA6B8A6F4366B7F07D7AAADCA69403E876D23C2071C124E337CFF2758F36`

### ZIP

`E:\XY switch\release\XY-Switch-1.0.0-codex-compat-fix-20260921.zip`

- 大小：5,652,460 字节
- SHA256：`C8D42C7DBF210162E1F8350C71F1B8F9B017F90A1B3C6A95DC64BDA494074DA7`
- 仅两个条目：上述 EXE、`RELEASE-README.txt`
- ZIP 中 EXE 的 SHA256 与源构建及单独发布 EXE 一致。

校验还包括：PE 各段边界不超出文件、dist/index.html 引用的 JS/CSS 存在、Tauri frontendDist 和图标存在、src / src-tauri/src / dist 文件时间早于最终 EXE。

校验脚本：`E:\XY switch\artifacts\2026-09-21-regression\verify-release.ps1`。该脚本为了防止覆盖既有发布包，在目标文件已存在时主动退出，不是可重复覆盖的安装工具。

没有启动本次正式 EXE，没有安装或覆盖旧版本，没有操作真实 Codex 配置。哈希一致性不等同于代码签名、恶意软件扫描或完整安全审计。

## 5. 使用方式与边界

1. 在 Codex 分组配置供应商地址、模型及凭证；凭证只在用户本机界面输入，不发给助手。
2. 上游支持 Responses：高级设置选择 Responses；否则选择 Chat Completions。
3. 保存并激活。Chat 桥接需保持 XY Switch 的本地代理运行。
4. 新建或刷新 Codex 会话后验证。

边界：
- Chat 桥接要求上游模型支持对应函数工具；不保证任何模型都可充当 Codex 编程模型。
- 不支持 Chat 桥接中的托管搜索工具；遇到不可支持的工具明确报错。
- Chat 桥接不提供服务端 `previous_response_id` 状态存储，需完整历史重放；本次真实 Codex 客户端路径已验证该方式。
- 未验证真实 Grok/Kimi 云端、正式 GUI、安装流程、跨平台及项目其他未实现模块。
- 本次重试仅访问项目源码、构建产物和合成测试结果；没有查询、查看或复制真实密钥、证书、密码与私人配置，也没有上传交付物。
