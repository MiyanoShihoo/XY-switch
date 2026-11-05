# 统一客户端能力与验收差距矩阵

核对日期：2026-11-04；增量更新：2026-11-05（Asia/Shanghai）。工作区：`E:\XY switch\workspaces\session-favorites-20261029`。

## 1. 范围与证据口径

依据[主计划第 3 节](<E:/XY switch/workspaces/session-favorites-20261029/docs/unified-client-marketplace-privacy-plan-20261103.md:31>)检查八个交付对象。本文记录当前实现、已有测试证据、真实客户端缺口及后续可执行步骤。

- 核对时 HEAD 为 `23ac1a319fec201980b4bee6ca96c76652f76756`。客户端管理器、驱动、配置适配器、前端及客户端测试存在未跟踪文件；下述实现属于当前工作树，不能归为该提交或已发布安装包的能力。
- 2026-11-04 矩阵初版通过读取源码、测试、既存报告/日志、公开官方资料及协作收据完成，没有新增现场操作；2026-11-05 的实现与本机验证另见第 6 节。初版引用的历史日志按原范围保留，不代表新增代码已通过；第 6 节单独记录本次重新执行的验证。
- **实现**指存在具体生产调用链；**测试**分别标明合成夹具、mock 或历史真实客户端；**现场**须绑定客户端版本、安装来源、实际配置/鉴权来源及运行收据。八行完整现场验收均保持待验收。
- `ClientManager::connections` 的 `config_detected` 只检查配置文件存在，所有行的 `native_verification` 均硬编码为 `pending_native`（C1）。这不是安装探测或现场验收状态机。
- 主代理市场联合验收另行记录。2026-11-04 **主代理协作回传**：批准的主机权限下原生整库 **690 通过 / 0 失败 / 7 忽略**，桌面和市场 debug 构建通过；原生投稿、发布恢复、安装恢复及**自有三桌面宿主工具调用**通过。`published-catalog.json` 与 `market-publish.json` 绑定原摘要，作者 username 未进入匿名目录；安装收据为 `market-install.json`，该安装阶段仅 `codex-desktop` / `claude-desktop` / `opencode-desktop` 显式启用，`codex-cli` 的 tools 为空（后续五 scope 重新授权及工具运行见 R8）；未信任发布者被本机原生阻断，临时公钥指纹经过 UI 核对。运行范围为**自有源码 debug WebView 与本机 HTTP 服务**。本任务未独立核对这些新收据，保留协作来源；已验证的本机发布、安装、宿主工具调用只按上述范围归档，八行真实第三方客户端仍待验收。后续现场结论由主代理接管。
- 同日**主代理协作回传，已核对落盘证据 R5–R8**：在同一隔离现场，合成 Claude Desktop 配置的共享注册及真实 `xy-switch.exe --shared-mcp-stdio` 已通过；协议初始化、授权工具列表、单次 UI 审核/拒绝、审批防重放、结构秘密阻断、普通文本脱敏、两个独立 stdio 会话使用同 ID `901` 的取消隔离均有对应记录。范围为**合成客户端配置与 MCP 调用方、自有 debug WebView/原生插件宿主、真实 XY 可执行文件 stdio 桥、本机 HTTP**，没有启动真实第三方客户端。审核/拒绝条目引用早期整体失败尝试中的已通过子项，具体边界见 R6。
- 此前 `shared_mcp_config` 失败按主代理确认是隔离 home 缺少客户端配置目标触发产品正常拒绝；创建合成配置夹具后 R5 注册成功，与 C9 的目标预检一致。它不再是当前注册阻塞，也不是此前 52 项沙箱目录句柄权限失败的延续。R7 已证明 Codex/OpenCode 两个品牌的共享注册与显式跨 Desktop/CLI 连接 scope 同意。**R8 已闭合插件 scope 的先负向、后显式授权及正向工具运行**：授权准确包摘要及 `codex-desktop,codex-cli,claude-desktop,opencode-desktop,opencode-cli` 五个 scope；`codex` / `opencode` / `claude-desktop` 三个真实 XY stdio owner 同 RPC ID `1701` 产生三条独立审核，各自确认后返回 result，三个调用参数摘要与截图原文相符。`claude-code` / `gemini-cli` 未授权。共享配置恢复/重启、真实第三方消费及八行完整客户端现场验收继续待验收；详细收据和计数边界见 R8。

## 2. 逐客户端矩阵

### 2.1 版本、配置路径与归属

路径列是**代码选定的目标**，没有检查本机真实凭据或确认原生应用正在使用这些路径。`home` 指管理器接收的用户目录；不得把配置存在、PATH 命令名或本任务宿主 Codex 的版本当作受测客户端版本。

| 客户端 / ID | 已有可归属的版本证据 | 当前代码目标与 owner | 安装及有效来源缺口 |
| --- | --- | --- | --- |
| Codex Desktop / `codex-desktop` | 未取得受测桌面版版本 | `codex_home/config.toml`；owner=`codex`；与 CLI 同一驱动、快照、接管目标和 MCP 目标 | 需确认安装通道、GUI 所用 app-server、版本、实际 `CODEX_HOME`、凭据存储与环境覆盖；选定 CLI 二进制不能证明 GUI 来源 |
| Codex CLI / `codex-cli` | **历史 0.160.0**，见 R4；本轮实际安装版本及绝对二进制路径未复核 | 同上；驱动启动设置的 `codexBinary`，留空使用 PATH 的 `codex app-server`；启动目录由 `CODEX_HOME` 或默认 `home/.codex` 解析（C12） | 历史报告不绑定当前路径设置或新增原生账号代码；需单独确认 CLI 二进制、鉴权来源与配置优先级 |
| Claude Desktop 标准 / `claude-desktop` | 未取得版本 | Windows：`home/AppData/Roaming/Claude/claude_desktop_config.json`；macOS：`home/Library/Application Support/Claude/…`；Linux 分支：`home/.config/Claude/…`；owner=`claude-desktop` | 这是代码的 MCP 文件定位；Linux 路径分支不证明厂商提供该平台客户端。登录/订阅账号来源没有探测 |
| Claude Desktop 3P / `claude-desktop-3p` | 未取得版本；官方字段引入版本不等于本机版本 | 必须显式选择已保存 JSON；Windows 限于 `home/AppData/Local/Claude-3p/configLibrary/`，macOS/Linux 对应 C3 的应用目录；拒绝 `_meta.json`；owner=`claude-desktop-3p` | 需核实具体 3P 版本保存、激活位置与标准 MCP 配置是否共用；组织 bootstrap 配置被拒绝自动改写 |
| Claude Code / `claude-code` | 未取得版本 | 卡片预览 `home/.claude/settings.json`，owner=`claude`；接管兼容旧 `home/.claude/claude.json`；MCP 在 `home/.claude.json`（C6/C9） | 无二进制/登录账号探测；需检查项目、环境及组织策略对用户配置的覆盖 |
| OpenCode Desktop / `opencode-desktop` | 未取得版本 | owner=`opencode`；配置按显式设置 → `OPENCODE_CONFIG` → `XDG_CONFIG_HOME/opencode/` 或 `home/.config/opencode/` 解析；与 CLI 共用 XY 驱动及快照 | XY 启动的 `opencode serve` 不证明桌面自带后台连接同一配置、用户数据、通道或数据库 |
| OpenCode CLI / `opencode-cli` | 未取得版本 | 同上；选定 `opencodeBinary` 或 PATH 的 `opencode`；JSON 与 JSONC 并存时拒绝自动选取 | V1/V2 按配置结构及显式选项辨识，未探测二进制版本；真实 TUI 项目覆盖及账号库需独立确认 |
| Gemini CLI / `gemini-cli` | 未取得版本 | `home/.gemini/settings.json`、`home/.gemini/.env`，owner=`gemini` | 无二进制或 Google/Vertex 鉴权来源探测；环境、项目配置和当前模型未读取 |

