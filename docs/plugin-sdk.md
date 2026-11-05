# XY Switch Plugin SDK V1

插件包、市场审核和本地宿主共用 `src-tauri/src/plugins/types.rs` 与 `src/types/plugins.ts`。所有输入校验在本地 Rust 执行，前端校验只用于提示。V1 接收有边界的 UTF-8 文本包，宿主不运行第三方脚本。

## 接入宿主

由主程序集成以下内容；插件模块自身负责初始化独立元数据表。

```rust
mod plugins;

let plugin_dir = data_dir.join("plugins-v1");
app.manage(plugins::PluginManager::new(database.clone(), plugin_dir)?);
```

`PluginManager::new(Database, PathBuf) -> PluginResult<PluginManager>` 接收已有数据库的 clone 与宿主配置的绝对本地目录。Windows 使用普通盘符路径；UNC、设备路径和 verbatim 前缀不被接受。路径中的 symlink、junction、reparse point 和目录替换均不被接受。包内容不能提供安装根目录。数据库和目录需要保持绑定；换目录需要明确迁移独立插件存储。

将以下项加入已有的 `tauri::generate_handler![...]`：

```rust
plugins::commands::plugins_validate_package,
plugins::commands::plugins_preview_trusted_key,
plugins::commands::plugins_configure_trusted_key,
plugins::commands::plugins_list_trusted_keys,
plugins::commands::plugins_revoke_trusted_key,
plugins::commands::plugins_preview_install,
plugins::commands::plugins_preview_installed,
plugins::commands::plugins_confirm_install,
plugins::commands::plugins_list_installed,
plugins::commands::plugins_disable,
plugins::commands::plugins_enable_client,
plugins::commands::plugins_uninstall,
plugins::commands::plugins_preview_rollback,
plugins::commands::plugins_apply_withdrawal,
plugins::commands::plugins_tools,
plugins::commands::plugins_skills,
plugins::commands::plugins_provider_mappings,
plugins::commands::plugins_preview_tool,
plugins::commands::plugins_execute_tool,
```

调用参数使用 camelCase。`packageJson`、`publishedJson`、`keyJson`、`confirmationJson`、`argumentsJson`、`withdrawalJson` 都是 JSON **字符串**，由 Rust 严格解析；传入已解析对象会失去重复键信息。只有本地桌面 WebView 可调用这些管理命令。主代理应沿用现有 Tauri 窗口及能力边界，不能把安装确认和信任配置暴露为公共 MCP 工具。

前端嵌入接口是 `src/features/plugins/PluginsPage.tsx` 的命名导出 `PluginsPage`，无必须 props，直接使用 `<PluginsPage />`。外层需要现有 `QueryClientProvider`。页面真实调用 `src/lib/plugins-service.ts`，使用 `useLanguage` / `t` / `translateMessage`、现有 UI 组件和 `usePageOperation`。桌面环境由页面检测；浏览器预览只展示桌面要求，不伪造安装、授权或执行结果。

Rust 使用已有的 `tauri`、`serde`（derive）、`serde_json`、`ring`、`rusqlite`、`uuid`（v4）、`regex`、`tokio`（macros、net、time、sync）、`reqwest`（stream、rustls-tls），Unix 路径操作使用 `libc`。Windows 目录/文件身份检查使用已有的 `windows-sys` 0.61（`Win32_Storage_FileSystem`），相对句柄操作链接系统 `ntdll` / `kernel32`，无需新增 Cargo 依赖。前端使用已有的 React 19、`@tanstack/react-query` 5 与 `@tauri-apps/api` 2。

## 固定包协议

```ts
interface PluginPackage {
  manifest: PluginManifest;
  files: Record<string, string>;
}
interface PublishedPlugin {
  package: PluginPackage;
  digest: string;
  signature: string;
  signingKeyId: string;
}
```

`PluginManifest` 全部字段必须存在，字段顺序以 Rust 类型定义为准：

```json
{
  "schemaVersion": 1,
  "id": "example.package-reader",
  "name": "Package Reader",
  "description": "Read approved text bundled with this plugin.",
  "author": "Example Publisher",
  "version": "1.0.0",
  "minimumHostVersion": "1.2.0",
  "kind": "mcp",
  "entry": "tools.json",
  "clients": ["codex-desktop", "codex-cli", "claude-desktop", "claude-desktop-3p", "claude-code", "opencode-desktop", "opencode-cli", "gemini-cli"],
  "permissions": {
    "network": [],
    "readPaths": ["package:docs/"],
    "writePaths": [],
    "process": false,
    "credentials": []
  },
  "dependencies": []
}
```

