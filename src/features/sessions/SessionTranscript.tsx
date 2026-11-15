import { useId, type ReactNode } from "react";
import { useLanguage, t } from "../../i18n/language";
import { formatSessionTime } from "../../lib/session-format";
import type { SessionMessage, SessionToolEvent } from "../../types/session";
import { Badge } from "../../ui";

const kinds: Record<SessionToolEvent["eventKind"], string> = {
  search: "搜索工具", call: "调用工具", approval: "审批记录", result: "工具结果",
};
const statuses: Record<SessionToolEvent["status"], string> = {
  unknown: "执行状态未知", requested: "等待执行", running: "正在执行", pending_approval: "等待审批",
  approved: "已批准", denied: "已拒绝", result_received: "结果已收到", completed: "已完成",
  succeeded: "执行成功", failed: "执行失败", cancelled: "已取消",
};

function ToolDetails({ tool, callTarget }: { tool?: SessionToolEvent | null; callTarget?: string }) {
  const tone = tool?.status === "succeeded" ? "success" : tool?.status === "failed" || tool?.status === "denied" ? "danger" : tool?.status === "pending_approval" || tool?.status === "cancelled" ? "warning" : "neutral";
  return <div className="session-tool" aria-label={t("工具事件")} data-event={tool?.eventKind ?? "result"} data-status={tool?.status ?? "unknown"}>
    <div className="session-tool__heading"><strong>{t(tool ? kinds[tool.eventKind] ?? "工具事件" : "工具结果")}</strong><Badge tone={tone}>{t(tool ? statuses[tool.status] ?? "执行状态未知" : "执行状态未知")}</Badge></div>
    {tool?.name && <code className="session-tool__name">{tool.name}</code>}
    {tool && <div className="session-tool__id"><span>{t("调用 ID")}</span>{callTarget
      ? <a href={"#" + callTarget} aria-label={t("查看关联调用 {id}", { id: tool.callId })}>{tool.callId}</a>
      : <code>{tool.callId || t("未知")}</code>}</div>}
    {tool?.approvalRequestId && <div className="session-tool__id"><span>{t("审批 ID")}</span><code>{tool.approvalRequestId}</code></div>}
  </div>;
}

function highlightContent(content: string, search: string) {
  if (!search) return content;
  const parts: ReactNode[] = [];
  const source = content.toLocaleLowerCase();
  const needle = search.toLocaleLowerCase();
  let offset = 0;
  let found = source.indexOf(needle);
  while (found !== -1) {
    parts.push(content.slice(offset, found), <mark key={found}>{content.slice(found, found + search.length)}</mark>);
    offset = found + search.length;
    found = source.indexOf(needle, offset);
  }
  parts.push(content.slice(offset));
  return parts;
}

export function SessionTranscript({ messages, search = "", activeIndex }: { messages: SessionMessage[]; search?: string; activeIndex?: number }) {
  useLanguage();
  const prefix = useId();
  const calls = new Map<string, string>();
  const approvalCalls = new Map<string, { callId: string; target: string } | null>();
  messages.forEach((message, index) => {
    const tool = message.tool;
    if (tool?.eventKind === "call" && tool.approvalRequestId && tool.callId) {
      approvalCalls.set(tool.approvalRequestId, approvalCalls.has(tool.approvalRequestId)
        ? null : { callId: tool.callId, target: prefix + "-call-" + index });
    }
  });
  return <>{messages.map((message, index) => {
    const id = prefix + "-call-" + index;
    const tool = message.tool;
    const call = tool && (tool.eventKind === "call" || tool.eventKind === "search");
    const approvedCall = tool?.eventKind === "approval" && tool.approvalRequestId ? approvalCalls.get(tool.approvalRequestId) : undefined;
    const callTarget = approvedCall && approvedCall.callId === tool?.callId ? approvedCall.target
      : tool?.callId && !call ? calls.get(tool.callId) : undefined;
    if (call && tool.callId) calls.set(tool.callId, id);
    return <article className="session-message" data-role={message.role} data-message-index={index} data-search-current={activeIndex === index || undefined} id={call ? id : undefined} tabIndex={-1} key={index}>
      <div className="session-message__meta"><strong>{message.role}</strong><time>{formatSessionTime(message.ts)}</time></div>
      {(tool || message.role === "tool") && <ToolDetails tool={tool} callTarget={callTarget} />}
      <pre>{highlightContent(message.content, search)}</pre>
    </article>;
  })}</>;
}
