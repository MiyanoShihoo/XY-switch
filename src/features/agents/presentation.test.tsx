import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { AgentInspector, type AgentSelection } from "./AgentInspector";
import { AgentsWorkspace } from "./AgentsWorkspace";
import { ContextDetails, UsageDetails } from "./presentation";
import { event, snapshot, usage } from "./test-fixtures";
import { money } from "./format";
import { ReplayControls } from "./ReplayControls";
import { TaskView } from "./TaskView";
import { clientName, eventKindText, useAgentText } from "./i18n";

vi.mock("../../i18n/language", () => ({ useLanguage: () => "en" }));
const noop = () => {};
function inspect(selection: AgentSelection) {
  return renderToStaticMarkup(createElement(AgentInspector, { selection, snapshot: snapshot(), now: 2000, onClose: noop, onSelect: noop, onLocate: noop, onEvents: noop, onReplay: noop }));
}

describe("truthful observation presentation", () => {
  it("keeps task cards and replay controls available when persisted dates are invalid", () => {
    const badDate = 9_007_199_254_740_991;
    const data = snapshot({ tasks: [{ id: "bad-date-task", agentId: "agent", requestedByAgentId: null, parentTaskId: null, kind: "turn", title: "Date fixture", status: "unknown", startedAt: badDate, endedAt: null, lastEventAt: badDate, summary: null, error: null, usage: usage(), evidenceEventId: "date-fixture" }] });
    const tasks = renderToStaticMarkup(createElement(TaskView, { snapshot: data, selectedId: null, onSelect: noop, onLocate: noop, now: 2000 }));
    expect(tasks).toContain("Date fixture");
    expect(tasks).not.toContain("datetime=");
    const replay = renderToStaticMarkup(createElement(ReplayControls, { at: badDate, displayedAt: badDate, history: { oldestAt: 1000, latestAt: badDate, eventCount: 1, retentionDays: 30, truncated: false }, ready: true, loading: false, failed: false, onChange: noop }));
    expect(replay).toContain("No replay history available");
    expect(replay).not.toContain('type="range"');
  });
  it("distinguishes unknown cost and absent context from measured zero", () => {
    const unknown = renderToStaticMarkup(createElement(UsageDetails, { usage: usage() }));
    expect(unknown).toContain("Actual cost</dt><dd>—</dd>");
    expect(unknown).toContain("Estimated cost</dt><dd>—</dd>");
    expect(unknown).not.toContain("$0");
    expect(money(0, "en-US")).toBe("$0.00");
    const context = renderToStaticMarkup(createElement(ContextDetails, { context: null }));
    expect(context).toContain("No context measurement"); expect(context).not.toContain("progress");
    const measured = renderToStaticMarkup(createElement(ContextDetails, { context: { usedTokens: 0, windowTokens: null, measuredAt: 1000, basis: "source_reported" } }));
    expect(measured).toContain("source_reported"); expect(measured).toContain("Measured</dt>");
  });

  it("keeps tool outcomes separate from agent lifecycle and preserves evidence metadata", () => {
    const failed = inspect({ kind: "event", event: event() });
    expect(failed).toContain("Tool failed"); expect(failed).toContain("tool_failed");
    expect(failed).toContain("not the agent&#x27;s lifecycle state");
    expect(failed).toContain("session/events.jsonl"); expect(failed).toContain("call-id");
    expect(failed).toContain("DeepSeek Harness");
    const returned = inspect({ kind: "event", event: event({ kind: "tool_returned", status: null }) });
    expect(returned).toContain('data-status="unknown"');
    const agent = inspect({ kind: "agent", id: "agent" });
    expect(agent).toContain('data-status="running"'); expect(agent).toContain('data-connection="disconnected"');
    expect(agent).not.toContain('data-status="failed"');
    expect(clientName("deepseek_harness")).toBe("DeepSeek Harness");
  });

  it("labels new lifecycle kinds and renders browser-only mode without sample agents", () => {
    function Labels() {
      const { t } = useAgentText();
      return createElement("span", null, ["agent_failed", "agent_interrupted", "tool_failed"].map(kind => eventKindText(kind, t)).join(" / "));
    }
    expect(renderToStaticMarkup(createElement(Labels))).toContain("Agent failed / Agent interrupted / Tool failed");
    const html = renderToStaticMarkup(createElement(AgentsWorkspace, { onReturn: noop }));
    expect(html).toContain("Native desktop app required");
    expect(html).toContain("Real data is not connected");
    expect(html).not.toContain('class="ag-node"');
    expect(html.match(/role="tab"/g)).toHaveLength(4);
  });
});
