export type AgentStatus = "running" | "waiting" | "completed" | "failed" | "interrupted" | "unknown";
export type SourceConnection = "connected" | "disconnected" | "unavailable";
export type CapabilityMode = "realtime" | "delayed" | "history" | "unsupported";

export interface AgentCapability {
  feature: string;
  mode: CapabilityMode;
  reason: string;
}

export interface AgentSource {
  id: string;
  clientId: string;
  name: string;
  connection: SourceConnection;
  mode: CapabilityMode;
  lastCheckedAt: number | null;
  lastEventAt: number | null;
  fileCount: number;
  detail: string;
  capabilities: AgentCapability[];
}

export interface AgentUsage {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  reasoningTokens: number;
  totalTokens: number;
  records: number;
  actualCostUsd: number | null;
  estimatedCostUsd: number | null;
  unknownCostRecords: number;
}

export interface AgentContext {
  usedTokens: number;
  windowTokens: number | null;
  measuredAt: number;
  basis: string;
}

export interface AgentUsageRecord {
  key: string;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  reasoningTokens: number;
  totalTokens: number;
  actualCostUsd: number | null;
  estimatedCostUsd: number | null;
  priceSource: string | null;
  priceVersion: string | null;
  basis: string;
}

export interface AgentEvent {
  id: string;
  sourceId: string;
  sourcePath: string;
  sourceLine: number;
  clientId: string;
  sessionId: string;
  agentId: string;
  taskId: string | null;
  timestamp: number;
  lifecycleAt?: number | null;
  observedAt: number;
  kind: string;
  title: string;
  summary: string | null;
  status: AgentStatus | null;
  relatedAgentId: string | null;
  parentAgentId: string | null;
  correlationId: string | null;
  name: string | null;
  role: string | null;
  model: string | null;
  workspace: string | null;
  usage: AgentUsageRecord | null;
  context: AgentContext | null;
}

export interface AgentNode {
  id: string;
  sessionId: string;
  clientId: string;
  sourceId: string;
  name: string;
  role: string | null;
  model: string | null;
  workspace: string | null;
  parentId: string | null;
  teamId: string;
  status: AgentStatus;
  connection: SourceConnection;
  createdAt: number;
  lastEventAt: number;
  startedAt: number | null;
  endedAt: number | null;
  currentTaskId: string | null;
  usage: AgentUsage;
  context: AgentContext | null;
  evidenceEventId: string;
}

export interface AgentTask {
  id: string;
  agentId: string;
  requestedByAgentId: string | null;
  parentTaskId: string | null;
  kind: "turn" | "assignment";
  title: string;
  status: AgentStatus;
  startedAt: number | null;
  endedAt: number | null;
  lastEventAt: number;
  summary: string | null;
  error: string | null;
  usage: AgentUsage;
  evidenceEventId: string;
}

export interface AgentEdge {
  id: string;
  sourceAgentId: string;
  targetAgentId: string;
  kind: "spawn" | "assignment" | "handoff";
  taskId: string | null;
  timestamp: number;
  evidenceEventId: string;
}

export interface AgentTeam {
  id: string;
  name: string;
  clientId: string;
  agentCount: number;
  runningCount: number;
  lastEventAt: number;
  usage: AgentUsage;
}

export interface AgentUsageGroup {
  dimension: "agent" | "task" | "session" | "team" | "client" | "model";
  id: string;
  label: string;
  usage: AgentUsage;
}

export interface AgentSnapshot {
  version: number;
  revision: number;
  generatedAt: number;
  replayAt: number | null;
  sources: AgentSource[];
  teams: AgentTeam[];
  agents: AgentNode[];
  edges: AgentEdge[];
  tasks: AgentTask[];
  usageGroups: AgentUsageGroup[];
  totals: AgentUsage;
  history: { oldestAt: number | null; latestAt: number | null; eventCount: number; retentionDays: number; truncated: boolean };
  warnings: string[];
}

export interface AgentSnapshotQuery { teamId?: string; at?: number }
export interface AgentEventScope { at: number | null }
export interface AgentEventQuery {
  teamId?: string;
  agentId?: string;
  taskId?: string;
  since?: number;
  until?: number;
  cursor?: string;
  limit?: number;
}
export interface AgentEventPage { events: AgentEvent[]; nextCursor: string | null; total: number }
