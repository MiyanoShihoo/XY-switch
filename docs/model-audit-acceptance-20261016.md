# build004 本地应用验收报告

验收日期：**2026-10-16（Asia/Shanghai）**。结论：**build004 本地原生验收通过，当前源码归档及 unsigned debug MSI 打包/载荷静态核验完成；不等同商业渠道、实际安装卸载或签名发布验收。** 原生审计 **32/32**、最终业务流程 **108/108**，共27张截图，均无页面异常。保留三个未完成的业务 harness 尝试与所有历史失败，不将局部通过转记为整轮成功。本文采用任务验收报告结构（flavor=null），应用单测/编译沿用已验证相同输入的历史证据。

## 1. 范围与被测版本

本轮落实用户“完成更新后的交接文档后完成验收”的请求：在独立数据目录、HOME 和 WebView2 profile 中运行真实 Tauri 桌面程序及 production renderer，通过真实 IPC、隔离 SQLite/客户端配置、随机假凭据和回环 HTTP/SSE 夹具验证；随后从该 build004 制作 debug MSI、静态解包并归档当前源码。没有修改产品源码，没有重跑应用单元测试或编译，没有读取日常商业 key、发送真实商业请求、覆盖旧副本、回滚、stage/commit，或实际安装/卸载软件。

| 对象 | Evidence：原始文件 | Finding：绑定结果 | Path：适用范围 |
| --- | --- | --- | --- |
| 当前源码 | [source-mcp-confirmed.json](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/source-mcp-confirmed.json>) | 219/219 产品、测试、构建输入匹配；清单 SHA-256 `0a7708c62794aea55838f4fc0bd0031df9b513b31c11ba64c6daa0731dc3bb8c` | 主树当前版本，不是旧 modified 副本 |
| 当前 exe | [build-binding-004.json](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/build-binding-004.json>)、[tested-build-004/xy-switch.exe](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/tested-build-004/xy-switch.exe>) | 34,470,400 字节；SHA-256 `f10a4fe59d42a87cf7f73e894a229394d2e971e1a1d4ca7bf3845c5c17e64d7f`；debug/custom-protocol，包含生产 renderer | 所有本轮原生结果均嵌入同一 source/renderer/exe 绑定 |
| 历史单元测试与构建 | [application-acceptance-20261015](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261015>) | 2026-10-16 00:25:24 Rust 198/0/3；00:02:55 前端16文件/94项；check、renderer、debug 构建成功；debug 编译00:25:33开始、00:26:07完成 | 本轮复核既有证据，未重跑；3项既有 cargo check warning 保留 |

主树与旧副本仍是219/216输入、206相同、10不同、主树新增3个，原54项集成范围53相同/1不同。这些有来源的启动隔离、删除确认和 MCP 事务修复无需回滚。两个锁文件和 Git index 哈希保持接受基线值，当前 master 无 HEAD 提交。

## 2. 原生审计验收

| Evidence | Finding | Path |
| --- | --- | --- |
| [run-001/native-result.json](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/run-001/native-result.json>)、[native-audit.result.json](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/native-audit.result.json>) | **32/32通过**，complete=true，退出码0；13:55:30–13:55:45；11张截图、0 pageErrors | 真实桌面 production renderer 与 Tauri IPC；合成审计/配对数据，覆盖启停、空态、notes、过滤、缺口和三档主题 |
| [run-001/cleanup.json](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/run-001/cleanup.json>) | PID26752与可执行路径/hash核对后清理，14:00:16完成 | 仅该轮程序及经归属核对的 WebView2 后代，不操作日常程序 |

旧目录 application-acceptance-20261015 的 run-005 为 build003 的历史32项通过；当前通过结果来自 application-acceptance-20261016 的 run-001，不能将同名数字或旧版本结果混用。

## 3. 最终业务流程验收

[run-005/workflow-001/workflow-result.json](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/run-005/workflow-001/workflow-result.json>)及[命令结果](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/native-workflow-retry3.result.json>)记录 **108/108通过、complete=true、退出码0**，19:28:30–19:29:01，16张截图、0 pageErrors。