- `id`、密钥 ID 是点分隔的小写标识，每段以字母开头，可包含数字和 `-`，每段最多 48 字符，总长最多 96。
- `version` 和 `minimumHostVersion` 使用严格 SemVer，最多 64 字节；核心数字不能有前导零，不能超出 `u64`。构建元数据不影响版本优先级。
- 八个客户端 ID 分别代表独立授权范围，桌面端授权不会自动授予同产品 CLI。Claude Desktop 标准模式与 3P 模式分别授权；共享同一配置的连接必须取得这两个 scope 的授权交集。
- 依赖格式为精确的 `id@version`，最多 32 项，同 ID 不允许重复或自依赖。不自动下载依赖；依赖及其传递依赖必须已安装为指定的活跃版本。运行时还必须已对同一客户端授权启用。
- 字段缺失、额外字段、重复 JSON 键和未知枚举均拒绝。包内可见公钥没有信任效力。

## 摘要与签名

权威计算接口：

```rust
plugins::validate_package(&package)?;
let bytes = plugins::canonical_package_bytes(&package)?;
let digest = plugins::package_digest(&package)?;
let payload = plugins::signing_payload(&package.manifest.id, &package.manifest.version, &digest)?;
```

`canonical_package_bytes` 精确等于 `serde_json::to_vec(&PluginPackage)`：无 BOM、无缩进、无尾换行。外层顺序是 `manifest`、`files`；manifest 顺序与示例一致，permissions 顺序是 `network`、`readPaths`、`writePaths`、`process`、`credentials`。`files` 是 `BTreeMap<String,String>`，键按 Rust 字符串字节顺序排序；数组保持输入顺序。JSON 文本文件的**原始字符串字节**也是摘要内容，不能重排 entry JSON 或改换行后保留旧摘要。不能使用通用递归键排序、RFC 8785/JCS 或浏览器的原始 `JSON.stringify` 替代 typed package 序列化。

摘要是上述字节的 SHA256 小写 64 字符 hex。Ed25519 原始签名为小写 128 字符 hex，签名原文是下面的 UTF-8 字节，最后没有换行：

```text
xy-switch-plugin-v1\n{id}\n{version}\n{digest}
```

此处 `\n` 表示一个 LF 字节。宿主查找用户预先独立配置、启用的 `signingKeyId`，公钥是 Ed25519 原始 32 字节的小写 hex。配置时需要明确确认 SHA256 公钥指纹；公钥与指纹应从发布者官方网站或已建立的信任源独立取得，包内字段和包内文本不能建立信任。同 ID 公钥不可静默替换。

同一 `id` 和 `version` 的摘要、签名、签名密钥不可改变。内容、entry、权限、客户端或依赖变化后，发布者必须提升版本，市场需重新审核并重新签名。本地安装也重新扫描并重新确认；旧签名无法证明修改后的包。市场审核通过和用户授权是两个独立步骤。

## 文本与路径边界

| 边界 | V1 限制 |
| --- | --- |
| canonical 包字节 | 2 MiB |
| 文件数量 | 128 |
| 单文件 UTF-8 字节 | 256 KiB |
| 文件路径 | ASCII 相对 `/` 路径，最多 240 字节、12 段 |
| 每段 | 最多 96 字节，仅字母、数字、`.`、`_`、`-` |
| 文件类型 | `.json`、`.md`、`.txt` |
| 本地历史版本 | 256 |
| 工具输入及输出 | 各最多 64 KiB，受限深度和节点数 |

拒绝绝对路径、`.` / `..`、反斜杠、Windows ADS、保留设备名、首尾点、隐藏文件、大小写别名、短文件名别名及文件/目录冲突。没有 ZIP、tar、二进制文件、文件模式或 symlink 字段。

宿主把完整签名 envelope 保存为 `<plugin_dir>/<digest>.json`，不解包文件路径。对象创建时同步并设只读，通过不覆盖的原子发布保存；SQL 事务只保存必要元数据、外部信任公钥和授权，不保存包正文。每次目录列举、启用、回滚和执行都重新检查对象、摘要、签名、隐私及撤回状态。仅只读位不能代替验证。

Windows 逐层固定祖先及存储目录句柄，拒绝共享删除并复核每层目录的 reparse 标记、卷身份和文件身份。最终存储目录允许共享写，以支持相对该句柄的原子发布；祖先仍只共享读。文件创建、重读、暂存清理和发布均相对固定目录句柄，暂存源文件句柄一直保持到发布完成。相对打开使用禁止跟随 reparse 的 NT 标志，发布使用不覆盖的相对句柄 rename，并验证普通文件、单一硬链接和只读状态。已有目录变成 junction 时也不能把文件操作导向外部路径。

包的全部元数据、文件路径、文本内容及 JSON 解码内容用强隐私策略检查。识别到凭据、个人信息、无法检查的附件或扫描失败即拒绝；失败只返回固定代码与说明，不能把原始错误、正文或 secret 放进日志、IPC 错误或 UI。扫描针对已识别的模式，不承诺能识别任意混淆或任意未知秘密；审核者仍应审阅全部文本。

