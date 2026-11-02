# XY Switch 安装包与插件业务验收

验收日期：2026-11-02；分支：`codex/e2ee-sync-20261029`；版本：1.2.0。

## 1. 结论与证据入口

六个安装包来自同一应用源码提交，Windows、Linux、macOS 的原生验收累计 **83/83 项通过**，六项 Codex 宿主插件分别完成代表性业务。安装、升级、卸载和配置恢复在 disposable GitHub-hosted runner 中执行；本轮转发上游为 synthetic loopback，新增收费 API 调用为 **0**。

本轮证明的环境与操作范围见第 3 节。生产签名、公证、macOS x86_64 执行、app.zip 单独执行和 Linux/macOS 原生加密导入导出列在下一阶段。

- [公开验收账本](evidence/native-plugin-acceptance-20261102/ledger.json)：源码、运行、八份下载归档、六包、83 项逐项结果、安装观察、插件业务和 QA 摘要。
- [构建来源原始记录](evidence/native-plugin-acceptance-20261102/source.json)、[构建运行回执](evidence/native-plugin-acceptance-20261102/runs/run-41419299553-status.json)、[原生运行回执](evidence/native-plugin-acceptance-20261102/runs/run-41452499188-status.json)。
- [Windows 安装器离线逆向报告](2026-11-02_reverse-xy-switch-installer-report.md)、[原始严格 case 审查](evidence/native-plugin-acceptance-20261102/reverse-case/report/case-review.json)。
- [证据目录说明](evidence/native-plugin-acceptance-20261102/README.md)：公开存档的用途、原始摘要及完整本地证据位置。

## 2. 同一源码、安装包与平台任务绑定

