# 安装包与插件业务验收证据

日期：2026-11-02；应用来源提交：`fe4946bd2fcd4c27a7432aeadb5a656e0e6e676b`；原生 harness 提交：`0279977f314913d72e0b85a187ccd17217ae95ed`。

阅读入口：[安装包与插件验收报告](../../native-plugin-acceptance-20261102.md)、[安装器离线逆向报告](../../2026-11-02_reverse-xy-switch-installer-report.md)。

| 路径 | 用途 |
| --- | --- |
| ledger.json | 精简公开总账：指定源码/运行、八个归档摘要、六包、83 项 native 检查、安装观察、六插件、QA 和存档文件摘要 |
| source.json | 最终 native-source job 的原始来源记录；生产输入一致性结果及三份原始平台 artifact 元数据 |
| runs/ | 已认证的 GitHub 只读查询回执；checkedAt 标记查询时间，包含真实 success/skipped 状态 |
| manifests/ | 三份构建 manifest 原件；与六包逐项核对，没有 sourceCommit 字段 |
| plugin-deliverables/ | 四份真实业务成果：DOCX、PDF、XLSX、PPTX |
| plugin-receipts/ | 原始生成/创建回执及最终逆向业务回执；隔离模板官方创建器 exitCode=0 |
| qa/ | 已查看的页面/幻灯片预览、工作簿检查及官方演示文稿 finalizer 原始结果 |
| reverse-case/ | scope、时间线、工作项、7 份 E 元数据及原始静态 JSON、原报告、严格审查与 Mermaid 导出回执 |

ledger.json 的 archivedFiles 逐项固定上述原件的字节数和 SHA256，reproductionScripts 固定两份已执行并原样复制到 scripts/native-acceptance 的离线解析脚本。复制记录通过逐字节比较；原始 JSON 中的路径与观测时间保持原值，带 UTF-8 BOM 的原始回执保留编码。

仓库根目录的 .gitattributes 为本证据目录及两份复现脚本设置 -text，保留原始字节。Git 暂存及检出时不转换这些文件的换行；可直接按总账核对字节数与 SHA256。

空白检查将原件的 CRLF 识别为合法换行；两份复现脚本原样保留的末尾空行也由各自属性允许。其余末尾空格、制表符等检查继续使用仓库规则。

公开 nativeChecks 选择四份 CI 原始结果中的来源字段、逐项检查及安装观察，排除凭据值和原始 HTTP 授权；每份原始结果的 SHA256、artifact 和目录仍可由总账定位。Windows sync 的原始数组是 reports，其余为 results。

完整原始包、ZIP、原生截图/日志、隔离模板、全部 reference 快照保留在本机项目 artifacts/native-plugin-acceptance-20261102。GitHub run 链接与 artifact ID/digest 固定本次来源；平台原始包需要从指定 run 下载并核对摘要。原始官方参考快照的来源和摘要见 reverse-case/evidence/reference-observation.json；公开存档包含相关观察，完整快照继续保留本地。

严格 case 审查回执对应原始完整 case；公开 scope/Evidence/report 为该次工作的逐字节存档。重新运行解析会产生新时间戳与摘要，新的 case 审查应更新自己的 Evidence 摘要和工作项。
