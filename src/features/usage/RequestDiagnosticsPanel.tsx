import { t } from "../../i18n/language";
import type { RequestDiagnostics, UsageLogEntry } from "../../types/usage";
import { TaskSummaryView } from "../governance/TaskPanel";
import { budgetReasonLabel } from "../governance/task-format";

const stages: Record<string, string> = {
  provider_selection: "选择供应商",
  provider_configuration: "供应商配置",
  credentials: "读取凭据",
  request_conversion: "转换请求",
  tool_bridge: "处理工具协议",
  budget: "预算检查",
  task_identity: "任务身份检查",
  upstream_send: "连接与发送",
  upstream_http: "上游 HTTP",
  response_read: "读取响应",
  response_conversion: "转换响应",
  response_stream: "响应流",
  client_response: "客户端接收",
};
const reasons: Record<string, string> = {
  loop_detected: "供应商端点指向本地代理自身。",
  provider_not_selected: "尚未选择供应商。",
  provider_disabled: "选中的供应商已禁用。",
  database_error: "无法读取本地配置。",
  credential_error: "无法读取供应商凭据。",
  api_key_missing: "可用供应商尚未配置 API Key。",
  invalid_json: "请求不是有效的 JSON 对象。",
  unsupported_operation: "不支持此 Gemini 操作。",
  invalid_diagnostic_target: "诊断对象无效。",
  budget_rejected: "本地预算规则拒绝了请求。",
  circuit_open: "供应商暂时处于熔断状态。",
  timeout: "上游请求超时。",
  connect: "无法连接供应商。",
  upstream_status: "供应商返回了失败的 HTTP 状态。",
  invalid_upstream_response: "供应商返回的响应格式无效。",
  stream_interrupted: "响应流在完成前中断。",
  stream_cancelled: "客户端在响应完成前关闭了接收。",
  response_error: "无法读取供应商响应。",
  response_conversion: "无法转换供应商响应。",
  invalid_request: "无法转换请求或供应商配置。",
  request_build: "供应商请求配置无效。",
  request: "上游请求未能完成。",
  request_cancelled: "任务已取消，后续重试已阻断。",
  upstream_unavailable: "没有供应商能够完成请求。",
};

export function forwardingStatusLabel(row: UsageLogEntry): string {
  if (row.diagnostics?.sendAttempts === 0) return t("本地拒绝") + (row.statusCode ? " · " + row.statusCode : "");
  return row.statusCode ? "HTTP " + row.statusCode + (row.success === false ? t(" · 未完成") : "") : t("未取得 HTTP");
}

function reason(code: string | null | undefined, fallback?: string | null): string {
  return code && reasons[code] ? t(reasons[code]) : fallback || t("未知");
}

export function RequestDiagnosticsPanel({ diagnostics, errorCode }: { diagnostics?: RequestDiagnostics | null; errorCode?: string | null }) {
  return <section className="request-diagnostics" aria-label={t("请求链路")}>
    <h3>{t("请求链路")}</h3>
    {!diagnostics ? <p className="muted">{t("此记录没有链路证据；发送次数和上游响应状态未知。")}</p> : <>
      <dl className="key-values">
        <dt>{t("失败阶段")}</dt><dd>{diagnostics.failureStage ? t(stages[diagnostics.failureStage] ?? "未知") : "—"}</dd>
        <dt>{t("发送尝试次数")}</dt><dd>{diagnostics.sendAttempts}</dd>
        <dt>{t("收到过上游 HTTP 响应")}</dt><dd>{diagnostics.upstreamResponseReceived ? t("是") : t("否")}</dd>
        {diagnostics.failureStage && <><dt>{t("失败原因")}</dt><dd>{reason(errorCode, diagnostics.failureReason)}</dd></>}
        {diagnostics.budgetReason && <><dt>{t("预算拒绝原因")}</dt><dd>{budgetReasonLabel(diagnostics.budgetReason)}</dd></>}
        {diagnostics.taskRequestId && <><dt>{t("客户端请求标识")}</dt><dd><code>{diagnostics.taskRequestId}</code></dd></>}
        {diagnostics.attemptId && <><dt>{t("发送标识")}</dt><dd><code>{diagnostics.attemptId}</code></dd></>}
      </dl>
      {diagnostics.attempts.length > 0 && <details><summary>{t("每次发送的结果")}</summary><ol>{diagnostics.attempts.map((attempt, index) => <li key={index}>
        <span>{t("尝试")} {index + 1}{" · "}{attempt.statusCode === null ? t("未取得 HTTP") : "HTTP " + attempt.statusCode}</span>
        {attempt.errorKind && <span className="usage-cell-note">{reason(attempt.errorKind)}</span>}
      </li>)}</ol></details>}
      {diagnostics.task && <details><summary>{t("任务累计与上限（记录快照）")}</summary><p className="muted">{t("此处显示记录生成时的任务快照；最新累计和状态请查看硬预算中的最近任务。")}</p><TaskSummaryView task={diagnostics.task} /></details>}
    </>}
  </section>;
}
