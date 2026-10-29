# E2EE 手动配置交换本地验收（2026-10-29）

## 结论与交付边界

开发分支 `codex/e2ee-sync-20261029` 增加桌面手动 `.xysync` 文件交换、冲突合并、备份回滚和原生收藏迁移。它与 1.2.0 发布候选 `b51387f9604d0ef19a8e06202504dc42b294ce03` 分离，不把开发代码算进 1.2.0 发布资产。

本轮没有再调用真实计费 API。先前 SuperAPI 真实代理验收与失败记录单独保留；本轮同步测试使用测试数据、本地临时文件/数据库与隔离浏览器。

## 验收结果

| 验收层 | 结果 | 证据 |
| --- | --- | --- |
| 全量 Rust library | 380 项：376 passed，4 ignored，0 failed | `artifacts/encrypted-sync-qa/rust-tests.txt` |
| 全量 Vitest | 43 files / 476 passed | `artifacts/encrypted-sync-qa/frontend-tests-final.txt` |
| TypeScript 检查 | 通过 | `artifacts/encrypted-sync-qa/lint-final.txt` |
| Renderer production build | 通过 | `artifacts/encrypted-sync-qa/renderer-build-final.txt` |
| 加密同步 UI（mock IPC） | 11/11 | `artifacts/encrypted-sync-qa/ui-2026-10-29T15-43-25-847Z/results.json` 与同目录截图 |
| 旧收藏浏览器兼容 UI | 8/8 | `artifacts/session-favorites-qa/2026-10-29T15-34-15-761Z` |
| 原生文件选择器 + UI + 加密导入整链 | 未完成 | 不用 mock 或纯后端测试替代 |
| E2EE 五平台 CI | 本记录时尚未完成 | 必须核对后续精确 feature 提交与 CI，不引用 1.2.0 的 CI 替代 |

本地证据根目录：`E:\XY switch\workspaces\session-favorites-20261029`。ignored 项不计为通过；系统凭据存储 round-trip 和原生安装验收由隔离 CI 单独运行。已有 Rust unused-import、前端 chunk-size / Tauri import-mix 警告不影响本轮退出码，也未为消除警告修改无关代码。

### 真实后端链路

20 项 E2EE Rust 测试覆盖随机化认证加密、错误口令、salt/nonce/tag 篡改、KDF 前限长、未知字段与重复 ID、URL 凭据拒绝、白名单导出、收藏迁移只执行一次、预览不应用传入配置、每项冲突必选、供应商凭据 ID 隔离、目标变化创建禁用副本、本机 advanced 保留、stale preview、事务失败原子回滚、保留用量历史、回滚冲突、备份保留限制、一次性票据及非覆盖文件操作。

`two_devices_exchange_real_encrypted_files_resolve_conflicts_and_rollback` 使用两台内存数据库和真实临时文件：导出 → 加密文件写入 → 另一端读取解密 → 冲突导入 → 反向交换 → 回滚。没有将 mocked invoke 结果当作密码学验证。

### 交互链路

11 项 mock IPC 浏览器用例覆盖：默认不自动同步；导出预览/口令/确认/清空；逐项冲突、备份与回滚；提交成功但刷新失败时不重复提交；代理运行阻止导入；错误口令和 stale preview；收藏迁移失败阻止交换；迁移不删除原 localStorage；迟到和过期票据清理；分页及窄屏；英文和移动/浏览器只读。

重点回归：同步写入成功后，收藏刷新失败应明确抛出并显示局部刷新失败，不把持久化成功误报为失败；收藏、主视图和备份列表用独立 settled 结果处理，仍保留已提交状态与备份，apply 只调用一次。

### 证据 → 结论 → 用户路径

| 证据 | 可支持的结论 | 用户路径 |
| --- | --- | --- |
| 真实双数据库/临时文件测试及篡改测试 | 当前文件格式可往返交换，认证失败不部分写入 | 源设备导出、目标设备预览并逐项确认 |
| 目标变化、UUID 隔离与禁用副本测试 | 传入配置不会继承另一目标的本机凭据 ID | 为新增/变更目标单独配置凭据后手动启用 |
| 事务、stale preview、回滚测试 | 导入与备份原子化，旧预览不覆盖新状态 | 重新预览或选择最新备份，不强制覆盖 |
| mock UI 与刷新失败回归 | UI 要求显式确认、逐项选择，保存成功不会因刷新失败丢失状态 | 收到“数据已保存”时刷新界面，不重复提交 |

## 复现命令

在开发副本运行；Windows 使用项目已有 Rust 工具链环境。UI 脚本只连接 loopback renderer 与独立测试浏览器，不连接用户日常浏览器。

~~~powershell
. 'E:/XY switch/artifacts/maintenance-20261027/rust-env.ps1'
cargo test --manifest-path src-tauri/Cargo.toml --lib --offline --locked -- --quiet
pnpm test
pnpm lint
pnpm build:renderer
~~~

先启动 `pnpm dev:renderer --host 127.0.0.1 --port 1431 --strictPort`，并启动使用独立 user-data-dir 的 headless Edge（CDP 9231），随后运行：

~~~powershell
pnpm exec node scripts/verify-encrypted-sync-ui.mjs
pnpm exec node scripts/verify-session-favorites-ui.mjs
~~~

UI 截图、results.json 与原始测试输出保留在 artifacts；可再生浏览器 profile、renderer dist 和编译缓存可在进程停止后清理，不删除测试结果。

## 尚未支持

没有远程云端同步、后台定时交换、账户服务、口令恢复、删除传播、移动端同步写入。原生 SQLite 备份含本机 advanced 配置且未用本同步口令加密；本机完整状态哈希/备份会读取 advanced，不应宣称“完全不读取高级配置”。同步白名单不能保证用户任意填入的文本都没有秘密。此阶段不扩大到自动写规则文件或自动执行恢复命令。
