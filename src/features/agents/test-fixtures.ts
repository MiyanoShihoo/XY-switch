import type { AgentEdge, AgentEvent, AgentNode, AgentSnapshot, AgentUsage } from "../../types/agents";

export function usage(overrides: Partial<AgentUsage> = {}): AgentUsage {
  return { inputTokens: 80, outputTokens: 20, cacheReadTokens: 40, cacheWriteTokens: 0, reasoningTokens: 10, totalTokens: 100, records: 1, actualCostUsd: null, estimatedCostUsd: null, unknownCostRecords: 1, ...overrides };
}
export function node(id: string, parentId: string | null = null, overrides: Partial<AgentNode> = {}): AgentNode {
  return { id, parentId, sessionId: id, clientId: "deepseek_harness", sourceId: "harness", name: id, role: "worker", model: null, workspace: null, teamId: "team", status: "running", connection: "disconnected", createdAt: 1, lastEventAt: 1000, startedAt: 1, endedAt: null, currentTaskId: null, usage: usage(), context: null, evidenceEventId: "evidence", ...overrides };
}
export function edge(sourceAgentId: string, targetAgentId: string, kind: AgentEdge["kind"] = "spawn"): AgentEdge {
  return { id: `${sourceAgentId}:${targetAgentId}:${kind}`, sourceAgentId, targetAgentId, kind, taskId: null, timestamp: 1000, evidenceEventId: "evidence" };
}
export function snapshot(overrides: Partial<AgentSnapshot> = {}): AgentSnapshot {
  return { version: 1, revision: 1, generatedAt: 2000, replayAt: null, sources: [], teams: [], agents: [node("agent")], edges: [], tasks: [], usageGroups: [], totals: usage(), history: { oldestAt: 1, latestAt: 2000, eventCount: 1, retentionDays: 30, truncated: false }, warnings: [], ...overrides };
}
export function event(overrides: Partial<AgentEvent> = {}): AgentEvent {
  return { id: "tool-event", sourceId: "harness", sourcePath: "session/events.jsonl", sourceLine: 7, clientId: "deepseek_harness", sessionId: "session", agentId: "agent", taskId: null, timestamp: 1000, observedAt: 1100, kind: "tool_failed", title: "Tool call", summary: null, status: "failed", relatedAgentId: null, parentAgentId: null, correlationId: "call-id", name: null, role: null, model: null, workspace: null, usage: null, context: null, ...overrides };
}