严格验证后的 `manifest.dependencies` 在临时隐私扫描视图中拆成 `{id, version}`，避免合法的 `id@semver` 被邮件模式误判。ID 和版本的全部内容仍分别接受检查；包内文件或任意嵌套 JSON 的同名字段没有此转换。原包、canonical 字节、digest 和签名原文保持不变。

## 声明式 entry

### MCP 工具

`files[entry]` 是一个 JSON 字符串，其内容例如：

```json
{
  "schemaVersion": 1,
  "tools": [{
    "name": "read_note",
    "description": "Read an approved bundled note.",
    "inputSchema": {
      "type": "object",
      "properties": {"path": {"type": "string", "maxLength": 240}},
      "required": ["path"],
      "additionalProperties": false
    },
    "operation": {"type": "read_text", "pathArgument": "path"}
  }]
}
```

`files` 还需包含 `docs/note.md` 的实际 Markdown 文本。调用输入为 `{"path":"docs/note.md"}`。路径始终引用签名包内虚拟文件，不能传 OS 文件路径。

工具最多 32 个。输入 schema 只支持顶层 object、最多 24 个属性，以及 `string`、`boolean`、`number`、`integer`、`object`、`array` 类型、`description`、字符串 `maxLength`、`enum` 和 `required`。必须设置 `additionalProperties:false`。不支持 `$ref`、动态 schema、eval、JS 或 shell。

| operation | 声明与逐次授权 | 实际能力 |
| --- | --- | --- |
| `json_passthrough` | 空权限仍需本次确认 | 返回通过 schema 和隐私检查的 JSON |
| `read_text` | `readPaths:["package:docs/"]` 或精确包内路径 | 只从重新验证后的 `package.files` 读取文本 |
| `http_get` | `network:["https://example.com"]` 和本次精确请求预览 | 有界 HTTPS 公网 GET；返回 JSON 或纯文本 |

`http_get` 使用 `{"type":"http_get","urlArgument":"url"}`，`url` 必须是 required string。network 只接收精确规范 HTTPS origin，不接受通配符、IP literal、用户名、密码、路径、query、fragment 或非标准端口。调用 URL 可以有路径与经检查的 query。DNS 解析的全部地址都必须属于允许的公网地址范围，解析结果被固定用于连接。拒绝私网、回环、链路本地、映射地址、文档地址、保留地址和混合 DNS 答案；IPv6 使用保守的公网范围。禁止跳转、系统代理、自定义请求头、认证、cookie、压缩附件和客户端凭据。仅接收 UTF-8 `application/json`、`application/*+json`、`text/plain` 或 `text/markdown`，请求总时限 10 秒，并发上限 4。响应在返回前检查并脱敏，敏感结构或无法检查内容拒绝返回。

### 技能

`kind:"skill"` 的 entry 为非空 `.md` 文本，权限必须全空。宿主只返回经过验证的 Markdown 字符串，不执行其中命令，不解析或渲染 HTML。主代理负责将技能作为不受信任的内容接入客户端，避免把文档文字提升为宿主权限。

### Provider

`kind:"provider"` 的 entry 为受控 JSON 模型映射，权限必须全空：

```json
{"schemaVersion":1,"upstreamFormat":"openai_responses","models":{"friendly-model":"upstream-model"}}
```

`upstreamFormat` 支持 `openai_chat`、`openai_responses`、`anthropic`、`gemini`。不包含 endpoint、API key、认证头、代理、模板代码或请求脚本。由主代理选择既有可信上游并应用映射，宿主本身不会建立 provider 连接。

## 实际能力隔离

默认没有任何 network、read、write、process 或 credential 授权。声明外部绝对文件路径可在预览中被识别，但 V1 不提供访问 OS 文件系统的权限实现。声明 `process:true`、非空 `credentials`、`writePaths` 或外部 `readPaths` 的包只能作为停用包存储，启用及运行明确失败。现阶段没有可供第三方进程使用的 OS 沙箱；清空环境变量不构成隔离，也不能作为放行条件。

网络和包内读取均由宿主固定 Rust 操作实现。插件无法提供可执行代码、shell 参数或动态执行器。每个工具调用还需要独立、一次性的精确授权；安装确认不能代替调用确认。

## 安装与生命周期

1. 独立配置信任公钥并核对指纹。
2. 传 `publishedJson` 给 `plugins_preview_install`。返回已验证 manifest、digest、签名密钥指纹、旧版本、新增权限及 5 分钟有效的一次性 `previewId`。
3. 用户明确选择客户端、审阅全量权限并核对精确摘要。
4. 给 `plugins_confirm_install` 传 `InstallConfirmation` JSON 字符串：`previewId`、`digest`、`acceptedPermissions`、非空 `clients`、`confirmed:true`、`enable`。
5. 宿主消费预览，重新验证包、签名、依赖、撤回和本地 revision，再原子发布对象和提交活跃版本/授权。确认 API 不重新接收包，不能在预览后换包。

