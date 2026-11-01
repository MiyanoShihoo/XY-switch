# 工作台 UI 重构与验收记录

日期：2026-10-30；续办与清理复核：2026-11-01。开发分支：`codex/e2ee-sync-20261029`。初始实现提交：`33bc9a9`。

本轮已完成确定可再生缓存清理及桌面工作台重构，将供应商首页常驻右栏移到独立板块。12.78 GiB 的清理已完成并核验。产品修复 `815e16c` 对应的第十轮 Linux 隔离 renderer CI 已成功：六组共 69 个浏览器场景、515 项单测和 74 项验收预检全部通过。36 张中英文、1440/820/480、亮暗主题矩阵及四张新焦点截图、两张诊断辅助截图已完成审阅；68 次键盘焦点检查均通过。没有发布新版本，旧 v1.2.0 安装包不包含本轮 UI。

## 使用新的工作台

| 原右栏功能 | 新位置 | 操作与状态约束 |
| --- | --- | --- |
| 端点、协议、URL 模式、客户端分组 | 供应商 → 配置详情 | 查看详情不切换当前供应商 |
| 默认模型、模型映射、请求参数 | 配置详情 → 模型与请求参数 | 映射表可直接查看；覆盖值不暴露，只显示数量；编辑仍使用现有高级编辑器 |
| 编辑、复制、删除 | 配置详情 → 配置管理 | 复制不携带密钥且副本默认禁用；删除需确认；内置供应商不可删除 |
| 最近连接状态、延迟、成功率、错误 | 连接诊断 → 连接测试 | 没有测试记录显示“尚未测试 / 无记录”，不把启用等同于测试通过 |
| 单供应商与全量连接测试 | 连接诊断 | 仅明确点击才发起测试；禁用对象不能发起测试 |
| 模型响应、低 Token 验证与证据 | 连接诊断 → 模型响应与验证 | 保留原有隐私、费用和预算门控；模块仅在该标签页选中时挂载 |

首页改为全宽供应商列表，保留显式切换、筛选、搜索、拖动与键盘排序。导航分为工作区、观测、管理三组，共九项；详情属于供应商板块，不新增一个常驻分栏。

地址可复制或刷新：

- `#providers`：供应商首页；无 hash 的地址也是首页。
- `#providers/{encodedId}/configuration`：指定供应商的配置详情。
- `#diagnostics`：查看当前供应商或列表中的首个供应商。
- `#diagnostics/{encodedId}`：查看指定诊断对象，不修改当前供应商。

浏览器后退、前进保留这些路由。供应商已删除时显示明确空状态；未知或损坏的外部 fragment 不改变当前工作区。“跳到内容”只移动焦点，不覆盖应用 hash。

## 清理范围与设计来源

2026-10-30 的早期浏览器缓存清理回执位于 `artifacts/ui-reconstruction-20261030/cleanup-receipt.json`：删除 34 个可再生浏览器缓存目录、478 个文件，释放 238,143,156 bytes（227.11 MiB）；保留的 2,471 个 profile 文件清理前后 SHA-256 一致。

2026-11-01 经用户确认后，按 `E:\XY switch\artifacts\storage-inventory-20261101` 的清单删除 22 个可再生目录、116,113 个文件，共 13,725,380,740 bytes（12.78 GiB）。E 盘实际空闲增加 10,508,800,000 bytes（9.79 GiB）；回执记录的 2,074 个受保护文件 SHA-256 和三个仓库 Git 状态均保持一致。清理结束时工作区为 7.40 GiB，随后只恢复当前开发仓库依赖与 renderer 构建输出。清理没有重复执行，删除的其他缓存没有批量恢复。

两个临时目录已获准在沙箱外只读检查，仍保留：`artifacts/model-audit-20261009/logs/design-fix-20261010/temp/shadow-170slmex` 与 `artifacts/model-audit-20261009/logs/adaptive-sampling-20261011/design-temp/shadow-o5gltf23`。各含 `synth.tiktoken`、`v0.tiktoken`、`v1.tiktoken`、`v2.tiktoken`，每处 5,577,888 bytes（5.32 MiB），由 `verify_proxy_design.input.py` 的 `write_tiktoken` 生成。它们不在此次批准的 22 项删除清单中。

