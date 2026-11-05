# 公共审核插件市场服务

**交付状态：尚未公共上线。** 此目录提供独立市场服务、市场页面和可审阅的部署配置。2026-11-04 的专项验证使用隔离数据库、合成账号及临时测试密钥，结果见 [专项验收记录](../deploy/marketplace/verification.md)。获批主机上的安装、TLS、公网访问及恢复演练仍须由部署负责人取得授权后执行。

示例域名 `market.example.invalid` 不能用于正式运行。切换前须落实配置、独立公钥信任、当前状态备份及外部验收。恢复工具要求独立、完整的当前状态参考；不能用旧备份覆盖新审批、身份或撤回记录。当前库和该参考同时丢失时，没有已经验证的灾难恢复流程。

## 主代理集成契约

在应用 crate 中公开 `marketplace` 模块；已有的 `plugins::{PluginPackage, PluginManifest, PublishedPlugin, PluginWithdrawal, WithdrawalReason}`、`validate_package`、`package_digest` 及公开 `plugins::validation` 由插件代理提供。市场复用真实隐私扫描 `crate::privacy::inspect`。依赖沿用 axum、tokio、rusqlite、ring、serde、serde_json、sha2、regex、zeroize、reqwest、uuid。

入口是 `Marketplace::open(MarketplaceConfig) -> Result<Marketplace, MarketplaceError>` 和 `marketplace::router(Marketplace) -> axum::Router`。不存在 `MarketplaceManager`，REST 不需要市场专用 Tauri 命令。独立 binary 名称为 `xy-marketplace`，其环境配置与应用账号、订阅账号、应用数据库无关。时间戳均为 Unix 秒，JSON 均为 camelCase；状态、审核决策使用 snake_case。

`MarketplaceConfig::from_env()` 返回验证后的配置；直接构造时的全部参数如下：

| 参数 | Rust 类型 | 作用 |
| --- | --- | --- |
| `database_path` | `PathBuf` | 独立市场 SQLite 文件的绝对路径 |
| `listen` | `SocketAddr` | 默认 loopback 监听地址 |
| `public_origin` | `String` | 无路径、查询和尾部斜杠的 HTTPS origin |
| `allowed_origins` | `Vec<String>` | 精确 CORS 白名单 |
| `reviewers` | `Vec<ReviewerConfig>` | 部署明确指定的独立审核者，1–64 人 |
| `signing_key_id` | `String` | 当前发布签名的稳定合法 ID |
| `signing_private_key` | `Vec<u8>` | Ed25519 PKCS#8 v2 DER 字节 |
| `verification_keys` | `BTreeMap<String, Vec<u8>>` | 历史签名 ID 对应的 32 字节公钥 |
| `session_ttl_seconds` | `u64` | 900–604800 秒 |
| `allow_development_http` | `bool` | 仅用于 loopback HTTP 开发 |
| `allow_non_loopback` | `bool` | 正式反代配置保留 false |

前端命名导出 `src/features/marketplace/MarketplacePage.tsx` 的 `MarketplacePage`，无必须 props，主入口可直接渲染 `<MarketplacePage />`。`src/lib/marketplace-service.ts` 导出 `MarketplaceClient(origin, options?)`；由 `VITE_XY_MARKETPLACE_ORIGIN` 提供初始 HTTPS 地址，也可在页面输入。身份、会话及上传草稿仅留页面/客户端内存，断开、换地址、退出或过期时清除；公共浏览不发送 bearer。

安装复用 `plugins_preview_install`、`plugins_confirm_install`、`plugins_apply_withdrawal`。主代理须保持真实原生命令和本地可信签名公钥配置；市场不调用 `plugins_configure_trusted_key` 自动信任返回的 key。确认安装会再次核对相同包、版本、摘要、签名和撤回，安装后发现的有效撤回交给原生插件模块处理。页面没有后台持续轮询已安装插件撤回的机制；离线客户端须在下次联网时经已验证流程处理。

