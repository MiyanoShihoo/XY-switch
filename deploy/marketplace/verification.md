# 公共市场专项验收记录

**状态：代码与本地专项验证已完成，公共服务尚未上线。** 最近一次留存证据日期为 2026-11-04，工作区为 `E:\XY switch\workspaces\session-favorites-20261029`。本记录沿用已结束的验证结果；最后的修补只涉及运行文档和本记录，没有重新执行完整测试。

## 已通过的验证

| 项目 | 实际结果 | 原始证据 |
| --- | --- | --- |
| 市场后端 | 15 passed / 0 failed | [rust-tests.log](verify/target/evidence/rust-tests.log) |
| 独立 binary CLI | 4 passed / 0 failed，同次 Cargo 执行成功结束 | [rust-tests.log](verify/target/evidence/rust-tests.log) |
| 市场前端定点测试 | 19 passed / 0 failed，1 test file | [frontend-tests.log](verify/target/evidence/frontend-tests.log) |
| 定点 TypeScript | exit 0 | [typescript.log](verify/target/evidence/typescript.log) |
| SQLite 备份恢复 | 9 passed / 0 failed | [backup-tests.log](verify/target/evidence/backup-tests.log) |
| 英文词条映射 | 159 source keys / 159 entries；missing、extra、empty 均为空 | [i18n-check.json](verify/target/evidence/i18n-check.json) |

Rust 记录使用 rustc 1.98.1、cargo 1.98.1；有一项 `privacy::inspect_note` 未使用警告，没有测试失败。证据位于被忽略的独立 `verify/target/evidence`，属于本地验收材料；交接时需要另行保全日志。

先前文档校验通过：15 个本地链接均存在，审核者 Python 示例经 AST 解析通过，7 个部署 Shell 示例经现有 WSL 的 `sh -n` 解析通过（exit 0）。示例未执行，记录为 `verify/target/evidence/documentation-checks.json`；不代表目标主机上的 nginx/systemd 验收。新增 [现场验收与失败回滚](acceptance.md) 提供受控外部操作顺序、合法 MCP 包、权限/宿主限制、证据记录和分阶段失败处理；全部现场步骤尚未执行。

## 已覆盖的行为

- 两个普通注册用户和独立审核者通过真实 axum 路由完成登录、投稿、审核、发布与公共下载。匿名读取私稿、越权读取/审核、客户端伪造角色和审核者自审均拒绝。
- 新插件、每个新版本和 requested changes 后的重投均须独立审核。批准绑定精确版本与摘要；同版本包不可替换，重复版本不可提交，已发布包不可原地改写。
- network、readPaths、writePaths、process、credentials 五个权限维度的扩展均重新 submitted，未经独立审核不能发布。
- 五类扩展的市场审核证据不代表宿主可以运行全部权限。当前原生预览阻断 writePaths/process/credentials 及包外读取；skill 的 permissions 必须全空。现场成功安装使用合法、受支持的 MCP 包。
- typed canonical JSON 摘要与实际 Ed25519 发布签名验证通过；独立原生信任验证拒绝未知 key 和篡改。预览后版本、摘要、签名变化、重复并发确认及撤回会阻断安装。
- 撤回验证原始投稿、批准、发布记录、版本、摘要和签名关联；篡改关联时拒绝且不产生撤回/状态/审计副作用。撤回后新下载不可用；并发发布与撤回、跨连接重复投稿/审核/发布及审计写失败回滚均覆盖。
- manifest 全字段、描述、permissions、dependencies、files 的路径与内容、嵌套编码及当前敏感身份值通过隐私检查；错误不回显测试 secret，拒绝包不落库。举报及审核自由说明不持久化，公开身份只 displayName。
- CLI 真实解析私钥并核对公钥；`--check-config` 不建库、不监听。`--verify-state` 只读现有市场库，拒绝缺失/空白/异库/篡改 schema，不同步审核者、公钥或审计。
- 备份覆盖 WAL；拒绝覆盖、异库、损坏文件/元数据、缺失不可变触发器，以及丢失新发布、批准、撤回、身份权限或审计的旧快照。恢复要求独立当前 reference，清除会话并保留当前限流。

## 验证边界与剩余验收

Rust 使用 [独立 verification crate](verify/Cargo.toml) 编译实际市场模块、插件 types/validation；[build.rs](verify/build.rs) 从实际 privacy 源码提取扫描核心，排除应用 DB、Tauri 与流处理集成。前端 fetch 和 native invoke 为测试替身；页面验收包含无必须 props 及未登录私有入口保护。以上证据不覆盖主应用总构建、真实桌面完整交互、目标主机 systemd/nginx 配置或外部 HTTPS 工作流。

Python 备份恢复工具明确 `signatureVerificationPerformed:false`，候选库还须实际服务 `--verify-state` 核验签名。当前支持在线备份、当前状态隔离恢复和保留原库的代码回滚；当前库与可信当前 reference 同时丢失时，没有已验证的灾难恢复能力。旧快照恢复不得移除新撤回/批准/权限记录。页面没有后台持续轮询已安装包撤回的机制；需下次联网经已验证流程处理。

