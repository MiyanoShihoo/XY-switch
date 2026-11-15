# XY Switch 产品界面与验证记录

最终复核日期：2026-11-06（Asia/Shanghai）。范围：Windows、macOS、Linux 共用的 React renderer；保留本文件名以维持现有文档链接。

## 设计依据

本轮以完成任务所需的信息和操作顺序为依据。每个主页面及插件管理、设置、安全与验收子页的任务、频率假设、信息优先级、两种结构方案、效率取舍和主 Agent 选择，记录在 [产品设计决策](ui-product-decisions-20261106.md)。没有使用频率遥测；高、中、低均为工作假设。

实现保留当前数据格式、调用契约、凭据边界和危险操作确认。已有结构能支持任务且没有截图或交互证据表明存在障碍的页面予以保留。

## 已落实的任务改进

| 场景 | 使用障碍与选择 | 当前实现与验收依据 |
| --- | --- | --- |
| 找到工作区 | 总览的重复导航按钮增加一次无意义点击；14 个功能需要可发现的分组 | 68px 活动栏切换五组，48px 顶栏呈现组内页面；总览只显示当前位置。760px 及以下由左侧抽屉访问全部目的地，保留当前组的横向入口、历史和焦点。 |
| 编辑供应商 | 常驻保存说明挤压技术字段，重复信息使简单修改路径变长 | 一个紧凑表单承载关联字段，高级选项按需展开，客户端写入提醒与保存动作邻近。稳定 ID、留空保留密钥、清除确认保留；长名称、端点及高级字段在 320/390px 检查。 |
| 浏览市场 | 两个简单筛选占用常驻侧栏；窄屏纵向堆叠使首屏看不到插件 | 桌面搜索、类型、客户端及提交同行；窄屏搜索直接可用，类型/客户端展开选择，收起后显示已应用条件。来源按需展开，列表占据完整内容宽度。390px 中文首个插件名称位于 y=565.56，名称、用途和兼容客户端同时进入首屏。 |
| 详情与安装 | 浏览上下文需要保留，授权决定必须绑定精确版本 | 目录打开详情，再进入明确安装预览；签名、完整权限、权限变化、目标客户端及复核确认保留。先选客户端但未复核、撤回复核勾选时，确认继续禁用。 |
| 本地插件授权审查 | 指纹、文件信息先于实际授权变化，新增权限默认折叠 | 插件/版本摘要后立即选择客户端，非空权限变化默认展开，完整权限继续可读，签名和包元数据可展开查阅。错误摘要、未选客户端时确认禁用；摘要与复核齐备才启用。 |
| 比较用量 | 汇总和图表纵向占据首屏，比较表出现太晚；密集序列存在溢出及未知费用误导风险 | 汇总与趋势并排、供应商/模型比较使用全宽。168 个时间点在 1440/390/320px 均无图表横向溢出。未知费用不生成虚构的正数刻度；全部已知零值只保留零基线，原始值入口保留。 |
| 阅读会话 | 阅读状态仍展示目录说明与刷新，重复头部和滚动区域挤占正文 | 窄屏阅读隐藏目录页头；返回、标题、收藏和恢复命令紧凑排列，来源/管理按需展开。390×844 首条消息顶部为 289px，正文与展开来源共用主滚动区；放大阅读使用弹窗正文滚动。关闭放大回到原触发按钮，返回目录回到原会话行。 |
| 其他工作区 | 每种内容有不同的比较与操作需求 | 客户端目录/详情、账号列表、MCP、提示词、Skills、设置和治理子页均完成任务与 A/B 取舍记录；保留按需打开的管理、证据和确认流程。 |

## 共享排版与密度

深浅主题共用相同组件、内容层级、间距、尺寸和状态语义，仅通过语义颜色变量切换。当前约定见 [工作区实现约定](../design-system/xy-switch/pages/workbench.md)，实际值以源文件为准。