任何信任、授权、安装、停用、卸载或撤回变化会递增本地 revision；尚未执行的旧预览全部失效。升级自动停用旧版本的全部客户端授权；新版本即使权限没有增加也需要完整确认。新增客户端同样需要重新预览和确认。重新启用已有客户端也重新检查完整对象与既有精确授权。

`plugins_disable` 可停用全部客户端或单个客户端。损坏或撤回包也可停用和卸载。`plugins_uninstall` 删除授权和活跃指针，保留不可变的签名历史缓存以支持明确回滚；这不是清除缓存操作。`plugins_preview_rollback` 只使用本地历史版本，降级也通过完整安装确认；不复用旧授权。撤回的版本禁止安装、运行和回滚。

撤回通知为 `PluginWithdrawal`，只接受已配置发布密钥对以下 LF 分隔内容的 Ed25519 签名：

```text
xy-switch-plugin-revocation-v1\n{id}\n{version}\n{digest}\n{reason}
```

`reason` 仅为 `security`、`privacy` 或 `withdrawn`，没有尾换行。通知的版本、digest 和密钥必须与本地版本元数据一致。市场 agent 应复用 `plugins::withdrawal_payload`。撤回 tombstone 不提供撤销接口。市场仅在客户端同步签名通知后影响离线宿主；本模块不自行轮询市场。

## 共享 MCP 集成

MCP 生命周期、认证、连接客户端 scope 和主程序确认队列由主代理负责。插件宿主的实际接口如下：

```rust
tools_for_client(&self, client: &str) -> PluginResult<Vec<VerifiedTool>>
skills_for_client(&self, client: &str) -> PluginResult<Vec<VerifiedSkill>>
provider_mappings_for_client(&self, client: &str) -> PluginResult<Vec<VerifiedProviderMapping>>
preview_tool_call(&self, id: &str, client: &str, name: &str, arguments: serde_json::Value) -> PluginResult<ToolCallPreview>
preview_tool_call_json(&self, id: &str, client: &str, tool: &str, arguments_json: &str) -> PluginResult<ToolCallPreview>
cancel_tool_preview(&self, preview_id: &str) -> PluginResult<()>
async execute_tool_for_clients(&self, clients: &[String], confirmation: ToolCallConfirmation) -> PluginResult<ToolExecutionResult>
async execute_tool(&self, client: &str, confirmation: ToolCallConfirmation) -> PluginResult<ToolExecutionResult>
```

`tools/list` 使用连接全部客户端的目录交集，只保留各客户端均验证通过且插件 ID、版本、digest 和工具一致的项。`tools/call` 由可信连接选择预览客户端、建立主程序确认队列；用户审核具体参数、最小权限和 `argumentsDigest` 后，调用 `execute_tool_for_clients(&connection.clients, confirmation)`。单客户端入口 `execute_tool` 委托给同一 scope 执行实现。MCP 工具名称由主程序集成插件命名空间。客户端身份与 scope 必须来自可信连接，不能来自工具参数或模型填写的字段。

调用预览有效期 2 分钟，绑定客户端、插件版本/digest、工具名称、typed JSON 参数摘要与精确最小权限。预览不返回完整参数；原参数只保存在宿主内存。`ToolCallConfirmation` 只接收该预览的 `previewId`、`pluginDigest`、`argumentsDigest`、`acceptedPermissions`、`confirmed`。scope 必须非空、受支持且没有重复；预览客户端必须属于 scope，可以不在首位。确认消费预览一次，不能重新提交参数或修改包。参数、插件、权限或本地状态变化后必须重新预览并获得确认。

执行时在同一授权边界内复验整个 scope：每个客户端对同一插件版本/digest 的启用状态、完整权限授权、可信签名、撤回状态以及同客户端的固定依赖及传递依赖均须通过。首次消费确认、网络 future 的每次 poll 之前和输出返回之前均执行此复验；即使数据库授权被改动而 revision 未递增，次要客户端撤权、有效权限减少和依赖撤权也会拒绝执行。正常生命周期变更还会递增 revision，并立即清空全部安装及工具预览。

锁顺序固定为插件内存 `state` → 数据库连接；授权检查与有副作用的对应 future poll 在同一锁边界内。future 返回 `Pending` 后释放这些锁，不把数据库 guard 跨 `.await` 保存。bridge 应在调用宿主前释放自身数据库 guard，避免反向取得锁。网络运行另监测 revision；已发送的请求无法撤销。

拒绝、超时、HTTP body 丢弃、连接断开和退出时，确认队列应立即调用 `cancel_tool_preview`，关闭最后一个预览不再被下一次创建清理的问题。此方法仅移除内存中的指定待确认参数，不返回参数、不访问数据库。规范 ID 是 36 字符小写连字符 UUID；重复取消或不存在的规范 ID 成功，非法 ID 返回 `plugin_confirmation`，内存锁中毒返回 `plugin_database`。取消后的执行返回 `plugin_preview_expired`。宿主没有额外的 Tauri 自动确认入口。