### 2.2 实现、测试及现场缺口

证据编号的完整路径、函数和测试边界见第 3 节；现场操作编号见第 4 节。

| 客户端 | 代码已支持 | 已测的具体范围 | 完整现场尚缺 |
| --- | --- | --- | --- |
| Codex Desktop | 借用 CLI app-server：设备授权、账号/套餐/原生限额读取、managed 刷新请求、取消登录、退出；Responses 接管和 Codex MCP（C1/C2/C6/C9） | T1/T2/T4 的合成测试；R7 为合成配置注册与连接 scope 同意，R8 为五插件 scope 授权后的 Codex owner 真实 XY stdio 工具/独立审核；没有真实 Codex GUI 运行证据 | 桌面安装与鉴权发现、GUI 订阅请求、实际 provider/model、GUI 工具、断连重启恢复、与 CLI 的凭据所有权协调（A1） |
| Codex CLI | 同一账号驱动；网关 provider/model/catalog；备份恢复及 MCP（C2/C6） | T2；R4 为真实 CLI + 生产代理 + 本地确定性上游夹具的三种协议接入、工具与恢复；R7 为合成配置注册；R8 为共配置未授权时拒绝、五 scope 显式授权后的 Codex owner 真实 XY stdio 工具 | 新设备授权/刷新/退出的真实运行、订阅上游、原生账号入池及切换、CLI/Desktop 并发刷新、真实 CLI 共享插件消费（A2）；R8 调用方为合成 |
| Claude Desktop 标准 | 独立卡片和原生登录提示；标准 MCP 配置导入/同步；stdio 工具及共享 HTTP→stdio 桥入口；无推理接管按钮（C1/C9） | MCP 配置/桥合成测试、前端卡片 mock（T4/T5）；R5/R6 为合成配置/调用方、自有 WebView/宿主与真实 XY stdio 桥；R8 增补三 owner 并发的 Claude Desktop owner 独立审核/工具结果 | XY 原生登录入口及授权状态读取尚缺实现；真实标准登录、订阅路由是否开放、官方桌面扩展和 Claude Desktop 实际 MCP 执行需分别验收（A3） |
| Claude Desktop 3P | 已保存配置选择校验；写 `inferenceProvider=gateway`、`inferenceGatewayBaseUrl`、本地网关 key；Messages 接管（C3/C6） | T3 的配置转换/组织配置拒绝；通用恢复机制测试及前端 mock，未取得 3P 原生恢复证据 | SSO/网关鉴权入口、激活及当前模式读取、桌面 Messages/流式/工具/模型发现/缓存字段、认证过期、预算和恢复；独立插件/共享 scope 已补齐，真实 3P 消费仍待验收（A4） |
| Claude Code | Anthropic 用户配置接管及 MCP；页面提示原生终端登录（C6/C9） | 用户配置保留、精确恢复、外部变更冲突测试（T6）；客户端 UI mock；R8 未选 scope 保持未授权 | XY 官方登录/账号状态/刷新及原生池绑定未接驱动；真实 CLI 请求、轮换、工具会话、取消和恢复（A5） |
| OpenCode Desktop | 受密码保护的回环原生 API；V1/V2 OAuth 方法、条件输入、自动/手动回调、取消、提供商连接读取及按连接退出；两版 provider/model/baseURL 和 MCP（C4/C5） | T7 的 14 项本地 HTTP 合成测试，未启动 OpenCode；配置转换测试；R7 为合成配置注册与连接 scope 同意；R8 为未授权空交集/拒绝及五 scope 授权后的 OpenCode owner 真实 XY stdio 工具/独立审核 | API-key 方法被过滤；GUI 后台来源与 XY 服务同库未证明；版本、GUI 推理/模型/工具、V2 账号同步与原生推理轮换、服务重启恢复的真实验收（A6） |
| OpenCode CLI | 与 Desktop 共用上述驱动及接管；运行原生服务时固定 home/所选配置，禁止项目覆盖（C4/C5） | 同 T7；测试中的 `child: None`；R7 为合成配置注册；R8 为共配置未授权时拒绝、五 scope 显式授权后的 OpenCode owner 真实 XY stdio 工具 | 真实 OAuth/API key、服务 API 版本兼容、TUI 全局/项目优先级、取消与退出、轮换及恢复；真实 CLI 共享插件消费仍待验收（A7），R8 调用方为合成 |
| Gemini CLI | 写 Gemini API key 网关环境、`GEMINI_MODEL` 并强制 `selectedType=gemini-api-key`；Gemini MCP 配置（C6/C9） | Gemini 配置转换/字段保留及通用恢复的合成测试（T6）；UI mock；R8 未选 scope 保持未授权；没有 Gemini 专属真实恢复证据 | Google 登录/Vertex 驱动、授权状态及原生池接入尚缺；真实选模、请求、工具、取消、退出和恢复（A8） |

### 2.3 共用要求的具体差距