依赖沿用 axum 0.8、tokio 1（rt-multi-thread/macros/net/time/sync/signal）、rusqlite 0.37（bundled）、ring 0.17、sha2、serde（derive）、serde_json、regex、zeroize、reqwest、uuid（v4）。前端沿用 React、现有 UI、`useLanguage/t/translateMessage` 和 plugins-service；未新增 npm 依赖。英文词条通过独立 [en-translations.json](../src/features/marketplace/en-translations.json) 交付，共 159 条；[部署目录映射](../deploy/marketplace/i18n.en.json) 保留为此前交接副本，两份内容一致。

主代理在 `src/i18n/en.ts` 导入 `marketplaceEnTranslations`，路径为 `../features/marketplace/en-translations.json`，再把 `...marketplaceEnTranslations` 合入已有 `export const en: Record<string, string>`，与现有插件 JSON 合并方式相同。市场任务仅交付独立 JSON，不写共享翻译文件；路由直接使用命名导出的 `MarketplacePage`。

## 客户端 API 契约

身份凭据只用于独立市场服务。注册或登录请求 `{username, displayName?, password}`；注册必须带 `displayName`，登录只带 `username/password`。拒绝客户端提供的 role、reviewer、userId 等多余字段。成功会话 `{token,tokenType:"Bearer",expiresAt,user:{userId,username,displayName,reviewer}}` 只返回当前账号。

| 方法与路径 | 身份要求 | 输入或返回 |
| --- | --- | --- |
| POST `/v1/auth/register` | 公开 | `{username,displayName,password}`，创建普通用户并返回会话 |
| POST `/v1/auth/login` | 公开 | `{username,password}`，返回会话 |
| GET `/v1/auth/me` | Bearer | 当前市场用户 |
| POST `/v1/auth/logout` | Bearer | 撤销当前会话 |
| POST `/v1/submissions` | 用户 | `{package,replacesSubmissionId?}`，返回 submitted 投稿 |
| GET `/v1/submissions` | 用户 | 仅自己的投稿；`limit/offset/status` |
| GET `/v1/submissions/{submissionId}` | 作者或审核者 | 私有包、digest、状态、reviewNote、permissionChanges |
| GET `/v1/submissions/{submissionId}/audit` | 作者或审核者 | 此投稿的审计分页 |
| POST `/v1/submissions/{submissionId}/publish` | 作者或审核者 | `{version,digest}`，仅精确 approved 版本可发布 |
| POST `/v1/submissions/{submissionId}/withdraw` | 作者或审核者 | `{version,digest,reason,note?}`；reason 为 security/privacy/withdrawn |
| GET `/v1/review/submissions` | 审核者 | 审核队列；`limit/offset/status`，默认 submitted |
| POST `/v1/review/submissions/{submissionId}/decision` | 独立审核者 | `{version,digest,decision,note?}`；decision 为 approve/request_changes/reject |
| GET `/v1/review/audit` | 审核者 | 全部审计分页 |
| GET `/v1/plugins` | 公开 | 每个插件最新已发布版本；`limit/offset/client/kind/q` |
| GET `/v1/plugins/{id}/versions` | 公开 | 此插件的全部已发布版本分页 |
| GET `/v1/plugins/{id}/versions/{version}` | 公开 | 已验证 `{package,digest,signature,signingKeyId}` |
| GET `/v1/plugins/{id}/versions/{version}/withdrawal` | 公开 | 仅曾发布后撤回的签名撤回通知；不包含包或私有说明 |
| POST `/v1/plugins/{id}/versions/{version}/reports` | 用户 | `{reason,note?}`；reason 为 security/privacy/malware/copyright/other |
| GET `/v1/review/reports` | 审核者 | 举报分页；`limit/offset/status`，默认 open |
| POST `/v1/review/reports/{reportId}/resolve` | 独立审核者 | `{resolution,note?}`；resolution 为 resolved/dismissed |
| GET `/healthz` | 公开 | 最小存活结果；不暴露配置、身份或统计 |
| GET `/readyz` | 公开 | 检查已有市场库身份与可读取性；不是全量签名验收 |

分页 `{items,limit,offset,hasMore}`，默认 20，最大 100。公开目录条目为 `{id,name,description,authorDisplayName,version,kind,clients,digest,signature,signingKeyId,publishedAt}`，没有用户名、内部作者 ID 或私有审核说明。