| Evidence：最终流程中的检查 | Finding | Path：覆盖与限制 |
| --- | --- | --- |
| 设置、导航、provider/prompt CRUD 和显式应用确认框 | 真实 UI/IPC 的保存、编辑、复制、排序、启停及删除通过；确认框默认取消、双向焦点陷阱、Escape/cancel保留记录、确认才删除 | 不使用原生 JavaScript dialog，不扩大 ACL |
| MCP四客户端选择与同步、取消选择/重选、编辑/toggle/删除 | Claude、Codex、Gemini、OpenCode 的隔离配置均同步，保留无关设置 | 合成配置路径；未修改用户日常客户端配置 |
| MCP失败注入与恢复 | 合成 Codex TOML 的无效段头引发 create/edit/toggle/delete 失败；数据库和所有客户端文件逐字节恢复，修复夹具后恢复创建与删除 | 验证事务与恢复，不将预设失败记为产品回归失败 |
| 随机假凭据 roundtrip 与模型发现 | 真 Credential Manager 读写回环通过，测试 key 随后移除；仅向回环 mock 发 HTTP | 不读取或导出日常商业 key，不认证真实供应商 |
| 回环代理、非流式、SSE、Responses→Chat桥接 | disabled Anthropic返回404且没有上游请求；成功SSE记录完成及3个文本delta；共13次代理请求、12成功、1个预设HTTP400 | 上游全部为回环合成HTTP/SSE，400为预期夹具 |
| 真影子采集及回归 | 6样本=1baseline+5pairs，5个不同本地增量；slope=1、intercept=0、xSpan=80、status=consistent | 使用已知本地tokenizer的合成语料；不能据此认证模型身份或供应商计费 |
| 只读SQLite与审计UI | audit_json/result_json中未发现本轮随机mock provider的请求/响应正文marker；UI展示13次请求，模型过滤12，shadow group为5配对 | 数据库断言仅针对该provider的audit/shadow字段，不扩大成“所有数据库从不存正文” |
| workflow资源清理 | 5/5通过：停止桌面proxy、删除且确认假凭据缺失、删除随机测试provider、恢复隔离设置、停止mock上游 | [run-005/cleanup.json](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/run-005/cleanup.json>)于19:29:05完成PID16652及后代清理 |

Session/Skills只验证空查询和未实现接口的显式拒绝，**不代表这两项业务功能已实现或验收通过**。

## 4. 失败尝试和进程清理追溯

| Evidence | Finding | Path |
| --- | --- | --- |
| [run-002](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/run-002>) | 75/76，complete=false：夹具routeTargets仅codex，Chat未分配到grok/opencode | harness路由修正；没有产品源码修改 |
| [run-003](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/run-003>) | 94/95，complete=false：从逆序日志误选新出现的HTTP400占位流 | 已存在的成功SSE有complete及3deltas；修正日志选择 |
| [run-004](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/native-desktop/run-004>) | 100条检查均true，但complete=false：假设了不存在的provider dropdown | 实际UI为provider pills+model/source dropdown；不把此轮视为通过 |
| [harness-retry3-manifest.json](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/harness-retry3-manifest.json>) | 最终脚本清单SHA-256 `57457b813fce4f44b935400b61c9fa7f471e813edf65e3c348045524be054d51` | run-005的最终108项成功绑定；此前所有manifest、stdout/stderr和失败目录保留 |
| 本轮run-001至run-005各cleanup.json | 5轮均completed=true，按run/路径/PID/创建时间归属核对后停止应用及WebView2后代 | 不宣称已复核所有系统进程；10月14日遗留PID28592的历史未核实状态不因本轮清理改变 |

## 5. debug MSI与静态载荷

| Evidence | Finding | Path |
| --- | --- | --- |
| [bundle.result.json](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/installer/retry1/bundle.result.json>)、[XY Switch_1.0.0_x64_en-US.msi](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/installer/retry1/target-build004/debug/bundle/msi/XY Switch_1.0.0_x64_en-US.msi>) | 19:44:43–19:44:58打包成功、exit=0；11,419,648字节；SHA-256 `fe57eda62c65d1b93190b9ab588aa54ccde6721e38aab2bee8db14c77b2f1d17` | unsigned debug，XY Switch1.0.0、x64，identifier com.xyswitch.desktop；不是release编译 |
| [静态解包结果](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/installer/retry1/static-extraction/extract.result.json>)、[payload-verification.json](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/installer/retry1/static-extraction/payload-verification.json>) | WiX dark仅解包，产品/架构/载荷核对成功；34,470,400字节的包内exe SHA-256 `d5b33b228a1cec68fc3aff2ffb02c73f79cfd29790f6a5c1a75087dfba7d7be2` | **不是与被测exe字节完全相同**：仅偏移27455976、27455977、27455978的bundle marker从UNK→MSI，其余字节一致；未从包内exe执行runtime |
| [product.wxs](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/installer/retry1/static-extraction/product.wxs>)及原始打包日志 | 4项WiX warning：ICE03 CustomAction.Target过长、ICE40 REINSTALLMODE、ICE57 HKCU/per-machine混合、ICE61缺最大升级版本 | warning未抹去；真实升级/卸载需隔离环境另验 |
| [第一次打包失败](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/installer/bundle.result.json>)及bundle-marker-diff.json | 原失败日志及临时marker状态保留；成功retry的staging exe已恢复为接受hash，build-root和tested copy未变 | 不覆盖历史尝试，不使用msiexec覆盖用户已安装应用 |

MSI带有完成页启动应用及缺少WebView2时下载bootstrapper等custom actions，本轮没有执行。Windows Sandbox不可用；实际安装、卸载、升级、安装后运行、签名校验、release/updater/autostart行为均**未验收**。静态代码字节一致性不能替代这些系统级步骤。

## 6. 当前源码归档

[XY-Switch-build004-source-20261016.zip](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/source-delivery/XY-Switch-build004-source-20261016.zip>)：**5,938,894字节**，SHA-256 `569b6b328b0a7be58c97899f27fd606377bed76f4a59f322d34b658168f1640d`。[source-archive-manifest.json](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/source-delivery/source-archive-manifest.json>)记录224项逐文件大小、SHA-256与CRC：**219/219 accepted inputs+5项extras**（.gitignore、LICENSE、README.md、CHANGELOG.md、SECURITY.md），所有内容与主树一致，ZIP CRC通过，无重复项。