尚未执行：主代理总构建/路由/i18n/原生命令验收；指定目标主机预检；外部两用户与独立审核者工作流；真实 HTTPS、证书/CORS、安装/更新/撤回；获批主机备份恢复与代码回滚演练。正式运行输入与步骤见 [部署文档](../../docs/marketplace-deployment.md)。

## 复跑命令

在仓库根执行；Rust 脚本用于 Linux/WSL 并指定独立 target，不启动主 Cargo。依赖须已存在本机缓存，脚本使用 `--locked --offline`。

~~~sh
sh deploy/marketplace/verify/run-linux.sh
npx vitest run src/marketplace.test.tsx
npx tsc -p deploy/marketplace/tsconfig.verify.json
python -m unittest discover -s deploy/marketplace -p sqlite_backup_test.py -v
~~~

## 集成入口

后端入口为 `MarketplaceConfig::from_env()`、`Marketplace::open(config)` 和 `marketplace::router(market)`；额外只读接口为 `Marketplace::verify_database(config)` 与 `market.verify_state()`。不存在 `MarketplaceManager` 或市场专用 Tauri 命令。独立 `xy-marketplace` 提供正常服务、`--check-config`、`--verify-state`、`--hash-password-stdin`。

前端命名导出 `MarketplacePage`，无必须 props；`MarketplaceClient(origin, options?)` 使用真实 REST，`VITE_XY_MARKETPLACE_ORIGIN` 提供初始服务地址。仅 HTTPS 与明确开发的精确 loopback HTTP；bearer 只留内存。安装复用 `plugins_preview_install`、`plugins_confirm_install`、`plugins_apply_withdrawal`，不自动信任服务返回的 key。

英文翻译通过独立 [src/features/marketplace/en-translations.json](../../src/features/marketplace/en-translations.json) 交付，由主代理在 `src/i18n/en.ts` 导入并展开到 `en` 对象；[i18n.en.json](i18n.en.json) 保留为内容一致的先前交接副本。依赖继续使用 axum、tokio、rusqlite、ring、sha2、serde、serde_json、reqwest、uuid、regex、zeroize；未新增 npm 依赖，主 Cargo 和共享入口由主代理整合。

## 完整交付文件

以下共 32 个交付文件，路径相对于仓库根，均在获准的市场写集；没有分支或提交操作。

| 类别 | 文件 |
| --- | --- |
| 后端 | `src-tauri/src/marketplace/api.rs`、`src-tauri/src/marketplace/auth.rs`、`src-tauri/src/marketplace/config.rs`、`src-tauri/src/marketplace/mod.rs`、`src-tauri/src/marketplace/privacy.rs`、`src-tauri/src/marketplace/requests.rs`、`src-tauri/src/marketplace/schema.rs`、`src-tauri/src/marketplace/store.rs`、`src-tauri/src/marketplace/tests.rs` |
| 独立服务 | `src-tauri/src/bin/xy-marketplace.rs` |
| 前端 | `src/types/marketplace.ts`、`src/lib/marketplace-service.ts`、`src/features/marketplace/MarketplacePage.tsx`、`src/features/marketplace/en-translations.json`、`src/marketplace.test.tsx` |
| 运行文档 | `docs/marketplace-deployment.md` |
| 部署配置 | `deploy/marketplace/.gitignore`、`deploy/marketplace/marketplace.env.example`、`deploy/marketplace/nginx.conf.example`、`deploy/marketplace/xy-marketplace.service` |
| 备份恢复 | `deploy/marketplace/sqlite_backup.py`、`deploy/marketplace/sqlite_backup_test.py` |
| 前端验收材料 | `deploy/marketplace/i18n.en.json`、`deploy/marketplace/tsconfig.verify.json` |
| Rust 专项 crate | `deploy/marketplace/verify/Cargo.toml`、`deploy/marketplace/verify/Cargo.lock`、`deploy/marketplace/verify/build.rs`、`deploy/marketplace/verify/lib.rs`、`deploy/marketplace/verify/run-linux.sh`、`deploy/marketplace/verify/tests/cli.rs` |
| 本验收记录 | `deploy/marketplace/verification.md` |
| 现场验收与失败回滚 | `deploy/marketplace/acceptance.md` |

## 正式公共运行所需输入与授权

1. 明确批准的主机/OS/架构、远程管理权限、部署负责人和窗口、正式制品 SHA256。
2. 已拥有域名与 DNS 修改权、有效 TLS 证书及私钥托管、反代/firewall 权限、精确 CORS origin 和服务监听地址。
3. 独立市场数据库与文件权限；独立审核者名单/公开展示名，以及负责人生成的 PBKDF2 salted passwordHash。
4. ring 兼容 Ed25519 PKCS#8 v2 DER 私钥、稳定 signingKeyId、历史公钥；独立于下载响应的客户端公钥/指纹分发和用户确认。
5. 私有加密备份存储、访问权限、保留策略、RPO/RTO、可信当前 reference 保全、恢复负责人及恢复/回滚演练授权。
6. 外部合成测试身份创建与上传/审核/安装/撤回验收授权；主代理完成共享模块、页面路由、英文词条与原生命令注册及正式构建验收。

这些输入没有作为正式执行授权提供给本任务。现有代码、模板和本地证据不能作为公共上线证据。
