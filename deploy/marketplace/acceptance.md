# 公共市场现场验收与失败回滚

**状态：待授权执行，公共服务尚未上线。** 本文件是指定主机、真实 HTTPS 与桌面应用的操作顺序和判定条件；所有现场步骤当前均为“未执行”。已有本地结果见 [专项验收记录](verification.md)，运行命令及配置见 [部署文档](../../docs/marketplace-deployment.md)。

## 先确定执行输入

部署负责人先登记获批的主机/OS/架构、制品 SHA256、配置版本、操作窗口、执行人和回滚负责人。主程序正式构建、页面路由、英文词条、原生命令与本地公钥信任须由主代理完成验收。

| 输入 | 必须明确的值或权限 |
| --- | --- |
| 服务及网络 | 实际 HTTPS origin、DNS 修改权、有效证书与私钥托管、反代/firewall 权限；后端 `127.0.0.1:47831` 或另一个批准的 loopback 地址 |
| CORS | `XY_MARKETPLACE_ALLOWED_ORIGINS` 为实际桌面/web origin 的精确列表；不使用 `*`；正式配置两个 development/non-loopback 开关均为 false |
| 独立身份 | 两个合成普通账号 A/B 的创建权限；R 是部署配置指定的独立审核者，负责人提供 salted passwordHash；不得连接订阅账号 |
| 签名及信任 | ring 可解析的 Ed25519 PKCS#8 v2 DER 私钥、稳定 signingKeyId、历史公钥；客户端通过独立渠道核对公钥/指纹并明确确认信任 |
| 数据和恢复 | 独立市场数据库与私有权限、加密备份目的地/访问权/保留期、RPO/RTO、可信当前完整 reference 保全方案与恢复负责人 |
| 外部操作 | 明确授权受控注册、上传、审核、安装、更新、撤回、两请求并发验收及备份恢复/制品回滚演练；正式公开入口切换单独批准 |

凭据由执行人通过应用或获批 API 工具输入，session 只留内存。请求历史、终端参数、截图和证据不得包含 password/bearer/private key；不导出未清理的 HAR。上传保护只使用合成标记，不读取真实凭据。下面包会公开展示，执行前须批准其内容和展示名。

## 准备无外部动作的验收包

A 的公开展示名设为 `Acceptance A`，包作者与其一致。B 使用 `Acceptance B`，复制该包并改为另一个唯一 id 和自己的 author。username 由执行人自选，不能作为包作者或公开证据。这个 MCP 包的工具只返回显式输入的 JSON，不读取本机文件、不启动进程、不访问网络。

~~~json
{
  "manifest": {
    "schemaVersion": 1,
    "id": "acceptance-mcp-a",
    "name": "Acceptance echo",
    "description": "Return text explicitly provided for the authorized acceptance run.",
    "author": "Acceptance A",
    "version": "1.0.0",
    "minimumHostVersion": "1.0.0",
    "kind": "mcp",
    "entry": "tools.json",
    "clients": ["codex-desktop", "claude-desktop", "opencode-desktop"],
    "permissions": {
      "network": [],
      "readPaths": [],
      "writePaths": [],
      "process": false,
      "credentials": []
    },
    "dependencies": []
  },
  "files": {
    "tools.json": "{\"schemaVersion\":1,\"tools\":[{\"name\":\"echo\",\"description\":\"Return explicitly supplied text\",\"inputSchema\":{\"type\":\"object\",\"properties\":{\"message\":{\"type\":\"string\",\"maxLength\":1000}},\"required\":[\"message\"],\"additionalProperties\":false},\"operation\":{\"type\":\"json_passthrough\"}}]}"
  }
}
~~~

id/version 永久唯一，重复演练需要新 id 或新版本。不得重用本地夹具的密钥或账号。若用 skill 包验收，permissions 必须全空；五类权限扩展使用这里的合法 MCP 包。

## 按阶段执行和记录

每步记录 UTC 时间、执行人、制品 SHA256、步骤 ID、实际 HTTP 状态/安全 error.code、非秘密的 id/version/digest、实际观察和判定。外部证据存放于获批私有位置；公开报告只保留 displayName，不保留 username/userId、私稿正文或凭据。任何步骤未执行、失败或证据缺失，均不得填“通过”。

### 1. 主机预检与外部传输

按部署文档“部署步骤”在获批主机执行预检，先保持公开入口关闭。