| 角色 | 提交 / 运行 | 结果 |
| --- | --- | --- |
| 应用生产源码及六包构建 | `fe4946bd2fcd4c27a7432aeadb5a656e0e6e676b`；[41419299553](https://github.com/MiyanoShihoo/XY-switch/actions/runs/41419299553) | website、frontend、renderer-ui、Windows desktop、Linux desktop、macos 六项必要构建任务成功 |
| 原生验收 harness | `0279977f314913d72e0b85a187ccd17217ae95ed`；[41452499188](https://github.com/MiyanoShihoo/XY-switch/actions/runs/41452499188) | native-source、Windows lifecycle、Linux lifecycle、macOS lifecycle、Windows sync 五项核心任务成功；website 成功 |
| 生产输入一致性 | source.json 的 `productionInputsUnchanged=true` | harness 与来源提交的 src、src-tauri、public、index.html、vite.config.ts、pnpm-lock.yaml 以及 package.json 版本/依赖字段一致 |

原生验收运行复用指定构建产物；该运行中的 frontend、renderer-ui、desktop、macos 构建任务为 skipped。两次运行的 GitHub 状态于北京时间 2026-11-02 23:24:36 和 23:24:39 重新读取，均为 completed/success。

三份 manifest 原样存档，均未包含 sourceCommit 字段。源码绑定由指定 run 的 head SHA、GitHub artifact ID/digest、下载 ZIP 校验及 [source.json](evidence/native-plugin-acceptance-20261102/source.json) 共同承担；source.json 原始 SHA256 为 `e497ecfee94b6bd8cb893332e2f06f3ff07ac4f58cb3ad6bf53cf61fba0f0773`。八个下载 ZIP 的字节数和 SHA256 均与对应 GitHub artifact 元数据一致。

| 平台 / 格式 | 文件 | 字节数 | SHA256 |
| --- | --- | ---: | --- |
| windows-x64 / nsis | XY-Switch-1.2.0-windows-x64-setup.exe | 5,275,657 | `6371977825a2d914b7d3d5124ba6d2fff6924e7db1406af39be27b23eac92869` |
| windows-x64 / msi | XY-Switch-1.2.0-windows-x64.msi | 7,139,328 | `7b6bbc30906af3eb88bb189d1e038dd25277a8cf2b18e9079cb3e621c042b7b5` |
| linux-x64 / deb | XY-Switch-1.2.0-linux-x64.deb | 6,989,396 | `5d0b68edee914e5c50ac3313fdbf30bde70e9698577363ca0f13331790a35b3e` |
| linux-x64 / appimage | XY-Switch-1.2.0-linux-x64.AppImage | 85,002,744 | `75bb16d3e9b222803e161a3e1452db4264920f8fd8719d1e1ebe16965f85bcbe` |
| macos-universal / app | XY-Switch-1.2.0-macos-universal.app.zip | 15,650,646 | `715bb8aa2c318a84602c4fd1623a0fa621293de7114f0a99f072f55c9880410d` |
| macos-universal / dmg | XY-Switch-1.2.0-macos-universal.dmg | 17,512,100 | `263a6da5c82755baa606ba076601b150acf428814a9beeace4be3c97363f7fc6` |

六包合计 **137,569,871 bytes / 131.196852684021 MiB**；每个文件均与本平台 [manifest](evidence/native-plugin-acceptance-20261102/manifests/windows-x64.json) 的条目核对。另两份清单为 [Linux](evidence/native-plugin-acceptance-20261102/manifests/linux-x64.json) 和 [macOS](evidence/native-plugin-acceptance-20261102/manifests/macos-universal.json)。业务输出和验收输入使用同一组包名、字节数与摘要。

六包的生产签名验证状态均为 `not-performed`。macOS 安装观察另外记录 ad-hoc 签名完整性 verified、Gatekeeper not-accepted；这些状态见账本的 installations。

## 3. 原生 GUI、接管、凭据与生命周期

升级基线为 **1.0.0 → 1.2.0**；旧版提交为 `1f0c8c197b9aab301bb3ac39683c1e5871e2045e`。以下用例使用隔离的应用目录、客户端配置和凭据夹具，操作对象为已安装/部署的真实应用窗口。

| 平台 | 实际环境 / 包 | 覆盖 |
| --- | --- | --- |
| Windows lifecycle | disposable Windows x64 runner；具体 Windows 版本未记录；NSIS、MSI | 两种格式各 6 组：旧版安装、升级保留配置、原生 GUI 接管及生产转发读取原生凭据、强制终止后的冷启动恢复、生产 IPC 配置导出/导入、卸载重装与配置恢复 |
| Linux lifecycle | Linux-6.8.0-1070-azure-x86_64-with-glibc2.35；deb、AppImage | 隔离凭据服务及两格式生命周期；真实 GUI 点击启动转发/接管 Codex、读取凭据、恢复客户端、停止监听、强制终止后的冷启动、卸载重装和离线 SQLite 备份恢复后 GUI 确认可见 |
| macOS lifecycle | macOS-14.8.9-arm64-arm-64bit-Mach-O；DMG | 隔离钥匙串、旧版安装/升级、GUI 接管/转发读取凭据、停止和恢复、强制终止后的冷启动、卸载重装、离线 SQLite 备份恢复后 GUI 确认可见 |
| Windows encrypted sync | disposable Windows x64 runner；额外生产 IPC / 原生文件选择器用例 | 加密导出与独立解密、目标端冲突及取消、错误口令无写入、导入预览与明确选择、备份、回滚预览及持久化恢复 |

| 原始结果文件（验收 artifact 内） | 通过数 | 原始 SHA256 |
| --- | ---: | --- |
| windows-lifecycle/lifecycle-results.json | 12/12 | `72f1612c3869f5e4ac5a5ac2817f678145e4ddc39c770533c09351586a15c460` |
| linux-lifecycle/lifecycle-results.json | 43/43 | `631fe79e63894c96d43e87ab52a73c5311aecfeeea66c5b75b401ee616183bc2` |
| macos-lifecycle/lifecycle-results.json | 22/22 | `a98a35a94ef14267d0ec399ba7abf77c73a69de8f3539635b4def262b9f86f70` |
| windows-sync/results.json | 6/6 | `fad69b9f9c21aaec62c0b86067c78c539056cb8e4eacb1fe3242bfe22aebb440` |

原始 Windows sync 使用 `reports` 数组，三个 lifecycle 文件使用 `results`。四份结果的 sourceRun 是十进制字符串；来源记录中是数字。审计验证十进制格式并以 BigInt 比较，四份结果均正确绑定 sourceRun、sourceCommit 与 harnessCommit。

Windows 用完整的已安装应用文件摘要核对每种格式：NSIS 为 `3ec3704e229ecca445295df7069b9ac7ac54f210e29bc6540575751bf20a8d86`，MSI 为 `34e68d76b2c7157e8b993bd6af073d8206e7dfa624b594a709d1a39ae8012c76`；对应各自包内 payload，逐项结果均记录 fullBinarySha256Matched=true。Linux/macOS 的旧装、升级及重装包摘要和已部署文件摘要保留在账本的 installations。

Linux/macOS 的 provider/prompt 由离线配置夹具准备，恢复使用离线 SQLite 备份，随后真实 GUI 确认配置可见。Windows 的配置恢复使用生产 IPC，额外六项用例覆盖原生加密导入导出流程。macOS Universal 已打包 x86_64 与 arm64，两架构中的实际执行仅覆盖 arm64；DMG 完成生命周期，app.zip 完成打包及摘要校验。

凭据读取由应用生产转发路径访问 OS 原生凭据存储，loopback 上游记录 authorizationMatched=true，公开结果只保留匹配标记、响应摘要及请求计数。

## 4. 六项插件的代表性业务

这里的插件为 **Codex 宿主插件技能及其本地运行时**；验收范围为它们生成业务成果、调用官方辅助工具并检查结果。XY Switch 内部插件协议需要独立验收。

| 插件 | 实际调用 / 业务 | 结果及证据 |
| --- | --- | --- |
| Documents | create_documents.py / python-docx；文档插件的 DOCX 渲染流程 | [两页安装恢复说明](evidence/native-plugin-acceptance-20261102/plugin-deliverables/XY-Switch-installation-and-recovery.docx)，一张表，两页均已目视检查；[调用回执](evidence/native-plugin-acceptance-20261102/plugin-receipts/docx-authoring-receipt.json) |
| PDF | create_documents.py / ReportLab；PDF 插件的页面渲染流程 | [两页安装包摘要说明](evidence/native-plugin-acceptance-20261102/plugin-deliverables/XY-Switch-package-checksums.pdf)，六条完整 SHA256，两页均已目视检查；[调用回执](evidence/native-plugin-acceptance-20261102/plugin-receipts/pdf-authoring-receipt.json) |
| Spreadsheets | create_workbook.mjs / Artifact Tool；生成、重新计算、重新导入 | [安装包体积工作簿](evidence/native-plugin-acceptance-20261102/plugin-deliverables/XY-Switch-package-sizes.xlsx)，15 个公式、错误 0；扰动 NSIS 字节数 +1 MiB 后各汇总变化，恢复及重新导入结果一致；[计算回执](evidence/native-plugin-acceptance-20261102/plugin-receipts/xlsx-authoring-receipt.json) |
| Presentations | create_presentation.mjs / Artifact Tool；官方 finalizer、重新导入及渲染 | [五页三平台验收汇报](evidence/native-plugin-acceptance-20261102/plugin-deliverables/XY-Switch-three-platform-acceptance.pptx)，第 2、3 页各有原生可编辑表格；完整性、布局、五页重新导入和目视检查通过；[调用回执](evidence/native-plugin-acceptance-20261102/plugin-receipts/pptx-authoring-receipt.json)、[finalizer 原始结果](evidence/native-plugin-acceptance-20261102/qa/pptx/validation.json)；实际 PowerPoint 运行另行验证 |
| Template Creator | 官方 create-template-skill.mjs，--home 指向隔离 template-home | 创建 artifact-template-xy-switch；[官方创建回执](evidence/native-plugin-acceptance-20261102/plugin-receipts/template-create-result.json) exitCode=0；manifest、agent metadata、DOCX/预览资产摘要的[独立核对](evidence/native-plugin-acceptance-20261102/plugin-receipts/template-acceptance-receipt.json)通过；范围为隔离模板创建 |
| reverse-skill | 自有 NSIS 外壳离线静态解析；Python / PowerShell 独立实现；官方 review_case.py | [正式报告](2026-11-02_reverse-xy-switch-installer-report.md)及[业务回执](evidence/native-plugin-acceptance-20261102/plugin-receipts/reverse-skill-business-receipt.json)；10 项独立比较一致；7 Evidence、6 workitems、5 Findings、1 Path；最终严格审查 PASS，errors=0、warnings=0 |

四份办公成果的字节数和 SHA256、十二份 QA 的原始摘要均已核对，复制进公开证据目录后的内容与原件逐字节一致。QA 包含 DOCX/PDF 各两页、XLSX 预览/计算检查、PPTX validation 及五页预览。

逆向结果保留 **2 validated + 3 candidate**。Python 与 PowerShell 为独立实现，imports 和 selected strings 来源于同一解析器；本轮静态分析覆盖安装器外壳。原始 case 的初始化时间线 401 字节逐字节保留，最终时间线为 3,650 字节；最终摘要和前缀核对见业务回执。

## 5. 失败归因与修复

| 事项 | 原因 / 处理 | 最终证据 |
| --- | --- | --- |
| 原生 CI harness | 修复注册的 MSI component 路径、旧 MSI 行身份选择、每格式完整 payload 摘要、同卷归档、隔离钥匙串、CDP/OCR 恢复及持久化 GUI 语言选择 | 7 个后续提交只修改验收/构建辅助输入；最终五项核心 native jobs 成功，生产输入与来源提交一致 |
| DOCX 页面布局 | 调整生成文档的表格边框、列宽 | 最终两页重新渲染并目视检查 |
| 模板辅助校验 | 辅助环境缺少 yaml 包；跨上下文数组 deepStrictEqual 造成误报。改为官方 YAML 字段的精确比较及标量核对 | 官方创建器各次 exitCode 均为 0；最终独立核对通过 |
| Mermaid SVG 导出 | 官方渲染器返回 exitCode=1：缺少 mmdc | [完整调用回执](evidence/native-plugin-acceptance-20261102/reverse-case/report/diagram-render-receipt.json)；[可编辑 Mermaid](evidence/native-plugin-acceptance-20261102/reverse-case/report/analysis-flow.mmd)和报告内流程图已交付，SVG 未生成 |

原生 harness 变更记录为 d5de7f6、32cce9d、a223a5c、8de3709、abd6be0、e1b7f32、0279977。失败归因与安装包真实运行结果分别记录，生产包继续绑定 fe4946bd2fcd4c27a7432aeadb5a656e0e6e676b。

## 6. 复核与复现

1. 阅读账本，核对两个 run 的提交、结论、jobs 和八份 artifact 元数据。需要原始安装包时从指定构建运行下载三份平台 artifact，先比较 ZIP 的字节数与 SHA256，再比较解出的六个包与三份 manifest。
2. 原生复现使用 [Build and verify 工作流](../.github/workflows/build.yml)的 workflow_dispatch：native_acceptance_only=true、native_source_run=41419299553、native_source_commit=fe4946bd2fcd4c27a7432aeadb5a656e0e6e676b；生产输入一致性门槛通过后在 disposable hosted runner 安装、升级、卸载和恢复。
3. 逐项 native 结果及安装观察保存在 ledger.json；四份原始结果的摘要与所属 artifact ID/digest 可用于校验原始下载记录。公开选择结果中保留匹配标记及摘要，原始授权值不进入公开账本。
4. 打开四份已归档业务成果；工作簿的动态重算结果、演示文稿的 finalizer 和重新导入结果分别见对应回执。两份离线静态脚本已原样归档：[inspect-pe.py](../scripts/native-acceptance/inspect-pe.py)、[inspect-header.ps1](../scripts/native-acceptance/inspect-header.ps1)，完整使用条件见逆向报告。

公开账本与业务成果随开发分支提交。完整下载目录、平台截图/日志和原始 reference 快照保留在本地 artifacts/native-plugin-acceptance-20261102。之前的磁盘清理记录仍在开发计划；本阶段保留本次验收证据。

## 7. 下一阶段

| 优先级 | 完善内容 | 验收标准 |
| --- | --- | --- |
| P1 | 正式安装包签名与 macOS 公证 | Windows 安装器/应用和 macOS 应用使用正式签名链验证；macOS 公证、Gatekeeper 接受状态有独立证据 |
| P1 | macOS x86_64 和 app.zip 的独立执行 | 对剩余架构与分发格式验证实际 GUI、原生凭据、接管、冷启动、安装/升级/卸载及恢复 |
| P2 | Linux/macOS 原生 GUI 加密同步 | 原生文件选择器、取消、错误口令无写入、预览与明确选择、备份和回滚逐项通过 |

这些事项作为独立的发版准备和平台扩展任务推进。当前 P0 安装包与六插件业务验收的执行环境、源码绑定、已证实范围及交付证据已完整记录。