- 正文字号 13px、行高 1.55；页面标题 token 为 22px。技术字段使用等宽字体，金额、数量和表格采用对齐的数字；长标识符可折行或通过完整详情读取。
- UI 字体使用 Segoe UI Variable、Segoe UI、PingFang SC、Microsoft YaHei UI 等系统字体；代码字体使用 Cascadia Code、SFMono-Regular、Consolas。
- 标准/紧凑控件为 32/28px；600px 及以下或粗指针环境为 40/36px。相关短字段可同行，复杂项通过已有展开区组织。
- 弹窗保留固定标题、确认区和滚动正文；键盘焦点、页签、返回焦点及危险操作确认继续属于验收内容。

## 实际内容与状态检查

证据位于 [2026-11-06 截图目录](../artifacts/ui-redesign-20261106/current)。截图来自正在运行的 renderer，使用具有业务含义的合成内容和隔离 IPC；它们不是设计稿，也不读取用户私有会话、凭据或账单。

| 内容与状态 | 检查范围 | 当前证据 |
| --- | --- | --- |
| 少量目录 | 三类插件，名称、用途、兼容客户端、版本，打开详情/返回 | marketplace-product-final-390-zh-light.png；marketplace-product-final-desktop-en-dark.png |
| 大量目录与长条件 | 60 项样例、每页 20 项、翻页边界、类型/客户端、长英文条件 | marketplace-product-many-final-320-en-dark.png；marketplace-product-filtered-final-320-en-dark.png |
| 搜索无结果 / 空市场 | 区分没有匹配项与没有公开发布项 | marketplace-product-no-results-final-320-en-dark.png；marketplace-product-empty-final-320-en-dark.png |
| 供应商长字段 | 中英文、已保存 ID、空密钥保持语义、长端点/名称及高级项 | provider-editor-product-long-desktop-zh-light.png；provider-editor-product-long-320-zh-light.png；provider-editor-current-advanced-long-390-en-dark.png |
| 用量少量/单点/空 | 保留范围和单位，单点显示精确值，空数据不生成统计 | usage-product-desktop.png；usage-product-single-390-zh-light.png；usage-product-empty-zh-light-desktop.png |
| 用量大量/未知/零 | 168 个小时、25 个长模型/供应商行；未知费用和已知零分别表达 | usage-product-many-final-zh-light-desktop.png；usage-product-many-final-390-zh-dark.png；usage-product-many-final-320-zh-dark.png；usage-product-unknown-final-zh-light-desktop.png；usage-product-unknown-final-390-en-dark.png；usage-product-zero-final-320-zh-dark.png |
| 插件权限 | 已授权范围、权限增加、精确摘要、全部权限、元数据访问、确认条件 | plugin-permission-product-final-390-zh-light.png；plugin-permission-product-final-320-en-dark.png |
| 会话阅读与长内容 | 少量/空目录、390px 明暗双语、1280px 双栏、390×360 长标题与正文、展开来源及删除确认 | session-reader-product-final2 的 reader-inline-* 截图和 *-geometry.json；最后返回入口修正后的中英明暗/宽度截图见 session-reader-product-final3 |
| 其他工作区 | 客户端、账号、MCP、提示词、Skills、设置及治理状态 | product-workspace-review-contact.png；product-operation-review-contact.png，以及各浏览器套件的分组截图 |

用量样例的趋势与汇总已核对；分组表中增加的长行是布局压力样例，不能作为可对账的真实账单。几何与交互检查保存在 [product-ui-checks.json](../artifacts/ui-redesign-20261106/current/product-ui-checks.json)。

## 独立截图审查

独立审美 Agent 实际查看了原页面、用量/供应商压力内容及跨工作区截图，要求只处理三项可定位障碍：窄屏市场首屏缺少插件内容、授权变化落在元数据之后、会话阅读头部及重复滚动占用正文空间。