| ID | 操作 | 通过条件 | 失败处理 |
| --- | --- | --- | --- |
| P1 | 校验制品 SHA256、审核 env/systemd/nginx 配置及私有文件权限；执行 `--check-config`、`systemd-analyze verify`、`nginx -t` | 制品匹配已批准版本，私钥真实解析，CORS/监听/TLS 路径符合已批准配置；全部 exit 0 | F1 |
| P2 | 获批后启动 loopback 服务，查询 `/healthz`、`/readyz`；在相同 env 下执行 `--verify-state` | 健康/就绪成功，现有库的 schema、摘要、批准、发布/撤回签名及关联验证成功 | F1；若现有库核验失败按 F2 |
| P3 | 只在受控验收入口使用外部设备访问实际 HTTPS；检查证书、重定向和健康/就绪 | 正常可信证书，无忽略验证选项；HTTP 不承载登录，后端端口不能从公网直接访问 | F1 |
| P4 | 从批准 origin 发 CORS 预检，并对不在名单的 origin 请求私有端点 | 批准 origin 得到精确 allow-origin；未批准 origin 为安全拒绝，不能读取私有响应 | F1；若私有数据可读按 F2 |

P3/P4 是真实外部访问证据；本地 loopback 成功不能替代。未获得外部验收授权时停在 P1/P2 已获准的步骤，不切换公开入口。

### 2. 独立身份与私稿隔离

使用应用的“连接市场”“注册市场账号”“登录市场”，并在获批、关闭历史导出的 API 工具中做服务器拒绝测试。普通用户页面不展示审核权限，仍须验证服务端不会接受越权请求。

| ID | 操作 | 通过条件 | 失败处理 |
| --- | --- | --- | --- |
| I1 | A/B 分别注册、登录，R 使用部署身份登录；查询 `GET /v1/auth/me` | A/B 均为普通用户，R 的 reviewer 由服务端确定；注册 201、登录/me 200 | F1；身份越权按 F2 |
| I2 | 注册请求额外添加 `role`、`reviewer` 或 `userId`；A 读取 `/v1/review/submissions` | 多余身份字段 400，普通用户审核入口 403；没有角色升级 | F2 |
| I3 | A/B 各通过“提交审核”发送 `POST /v1/submissions` 的 `{package}` | 两者都得到 201/submitted；“我的审核状态”仅显示本人投稿 | F1 |
| I4 | 匿名请求 A 私稿详情、本人投稿列表、私有审计；B 请求 A 详情；匿名浏览 `/v1/plugins` | 匿名私有请求 401，B 读 A 私稿 403；目录 200 且不含 submitted/approved/rejected 等未发布版本 | F2 |
| I5 | 退出 A 后重用旧 session 请求 me；切换服务地址/重开页面 | 退出 204，旧 session 401；认证信息不在持久存储，地址改变或重开需重新登录 | F2 |

### 3. 精确版本审核、发布和更新

从投稿响应或只读“投稿审阅”取得实际 `submissionId`、`version`、`digest`。决策请求只使用 `{version,digest,decision,note?}`，路径为 `POST /v1/review/submissions/{submissionId}/decision`；decision 为 `approve`、`request_changes`、`reject`。发布为 `POST /v1/submissions/{submissionId}/publish`，正文只有 `{version,digest}`。note 只填无个人信息的必要说明，不以自由说明留存作为验收条件。

| ID | 操作 | 通过条件 | 失败处理 |
| --- | --- | --- | --- |
| R1 | R 自己投稿，再对本人投稿 approve/request_changes/reject；A 对本人/其他人调用决策 | 全部 403，无审核状态变化；审核者也不能自审 | F2 |
| R2 | A 在 submitted 状态发布；R 使用错误 version 或不同的合法 64 位 hex digest 作决策 | 发布和错误精确绑定均 409，原投稿/批准未改变 | F2 |
| R3 | R 审阅包全文、完整权限与本次新增权限，勾选确认后“批准此版本” | 精确包得到 approved；匿名目录和下载仍不含该版本 | F2 |
| R4 | 已批准后用同 id/version 再上传修改 files 的包；发布请求额外带 package；尝试 PUT/PATCH 投稿 | 重复版本 409，额外字段 400；没有编辑 API，PUT/PATCH 被拒绝；批准包仍是原摘要 | F2 |
| R5 | A 或 R “发布已批准版本”，匿名下载 `/v1/plugins/{id}/versions/{version}` | 返回 published 的原包、原 digest、真实 hex Ed25519 signature/signingKeyId；公开身份仅 displayName | F2 |
| R6 | 对 A/B 新版本均“提交新版本”；另一个投稿先 request_changes 或 reject，再以新版本重投 | 每次均重新 submitted；旧批准不能发布新版本；新版本须另一身份精确批准 | F2 |
| R7 | 两请求同时提交相同 id/version；同时批准/拒绝同一 submitted；两请求同时发布 | 唯一版本、恰好一个合法状态迁移、最多一条发布记录；失败请求无审计/状态副作用 | F2 |