实际调用了 UI/UX Pro Max 的设计系统、开发者工具、导航 UX 与 React 状态/焦点搜索。保留生成的 [MASTER](../design-system/xy-switch/MASTER.md) 和 [工作台 override](../design-system/xy-switch/pages/workbench.md)。生成器给出的 FAQ / documentation landing pattern 不适合桌面工作台，已明确舍弃；采用 Minimalism / Swiss、Slate 与 teal、亮暗配套的语义 tokens、本地系统字体和低动效。

桌面导航宽 220px，窄桌面改为 72px 图标栏，紧凑视口使用横向导航。主体只有一个主要滚动区域；页面标题、卡片、操作条和焦点样式统一。没有引入远程字体、图片请求或装饰性动效依赖。

## 验证边界与证据

| 验证 | 当前结果 | 证据 |
| --- | --- | --- |
| TypeScript lint | 最终源提交通过 | `artifacts/ui-reconstruction-20261030/ci-41310544444/logs/renderer-ui/6_Run pnpm lint.txt` |
| 全量单元测试 | 最终源提交 45 文件、515 项通过 | `artifacts/ui-reconstruction-20261030/ci-41310544444/logs/renderer-ui/7_Run pnpm test.txt` |
| renderer 构建 | 最终源提交通过 | `artifacts/ui-reconstruction-20261030/ci-41310544444/logs/renderer-ui/9_Run pnpm buildrenderer.txt` |
| renderer 验收预检 | 74 项通过（19 项来源、13 项就绪、6 组静态表达式、21 项文档隔离、15 项异步布尔谓词） | `artifacts/ui-reconstruction-20261030/ci-41310544444/logs/renderer-ui/8_Run pnpm testuinetwork.txt` |
| 本机隔离浏览器 | 未通过，不计为验收成功 | `artifacts/ui-reconstruction-20261030/browser-local-navigation/headless-acceptance.json` |
| Linux renderer 浏览器 | 第十轮六组、69/69 场景通过；浏览器与服务器正常关闭 | `artifacts/ui-reconstruction-20261030/ci-41310544444/ui/headless-acceptance.json`、各组 `results.json`、`renderer-ci-verification.json` |
| 完整截图与键盘焦点 | 36 张矩阵、4 张焦点、2 张辅助截图审阅通过；116 次完整可见性测量、68 次键盘焦点检查通过 | `artifacts/ui-reconstruction-20261030/screenshot-review-final-20261101.json`、`ci-41310544444/ui/navigation/acceptance.json` |
| 语义 token 对比度 | 24 组通过，最低 4.505:1 | `artifacts/ui-reconstruction-20261030/ci-41310544444/ui/navigation/acceptance.json` |
| 副本禁用回归 | 14 项相关测试、lint 与 renderer 构建通过 | `artifacts/ui-reconstruction-20261030/duplicate-provider-tests.log`、`duplicate-provider-lint.log`、`duplicate-provider-renderer-build.log` |
| 打包原生应用 | 本轮未运行 | 本轮只验收 renderer；先前原生同步验收不替代本轮视觉验收 |

本机浏览器被 Kaspersky 注入的跨域请求污染，精确来源断言拒绝 `http://gc.kis.v2.scr.kaspersky-labs.com`。没有关闭安全软件，也没有为它放宽网络断言。隔离浏览器与本地服务器已关闭。随后以同一开发分支提交运行一次性 Linux CI，renderer-only 模式跳过原生打包、安装与发布任务。

首轮 Linux run `41050737008` 在读取 `/json/version` 时触发 5 秒超时，六组界面场景均未开始，不计为通过。提交 `251cc65` 将 CDP 启动改为 60 秒总时限内的有界重试，校验 endpoint 与独立 profile 精确匹配，并记录失败阶段、探测次数与堆栈；不增加任何来源白名单。初次 CI 的产物 SHA-256 已与 GitHub 广告的 digest 核对一致，保存于 `artifacts/ui-reconstruction-20261030/ci-41050737008/`。

第二轮 run `41053448214` 的 CDP 启动通过，导航第一项的旧断言比较 `page.clientWidth` 与列表 `clientWidth`，没有扣掉有意保留的页面左右 padding。失败截图确认列表已填满内容区，右栏已移除。提交 `5af7277` 改为断言列表左右边缘与页面 content box 精确对齐，同时把该检查扩展到全部首页截图，记录测量值；也修正键盘排序表达式转义，并为六组脚本补充浏览器表达式语法预检。