输入先严格检查字节、深度、字段 schema 及完整隐私，再写入待确认内存；URL、解码 query 和传输检查也在创建预览前执行。`inspect_package_privacy(serde_json::Value) -> PluginResult<()>` 可供其他受控入口执行同类拒绝检查。输出先检查大小、附件与敏感结构，再对文本进行强策略脱敏和复验；无法检查或敏感结构返回 `plugin_privacy`，成功结果的 `redacted` 表示是否执行过脱敏。

当前宿主扫描调用的 known-secrets 集合为空，只识别既有隐私规则覆盖的内容。任意不透明 secret 或连接 token 必须由持有其值的 bridge 在参数进入宿主前及结果交付前使用现有 `crate::privacy` 的 known-secrets 能力处理，不能声称宿主已识别所有秘密。声明 credential 或 process 访问仍明确拒绝运行。

所有错误由固定 `PluginError {code,message}` 返回。前端应显示固定代码说明，不能显示未知 IPC 错误对象、原始 JSON、URL 错误或堆栈。

## Windows 验证与集成复验

普通本地盘符路径（含空格和中文）已在实际 store 源码的独立 Windows 测试中验证：创建、只读原子发布、重读、幂等、不覆盖、源句柄防替换、暂存清理、目录固定、junction 根/祖先/竞态、硬链接及只读属性检查均通过。最终目录原先只共享读会造成 Win32 32 发布失败，现共享读写且继续拒绝共享删除。

本会话沙箱打开 `C:\Users\xxy` 祖先目录得到 Win32 5；宿主继续拒绝无法验证的祖先。集成测试应把 TEMP/TMP 放在允许访问的工作区，再使用主线程已有的 MSVC 环境：

```powershell
$pluginWorkspace = 'E:\XY switch\workspaces\session-favorites-20261029'
. (Join-Path $pluginWorkspace '.cache\msvc-test-env.ps1')
$pluginTestTemp = Join-Path $pluginWorkspace '.cache\plugin-host-test-temp'
New-Item -ItemType Directory -Path $pluginTestTemp -Force | Out-Null
$env:TEMP = $pluginTestTemp
$env:TMP = $pluginTestTemp
Push-Location (Join-Path $pluginWorkspace 'src-tauri')
cargo test --lib plugins:: -- --test-threads=1
cargo test --lib shared_mcp:: -- --test-threads=1
Pop-Location
```

2026-11-04 已直接执行主线程生成的实际应用测试程序 `src-tauri/target/debug/deps/xy_switch_lib-0f4a0e1ea7dcdd28.exe`，程序生成于本地时间 02:25:18，晚于本轮全部插件行为修改。`plugins::` **45/45 通过**，其中 Windows store **8/8 通过**；包含真实应用 Database 的初始化、签名安装、生命周期、完整 client scope 的执行授权及依赖复验、取消和隐私测试。之后仅整理插件 Rust 格式与 SDK 文档；没有自行启动 Cargo。全部 10 个插件 Rust 文件按项目 edition 2024、`skip_children=true` 执行 Rustfmt 检查通过，证据为 `.cache/plugin-rustfmt-check-20261104.log`。格式后的统一重编译仍由主线程执行。

同一程序的 `shared_mcp::` **2/21 通过，19 项失败**，耗时 13.49 秒。原先 `Fixture::new → PluginManager::new → plugin_storage_boundary` 的初始化阻断已消除；18 项失败位于 `shared_mcp/tests.rs:59` 的连接注册 `shared_mcp_config`，另 1 项位于 `shared_mcp/manager_tests.rs:41` 的注册失败清理断言。`SharedMcpManager::register → McpManager::save_server` 的注册/配置问题由主线程继续修补并统一重编译；此结果不能表述为共享 MCP 集成全部通过。

当前会话的可直接重跑证据命令：

```powershell
& 'E:\XY switch\workspaces\session-favorites-20261029\src-tauri\target\debug\deps\xy_switch_lib-0f4a0e1ea7dcdd28.exe' plugins:: --nocapture --test-threads=1
& 'E:\XY switch\workspaces\session-favorites-20261029\src-tauri\target\debug\deps\xy_switch_lib-0f4a0e1ea7dcdd28.exe' shared_mcp:: --nocapture --test-threads=1
```

运行前沿用工作区 TEMP/TMP。实际应用对应日志是 `.cache/plugin-application-tests-20261104.log`（退出码 0）和 `.cache/plugin-shared-mcp-regression-20261104.log`（退出码 101）；没有覆盖主线程旧日志。早前独立核心 45/45 与专门 Windows store 8/8 的日志分别是 `.cache/plugin-host-isolated-tests-20261104.log`、`.cache/plugin-store-windows-tests-20261104.log`，仅作为隔离定位证据。前端 `node node_modules/vitest/vitest.mjs run src/plugins.test.tsx` 16/16 通过；以插件前端文件为 roots 并使用当前 `tsconfig.app.json` 选项的定向 TypeScript 检查在本地时间 03:00 重新执行，为 0 diagnostics，证据为 `.cache/plugin-frontend-tests-20261104.log`、`.cache/plugin-typescript-check-20261104.log`。