| 主计划要求 | 当前证据 → 结论 | 闭合方式 |
| --- | --- | --- |
| 安装版、安装通道、桌面/CLI 独立识别 | C1 只有独立 ID；`config_detected`/C9 `is_installed` 检查文件或目录，未查询二进制版本 | 实现安装/有效来源发现后，逐行记录实际二进制、版本、OS/架构及 GUI 后台来源；不能按共享 owner 合并收据 |
| 明确授权来源、读取与写入预览 | C8 有授权确认和路径设置；Codex 卡片只列 config，C6 实际还改 `auth.json` 与 model catalog；Claude Code 可能选旧配置 | 预览覆盖实际写入集、备份与有效来源；原生授权模式和安全存储引用仍须实际探测，不导出秘密值 |
| 原生刷新协调与账号池 | C1 已通过 `NativePoolBridge` 持有共享 `AccountPool`；Codex managed ChatGPT 与 OpenCode V2 凭据连接可显式绑定、同步授权/额度和撤销。相同 owner 操作串行，同一绑定的重叠刷新合并；原生引用不进入 HTTP 凭据轮换 | 原生推理调度/轮换适配器尚缺；真实原生进程并行刷新、实际订阅请求及凭据所有权仍需逐客户端验收 |
| 锁、备份、原子写、恢复与冲突选择 | C7 owner 锁覆盖 XY 进程内登录/接管/MCP 写入；C6 有加密备份、原子替换和 hash 冲突保护；路径变更要求重启 | 跨原生进程刷新锁未证明；实际外部变更时的差异展示及用户恢复选择尚未证明。需逐实际配置执行恢复与冲突用例 |
| 显示有效账号、provider/model、来源 | C1 有脱敏账号模式/套餐/限额/provider 连接快照；C8 显示描述与配置路径；模式字符串是静态声明 | 补有效 provider/model/安装来源/鉴权来源探测；未知额度保持未知，不能由登录成功推断额度或订阅可路由 |
| 自有插件与共享 MCP 的八行适配 | C10 的插件兼容名单已含八个 ID，新增独立 `claude-desktop-3p` scope；标准 Desktop 与 3P 共用配置 owner，连接取两者授权交集 | 独立 scope 与合成桥测试已补齐；实际两种模式的 MCP 消费、共享桥和官方桌面扩展仍分别验收 |

## 3. 当前源码与测试证据

### 3.1 生产代码索引

| 编号 | 具体路径 / 函数 | 能证明的范围 |
| --- | --- | --- |
| C1 | [clients.rs:129](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/clients.rs:129>) `ClientManager::connections`；同文件 `read_account`、`start_login`、`login_methods`、`disconnect_provider` | 八行身份/owner/路径/模式；只有 Codex/OpenCode 接原生账号操作，Desktop/CLI 各共用一个品牌驱动及快照；通过 `NativePoolBridge` 持有共享 `AccountPool`，原生操作按 owner 串行 |
| C2 | [clients/codex.rs:76](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/clients/codex.rs:76>) `CodexDriver::spawn`；`account`（189）、`start_login`（217）、`login_progress`（256）、`cancel_login`（268）、`logout`（280） | 选定或 PATH 的 `codex app-server`；设 `CODEX_HOME`、移除 `OPENAI_API_KEY`；初始化及有界 RPC、设备授权与账号/限额读取。`account/read` 传 `refreshToken`，managed 生命周期由 app-server 持有；识别 external 模式的快照不证明 XY 已提供外部令牌刷新回调。没有桌面内部二进制发现或实际订阅请求验收 |
| C3 | [client_config.rs:140](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/client_config.rs:140>) `ClientTargets::resolve`、`validate_settings`（164）、`detect_opencode_schema`（187）、`managed_opencode`（197）、`managed_claude_desktop`（215） | OpenCode 配置覆盖/JSONC/版本校验；3P 限已保存 configLibrary JSON、拒绝 `_meta.json` 和组织 bootstrap；两版 provider/model/baseURL、3P gateway 字段转换；没有激活或 SSO 实现 |
| C4 | [clients/opencode.rs:315](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/clients/opencode.rs:315>) `OpenCodeDriver::spawn`、`LocalApi::send`（65） | 受随机密码保护的 `serve --hostname=127.0.0.1 --port=0`；固定 home/配置并禁项目覆盖，移除内容/目录覆盖；检查回环端点、禁重定向、限制响应；不验证 GUI 后台的实际数据库 |
| C5 | [clients/opencode.rs:257](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/clients/opencode.rs:257>) `parse_methods`、`account`（386）、`disconnect`（397）、`start_login`（430）、`login_progress`（497）、`complete_login`（524）、`cancel_login`（545） | OAuth 和条件输入/回调/连接退出；`:275` 显式过滤非 OAuth 方法，API-key 登录不能据此标为支持；OpenCode 快照未生成真实额度或刷新保证 |
| C6 | [takeover.rs:181](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/takeover.rs:181>) `AppTakeoverManager::files`、`build_managed_files`（845）、`restore_file`（774）、`atomic_write`（1271）；`managed_gemini_env`（1194）、`managed_gemini_settings`（1233） | Claude/Codex/Gemini/OpenCode/3P 配置转换；Codex 三文件写入集；备份与恢复冲突保护；Gemini 同时写 API-key baseURL、鉴权模式和 `GEMINI_MODEL`；重复模型变量归一化、非法模型拒绝及原值恢复已有合成测试 |
| C7 | [client_config.rs:20](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/client_config.rs:20>) `ClientConfigLocks::acquire`、`require_restart`（30）；[clients.rs:342](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/clients.rs:342>) `save_client_settings` | XY 内部配置 owner 排他、登录期间锁、路径保存后旧管理器失效；并非与原生凭据刷新进程共享的锁 |
| C8 | [ClientConnectionsPage.tsx:141](<E:/XY switch/workspaces/session-favorites-20261029/src/features/clients/ClientConnectionsPage.tsx:141>) 账号操作与路径设置（153）；[clients-service.ts:50](<E:/XY switch/workspaces/session-favorites-20261029/src/lib/clients-service.ts:50>) `readClientAccount`、`startOpenCodeLogin`、`completeClientLogin`、`clientAuthorizationUrl`；[OpenCodeLoginForm.tsx:1](<E:/XY switch/workspaces/session-favorites-20261029/src/features/clients/OpenCodeLoginForm.tsx:1>) | Codex/OpenCode 账号按钮、逐操作授权；安全链接及通用错误提示；请求引用 finally 清理、条件私密输入与有界授权码。其他客户端原生登录仅提示，页面不探测当前安装/有效 provider/model |
| C9 | [mcp.rs:658](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/mcp.rs:658>) `McpManager::config_path`、`is_installed`（668）、`write_app_entry`（678）、`write_json_entry_schema`（826） | 五组配置读写；Claude Code `.claude.json`、Codex TOML、Gemini/标准 Desktop `mcpServers`、OpenCode V1 `mcp` / V2 `mcp.servers`。保存前预检目标（613–614），检测取配置文件或目录是否存在（668–674），不能证明真实客户端已安装。标准 Desktop 直接配置只接受 stdio，HTTP 走共享桥 |
| C10 | [plugins/types.rs:11](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/plugins/types.rs:11>) `SUPPORTED_CLIENTS`；[shared_mcp/types.rs:54](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/shared_mcp/types.rs:54>) `clients_for_owner`；[shared_mcp/manager.rs:210](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/shared_mcp/manager.rs:210>) `SharedMcpManager::register` | 自有插件兼容声明、共享 owner 范围和授权注册代码存在。注册用当前 XY 可执行文件及 `--shared-mcp-stdio <connection-id>` 构造配置（230–231），`manager.save_server` 失败时删除临时 secret 引用并统一返回 `shared_mcp_config`（234），该错误没有保留具体保存失败原因。R5/R7 已在合成配置目标上完成三个 owner 注册，R6/R8 已运行三个 owner 的真实 XY stdio 桥；先前隔离目标缺失的拒绝已恢复。`tools`（350）按插件 ID/摘要/工具取连接内各客户端授权交集；`call`（413，415）在创建待审核前拒绝未授权工具，运行键为 `(connection.id, session_id, request_key)`（419），`decide`（388）只处理指定审核。R8 已确认五插件 scope 与连接 scope 的授权闭环。已收录独立 3P ID，标准/3P 共用配置时取授权交集；共用 owner 的桥运行不证明两个真实客户端已消费 |
| C11 | [accounts.rs](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/accounts.rs>) `AccountPool::update_native_state`、`invalidate_native_checks`；[clients/pool.rs](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/clients/pool.rs>) `NativePoolBridge`、`NativeOperations`；[clients.rs](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/clients.rs>) `bind_account`、`refresh_pool_account` | 原生账号入池、目的地确认、元数据同步、刷新协调和撤销已有生产调用链。运行时核验超过 300 秒或重启后要求重查；相同引用不能清除推理 401/冷却。没有注册原生 OAuth bearer 刷新器；`NativeManaged` 不作为 HTTP 网关凭据，原生实际推理调度仍待接入 |
| C12 | [lib.rs:202](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/lib.rs:202>) 初始化配置覆盖及管理器；[takeover.rs:19](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/takeover.rs:19>) `resolve_codex_home` | `CODEX_HOME`、`OPENCODE_CONFIG`、`XDG_CONFIG_HOME` 进入路径解析；默认 Codex home 及相对路径拒绝。这里只证明 XY 的路径来源，原生 GUI/CLI 的有效来源需另验 |