签名的 UTF8 payload 是 `xy-switch-plugin-v1\n{id}\n{version}\n{digest}`。digest 由插件模块对 typed canonical serde_json 算 SHA256；不能以原始上传文本或普通对象序列化替代。原生安装预览使用独立可信公钥验证 payload，不从下载响应自动建立信任。

为五类扩展分别使用五个新 MCP id：各先发布空权限 1.0.0，再提交 1.1.0，且只添加下面一个维度。每次都要观察 submitted、精确的新增权限及批准前发布 409，再由 R 审阅批准。

| ID | 1.1.0 新增权限 | 市场及宿主判定 |
| --- | --- | --- |
| E1 | `network:["https://example.com"]` | 必须新审核；批准发布后安装需完整权限确认，工具本身无网络调用 |
| E2 | `readPaths:["package:tools.json"]` | 必须新审核；只声明包内读取，安装需完整权限确认 |
| E3 | `writePaths:["package:tools.json"]` | 必须新审核；当前宿主不支持写权限，原生预览显示阻断，确认不能安装 |
| E4 | `process:true` | 必须新审核；当前宿主不支持进程权限，原生预览显示阻断，确认不能安装 |
| E5 | `credentials:["acceptance-credential"]` | 名称不是凭据值，必须新审核；当前宿主不支持凭据权限，原生预览显示阻断，确认不能安装 |

该矩阵按当前 [插件 validation](../../src-tauri/src/plugins/validation.rs) 和 [权限审核测试](../../src-tauri/src/marketplace/tests.rs) 编写。市场审核通过不会解除宿主运行能力限制；不将 skill 包加权限、包外读取或不支持的运行能力当成可安装成功项。

### 4. 安装、撤回、隐私和举报

| ID | 操作 | 通过条件 | 失败处理 |
| --- | --- | --- | --- |
| S1 | 用尚未信任该 key 的验收客户端“预览安装”，再由用户从独立渠道核对指纹并配置本地信任，重新预览/确认无阻断的基础包 | 未知 key 阻断，服务返回 key 不自动信任；已信任时原生命令验证原 id/version/digest/signature 后单次安装 | F2 |
| S2 | 在受控响应副本分别改变 files、digest、signature、signingKeyId；另做预览后撤回/变化及重复并发确认 | 篡改和预览后变化均阻断；原机安装状态不受失败请求改变，重复确认不重复安装 | F2 |
| S3 | 作者/R 对发布调用 `/withdraw`，先用错误 digest，再用精确 `{version,digest,reason,note?}`；reason 为 security/privacy/withdrawn | 错误 digest 409 且无副作用；正确撤回后目录/版本历史不列该版本，新下载 404 | F2 |
| S4 | 读取 `GET /v1/plugins/{id}/versions/{version}/withdrawal`，由原生流程核验并处理安装撤回；修改通知副本再校验 | 签名通知关联原发布的 id/version/digest；有效通知处理撤回，错误摘要/签名通知被拒绝 | F2 |
| S5 | 对单独的已批准版本发两请求，分别 publish 与 withdraw；再请求新下载 | 最终为 withdrawn；可以没有发布或有一条带合法撤回的发布，无可新下载版本/孤立通知 | F2 |
| S6 | 离线客户端保留已安装包，再恢复联网并触发已验证的撤回处理 | 联网后真实通知被核验并交原生模块处理；记录人工触发步骤和时延，不声称后台实时撤回 | F1；校验绕过按 F2 |
| S7 | 在合成包 manifest 全字段、permissions/dependencies、files 名称/正文及嵌套编码逐项加入测试 secret 标记；分别做客户端预检和受控 REST 直传 | 隐私或 schema 拒绝（422/400），无待审/公开记录；UI/error/私有日志/数据库不含拒绝内容或秘密回显 | F2 |
| S8 | B 举报 A 的已发布版本，R 查看并处理举报，A/B 尝试进入举报审核；读取获准的审计 | 注册用户可举报，普通用户审核 403；审核/发布/撤回/举报有可追溯记录，公开不暴露内部身份 | F2 |