市场和授权审查修正后，独立 Agent 再次查看 390px 中文浅色、320px 英文深色、桌面及长已应用筛选条件截图，确认两项原问题已消除。随后复看会话页 5 张终版截图与 2 张原图，确认首屏正文前移，阅读、管理及删除确认层级清楚，未发现新增视觉阻断。单一主滚动的结论由浏览器几何/交互断言证明；390×360 的首屏截图不能代表屏外长正文完整性。已有用量与供应商改进通过其前次复核。各次原始截图保留用于对照。

主 Agent 补查桌面终图时发现返回目录按钮被共享样式意外显示，已修正会话局部选择器。追加的 12 种语言、主题和宽度组合确认：目录同屏时隐藏返回入口，目录隐藏时显示返回入口；桌面关闭放大与窄屏返回分别恢复到正确目标。

## 验证记录与范围

下表将此前已完成的回归与本次最终修正的定向检查分开。旧失败记录保留；同名检查以时间较晚的成功复跑为准，不把同一组重复计数。

| 检查 | 已确认结果 | 证据/范围 |
| --- | --- | --- |
| 前端完整基线 | 54 文件、680 项通过 | 在最后产品细化之前完成；不表述为最终源代码的重新全量测试 |
| 用量、导航与语言定向检查 | 6 文件、81 项通过 | language、AppShell、workspace-navigation、usage-format、ModelPricingModal、usage-pricing-service |
| 本次市场与插件定向检查 | 2 文件、44 项通过 | plugins.test.tsx、marketplace.test.tsx；包含权限顺序、完整信息和原有服务安全契约 |
| 本次会话定向单测 | 6 文件、86 项通过 | sessions、session-service、session-filter、session-format、session-favorites、native-session-favorites；结果来自本轮实际运行记录，不将 Vitest 累积缓存作为本次完整日志 |
| 综合工作区 | 15 组通过 | workspace-final/results.json |
| 安全与验收 | 17 组通过 | governance-final2/results.json |
| 导航与焦点 | 8 个不同组通过 | navigation、navigation-complete、navigation-focus-final；最新 home/响应式两组由 navigation-product-final 覆盖 |
| 语言与主题 | 16 组通过、96 页状态记录 | i18n-product-final/results.json；包含语言持久化/回滚、编辑草稿、原始内容与窄屏能力 |
| 加密同步 | 12 组通过 | encrypted-sync-product-final/results.json；57 个对象跨 8 页、跨页决策、未决定项阻断、改动重置确认、期限/错误/回滚 |
| 会话回归基线 | 9 个不同组通过 | sessions-core 中 favorites-lifecycle，较晚的 sessions-final 中其余 8 组；早期焦点失败由后者成功结果覆盖 |
| 本次会话阅读回归 | 9 组通过 | session-reader-product-final2/results.json，2026-11-06 17:31:22（UTC+8）；包括正文查找、收藏、跨窗口、存储错误恢复、删除失败保留、关闭/返回焦点、中英明暗及窄屏长内容 |
| 返回入口最终修正 | 受影响的 2 组通过 | bilingual-responsive-layout 的最新成功记录见 session-reader-product-final3/results.json；filters-and-keyboard 的最新成功记录见 session-reader-focus-final/results.json。不重复计入 9 个不同组 |
| 最终 renderer 构建 | 通过，1923 模块 | current/renderer-build-final.log；TypeScript 与 Vite 成功。保留原有 Tauri 混合导入和大于 500kB 分包提示 |

最后的返回按钮修正后，旧键盘脚本仍假定桌面存在返回动作。已将该检查改为先验证桌面关闭放大的焦点，再缩窄窗口验证目录返回；定向复跑通过，早期失败记录保留在 session-reader-product-final3。当前产品检查记录共 16 项，主审浏览器记录的意外外部请求与页面错误均为 0。

### 边界

这些记录验证 renderer 信息组织、交互状态和合成 IPC 契约。它们不构成本机加密/文件事务、真实账号登录、上游费用、真实市场安装与权限、操作系统 WebView 或三平台安装包的验收结果。当前源码构建不替换已发布安装包。