### 3.2 测试索引及边界

| 编号 | 当前测试定义 | 已覆盖 / 不能据此认定 |
| --- | --- | --- |
| T1 | [clients.rs:391](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/clients.rs:391>) `desktop_and_cli_have_independent_rows_and_explicit_shared_owners`，1 项 | 八行与共享 owner；不启动客户端、不证明桌面/CLI 的独立实际路径 |
| T2 | [clients/codex.rs:303](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/clients/codex.rs:303>) `fixture` 及后续 7 项 | 初始化、脱敏快照、登录完成先于响应、非法授权 URL、取消失败、帧大小/超时及未知额度；夹具明确 `child: None`，没有真实 app-server 进程 |
| T3 | [client_config.rs:231](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/client_config.rs:231>) 后续 5 项 | 同 owner 排他、路径保存后的重启约束、两版 OpenCode 转换、3P 字段保留/组织配置拒绝、相对覆盖拒绝；合成配置不证明原生消费 |
| T4 | [clients.test.tsx:58](<E:/XY switch/workspaces/session-favorites-20261029/src/clients.test.tsx:58>) 11 项 | Tauri invoke 为 mock：八入口、授权、输入/授权码清理、失败错误收敛、页面/弹窗关闭、安全 URL、条件字段和长度约束；浏览器渲染不证明原生登录 |
| T5 | [shared_mcp/tests.rs:1](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/shared_mcp/tests.rs:1>) 夹具及 `http_tests.rs`、`manager_tests.rs`、`bridge_tests.rs` 共 25 项 | 临时 home、内存 secret、合成签名 echo 插件，覆盖 HTTP/stdio、审批、scope 交集、取消、撤销、配置恢复等；不是任一第三方客户端连接或共享插件现场执行 |
| T6 | [takeover.rs:1512](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/takeover.rs:1512>) `claude_takeover_restores_exact_original`、`codex_takeover_restores_config_and_auth_exactly`、`startup_recovery_restores_active_takeover`、`restore_does_not_overwrite_external_change`；[takeover.rs:1572](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/takeover.rs:1572>) `codex_and_gemini_transforms_preserve_unrelated_settings`；[takeover_regression_tests.rs:260](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/takeover_regression_tests.rs:260>) Codex home 回归 | 合成文件的保留、恢复、外部变更保护及 Codex home 一致性；Gemini 有配置转换样本，跨版本客户端实际读取、Gemini/3P/OpenCode 原生恢复和跨进程刷新协调仍未证明 |
| T7 | [clients/opencode_tests.rs:12](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/clients/opencode_tests.rs:12>) 自建回环 HTTP server，14 项 | V1/V2 OAuth 索引、自动/手动回调、单次提交、取消、来源重查退出、输入/错误脱敏、有界输出；`:58` 构造 `child: None`，未启动 OpenCode，也未验证 GUI/TUI |

2026-11-04 基线客户端后端定义合计 **27 项**（manager 1 + Codex 7 + OpenCode 14 + config 5）；前端客户端定义 11 项。2026-11-05 新增原生池桥接与登录期间操作隔离测试，最新运行见增量记录；这些测试与真实客户端验收数量没有换算关系。

### 3.3 已有运行记录