第三轮 run `41055358848` 的首页信息架构和历史／焦点场景通过，配置管理暴露了实际实现遗漏：复制供应商继承了原对象的 `enabled: true`。提交 `095efb3` 显式将副本设为禁用，并用回归断言确认首个及连续副本均禁用、原对象仍启用；继续保留唯一 ID、独立高级参数、不复制 API Key 和不激活副本的行为。14 项相关测试、类型检查和 renderer 构建通过。前三轮日志、元数据和 renderer 产物均保存在各自 `ci-<runId>` 目录，ZIP SHA-256 与 GitHub artifact digest 已核对。

renderer 构建仍有既有的 Tauri core 动静态混用和大 chunk 警告；构建成功，但本轮未声称已经优化应用启动体积。

## 第四轮失败与续接修复

第四轮的首页信息架构、历史／焦点、配置管理通过；诊断标签场景报告了一次 `set_current_provider` 调用。失败快照中仍有上一场景创建的第四个供应商、Relay 当前状态及显式切换提示。前一场景本来就要求手动切换并断言恰好一次调用；下一场景的 `reset()` 仅改变 URL fragment，旧 CDP helper 没有要求新文档，导致调用历史和配置状态跨场景残留。这一轮失败不能作为诊断页自动切换的证明。

| 证据 | 发现 | 修复路径 |
| --- | --- | --- |
| 第四轮 `navigation/verification.log` 与 `diagnostics-tabs-and-default-off-failure.json` | 显式切换后的旧 fixture 进入下一场景 | `scripts/workspace-qa/document-navigation.mjs` 保证新主文档 loader；同文档导航显式 reload |
| `scripts/workspace-qa/document-navigation.test.mjs` 的 17 项单测 | 防止旧 DOM complete 提前放行，等待精确目标 URL；超时和非导航异常仍失败 | 共享 `cdp.mjs` 使用有界新文档就绪检查；不扩大网络白名单 |
| `verify-workspace-navigation-ui.mjs` 的 reset 基线断言 | 每一场景须恢复 3 个供应商、Orion 当前配置，且无隐式切换／测试调用 | 保留全部原自动请求断言；不手动清空调用记录掩盖错误 |

以上修复为提交 `da1ed37880690c8081fc2575325e8efaf28f0265`。本次只修改验收代码及其预检入口，没有修改产品 TypeScript 源码，因此没有重复运行此前已经通过的应用 lint、全量单测或 renderer 构建。共享来源策略、启动就绪、静态表达式和新文档隔离共 55 项在本地通过；仍需真实隔离浏览器验证新 fixture 的完整行为。

此前两次推送审批超时的命令均未执行；用户再次批准后，提交 `da1ed37` 与文档提交 `97b4b20` 已成功推送，并调度第五轮 renderer-only CI。第五轮下载的 artifact SHA-256 与 GitHub digest 一致；完整日志下载中断后进行了有界重试，复用已核对的 artifact，没有重复下载安装包或运行原生应用。

## 第五轮失败与共享谓词修复

第五轮的首页信息架构、历史／焦点、配置管理、诊断标签默认关闭、诊断连接显式测试均通过。键盘排序场景在按 Enter 后的就绪等待报告 `Object reference chain is too long`；失败快照显示拖动手柄已具有 `aria-pressed="true"`，没有 renderer exception。等待表达式返回 `document.querySelector(...)` 的 DOM 节点，原共享 helper 使用 `Runtime.evaluate(returnByValue: true)`，把 React 所属节点的循环对象关系送入了 CDP 序列化边界。这是就绪检查错误，不能据此判断键盘排序实现失败。

提交 `82cfd508ffca78f60cfbf2f6ef9eac5e9fe3ea9e` 新增 `scripts/workspace-qa/runtime-predicate.mjs`，在浏览器内等待谓词结果后转换为布尔值，避免跨 CDP 传递 DOM 对象。15 项新增回归覆盖循环节点、缺失节点轮询、falsey 值、异步 false／节点、Promise 拒绝、CDP 错误及有界超时；六组静态表达式预检也按实际谓词包装后编译。来源策略、文档隔离、自动请求断言保持不变；没有手动清空 fixture 调用历史，没有修改产品 TypeScript 源码。

本地 `pnpm run test:ui:network` 共 70 项通过，日志 SHA-256 为 `d78bf7678a5c3e2c9fb848720f8787254123f1f3b3b2c41d5888b49f0e2283b9`。第五轮 CI 自身的 lint、45 文件／515 项单测、55 项当时预检及 renderer 构建也通过；本地没有重复运行不受本修复影响的应用检查。

