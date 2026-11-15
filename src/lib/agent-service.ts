import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AgentEventPage, AgentEventQuery, AgentEventScope, AgentSnapshot, AgentSnapshotQuery } from "../types/agents";

export function isAgentObservationAvailable(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function requireNative(): void {
  if (!isAgentObservationAvailable()) {
    throw new Error("Agent 编队需要原生桌面程序读取本地事件。浏览器预览没有连接真实数据源。");
  }
}

export async function getAgentSnapshot(query: AgentSnapshotQuery = {}): Promise<AgentSnapshot> {
  requireNative();
  return invoke<AgentSnapshot>("get_agent_snapshot", { query });
}

export async function getAgentEvents(query: AgentEventQuery = {}, scope: AgentEventScope = { at: null }): Promise<AgentEventPage> {
  requireNative();
  return invoke<AgentEventPage>("get_agent_events", { query, scope });
}

export async function setAgentRetention(retentionDays: number): Promise<void> {
  requireNative();
  return invoke("set_agent_retention", { retentionDays });
}

export async function subscribeAgentChanges(onChange: () => void): Promise<UnlistenFn> {
  requireNative();
  return listen("agent-observation-changed", onChange);
}