| 编号 | 记录 | 可使用的结论 |
| --- | --- | --- |
| R1 | [unified-native-full-tests-20261104.log:75](<E:/XY switch/workspaces/session-favorites-20261029/.cache/unified-native-full-tests-20261104.log:75>)；结果在 772 行 | 较早的已有日志：695 项，688 通过、0 失败、7 忽略；不替代本轮当前工作树验证，也不与最新 690 项协作回传混为同一次运行 |
| R2 | [unified-opencode-loopback-tests-20261104.log:78](<E:/XY switch/workspaces/session-favorites-20261029/.cache/unified-opencode-loopback-tests-20261104.log:78>) | 已有 14 项通过，覆盖 T7 回环夹具；日志名称中的 native/loopback 不使它成为 OpenCode 原生客户端验收 |
| R3 | [unified-resume-frontend-tests-20261104.log:5](<E:/XY switch/workspaces/session-favorites-20261029/.cache/unified-resume-frontend-tests-20261104.log:5>) | 已有 53 文件、615 项通过；不证明浏览器外的账号、客户端或插件连接 |
| R4 | [responses-budget-acceptance-20261102.md:44](<E:/XY switch/workspaces/session-favorites-20261029/docs/responses-budget-acceptance-20261102.md:44>)；版本在 49 行，边界在 59 行 | 历史真实 Codex CLI **0.160.0**：Chat 桥接、原生直连、原生预算代理；生产代理连接本地确定性夹具，包含工具及恢复，报告记录新增收费请求 0。不能证明当前 OAuth/订阅上游、Desktop、原生账号池或共享 MCP |
| R5 | [native-shared-registration.json:1](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/unified-native-2026-11-04T08-20-52-989Z-faebd673/native-shared-registration.json:1>)；`registeredAt=2026-11-04T09:41:31.028Z` | `passed=true`，owner=`claude-desktop`，合成客户端目标的注册、原基线保留及配置不含秘密引用值。挂载命令为 `E:\XY switch\workspaces\session-favorites-20261029\src-tauri\target\debug\xy-switch.exe --shared-mcp-stdio <connection-id>`；这是 XY 桥目标，不是 Claude Desktop 安装/版本证据 |
| R6 | [native-shared-tools.json:1](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/unified-native-2026-11-04T08-20-52-989Z-faebd673/native-shared-tools.json:1>)；`startedAt=2026-11-04T09:57:03.296Z`；审核/拒绝来源为 [native-shared-tools-host-attempt-3.json:1](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/unified-native-2026-11-04T08-20-52-989Z-faebd673/native-shared-tools-host-attempt-3.json:1>) | 汇总 `passed=true`，六项 checks 通过。真实 XY 桥 PID `32248` / `24804` 均退出 0；transcript 记录 `initialize`、`tools/list`、结构秘密调用返回 `shared_mcp_privacy`、普通文本脱敏后调用返回 result（记录 redactions=1），主会话取消 `901` 后独立会话同 ID 仍返回 result。审核、拒绝及一次审批重放被拒的两项 checks 引用早期尝试；该来源整体 `passed=false`，只采纳其已通过子项，不宣称整次尝试通过。范围是自有 debug WebView/原生插件宿主、真实 XY stdio 桥及本机 HTTP，客户端配置与 MCP 调用方均为合成；不证明 Claude Desktop 实际消费或全部隐私外发路径 |
| R7 | [native-shared-scopes.json:1](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/unified-native-2026-11-04T08-20-52-989Z-faebd673/native-shared-scopes.json:1>)；`registeredAt=2026-11-04T10:00:01.271Z` | `passed=true`，合成配置的 `codex` / `opencode` 注册均 active，各映射 Desktop+CLI，`requiredExplicitScopeConsent=true`。此阶段只证明注册与连接 scope 同意，尚未给 CLI 插件授权；后续空交集、显式插件授权与正向运行由 R8 归档。不证明真实 Codex/OpenCode 客户端连接 |
| R8 | [native-shared-scope-tools.json:1](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/unified-native-2026-11-04T08-20-52-989Z-faebd673/native-shared-scope-tools.json:1>)；`startedAt=2026-11-04T10:12:41.746Z`、`completedAt=2026-11-04T10:12:46.144Z`；负向来源为 [native-shared-scope-first-host-attempt.json:1](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/unified-native-2026-11-04T08-20-52-989Z-faebd673/native-shared-scope-first-host-attempt.json:1>) | 汇总 `passed=true`，八项 checks 全通过：Codex/OpenCode 共配置连接在 CLI 未授权阶段空工具/`shared_mcp_tool` 拒绝；原生插件 UI 绑定准确摘要与五 scope；三个 owner 同 ID `1701`、`pendingCount=3`，分别审核后各自返回准确参数的 result；未选 Claude Code/Gemini scope 保持未授权。三个真实 XY stdio 进程均退出 0，transcript 中三个独立 `argumentsDigest` 与审核截图的各自 message 计算结果一致。首次负向来源整体 `passed=false`（后续授权弹窗定位等待超时），只采纳两项 `passed=true` 的拒绝子项。配置与 MCP 调用方均为合成，自有源码 debug WebView/原生插件宿主、真实 XY stdio 桥、本机 HTTP；没有真实第三方客户端执行 |

主代理协作回传的 690 项原生通过、本机市场投稿/发布/安装恢复及自有三桌面宿主工具调用见第 1 节；本任务未独立核对这些市场收据。R5–R8 及 R6/R8 引用的已通过子项按落盘内容核对，共享注册、五 scope 授权及三个真实 XY 桥 owner 只在上述隔离范围归档。R1–R3 的计数已在此前静态检查中核对既存日志，收据复核均不构成本轮重跑。

R8 的原生截图为 [五 scope 与精确摘要确认](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/unified-native-2026-11-04T08-20-52-989Z-faebd673/native-shared-cli-scope-consent.png>)、[三条待审核](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/unified-native-2026-11-04T08-20-52-989Z-faebd673/native-shared-three-owner-pending.png>)，以及 [Codex](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/unified-native-2026-11-04T08-20-52-989Z-faebd673/native-shared-codex-execution.png>) / [OpenCode](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/unified-native-2026-11-04T08-20-52-989Z-faebd673/native-shared-opencode-execution.png>) / [Claude Desktop](<E:/XY switch/workspaces/session-favorites-20261029/artifacts/unified-native-2026-11-04T08-20-52-989Z-faebd673/native-shared-claude-desktop-execution.png>) 的独立参数/权限审核。五个勾选 scope、两个未选 scope、精确摘要输入及完整审核确认均可见；三张审核截图的背景待办分别为 3/2/1 条。**负向 `pending=0`、每次确认后剩余 2→1→0、未选 scope 的工具数组为空，采纳主代理协作回传**：JSON 保存 `pendingCount=3`、八项通过断言、原生命令名及 transcript 结果类型，没有保存这些查询的完整返回值或最终清零截图。`tools/list` 空列表和工具返回精确参数由通过断言支持；本次独立核对限于断言、transcript、摘要及现有截图，没有把缺失的原始返回值写成已独立复现。

## 4. 最小可执行现场步骤

以下步骤交给后续获得独立现场授权的操作者执行，**本文没有执行这些操作**。桌面端使用独立测试 OS 用户/虚拟机；CLI 使用隔离用户目录及空项目，保持与主代理当前市场运行环境分离。先完成一行的最小闭环，再执行跨客户端联合用例。暂未安装的客户端记录为未安装，不以安装计划代替收据。

### 4.1 每行的记录与工具收据

每行记录 OS/架构、安装来源/通道、原生版本、绝对二进制或桌面后台路径、XY 构建来源、有效配置绝对路径、鉴权模式及凭据所有者、provider/model、是否经过 XY、任务/会话标识、执行时间和收据路径。版本由原生“关于”页或选定二进制的 `--version` 获取；路径和配置优先级分别核对。证据仅保留脱敏状态和非秘密配置字段，密码、令牌、设备码、SSO 回调和邮箱不进入报告。

需要先证明直接 MCP 调用时，可使用已存在的 [live_receipt_mcp.mjs:6](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/governance/live_receipt_mcp.mjs:6>)：由操作者准备独立空收据目录和新 nonce，在该客户端的 stdio MCP 配置中以已安装 Node 的绝对路径作 command，args 依次为脚本绝对路径、收据目录、nonce。在**原生会话**要求调用一次 `receipt`，参数为 `a=17`、`b=25`、本次 nonce。

通过需同时看到原生工具结果、`receipt.json` 中的 sum=42 和新随机收据，以及 `mcp-calls.jsonl` 中对应一次 `valid=true, duplicate=false` 调用。脚本会写上述文件，重复调用返回错误；它只提供直接工具消费证据，不能证明订阅上游、安装包可信、自有插件宿主或共享实例复用。共享插件使用 4.3 的独立收据。

### A1. Codex Desktop