第六轮 run `41066338656` 使用 `82cfd508ffca78f60cfbf2f6ef9eac5e9fe3ea9e`，以 `renderer_acceptance_only=true`、`native_acceptance_only=false` 调度。调度后曾有两次只读查询被自动审批服务的 Azure 资源路由错误拦截，操作没有执行。2026-11-01 已取得该轮终态 failure 与摘要匹配的产物：前三个导航场景通过，在 `diagnostics-tabs-and-default-off` 等待 fragment 对应的新文档时超时。这段是历史失败记录，不代表最终验收状态。

已实际查看第五轮已通过场景的 1440px 中文亮色供应商首页和诊断模型默认关闭截图：首页为单一全宽列表，没有常驻右侧 inspector；诊断页为独立页面，标签焦点可见，模型验证开关关闭。这两张局部证据不替代完整 1440／820／480 × 中英 × 亮暗截图矩阵、对比度及六组回归验收。

第五轮 artifact SHA-256：`956ed9a80a139ca728caa145ce27ce37ca3264078d75d9878d6a64e5f68c75b1`。完整 CI 日志 ZIP SHA-256：`fdeec2af62dc8ea2118afb8f9dd874eab987e14c0f2b9aea164e756d36caefb4`。详细状态、场景结果和后续待办保存在 `artifacts/ui-reconstruction-20261030/continuation-receipt.json`。

## 第六至十轮续办与焦点修复

| 轮次 / 源提交 | 权威终态与实际覆盖 | 修复与证据 |
| --- | --- | --- |
| 第六轮 `41066338656` / `82cfd50` | 导航 3 项通过，fragment 就绪等待失败 | `b076b8b` 等待 URL 提交后再刷新 fixture；新增 4 项回归，预检由 70 增至 74 项 |
| 第七轮 `41307701517` / `b076b8b` | 导航 7 项全部通过；工作台在消息读取重试场景失败 | `7addf0f` 将重试按钮限定到消息或目录对应错误面板，并断言相应 IPC 恰好重试一次 |
| 第八轮 `41308754538` / `7addf0f` | 导航 7 项、工作台 15 项、多语言组通过；会话 7/8 项通过 | 失败快照已显示英文 `No favorite sessions yet`；`localStorage.clear()` 同时移除语言偏好，界面正确回到系统语言。`561ce1c` 按实际系统语言等待空状态，并继续检查存储键为空与收藏过滤保持启用 |
| 第九轮 `41310163370` / `561ce1c` | 原导航 7 项及 36 张矩阵通过；新增焦点场景失败 | 480px 首个导航项的键盘焦点边界为 left=-41，轮廓被裁切。`815e16c` 在导航接收焦点时显式滚入可见区域；保留完整按钮和 4px 焦点轮廓的几何断言 |
| 第十轮 `41310544444` / `815e16c` | 导航 8/8、工作台 15/15、多语言 16/16、会话 8/8、治理 11/11、同步 11/11，全部通过 | 真实浏览器证实每个导航焦点完整可见；跨窗口清空按系统语言正确显示空状态；来源策略、供应商操作门控与六组回归断言全部保留 |

第七、八轮的 36 张导航矩阵 SHA-256 逐项相同，已逐张实际查看；第九轮四张变化的 480px 诊断图也已实际查看。最终第十轮 36 张矩阵全部与这些已审阅图片逐项 SHA-256 一致，其中 32 张复用第七轮、4 张复用第九轮审阅；另实际查看第十轮四张新焦点截图。模型默认关闭、连接失败两张辅助截图与已查看的第八轮图片摘要一致。最终真实键盘验收覆盖 480px × 中英 × 亮暗下九项 Tab、八项 Shift+Tab，以及 480→820→480；四种语言/主题组合各 17 次焦点检查全部通过。

| 轮次 | artifact ZIP SHA-256 | logs ZIP SHA-256 |
| --- | --- | --- |
| 六 | `66a6b78c1f5b481b3af092564c7bfc55cbe4ddfaf43ace9b95dce2d1b506309e` | `107ec4359abced4ed66a5d93895c431e175de6322df93b4b6f22d345588217a5` |
| 七 | `8f0405770d2cf1e7cb4a1168cc42324611d8b0d114061c052ba23dbc8b44f592` | `8d3306c24eb5e86a4e010c6173d4b94e203f666ffb7cef4e05a63e83fe9c7d83` |
| 八 | `fa688dbf32acac58e6294c37618a0f2302dfad89b6dfd2c033294b54ebb4f42f` | `5e8ba00a80318138703e5f0f24e8625255cd6a887db944c499dbc1e0cb7a100f` |
| 九 | `a8c41a415354dd9a2fab446a6359ff8a467f80bbc5a6530ae1fbda1706e65025` | `218717cb5c5a13ad17ebeb7d840743c870c632256829ed3540cc73c60d0f585a` |
| 十 | `627a0045d28adc36b7578592455772570f1af1ae22f4bb0bb3acbfbf3ce9c16c` | `37f21f539f22c11e7574056b1dc7b8d55c3d0e513aed4683cc6e79b510df4d5c` |