同一个插件 ID 始终归属原作者，`(id,version)` 永久唯一。包全文不可原地修改；修改被要求修订的包也须增加版本号并新提交。所有更新重新进入 submitted；批准请求携带精确版本和 SHA256 digest，审核者不能是作者。发布重新校验包和隐私规则，使用显式配置的 Ed25519 私钥签名。撤回使公开目录与新下载停止返回包，并保留审计及签名撤回元数据。

签名载荷 UTF8 为 `xy-switch-plugin-v1\n{id}\n{version}\n{digest}`；digest 使用 typed serde_json 的 canonical package bytes，签名为小写 hex。签名公钥须由部署负责人通过可信渠道交给客户端，不从下载响应自动建立信任。

所有错误使用固定 `{error:{code,message}}`，不回显请求内容。HTTP 401/403/404/409/422/429/503 分别表示认证、权限、不可见资源、版本或状态冲突、隐私拒绝、限流、安全失败。匿名不能读取投稿、审核队列、举报或审计；withdrawn 与未发布版本的公共下载均返回 404。

## 正式配置与数据去向

配置模板为 [marketplace.env.example](../deploy/marketplace/marketplace.env.example)。服务从指定文件读取审核者及密钥，不信任客户端 role、不连接 OIDC/订阅身份，不读取桌面账号或代理数据库。

| 环境变量 | 必需或默认 | 正式要求 |
| --- | --- | --- |
| `XY_MARKETPLACE_DATABASE` | 必需 | 独立市场库绝对路径；不能指向应用现有 DB |
| `XY_MARKETPLACE_LISTEN` | `127.0.0.1:47831` | 与反代相同，端口不对公网开放 |
| `XY_MARKETPLACE_PUBLIC_ORIGIN` | 必需 | 已获批 HTTPS origin，无尾部 `/` |
| `XY_MARKETPLACE_ALLOWED_ORIGINS` | 默认 public origin | 精确 JSON 字符串数组；不用 `*` |
| `XY_MARKETPLACE_REVIEWERS_FILE` | 必需 | 私有审核者 JSON 文件 |
| `XY_MARKETPLACE_SIGNING_KEY_ID` | 必需 | 插件 ID 规则、最长 64 字符；新 key 用新 ID |
| `XY_MARKETPLACE_SIGNING_KEY_FILE` | 必需 | 私有 Ed25519 PKCS#8 v2 DER，非 PEM |
| `XY_MARKETPLACE_VERIFICATION_KEYS_FILE` | 首次可省略 | 历史公钥 JSON 数组；保留发布/撤回所用公钥 |
| `XY_MARKETPLACE_SESSION_TTL_SECONDS` | `86400` | 900–604800 秒 |
| `XY_MARKETPLACE_ALLOW_DEVELOPMENT_HTTP` | `false` | 公共环境保持 false；仅明确开发允许精确 loopback |
| `XY_MARKETPLACE_ALLOW_NON_LOOPBACK` | `false` | TLS 反代模式保持 false |

普通注册用户都可投稿。审核者配置为 `{username,displayName,passwordHash}`，用户名须为规范化 ASCII，公开展示名通过隐私检查。`passwordHash` 为 `{algorithm:"pbkdf2-sha256",iterations:600000,saltHex,hashHex}`，salt/hash 均为 32 字节小写 hex。`--hash-password-stdin` 使用随机 salt，只接受管道 stdin，不接受明文命令行参数或交互终端 stdin。

负责人可在获批主机终端创建首个审核者文件；先按下一节建立目录和服务组。示例仅把密码交给本机子进程 stdin，文件独占创建；密码不进入环境变量、命令历史或协作对话。

~~~sh
set -eu
sudo python3 - <<'PY'
import getpass, json, os, subprocess

with open("/dev/tty", "r", encoding="utf-8") as terminal:
    print("Reviewer username: ", end="", flush=True)
    username = terminal.readline().strip()
    print("Public display name: ", end="", flush=True)
    display_name = terminal.readline().strip()
password = getpass.getpass("Reviewer password: ")
if password != getpass.getpass("Confirm password: "):
    raise SystemExit("Password confirmation failed")