1. 从受测桌面“关于”页和安装来源记录版本；确认 GUI 后台路径、有效 `CODEX_HOME` 与配置/鉴权来源。将 XY 选定 app-server 的来源与 GUI 后台逐项对齐；无法对齐时记录来源缺口，不沿用 CLI 结果（C1/C2/C12）。
2. 先在关闭接管的条件下，经 XY 的该卡片执行“读取账号 → 设备授权 → 完成检查 → 刷新账号”。操作者在官方授权页完成登录；保存账号模式/套餐/额度是否已知，单独在 GUI 发一条最小订阅请求并记录实际供应商与模型。
3. 在隔离网关账号下启用接管，核对 C6 的实际写入集；从 GUI 发一条请求并调用一次 4.1 工具，核对代理关联事件和原生工具收据。账号读取成功不能替代这一步。
4. 发起第二次授权后取消；在独立测试账号上退出，再做断连、GUI/XY 冷启动与退出接管恢复。与 CLI 同时存在时追加 4.3 B1/B2；目前没有该桌面原生闭环收据。

### A2. Codex CLI

1. 对选定 CLI 绝对二进制运行 `--version`，固定隔离 `CODEX_HOME` 和空项目；记录与 XY `codexBinary` 的对应关系。R4 的 0.160.0 只属于历史运行。
2. 用 CLI 卡片完成设备授权、读取和刷新；从该二进制开启实际 CLI 会话，先完成关闭接管的授权订阅请求，再在接管后完成网关请求和 4.1 工具。分别记录直接订阅和网关来源，保持同一会话执行一次恢复（C2/C6）。
3. 验证登录取消、CLI 任务取消、退出和原配置恢复；同时运行 A1 检查同一凭据来源的刷新/锁。账号池绑定已接入 C11；原生账号轮换仍须补齐实际推理调度适配器，手工更换登录账号不算池调度通过。

### A3. Claude Desktop 标准登录

1. 记录原生安装版本、通道及当前模式；在原生界面完成已授权测试账号登录，记录原生授权状态和一条订阅请求。XY 当前只有登录提示，**XY 登录入口/账号发现/刷新在此仍是实现缺口**（C1/C8）。
2. 在 XY 的标准 Desktop MCP 入口挂载 4.1 的 stdio 工具，核对 C9 的实际配置来源；从原生会话执行一次工具。共享 HTTP 服务另经 XY stdio 桥执行 4.3 B3；若声明支持官方桌面扩展，再独立安装/调用该类型并记录来源。
3. 在原生界面测试取消、退出与重启；撤销 XY 的测试 MCP 挂载后确认原配置保留。标准订阅是否允许 XY 推理路由独立调查；当前无接管按钮、无桌面路由收据，不能从 MCP 成功推断其开放。

### A4. Claude Desktop 3P

1. 在已安装的受测版本中保存并激活一个独立 3P 测试配置，记录网关/SSO 鉴权方式、配置归属和实际保存位置。XY 只接受 C3 的 configLibrary 内具体 JSON；原生实际路径不匹配或配置受组织 bootstrap 管理时，记录接口/版本条件并停止自动改写。
2. 在 XY 选择该已保存配置，预览 C3/C6 写入集后启用接管；在原生 3P 界面重新激活并重启，核对实际生效模式。从桌面发送 Messages 普通/流式请求、选择模型并执行工具；核对模型发现和 `cache_control` 保留。写入成功不证明配置激活。
3. 在专用测试鉴权下触发一次到期/失效并观察 SSO 或网关重新鉴权，再执行预算发送前阻断、任务取消、关闭接管及恢复。XY 当前没有 SSO 流程或模式探测，相关集成项保留实现缺口。
4. MCP 配置复用标准 Desktop 的行为需要原生核对；独立 3P 插件兼容 ID 和共享 scope 已补齐（C10），同配置连接要求标准与 3P 的授权交集。标准 Desktop 的历史工具调用不能填充 3P 真实客户端权限格子。

### A5. Claude Code

1. 以选定绝对二进制记录版本，在隔离空项目的原生登录入口完成测试账号登录。核对用户 `settings.json`、兼容旧 `home/.claude/claude.json`、项目配置和环境覆盖；MCP 另查 `home/.claude.json`（C6/C9）。XY 原生登录/账号状态读取没有生产驱动。
2. 预览并启用 Anthropic 接管，启动实际 CLI 会话；核对当前 provider/model、一次 Messages 请求及 4.1 工具，恢复同一工具会话。原生登录请求和网关请求分开记录。
3. 原生 `/logout` 后再登录，测试任务取消、接管关闭及外部配置修改恢复。刷新/账号轮换在接入 C11 的生产原生绑定后执行 B2；原生手工登录只证明客户端自身能力。

### A6. OpenCode Desktop

1. 记录桌面版本、安装通道、GUI 后台二进制/服务地址/数据目录及配置；同时记录 XY 所选 `opencodeBinary`、配置、V1/V2 判定及 C4 回环服务的来源。V2 可由同一受测 CLI 的 `opencode debug paths db` 获取数据库**路径**，不打开凭据库；不能假定与 GUI 同库。
2. 从 XY 的 Desktop 卡片列出方法并完成一条实际 OAuth 的自动或手动回调；随后在 GUI 确认对应连接。另检查原生 API-key 登录，XY `parse_methods` 会过滤该方法，缺少方法时记录实现缺口（C5）。
3. 接管后在真实 GUI 选择 `xy-switch/模型`，完成一条请求和工具调用，核对实际 provider/model/baseURL 来源；执行授权取消、指定连接退出、GUI 后台及 XY 冷启动、配置恢复。XY 回环 API 的成功结果不能替代 GUI 会话。

### A7. OpenCode CLI

1. 对所选绝对二进制运行 `--version`，固定隔离用户目录、全局配置和空项目，记录 V1/V2 与账号存储来源；先以同一用户环境确认 XY 服务与真实 CLI 使用同一来源。
2. 从 XY CLI 卡片完成实际 OAuth，并从原生 TUI 验证连接、选模、网关请求和工具。V2 可使用原生 `/connect`、`/models` 核对；API key/授权命令另在原生入口验证，当前 XY OAuth 筛选不覆盖这些方法。
3. 在另一隔离项目设置合法项目模型覆盖，记录原生 TUI 实际优先级及 XY 全局接管边界；再验证授权取消、任务取消、指定连接退出、服务重启及原配置恢复（C3/C4/C5）。当前没有原生刷新额度或账号池选择闭环。

### A8. Gemini CLI

1. 记录绝对二进制、版本、用户/项目配置及 `.env` 优先级；从原生鉴权入口分别标明实际测试的 Google 登录、API key 或 Vertex 模式。XY 当前没有这些原生登录/状态驱动（C1/C8）。
2. 在 API-key 网关模式启用接管，核对 `GOOGLE_GEMINI_BASE_URL`、鉴权类型和实际请求。C6 已写 `GEMINI_MODEL`，需核对该值在真实客户端是否生效，以及项目覆盖、原生 `/model` 选择与关闭接管恢复的优先级；合成配置转换不代表真实选模验收。
3. 在实际 CLI 会话完成一条请求、一次 4.1 工具、任务取消，再通过该版本的原生鉴权入口退出并重启、关闭接管恢复。Google/Vertex 的原生账号池、刷新/轮换及 Gemini 专属原生恢复仍待对应实现和收据。