剩余集成验收：主线程完成共享 MCP 注册/失败清理修补后，统一重编译并完整回归；入口与英文键合并后的真实桌面安装、管理和确认交互；bridge 对连接 token 等不透明已知秘密的输入/输出保护验收。宿主范围当前没有已执行测试定位出的剩余失败，现有测试结果不代替这些集成验收。

## 英文翻译合并清单

页面与 service 使用中文原文作为 `t` / `translateMessage` 键。以下 153 项在当前 `src/i18n/en.ts` 缺失，由主线程合并；保留 `{plugin}`、`{client}`、`{publisher}`、`{count}` 占位符。通用已有键及产品名称复用既有翻译。

```json
{
  "Ed25519 公钥（小写 hex）": "Ed25519 public key (lowercase hex)",
  "仅允许明确授权的 HTTPS 站点和安全的公网解析地址。": "Only explicitly approved HTTPS sites and safe public DNS addresses are allowed.",
  "仅接收 UTF-8 JSON 文本包，最多 2 MiB；所有文件都会接受路径与隐私检查。": "Accepts UTF-8 JSON text packages up to 2 MiB. Every file is checked for safe paths and private information.",
  "保留原始 JSON；宿主会拒绝重复字段和篡改的摘要，包内公钥不能建立信任。": "Keep the original JSON. Duplicate fields and altered digests are rejected; a public key inside the package cannot establish trust.",
  "信任只来自本地配置的公钥；请独立核对指纹。": "Trust comes only from locally configured public keys. Verify the fingerprint independently.",
  "停用 {plugin} 的 {client} 授权？待确认工具调用会立即作废。": "Disable {client} access to {plugin}? Pending tool approvals will be invalidated immediately.",
  "停用 {plugin} 的全部客户端授权？待确认工具调用会立即作废。": "Disable all client access to {plugin}? Pending tool approvals will be invalidated immediately.",
  "停用全部客户端": "Disable for all clients",
  "停用插件": "Disable plugin",
  "公钥指纹": "Public key fingerprint",
  "写文件": "Write files",
  "卸载 {plugin} 会删除活跃指针和客户端授权；保留不可变签名历史以便重新审核回滚。": "Uninstalling {plugin} removes the active version and client grants. Immutable signed history is kept for reviewed rollback.",
  "卸载保留不可变历史缓存。回滚会重新审核完整权限与客户端授权。": "Uninstalling keeps immutable history. Rollback requires a fresh review of all permissions and client grants.",
  "卸载插件": "Uninstall plugin",
  "发布者": "Publisher",
  "发布者信任": "Publisher trust",
  "发布者信任密钥已保存。": "Publisher trust key saved.",
  "发布者已撤回这个版本，安装、运行和回滚已阻止。": "The publisher withdrew this version. Installation, execution, and rollback are blocked.",
  "发布者撤回": "Publisher withdrawal",
  "发布者标签": "Publisher label",
  "发布者签名无法通过本地信任密钥验证。": "The publisher signature could not be verified with a locally trusted key.",
  "发现隐私信息或无法完整检查的内容，操作已阻止。": "Private information or content that cannot be fully inspected was found. The operation is blocked.",
  "受限文本请求未完成；跳转、代理和认证均不被接受。": "The restricted text request did not complete. Redirects, proxies, and authentication are not accepted.",
  "各客户端新增权限": "Added permissions by client",
  "吊销 {publisher} 的信任后，依赖此密钥的插件将停止运行，待确认调用会作废。": "Revoking trust in {publisher} stops plugins signed by this key and invalidates pending approvals.",
  "吊销信任": "Revoke trust",
  "吊销发布者信任": "Revoke publisher trust",
  "同一信任密钥名称不能替换为另一个公钥。": "A trusted key name cannot be replaced with a different public key.",
  "同一插件版本不可覆盖；修改内容后须升级版本并重新审核。": "A plugin version cannot be overwritten. Content changes require a new version and review.",
  "向已授权公网 HTTPS 站点发送一次 GET 请求。": "Send one GET request to an approved public HTTPS site.",
  "固定版本依赖": "Pinned dependencies",
  "固定版本依赖缺失、停用、无法验证或存在循环。": "A pinned dependency is missing, disabled, unverifiable, or cyclic.",
  "安全问题": "Security issue",
  "审核全部客户端": "Review all clients",
  "审核回滚": "Review rollback",
  "审核授权": "Review access",
  "审核插件安装与客户端授权": "Review plugin installation and client access",
  "审核撤回通知": "Review withdrawal notice",
  "客户端授权状态": "Client access status",
  "宿主计算的公钥指纹": "Public key fingerprint calculated by the host",
  "宿主计算的参数摘要": "Arguments digest calculated by the host",
  "密钥标识": "Key ID",
  "导入发布者签名的撤回通知。宿主会核对版本、摘要、原因及原发布者密钥。": "Import a publisher-signed withdrawal notice. The host verifies the version, digest, reason, and original publisher key.",
  "导入已签名的文本插件，按客户端授权，并逐次确认工具调用。": "Import signed text plugins, grant access per client, and confirm each tool call.",
  "导入签名插件包": "Import signed plugin package",
  "尚未安装本地插件。请先配置信任密钥，再导入签名包。": "No local plugins installed. Configure a trusted key, then import a signed package.",
  "尚未配置信任密钥。": "No trusted keys configured.",
  "工具参数 JSON": "Tool arguments JSON",
  "工具输入或输出超过限制，或与声明的格式不符。": "Tool input or output exceeds the limit or does not match its declared format.",
  "工具输入格式": "Tool input format",
  "工具返回结果": "Tool result",
  "已信任": "Trusted",
  "已卸载，保留签名历史缓存": "Uninstalled; signed history retained",
  "已吊销": "Revoked",
  "已安装及历史记录：{count}": "Installed plugins and history: {count}",
  "已安装插件": "Installed plugins",
  "已撤回": "Withdrawn",
  "已验证工具": "Verified tools",
  "已验证工具调用": "Verified tool calls",
  "应用签名撤回通知": "Apply signed withdrawal notice",
  "当前宿主不提供凭据访问所需的系统隔离，拒绝启用。": "The host lacks the OS isolation required for credential access. Activation is denied.",
  "当前宿主只允许读取已授权的插件包内文本。": "The host only allows approved text reads within the plugin package.",
  "当前宿主缺少第三方进程所需的系统隔离，拒绝启用。": "The host lacks the OS isolation required for third-party processes. Activation is denied.",
  "当前插件宿主不允许写文件，拒绝启用。": "The plugin host does not allow file writes. Activation is denied.",
  "我已审核完整权限、客户端、版本与精确摘要。": "I have reviewed all permissions, clients, the version, and the exact digest.",
  "我已通过独立可信来源核对该公钥与指纹。": "I have verified this public key and fingerprint through an independent trusted source.",
  "我确认应用此签名撤回通知并停用对应版本。": "I confirm applying this signed withdrawal notice and disabling the matching version.",
  "我确认此客户端、本次参数及列出的最小权限。": "I confirm this client, these arguments, and the listed minimum permissions.",
  "找不到所请求的本地插件版本或包内文件。": "The requested local plugin version or packaged file was not found.",
  "插件": "Plugin",
  "插件 / 工具": "Plugin / tool",
  "插件 JSON 无效，或包含缺失、额外、重复的字段。": "Plugin JSON is invalid or contains missing, extra, or duplicate fields.",
  "插件入口不是支持的工具声明、Markdown 技能或模型映射。": "The plugin entry is not a supported tool declaration, Markdown skill, or model mapping.",
  "插件内容与精确校验摘要不一致，请重新预览。": "Plugin content does not match the exact digest. Preview it again.",
  "插件包精确摘要": "Exact plugin package digest",
  "插件存储目录或文件的安全边界已变化，操作已阻止。": "The plugin storage directory or file boundary changed. The operation is blocked.",
  "插件安装状态已更新。": "Plugin installation state updated.",
  "插件尚未对这个客户端授权启用。": "The plugin is not approved and enabled for this client.",
  "插件操作未完成，请重新预览；原始错误内容已隐藏。": "The plugin operation did not complete. Preview it again; raw error details are hidden.",
  "插件文本超过大小限制：包最多 2 MiB，单文件最多 256 KiB。": "Plugin text exceeds the size limit: 2 MiB per package and 256 KiB per file.",
  "插件权限": "Plugin permissions",
  "插件权限声明不符合本地限制。": "Plugin permission declarations do not meet local restrictions.",
  "插件版本号不符合支持的语义版本格式。": "The plugin version does not match the supported semantic version format.",
  "插件管理操作已完成。": "Plugin management operation completed.",
  "插件路径包含不安全路径、设备名或文件别名。": "A plugin path contains an unsafe path, device name, or file alias.",
  "插件需要更新版本的 XY Switch。": "This plugin requires a newer version of XY Switch.",
  "撤回原因": "Withdrawal reason",
  "撤回标记不可撤销；已卸载版本的历史缓存也会受到约束。": "Withdrawal cannot be undone and also applies to cached history of uninstalled versions.",
  "撤回通知 JSON": "Withdrawal notice JSON",
  "撤回通知已验证并应用。": "Withdrawal notice verified and applied.",
  "文件数量 / 文本字节": "File count / text bytes",
  "无法安全读取或保存插件元数据。": "Plugin metadata could not be safely read or saved.",
  "无法安全读取插件文件；请检查存储目录。": "Plugin files could not be safely read. Check the storage directory.",
  "无活跃版本": "No active version",
  "本地插件": "Local plugins",
  "本地插件、预览或并发执行数量已达到限制。": "The local plugin, preview, or concurrent execution limit has been reached.",
  "本地插件安装、信任配置与执行需要桌面应用。": "Local plugin installation, trust configuration, and execution require the desktop app.",
  "本次工具参数": "Arguments for this call",
  "本次授权客户端": "Clients approved in this review",
  "本次确认会停用旧客户端授权，仅按下方选择重新授权。": "This confirmation disables previous client grants and grants access only to the clients selected below.",
  "正在读取信任密钥…": "Loading trusted keys…",
  "正在读取已验证工具…": "Loading verified tools…",
  "正在读取本地插件…": "Loading local plugins…",
  "此客户端没有已启用且验证通过的工具。": "This client has no enabled and verified tools.",
  "此插件未声明所请求的工具。": "The plugin does not declare the requested tool.",
  "添加信任密钥": "Add trusted key",
  "添加发布者信任密钥": "Add publisher trust key",
  "版本": "Version",
  "版本历史（{count}）": "Version history ({count})",
  "申请进程访问（当前缺少系统隔离，拒绝运行）": "Process access requested (OS isolation unavailable; execution denied)",
  "确认保存并授权": "Confirm save and access grants",
  "确认信任此公钥": "Confirm trust in this public key",
  "确认停用": "Confirm disable",
  "确认卸载": "Confirm uninstall",
  "确认吊销": "Confirm revocation",
  "确认后启用所选客户端": "Enable selected clients after confirmation",
  "确认并执行这一次调用": "Confirm and execute this call",
  "确认这一次工具调用": "Confirm this tool call",
  "确认验证并应用撤回": "Confirm verification and apply withdrawal",
  "签名包 JSON": "Signed package JSON",
  "签名密钥": "Signing key",
  "签名将在确认后由宿主验证。验证成功后，此版本将禁止安装、运行和回滚。": "The host verifies the signature after confirmation. A verified notice blocks installation, execution, and rollback of this version.",
  "签名撤回通知": "Signed withdrawal notice",
  "网络站点": "Network sites",
  "访问凭据": "Credential access",
  "请从独立可信来源取得公钥；插件包内的公钥不能建立信任。": "Obtain the public key from an independent trusted source. A key inside the package cannot establish trust.",
  "请从独立可信来源取得发布者公钥，并在本地确认其指纹。": "Obtain the publisher public key from an independent trusted source and confirm its fingerprint locally.",
  "请明确确认本次预览、完整权限、客户端与精确摘要。": "Explicitly confirm this preview, all permissions, the clients, and the exact digest.",
  "请明确选择支持的客户端。": "Explicitly select a supported client.",
  "读取已签名插件包内的文本。": "Read text within the signed plugin package.",
  "读取文本": "Read text",
  "调用客户端": "Calling client",
  "调用确认仅使用一次。修改参数、客户端、插件版本或权限后必须重新预览。": "Call approval is used once. Preview again after changing arguments, the client, the plugin version, or permissions.",
  "输入本次参数的精确摘要": "Enter the exact digest of these arguments",
  "输入本次插件包的精确摘要": "Enter the exact digest of this plugin package",
  "输入独立来源的精确指纹": "Enter the exact fingerprint from an independent source",
  "输出已检查并脱敏。": "Output inspected and redacted.",
  "输出已通过宿主隐私检查。": "Output passed the host privacy check.",
  "运行进程": "Run processes",
  "返回本次明确确认的 JSON 输入。": "Return the JSON input explicitly confirmed for this call.",
  "这里只列出所选客户端已启用且验证通过的工具。安装授权不会自动批准工具调用。": "Only tools enabled and verified for the selected client are listed. Installation grants do not automatically approve tool calls.",
  "选择已验证工具": "Select a verified tool",
  "选择签名包 JSON 文件": "Choose a signed package JSON file",
  "选择调用客户端": "Select the calling client",
  "隐私问题": "Privacy issue",
  "预览公钥指纹": "Preview public key fingerprint",
  "预览已过期或本地状态已变化，请重新审核并预览。": "The preview expired or local state changed. Review and preview again.",
  "预览本次工具调用": "Preview this tool call",
  "验证签名并预览安装": "Verify signature and preview installation",
  "验证通过": "Verified",
  "默认保持停用。进程、凭据、写文件和外部文件读取当前无法授权运行。": "Disabled by default. Process, credential, file write, and external file read access cannot currently be approved for execution.",
  "（当前不可运行）": " (execution unavailable)",
  "（当前缺少系统隔离，拒绝运行）": " (OS isolation unavailable; execution denied)"
}
```