result = subprocess.run(
    ["/opt/xy-marketplace/current/xy-marketplace", "--hash-password-stdin"],
    input=password, text=True, capture_output=True, check=False,
)
del password
if result.returncode:
    raise SystemExit("Reviewer password hash was not created")
reviewers = [{"username": username, "displayName": display_name,
              "passwordHash": json.loads(result.stdout)}]
descriptor = os.open("/etc/xy-marketplace/reviewers.json",
                     os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(descriptor, "w", encoding="utf-8") as output:
    json.dump(reviewers, output, ensure_ascii=False, separators=(",", ":"))
    output.flush()
    os.fsync(output.fileno())
PY
sudo chown root:xy-marketplace /etc/xy-marketplace/reviewers.json
sudo chmod 0640 /etc/xy-marketplace/reviewers.json
~~~

用户名/展示名仍以 `--check-config` 为准。重复用户名或直接把已有普通用户提升为审核者会拒绝。负责人审阅审核者配置后重启，事务同步角色、撤销受影响会话并记录审计；代码回滚保留当前审核者配置。

当前 `Ed25519KeyPair::from_pkcs8` 要求带匹配公钥的 PKCS#8 v2 DER。密钥负责人须通过与 `Ed25519KeyPair::generate_pkcs8` 兼容的已审阅工具离线提供正式密钥；v1/PEM 不能作为此配置。服务仅使用显式私钥，没有生成生产密钥。历史公钥文件是 `[{id,publicKeyHex}]` 数组，publicKeyHex 为 32 字节小写 hex；当前 key 若显式列出必须与私钥匹配。历史 ID 不可绑定新公钥，已经发布/撤回使用的公钥不得丢弃。客户端公钥/指纹由负责人通过独立可信渠道分发和确认，下载响应不能自动建立信任。

| 数据 | 存放位置/接收方 | 公开边界 |
| --- | --- | --- |
| 用户名、展示名、PBKDF2 salt/hash、内部用户 ID | 独立 SQLite `users` | 仅当前会话可见自己的身份；公共只展示 displayName |
| bearer SHA256、过期时间、用户名哈希限流窗口 | `sessions` / `login_attempts` | 明文 token 只返回客户端一次并留内存；不记录密码/IP |
| 全包、文件、摘要、版本、决定 | `submissions` / `releases` / `plugins` | 未发布私有；仅已发布且验证通过的资料公开 |
| 撤回、举报理由、状态及内部关联 | `withdrawals` / `reports` / `audit` | 公共仅签名撤回通知；私有举报/审计按权限读取 |
| 私钥/审核者配置 | `/etc/xy-marketplace` 和服务内存 | 不进入包/API；数据库仅留公钥 |
| 备份数据库和 manifest | 负责人指定的私有目录 | 含密码哈希、会话哈希和私稿；不放网页根目录/公共桶 |
| 日志 | systemd journal；nginx 关闭 access log | 应用仅固定码/启动状态，不记录正文、密码、token 或原始错误 |

上传检查覆盖所有 manifest/permissions/dependencies/files 名称与内容、嵌套编码及当前 token/内部用户 ID，拒绝时无回显且不落包。审核/举报/撤回自由说明也先检查；不持久化这些自由说明，只存固定状态/理由及审计关联。包本身是发布资料，作者须复核；隐私检测覆盖现有规则，不能保证识别任意未知秘密。

## 在获批主机安装并预检

以下步骤是负责人审阅后的 Linux/systemd/nginx 操作材料，尚未执行。输入是主代理完成应用集成、针对目标 OS/架构构建并校验过的 `./xy-marketplace` 制品及本仓库部署文件。隔离 verification crate 不能代替正式应用构建或目标主机验收。

1. 确认主机、域名/DNS/TLS、系统服务用户、独立存储、备份、审核者和密钥。配置/数据库/备份均不在网页根目录；47831 仅 loopback，对外只开放 HTTPS/必需的 HTTP 跳转。
2. 建立按制品 SHA256 命名的只读版本目录及私有存储。用户创建命令仅用于该系统服务用户尚不存在的初次安装。

~~~sh
set -eu
release_id=$(sha256sum ./xy-marketplace | cut -d ' ' -f1)
sudo useradd --system --home-dir /var/lib/xy-marketplace --shell /usr/sbin/nologin xy-marketplace
sudo install -d -m 0755 /opt/xy-marketplace/releases
sudo install -d -m 0755 "/opt/xy-marketplace/releases/$release_id"
sudo install -m 0755 ./xy-marketplace "/opt/xy-marketplace/releases/$release_id/xy-marketplace"
sudo install -m 0644 ./deploy/marketplace/sqlite_backup.py "/opt/xy-marketplace/releases/$release_id/sqlite_backup.py"
sudo install -d -o root -g xy-marketplace -m 0750 /etc/xy-marketplace
sudo install -d -o xy-marketplace -g xy-marketplace -m 0700 /var/lib/xy-marketplace
sudo ln -s "/opt/xy-marketplace/releases/$release_id" /opt/xy-marketplace/current
~~~

3. 填写 env 模板，安装为 `/etc/xy-marketplace/marketplace.env`，属 root:xy-marketplace、0640。提供前节 reviewers、私钥和历史公钥文件。私钥仅允许服务读取，不作为环境值、Git 文件或前端资源。
4. 在服务身份加载负责人控制的 env，真实解析密钥并预检。此命令不创建数据库、不改审核者/公钥、不监听。env 模板兼容 POSIX 加载和 systemd `EnvironmentFile`。

~~~sh
sudo -u xy-marketplace /bin/sh -eu -c '
  set -a
  . /etc/xy-marketplace/marketplace.env
  set +a
  exec /opt/xy-marketplace/current/xy-marketplace --check-config
'
~~~

5. 安装 [xy-marketplace.service](../deploy/marketplace/xy-marketplace.service) 到 `/etc/systemd/system/xy-marketplace.service`。把 [nginx.conf.example](../deploy/marketplace/nginx.conf.example) 安装到主机实际 nginx 包含目录，替换获批域名和有效证书/私钥路径。保留 body 限制、精确 upstream、认证限流、关闭缓存/请求日志和 TLS。
6. 在目标主机验证配置，通过后先启动 loopback 后端。首次正常启动初始化独立市场库；之后全量只读核验。`--verify-state` 对不存在、空白、其他服务或篡改 schema 的库拒绝，不初始化或同步审核者。

~~~sh
set -eu
sudo systemd-analyze verify /etc/systemd/system/xy-marketplace.service
sudo nginx -t
sudo systemctl daemon-reload
sudo systemctl start xy-marketplace
curl --fail --silent --show-error http://127.0.0.1:47831/healthz
curl --fail --silent --show-error http://127.0.0.1:47831/readyz
sudo -u xy-marketplace /bin/sh -eu -c '
  set -a
  . /etc/xy-marketplace/marketplace.env
  set +a
  exec /opt/xy-marketplace/current/xy-marketplace --verify-state
'
~~~

7. 获批外部验收窗口后加载 nginx，独立外部客户端确认正常 HTTPS 证书、精确 CORS、健康/就绪及完整工作流。记录制品/配置/负责人，验收通过后才 `systemctl enable xy-marketplace`。真实密码/token 不作为 curl 参数或日志；HTTPS 不使用 `curl -k`。

nginx/systemd 模板尚未在指定正式主机执行语法检查；没有该主机、DNS、证书或公网可达验收证据。服务不内置 TLS，正式运行依赖反代和 firewall。nginx 对体积超限、认证限流和后端故障返回固定 JSON；系统日志保留仍由运维确认。

## 外部功能验收

使用获授权的普通测试身份 A/B 和独立审核者 R，经实际 HTTPS、主应用 UI 和真实原生命令验收，记录状态码、版本、摘要和无秘密截图。私有内容不能作为公开证据。逐步操作、合法无外部动作验收包、权限能力判定、现场记录及失败处理见 [现场验收与失败回滚](../deploy/marketplace/acceptance.md)；其中所有现场步骤均待授权执行。

| 步骤 | 必须观察到的结果 |
| --- | --- |
| A/B 注册、登录、各自投稿 | 两人均可提交，均为 submitted；B 不能读 A 私稿 |
| 匿名读私稿/审核/审计，伪造 role | 401/越权/多余身份字段拒绝；公共目录没有待审包 |
| R 自己投稿后尝试批准，A 尝试审核 | 自审拒绝，普通用户没有审核权限 |
| 错误 version/digest 决定或发布 | 冲突拒绝，状态不变 |
| 批准前发布、批准后换包、重复版本 | 全部拒绝；没有包编辑 API，版本永久唯一 |
| R 批准后 A/R 发布 | 返回真实签名包，独立公钥和原生 digest/signature 预览通过 |
| A/B 新版本，扩大五类权限 | 合法 MCP 每个新版本重新 submitted，旧批准无效；安装仍须宿主支持及完整权限确认，当前 writePaths/process/credentials 在原生预览被阻断 |
| 修改包/摘要/签名或陌生 key | 预览/安装拒绝；不自动信任返回的 key |
| 竞争投稿、审核、发布/撤回、重复确认 | 唯一版本、合法状态、单次安装，无可新下载的 withdrawn |
| 作者/R 撤回发布 | 目录/新下载无该版本；精确 digest 的签名通知验证并处理安装撤回 |
| 秘密字段、描述、权限、文件名/正文/编码上传 | 隐私拒绝且错误、库、日志不含被拒绝内容 |
| 备份、隔离恢复和原生核验 | 不丢审批/版本/身份/撤回/审计；恢复副本清除 sessions，原库保留 |

本地 Rust 覆盖真实 loopback HTTP、服务角色/事务竞态及独立原生签名验收。前端传输/native invoke 是测试替身，不作为外部 HTTPS/完整桌面交互证据。离线已安装客户端不会立即获知撤回，恢复联网后的处理仍需正式验收。

## 私有备份与隔离恢复

程序使用 Python 3 标准库/SQLite backup API，包含已提交 WAL，不直接 `cp` 活跃库。快照为独占新建私有目录，含 `marketplace.sqlite3` 和 `manifest.json`，拒绝覆盖。负责人须先安排获批私有父目录、独立加密副本、访问权限、保留期限和恢复责任人。

下面私有目录是运维示例，实际路径须获批。SHA256/manifest 检查拷贝完整性，不能证明整个快照未被特权管理员替换；快照和独立参考来源必须可信。

~~~sh
set -eu
sudo install -d -o xy-marketplace -g xy-marketplace -m 0700 /var/backups/xy-marketplace-private
snapshot="/var/backups/xy-marketplace-private/snapshot-$(date -u +%Y%m%dT%H%M%SZ)"
sudo -u xy-marketplace python3 /opt/xy-marketplace/current/sqlite_backup.py backup \
  --database /var/lib/xy-marketplace/marketplace.sqlite3 --output "$snapshot"
sudo -u xy-marketplace python3 /opt/xy-marketplace/current/sqlite_backup.py inspect \
  --snapshot "$snapshot"
~~~

输出 `signatureVerificationPerformed:false` 明确表示 Python 不验 Ed25519。用 `--verify-state` 验候选 schema、全文摘要、精确批准、发布签名和撤回关联；不监听、不把新配置审核者/公钥写进候选库。

`--reference` 必须是不同文件/不同 inode 的独立当前完整库。snapshot 与 reference 的 durable tables 和审计 sequence 须精确相同；恢复副本 sessions 全部清除，限流保留参考当前值。在线备份可能落后，切换前先停止服务并取最终快照。以下隔离演练始终保留原库。

~~~sh
set -eu
sudo systemctl stop xy-marketplace
snapshot="/var/backups/xy-marketplace-private/final-$(date -u +%Y%m%dT%H%M%SZ)"
sudo -u xy-marketplace python3 /opt/xy-marketplace/current/sqlite_backup.py backup \
  --database /var/lib/xy-marketplace/marketplace.sqlite3 --output "$snapshot"
sudo install -d -o xy-marketplace -g xy-marketplace -m 0700 /var/lib/xy-marketplace/restore-rehearsal
candidate="/var/lib/xy-marketplace/restore-rehearsal/candidate-$(date -u +%Y%m%dT%H%M%SZ).sqlite3"
sudo -u xy-marketplace python3 /opt/xy-marketplace/current/sqlite_backup.py restore \
  --snapshot "$snapshot" --reference /var/lib/xy-marketplace/marketplace.sqlite3 --output "$candidate"
sudo -u xy-marketplace /bin/sh -eu -c '
  set -a
  . /etc/xy-marketplace/marketplace.env
  set +a
  export XY_MARKETPLACE_DATABASE="$1"
  exec /opt/xy-marketplace/current/xy-marketplace --verify-state
' sh "$candidate"
sudo systemctl start xy-marketplace
~~~

任一步失败即中止候选切换，原库未覆盖；负责人确认当前库有效后恢复服务。候选仅用于隔离演练，实际引用候选需要另行审阅数据库路径并批准切换；保留原库/WAL，不移动/删除正在使用的库，不以主应用 DB 作参考。

**恢复限制：** 快照落后于审批、发布、撤回、身份/权限或审计时拒绝，不能删新记录使恢复“成功”。当前完整库/可信参考丢失时，无法证明旧快照安全而拒绝。运维须落实可保全当前参考的复制/外部存储并演练，或另行授权最新撤回/权限记录保全的灾难恢复实现。当前仅具有在线备份、当前状态隔离恢复及保留原库的代码回滚能力。

## 代码回滚与密钥轮换

回滚保留当前库、审核者配置、当前签名 ID/私钥、历史公钥和撤回；不恢复旧权限或应用 DB。目标制品须能读当前 schema/config。

1. 负责人批准已知 `/opt/xy-marketplace/releases/<制品 SHA256>`，校验制品，停服务并取最终快照，保留原 current 目标和配置副本供审计。
2. 服务身份加载**当前** env，目标 binary 分别 `--check-config`、`--verify-state`。任何失败即停；不降 schema、删除触发器或丢弃历史 key 来适配旧代码。
3. 两项通过只切换制品 symlink，原库不变。`rollback_release` 是负责人输入的已审阅 64 位 SHA256。

~~~sh
set -eu
printf '%s' "$rollback_release" | grep -Eq '^[0-9a-f]{64}$'
test -x "/opt/xy-marketplace/releases/$rollback_release/xy-marketplace"
test ! -e /opt/xy-marketplace/current.next
test ! -L /opt/xy-marketplace/current.next
sudo ln -s "/opt/xy-marketplace/releases/$rollback_release" /opt/xy-marketplace/current.next
sudo mv -Tf /opt/xy-marketplace/current.next /opt/xy-marketplace/current
sudo systemctl start xy-marketplace
curl --fail --silent --show-error http://127.0.0.1:47831/readyz
~~~

4. 重验外部 HTTPS、安装、撤回，记录制品/当前审计 sequence。失败保留原库，按同一预检选上次可用制品，不恢复早于新撤回的快照。

密钥轮换先独立向客户端分发并确认新公钥/指纹，再提供新私钥/新 ID，历史公钥继续保留。`--check-config`、当前库 `--verify-state` 通过并批准重启后登记新公钥。旧发布不重签/换包，旧撤回保留原签名；私钥轮换和本地信任变更另行授权。

## 尚需输入与授权

| 输入/授权 | 负责人需要提供或确认 |
| --- | --- |
| 公共运行 | 获批主机/OS/架构、远程管理权限、部署窗口、负责人、制品 SHA256 |
| 网络/TLS | 已拥有域名、DNS 修改权、有效证书/私钥托管、反代/firewall、精确 CORS |
| 身份/签名 | 独立审核者名单、负责人生成的 salted hashes、PKCS#8 v2 私钥、稳定 ID、历史公钥 |
| 客户端信任 | 独立于服务响应的公钥/指纹分发、用户明确确认 |
| 存储/恢复 | 独立磁盘/权限、私有加密备份、保留/RPO/RTO、当前参考保全和恢复负责人 |
| 验收/集成 | 两普通测试身份和独立审核者外部验收授权；主模块/命令/页面路由/词条/总构建验收 |

截至专项日期，以上未作为正式执行授权提供给本任务。未注册域名、访问正式主机、创建真实账号、使用真实私钥或对公网启动；本地专项只证明当前独占代码合同和防护行为。