### 4.3 联合验收与明确停止条件

| 编号 / 范围 | 最小操作与通过证据 | 当前差距与边界 |
| --- | --- | --- |
| B1 配置、断连和外部变更 | 每行先核对实际写入预览/备份；接管期间由原生设置界面修改一项合法非秘密配置，再退出接管。须保留外部变更、显示冲突及恢复选择；无冲突时恢复原文件/原存在状态；断连与冷启动后不遗留错误接管。证据绑定实际配置来源与恢复事件（C6/C7/T6） | 当前锁在 XY 内部；跨原生刷新进程的凭据所有权协调，以及真实差异选择界面尚未验收。路径保存要求 XY 重启，不能用旧管理器结果验新路径 |
| B2 原生账号池、预算、轮换和取消 | 原生状态/刷新/选择链接好后，用两套独立测试账号验证并发过期刷新、额度拒绝、账号轮换和等待刷新时取消；同一 task/session 的尝试、预留、费用/未知费用连续，工具业务动作无重复，取消后无新增发送。恢复后状态及额度来源可追溯（C11，主计划 4.1/4.2） | 通用代理池测试可先验证调度；原生客户端 `update_native_state`/`register_refresher` 尚无生产调用者，当前不能执行原生池完整闭环。订阅限额与美元费用分开，缺来源维持未知 |
| B3 自有插件与共享 MCP | 在每个真实客户端隔离现场记录共享注册、实际 stdio 桥启动及 `initialize`、`tools/list`、`tools/call` 的关联收据。用已批准、摘要绑定的同一个已安装工具包，只显式授权三个桌面 ID；分别从真实 Codex Desktop、Claude Desktop、OpenCode Desktop 同时执行一次工具，绑定包 ID/版本/摘要、客户端/任务、运行实例与独立随机收据。原生 Codex CLI 同时连接时应看不到未授权工具；品牌连接同时映射 CLI 时，先验证未授权 CLI 导致的工具空交集，再获得显式插件 scope 同意并验证正向结果；用第二个独立用户验证隔离。再验证权限拒绝、取消、一个客户端退出、插件崩溃、XY 冷启动/重启，检查其他客户端连接及会话不受误停/泄漏（C9/C10/T5） | 已归档自有三桌面宿主调用及 R5–R8：三个 owner 的合成配置注册，R6 的协议/工具/审核/拒绝/审批防重放/脱敏与同 ID 取消隔离；R8 的共配置未授权空交集/拒绝、五插件 scope 显式授权、三真实 XY stdio owner 同 ID `1701` 的并发独立审核与准确参数结果。计数证据边界见 R8。真实客户端身份/scope 消费、跨用户、撤销/配置恢复/崩溃重启仍待验收；七个兼容 ID、3P 独立 scope 和全部包类型不能由这些代表用例代替 |
| B4 插件完整生命周期与市场来源 | 每个声称支持的包类型依次安装 → 原生实际业务 → 升级新增权限确认 → 回滚 → 停用/卸载；核对批准版本摘要、签名信任提示、配置恢复和不再可调用。跨设备普通用户使用同一公开版本源再安装运行；绑定部署地址/版本与匿名目录隐私检查 | 投稿、发布/安装恢复、临时公钥 UI 核对的最新范围见第 1 节。公共 HTTPS 服务、跨用户/跨设备，以及真实第三方客户端业务运行没有在本矩阵验收；这些现场结论交给主代理 |
| B5 输入、流式、工具与诊断隐私 | 用合成秘密/个人信息分别进入输入、工具参数/结果、鉴权失败、工具异常、SSE 分块、附件/压缩包和崩溃路径；对每行核对实际模型外发、显示、磁盘及诊断导出。严格扫描失败需阻止外发；授权本地执行器取得允许的执行值，模型/其他会话不能获得原值和脱敏映射；同时记录业务是否完成和延迟 | [proxy.rs:1540](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/proxy.rs:1540>) `inspect_request_privacy` 和 [proxy.rs:1191](<E:/XY switch/workspaces/session-favorites-20261029/src-tauri/src/proxy.rs:1191>) `inspect_response_privacy` 证明代理检查入口；只有实际经过 XY 的请求才能据此验证。R6 已覆盖自有宿主/真实 XY 桥的合成结构秘密阻断与普通文本脱敏；模型外发、工具结果、SSE、附件/压缩包、持久化和诊断导出仍待实际路径证据。原生直连订阅、外部 MCP 或官方扩展不自动受其保护，T4/T5 与这两项桥用例不能代替其真实外发验收 |

## 5. 官方来源与适用边界

2026-11-04 对下表所标“本轮读取”的公开页面进行了获批准的只读 HTTPS 获取，均返回 HTTP 200；没有下载/安装客户端，也没有写入官方页面。资料说明接口或字段的语义，不能证明本机安装版本、XY 的调用成功或第三方实际消费。官方页面更新后仍需与受测版本绑定。

