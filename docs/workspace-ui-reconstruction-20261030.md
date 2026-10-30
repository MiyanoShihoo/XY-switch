# 工作台 UI 重构与验收记录

日期：2026-10-30。开发分支：`codex/e2ee-sync-20261029`。实现提交：`33bc9a9`。

本轮目标是清除确定可再生的缓存，调用 UI/UX skill 重构桌面工作台，将供应商首页常驻右栏移到独立板块。核心实现及基础验证已完成；第四轮 Linux renderer run `41057246974` 已失败，不计为浏览器验收成功。本次续接定位该轮失败为 fragment 导航未重建文档、沿用了上一场景的合成状态；修复提交 `da1ed37` 及 55 项预检已在本地完成。推送自动审批连续两次超时，命令未执行，新修复尚未送交 CI。六组界面验收、成功截图审阅和最终推送仍待完成。没有发布新版本，旧 v1.2.0 安装包不包含本轮 UI。

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

清理回执位于 `artifacts/ui-reconstruction-20261030/cleanup-receipt.json`：删除 34 个可再生浏览器缓存目录、478 个文件，释放 238,143,156 bytes（227.11 MiB）。保留的 2,471 个 profile 文件清理前后 SHA-256 一致。没有删除源码、发布包、历史、凭据、截图、日志或恢复资料，也没有重复执行清理脚本。

实际调用了 UI/UX Pro Max 的设计系统、开发者工具、导航 UX 与 React 状态/焦点搜索。保留生成的 [MASTER](../design-system/xy-switch/MASTER.md) 和 [工作台 override](../design-system/xy-switch/pages/workbench.md)。生成器给出的 FAQ / documentation landing pattern 不适合桌面工作台，已明确舍弃；采用 Minimalism / Swiss、Slate 与 teal、亮暗配套的语义 tokens、本地系统字体和低动效。

桌面导航宽 220px，窄桌面改为 72px 图标栏，紧凑视口使用横向导航。主体只有一个主要滚动区域；页面标题、卡片、操作条和焦点样式统一。没有引入远程字体、图片请求或装饰性动效依赖。

## 验证边界与证据

| 验证 | 当前结果 | 证据 |
| --- | --- | --- |
| TypeScript lint | 通过 | `artifacts/ui-reconstruction-20261030/lint.log` |
| 全量单元测试 | 45 文件、515 项通过 | `artifacts/ui-reconstruction-20261030/unit-tests-final.log` |
| renderer 构建 | 通过 | `artifacts/ui-reconstruction-20261030/renderer-build.log` |
| renderer 验收预检 | 55 项通过（19 项来源、13 项就绪、6 组静态表达式、17 项文档隔离） | `artifacts/ui-reconstruction-20261030/document-isolation-tests.log` |
| 本机隔离浏览器 | 未通过，不计为验收成功 | `artifacts/ui-reconstruction-20261030/browser-local-navigation/headless-acceptance.json` |
| Linux renderer 浏览器 | 第四轮失败；本地文档隔离修复等待推送和重新调度，未通过完整验收 | `artifacts/ui-reconstruction-20261030/ci-41057246974/ui/headless-acceptance.json`、`navigation/verification.log` |
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

本次推送 helper 的两次审批检查均超时，进程没有创建，没有推送或调度新的 run。剩余工作是：批准并推送本地提交；执行 renderer-only Linux CI；下载并核对本轮 artifact；实际检查成功的中英／亮暗／480px 截图、焦点和对比度；按真实结果更新本页与汇总回执。当前没有新增的成功截图或新的原生应用验收结果。

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

新增导航脚本覆盖首页信息架构、路由与焦点、配置保护、诊断标签键盘行为、显式连接测试、键盘排序，以及 1440 / 820 / 480 × 中英 × 亮暗 × 首页 / 详情 / 诊断截图矩阵、语义 token 对比度和 reduced-motion。场景已实现，不等同于浏览器验收已经通过。

共享 CDP 会记录 HTTP 和 WebSocket 请求，并拒绝任何非精确本地来源。另一个 localhost 域名、另一个端口或不匹配的 WebSocket 协议都不是白名单；所有供应商动作使用合成 IPC fixture，不调用真实供应商或付费接口。

GitHub Actions 的 `Build and verify` 支持 `renderer_acceptance_only=true`、`native_acceptance_only=false`，在该模式只运行 `renderer-ui`。Windows、macOS、Linux 的产品支持范围不变，macOS Universal 保留；没有重新加入 iOS。