S7 的无落库/无日志断言由获批运维在受控合成数据库/私有日志中检查，并只记录“命中/未命中”的结果。不要为验收修改当前库或删除触发器；关联篡改与审计事务失败已有隔离专项测试，线上验收只操作受控响应副本。S1/S2 必须通过真实 `plugins_preview_install`/`plugins_confirm_install`；S4/S6 使用 `plugins_apply_withdrawal`，测试替身不能替代这些证据。

### 5. 备份、隔离恢复及代码回滚

按部署文档“私有备份与隔离恢复”“代码回滚与密钥轮换”的现有命令执行。Python 工具使用 SQLite backup API，输出 `signatureVerificationPerformed:false`；必须再执行实际 binary 的 `--verify-state`，不能把 Python manifest 检查记为签名验收。

| ID | 操作 | 通过条件 | 失败处理 |
| --- | --- | --- | --- |
| B1 | 在允许在线备份的窗口取私有新快照并 inspect；检查加密副本及访问权 | 完整包含已提交 WAL，拒绝覆盖旧路径；备份离开主机前实际加密且访问受控 | F3 |
| B2 | 保留该旧快照，再产生新批准/发布/撤回及身份或审核权限变动；对独立候选路径尝试用旧快照恢复 | 因 durable state/审计 sequence 不一致而拒绝；当前库和新撤回/权限记录完整 | F3 |
| B3 | 关闭入口、停服务后取最终快照，用不同文件/inode 的可信当前完整 reference 恢复到新候选；执行候选 `--verify-state` | schema/摘要/批准/签名/撤回/审计通过；候选 sessions 清空，当前限流保留；原库及 WAL 不覆盖 | F3 |
| B4 | 获批制品回滚演练：对已审阅的目标 SHA256 预检，保持当前库/config/keys，切换制品 symlink 再验证就绪与 HTTPS/安装/撤回 | 当前 schema 与配置兼容；最新批准、身份权限、公钥与撤回未回退；预检和真实流程都通过 | F4 |

恢复候选只用于隔离演练，引用候选的数据库路径切换需要额外批准。当前库与可信当前 reference 同时丢失时，本方案无法证明旧快照不会复活撤回版本/旧权限，保持离线并由负责人安排另行审阅的恢复方案。

## 失败时按阶段处理

| 处理 ID | 触发 | 执行动作与恢复条件 |
| --- | --- | --- |
| F1 | 公开切换前配置、TLS、集成或普通功能验收失败 | 停止该阶段并保持公开入口关闭，保留非秘密证据；仅在修复项对应验收重新通过后继续。不能仅凭 healthz 通过切换 |
| F2 | 身份越权、自审/换包/版本绕过、秘密回显、签名/摘要关联异常或 withdrawn 仍可下载 | 立即关闭市场公开入口并停止市场写入/服务；负责人保全当前库及 WAL、config、keys 和审计于私有存储。核验失败时保持离线；若为单包问题，由独立授权审核者在修复且核验通过后精确撤回。凭据处理/密钥轮换另行授权，不自动删除或回放旧库 |
| F3 | 备份缺失/不完整、恢复 reference 落后、候选签名或审计检查失败 | 中止候选切换，保留原库和失败证据，不覆盖或删当前库、WAL、审批、撤回、身份权限。当前原库实际 `--verify-state` 通过且负责人批准后才恢复原服务；原库无法核验时保持离线 |
| F4 | 制品回滚预检、schema/config 兼容或回滚后验收失败 | 保持入口关闭；仅选择另一个已批准、与当前库/config/keys 兼容的制品重复只读预检。保留当前数据；没有兼容制品就保持离线，不通过恢复旧快照让旧程序启动 |

关闭入口、停止服务、切换制品和恢复服务均由具有相应运维权限的负责人执行，并登记操作时间/制品/当前审计 sequence。停止服务不自动删除任何数据，代码回滚不撤销用户撤回。模板没有执行自动部署、自动密钥轮换或自动数据库替换。

## 出具现场结果和切换批准

负责人为 P/I/R/E/S/B 的每个 ID 登记 `未执行 / 通过 / 失败`、私有证据位置和失败处理；当前全部为 `未执行`。公开身份只使用 displayName，日志和截图先人工确认无秘密。备份 reference 保全、真实原生校验、离线撤回时延和恢复限制均须写入现场结论。

全部必要项有真实证据、失败项已修复重验、回滚负责人/窗口就绪后，用户明确批准实际主机/域名的公开入口切换。该批准和切换后的外部结果产生前，交付状态一直是“公共部署准备完成，尚未公共上线”。