| 编号 / 官方来源 | 核对内容与矩阵约束 |
| --- | --- |
| S1 [Codex app-server](https://developers.openai.com/codex/app-server)，本轮读取 | `account/read`、设备授权、取消、退出和额度 API；managed ChatGPT OAuth 由 Codex 持久化/刷新，external tokens 模式由宿主响应 `account/chatgptAuthTokens/refresh`。C2 的 managed 刷新请求不能泛化为 external 生命周期或桌面订阅已验收 |
| S2 主计划已有 [Codex provider 配置](https://learn.chatgpt.com/docs/config-file/config-advanced)、[Windows 部署](https://learn.chatgpt.com/docs/enterprise/windows-deployment)、[ChatGPT 授权配置](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server) | 沿用主计划 2026-11-03 的来源记录，三个页面本轮未重新获取；当前 API 语义核对采用 S1。不能用部署文档、接口存在或宿主 Codex 版本代替受测 Desktop/CLI 安装、权限范围与配置优先级 |
| S3 [Claude Desktop 3P 安装](https://claude.com/docs/third-party/claude-desktop/installation)，本轮读取 | 部署/原生配置生效与当前版本的支持范围单独核对。该安装页未检出 configLibrary 字段不能证明 C3 路径错误；也不能证明其路径已适用于本机，必须执行 A4 的实际保存/激活核对 |
| S4 [Claude Desktop 3P gateway](https://claude.com/docs/third-party/claude-desktop/gateway)，本轮读取 | Messages 兼容、自托管 gateway 及 prompt caching 语义；A4 要保留流式、工具及 `cache_control`，实际模型发现/SSO/失效/预算仍需各自运行证据 |
| S5 [Claude Desktop 本地 MCP](https://support.claude.com/en/articles/10949351-getting-started-with-local-mcp-servers-on-claude-desktop)，本轮读取 | 官方桌面扩展安装及组织权限控制有单独流程；其可用性不证明 C9 的原始 stdio 配置或共享桥消费，扩展/直接 MCP/共享插件分别验收 |
| S6 [Claude Code authentication](https://code.claude.com/docs/en/authentication)，本轮读取 | 原生账号/Console 登录和 `/logout`；原生自身支持这些入口不等于 XY 已有对应驱动、刷新协调或账号池绑定 |
| S7 [OpenCode config](https://opencode.ai/docs/config/)，本轮读取；主计划另引用 [providers](https://opencode.ai/docs/providers/) 和 [桌面排障](https://opencode.ai/docs/troubleshooting/) | JSON/JSONC、全局/自定义/项目/内联配置优先级；C4 禁用项目覆盖的受控服务与真实 GUI/TUI 有不同上下文。后两个链接保留主计划出处，本轮未重取 |
| S8 [OpenCode V2 CLI providers](https://opencode.ai/v2/docs/cli/providers)，本轮读取 | V2 API key/OAuth 存服务 SQLite；可用 `opencode debug paths db` 查询路径，通道、`XDG_DATA_HOME`、`OPENCODE_DB` 可影响位置。V1 auth.json 的迁移来源不能用作 V2 当前凭据所有者假设；原生 API-key/命令入口不被 C5 的 OAuth 筛选覆盖 |
| S9 [Gemini CLI configuration reference](https://geminicli.com/docs/reference/configuration/)，本轮读取 | `GEMINI_MODEL` 设置默认模型；`GOOGLE_GEMINI_BASE_URL` 适用于 gemini-api-key，Vertex 使用不同的 base URL 项；`security.auth.selectedType` 是所选鉴权类型。C6 的 API-key 接管不能泛化为 Google/Vertex 登录，也没有完成默认模型写入。旧 `/docs/get-started/configuration/` 返回页面但未取得所需字段，另试官方仓库旧鉴权路径返回 404，未将两者计作鉴权依据 |

## 6. 交付状态与待验收清单

2026-11-05 已将 Codex managed ChatGPT / OpenCode V2 原生账号绑定、状态同步、刷新所有权与登录互斥接入生产调用链；原生账号状态核验具有 300 秒运行时有效期，重启、退出、读取失败或配置失效后需要重新同步。身份只保存带盐摘要与不透明引用，原生凭据不进入 HTTP bearer 轮换。Claude Desktop 3P 已有独立插件 scope，共用配置按标准/3P 授权交集执行；Gemini 接管已写入默认模型并保存恢复基线。

本次原生整库验证在当前源码上执行：**719 通过 / 0 失败 / 7 忽略**，执行耗时 175.14 秒；命令为 `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`。其中 19 项原生池桥测试覆盖候选时效、精确目的地、跨 Desktop/CLI 去重、凭据代际、401/冷却保持、持久化回滚、同键并发刷新、不同键隔离与退出序列化。本机日志保存在忽略目录 `.cache/unified-integration-native-tests-20261105.log`。这些是单元/合成验证，不增加八行真实第三方客户端通过项。

- **版本/路径**：只有历史 Codex CLI 0.160.0 可归属；八行当前安装版本、绝对受测二进制及 GUI 后台有效来源没有本轮证据。第 2.1 节仅为代码目标路径。
- **生产接入**：仍需安装/有效来源发现、Claude 标准/Code/Gemini 原生登录与状态、3P SSO/激活/有效模式、OpenCode API-key/命令入口、原生推理选择/调度适配器及跨进程凭据归属验收。已实现的原生绑定/同步、Gemini 模型配置与 3P scope 不再列作缺失功能。
- **逐客户端运行**：A1–A8 均待实际版本上的登录、请求、工具、取消、退出、断连/重启与恢复；Desktop/CLI 独立收据不能按同 owner 合并。API/合成测试及历史 CLI 夹具不能填充订阅上游格子。
- **共享 MCP**：R5–R8 已证明合成 Claude Desktop/Codex/OpenCode 配置注册、共配置未授权拒绝、五插件 scope 显式授权，以及 `codex` / `opencode` / `claude-desktop` 三 owner 的真实 XY stdio 工具与同 ID 并发独立审核；先前隔离 home 目标缺失已恢复。之后的本机签名撤回、冷启动拒绝、三个 owner 撤销及再次重启清理已有通过收据，详见主计划第 11.2 节。跨用户及真实第三方客户端消费仍待验收；历史收据只有七个可用插件 scope，不能追认为新增 3P scope 的现场验收。
- **联合闭环**：B1–B5 的原生并发刷新、稳定预算/轮换/取消、真实三桌面同时共享自有插件、真实未授权 CLI 拒绝、业务生命周期与实际隐私外发/留存仍需现场证据。
- **市场交接**：投稿、发布/安装恢复、自有三桌面宿主工具调用及匿名目录/信任检查采纳第 1 节主代理协作回传；自有 debug WebView、本机 HTTP、临时公钥的结论保持原范围。公共部署、跨设备及真实第三方消费由主代理继续归档。

前端全量验证：**53 个测试文件、645 项全部通过**，包含客户端/账号绑定、授权、登录互斥和翻译目录；`tsc -b` 与 `vite build` 均退出 0。本机日志为 `.cache/unified-integration-frontend-tests-20261105.log` 和 `.cache/unified-integration-renderer-build-20261105.log`。构建保留已有的 Tauri 混合导入与单包体积提示，没有将浏览器构建解释为真实桌面客户端验收。

### 6.1 仍需实现的最高优先级缺口

以下是本次集成之后仍需完成的功能与现场闭环。原生绑定和状态同步已有实现与合成验证，不能据此宣称原生推理轮换已完成。

| 顺序 | 当前代码证据 | 最小实现闭环 |
| --- | --- | --- |
| 1 原生推理调度与轮换 | `ClientManager` 已通过 `NativePoolBridge` 持有共享 `AccountPool`，绑定与刷新命令已接入；HTTP 账号选择明确排除原生引用，尚无实际原生推理调度适配器 | 将任务选择、请求、取消、额度与失败结果接到正确的原生凭据所有者；验证实际原生后台切换及并发刷新，不能将元数据同步视为可用 HTTP bearer |
| 2 原生鉴权与有效状态驱动覆盖 | C1 的 `read_account`/`start_login`/`login_methods` 仅分派 Codex/OpenCode；Claude 标准 Desktop、Claude Code、Gemini 没有对应驱动；C3 只校验/选择 3P JSON，3P SSO、激活及模式仍为提示 | 按每个原生客户端支持的入口接登录/取消/退出及有效账号状态；为 3P 接独立 SSO/网关鉴权和保存配置的激活/当前模式读取。返回实际鉴权/配置来源与未知状态，连接成功后进入第 1 项池绑定 |
| 3 真实客户端消费与来源验收 | 独立 3P scope 与标准/3P 授权交集已经实现；既存现场仅覆盖合成配置与真实 XY stdio 桥 | 按 A1–A8 核对实际安装版本、鉴权/配置来源和有效模式，取得请求、工具、取消、退出及恢复收据；3P 独立验收不能由标准 Desktop 桥结果替代 |