各 artifact 已校验来源提交、分支、workflow_dispatch、GitHub 广告 digest 与本地 SHA-256；全部非 renderer job 均 skipped。完整状态、日志、截图位于各 `artifacts/ui-reconstruction-20261030/ci-{runId}` 子目录。

## 最终验收与提交

[第十轮 CI](https://github.com/MiyanoShihoo/XY-switch/actions/runs/41310544444)，attempt 1，源提交 `815e16cf01fe2074876b59e1f428d827569cff22`，权威终态为 `success`。GitHub artifact `13084784410` 的广告 digest 与下载 ZIP SHA-256 一致。六组使用独立 Chromium profile、合成 IPC fixture 和 Noto Sans CJK SC 字体，未执行打包原生应用或真实供应商请求；所有非 renderer job 均 skipped。

最终报告记录的来源仅为 `http://127.0.0.1:5173`，异常数为 0；HTTP 和匹配的 WebSocket 来源检查保持精确，浏览器与服务器均确认关闭。导航的 116 条几何测量没有裁切，68 条键盘焦点记录均为 solid 样式、4px 完整轮廓。矩阵审阅依靠逐张实际查看及严格摘要复用，审阅记录明确标注每张图片的来源、SHA-256 和直接查看或复用方式。

最终核验清单位于 `artifacts/ui-reconstruction-20261030/final-acceptance-audit-20261101.json`；逐张审阅位于 `screenshot-review-final-20261101.json`；本地续办与推送回执位于 `continuation-receipt.json`。这些本地证据不随整个 artifact 目录强制入库，可审计的结论及 ZIP 摘要已写入本记录。

浏览器验收对应上述产品源提交；收尾文档在其后独立提交，产品源码、验收脚本、锁文件与 CI 配置保持相同。最终文档提交和远端 HEAD 的一致性写入本地回执，不把文档提交描述为重新运行过浏览器验收的源提交。开发与推送仍仅限 `codex/e2ee-sync-20261029`，外层旧仓库及暂存文件保留，未修改 `main` 或发布 Release。

## 重现 renderer 验收

在仓库根目录运行；依赖使用既有锁文件，不运行 `pnpm dev` 或 `pnpm build`，这两条会进入 Tauri。

```powershell
pnpm lint
pnpm test
pnpm test:ui:network
pnpm build:renderer
pnpm test:ui:headless
```

headless 入口需要现有 Chromium / Edge / Chrome；它不会安装浏览器。可用 `CHROME_BIN` 指定绝对路径。Linux 截图要求 Noto CJK 字体。入口自动启动只绑定 `127.0.0.1` 的 Vite renderer、使用独立 profile、顺序运行 navigation / workspace / i18n / sessions / governance / sync 六组脚本，并在结束时关闭浏览器与服务器。`QA_OUTPUT` 只能指向本仓库 `artifacts` 下的子目录。

新增导航脚本覆盖首页信息架构、路由与焦点、配置保护、诊断标签键盘行为、显式连接测试、键盘排序，以及 1440 / 820 / 480 × 中英 × 亮暗 × 首页 / 详情 / 诊断截图矩阵、语义 token 对比度、reduced-motion 和紧凑导航键盘焦点。上述八个导航场景已在第十轮真实浏览器中全部通过。

共享 CDP 会记录 HTTP 和 WebSocket 请求，并拒绝任何非精确本地来源。另一个 localhost 域名、另一个端口或不匹配的 WebSocket 协议都不是白名单；所有供应商动作使用合成 IPC fixture，不调用真实供应商或付费接口。

GitHub Actions 的 `Build and verify` 支持 `renderer_acceptance_only=true`、`native_acceptance_only=false`，在该模式只运行 `renderer-ui`。Windows、macOS、Linux 的产品支持范围不变，macOS Universal 保留；没有重新加入 iOS。