这份源码包不包含本篇、主交接文档、复核JSON、exe/MSI、日志、真实配置、.git、node_modules或构建缓存；文档、证据与二进制独立随附，避免archive/JSON循环hash。entry时间归一化至00:26:08只是可复现归档元数据，不是本轮执行时间。旧ZIP、副本、MAIN_BEFORE.zip与修复前备份保留，不冒充当前源码包。

## 7. 未完成项与验收判据

| 项目 | 结论 | 完成判据 |
| --- | --- | --- |
| 真实供应商/官方直连指纹 | 未验收 | 用明确授权的真实渠道和独立同语料观察，分协议记录缓存、模板、排除项；模型自报与回归不能认证身份 |
| 实际安装卸载与升级 | 未验收 | 在隔离Windows环境验证上述4项WiX warning的影响、注册信息、WebView2前置条件、安装后启动与干净卸载/升级 |
| 签名release/updater/autostart | 未验收 | 独立release构建、签名、发布产物与实际系统行为取证；不能用debug MSI替代 |
| Session/Skills完整业务、主动探针、高并发分层、实时通知等 | 未交付完整实现 | 另行定义实现与验收，当前只覆盖相应拒绝/空态，不并入已通过业务项 |

上述边界不阻止本地build004的审计、业务流程、源码归档及debug MSI静态交接完成；也不能把“本地通过”写成“全部发布验收通过”。

## 8. 文档验证、移交和缓存维护

[文档校验回归结果](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/documentation-tests.result.json>)记录 **67项通过、exit=0**，20:26:03–20:26:18：保留原33项并新增34项，覆盖错误版本/输入、异常/未完成结果、命令失败、资源及PID归属清理、正文持久化边界、MSI三字节差异与告警、归档完整性、绝对Windows文件链接以及check-only不改存量文件。它们是文档工具测试，不计入应用Rust198或前端94。

交付入口：[主交接文档](<E:/XY switch/docs/model-audit-integration.md>)、[schema7复核JSON](<E:/XY switch/docs/model-audit-handoff-verification-20261014.json>)、[完整本轮原始证据](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016>)及其before/previous原字节备份。refresh_snapshot.py汇总存量raw/native/package/archive文件；verify_handoff.py验证正文、目录、UTF-8、围栏、PowerShell静态语法与live evidence；--check-only复算但不改保存元数据。命令围栏只作语法解析，不执行应用；运行器不允许覆盖既有同名尝试。详细复验入口见主文档第6.3–6.4节。

用户追加的空间清理与验收分开记账：[批准后状态汇总](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/approved-execution-20261016/maintenance-status.json>)与[实际删除结果（completed=false）](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/cleanup-result.json>)记录 **2026-10-16 21:53:42–21:55:29已批准并部分执行**，不是全量成功。原35项目标、29,486文件、38,667,910,535逻辑字节中，已删除24项目标、17,130文件、21,522,055,050逻辑字节；E:空闲空间由185,645,019,136增至201,744,273,408字节，实际观测增量 **16,099,254,272字节（14.994 GiB）**。逻辑量可能重复硬链接，磁盘增量可能包含同期其他活动，二者不互相替代。清理前后5,915个受保护文件SHA-256及字节数一致，其中release5,038项，protectedFilesChanged为空。剩余11项目标、12,356文件、17,145,855,485逻辑字节（15.968 GiB）全部在src-tauri/target/debug，沙盒拒绝访问；两次沙盒外审核超时且未启动进程，见[沙盒外审核超时记录](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/approved-execution-20261016/resume-permission-review-timeouts.json>)。此前四次批准前超时及0字节释放结论仅保留为历史记录，见[批准前四次未执行状态](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/continuation-20261016/cleanup-status.json>)；[会话读取来源](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/continuation-20261016/session-read-summary.json>)和[本次正文及元数据修改前原字节备份](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/approved-execution-20261016/before-documentation-manifest.json>)保留来源与改前状态。原执行completed=false和失败记录不改写成成功。

清理范围仅为原批准的可重建调试缓存；源码、release、依赖、全部验收日志/截图/SQLite夹具、归档及hash绑定exe/MSI保留，不修改ACL/所有权。原脚本首次因JSON时间字段读取转换在删除前误报，失败原文和仅保留ISO字符串的修正均另存。[部分清理后只读复核](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/documentation-check-only-post-cleanup-partial-approved.result.json>)记录 **exit0、869个存量文件零变化**；它是部分清理后复核，不是剩余11项的成功验收。本次仅继续空间维护及文档/元数据收尾，不重跑应用或已绿的67项文档工具测试；本地32+108项通过与实际安装/真实渠道/签名发布未验收的口径不变。后续只可在权限审核可执行时继续[11项残余清单及续跑准备](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/approved-execution-20261016/resume-permission-retry>)，并为已明确修改的文档元数据更新预期保护哈希；不能直接重跑原计划或将当前维护completed写成true。
