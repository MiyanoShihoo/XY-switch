import { useMemo } from "react";
import { useLanguage } from "../../i18n/language";
import type { AgentStatus, CapabilityMode, SourceConnection } from "../../types/agents";

const english = {
  "Agent 编队": "Agent teams", "只读观测": "Read-only observation", "返回配置工作区": "Back to configuration",
  "跟随真实执行，读懂成员之间的协作。": "Follow real execution and understand how agents work together.",
  "团队": "Team", "所有团队": "All teams", "刷新": "Refresh", "正在读取": "Reading", "等待首次快照": "Waiting for the first snapshot",
  "实时观测": "Live observation", "历史回放": "History replay", "返回实时": "Return to live", "后端投影": "Backend projection",
  "累计 Token": "Cumulative tokens", "用量记录": "Usage records", "成员": "Agents", "任务": "Tasks", "运行中": "Running",
  "等待中": "Waiting", "已完成": "Completed", "失败": "Failed", "已中断": "Interrupted", "未知": "Unknown",
  "已连接": "Connected", "已断开": "Disconnected", "不可用": "Unavailable", "实时": "Real-time", "延迟采集": "Delayed collection",
  "仅历史": "History only", "未支持": "Unsupported", "源": "Source", "数据源": "Data sources", "源连接": "Source connection",
  "拓扑": "Topology", "时间线": "Timeline", "资源": "Resources", "查看详情": "View details", "收起详情": "Close details",
  "展开详情": "Open details", "详情": "Details", "快照生成于": "Snapshot generated", "最后读取成功": "Last successful read",
  "真实数据未连接": "Real data is not connected", "需要原生桌面程序": "Native desktop app required",
  "请在 XY Switch 桌面程序中打开 Agent 编队，读取本地客户端的真实事件。浏览器预览不连接本地观测源。": "Open Agent teams in the XY Switch desktop app to read actual events from local clients. Browser preview is not connected to local observation sources.",
  "尚无成员记录": "No agent records yet", "尚无任务记录": "No task records yet", "此时刻没有成员": "No agents at this point in time",
  "读取本地事件后，成员、协作关系与任务会出现在这里。": "Agents, relationships and tasks will appear after local events have been read.",
  "连接中断，保留上次快照": "Connection interrupted · showing the last snapshot",
  "读取失败": "Read failed", "重试": "Retry", "通知连接中断，正在低频校准": "Change notifications disconnected; periodic reconciliation is active",
  "正在加载所选范围，当前仍显示上次成功快照。": "Loading the selected scope. The last successful snapshot is still displayed.",
  "所选范围读取失败，当前仍显示上次成功快照。": "The selected scope could not be read. The last successful snapshot is still displayed.",
  "没有收到新事件不会改变成员的执行状态。": "An absence of new events does not change an agent’s lifecycle state.",
  "观测说明": "Observation notes", "源能力与连接详情": "Source capabilities and connection details", "没有可用数据源": "No available data sources",
  "尚未报告": "Not reported", "没有用量记录": "No usage records", "没有上下文测量": "No context measurement",
  "输入": "Input", "输出": "Output", "缓存读取": "Cache read", "缓存写入": "Cache write", "推理": "Reasoning",
  "总量": "Total", "实际费用": "Actual cost", "估算费用": "Estimated cost", "费用未知记录": "Records with unknown cost",
  "缓存和推理为细分项，不额外叠加到总量。": "Cache and reasoning are breakdowns, not additional tokens added to the total.",
  "费用估算截至快照时刻；实际费用仅展示来源明确报告的值。": "Cost estimates are shown as of this snapshot. Actual cost is shown only when explicitly reported by the source.",
  "来源": "Source", "上下文": "Context", "测量于": "Measured", "测量依据": "Measurement basis", "窗口未提供": "Window not reported",
  "当前任务": "Current task", "没有当前任务": "No current task", "经过时间": "Elapsed", "模型": "Model", "角色": "Role",
  "客户端": "Client", "未报告角色": "Role not reported", "未报告模型": "Model not reported", "未报告任务标题": "Task title not reported",
  "定位成员": "Locate agent", "搜索成员名称、角色或 ID": "Search agent name, role or ID", "没有匹配成员": "No matching agents",
  "自动布局": "Auto layout", "适应画布": "Fit to canvas", "放大": "Zoom in", "缩小": "Zoom out", "展开全部": "Expand all",
  "折叠子层": "Collapse descendants", "展开子层": "Expand descendants", "隐藏成员": "Hidden agents", "可见成员": "Visible agents",
  "层级": "Depth", "全部层级": "All levels", "仅根节点": "Roots only", "两层": "Two levels", "三层": "Three levels",
  "上游": "Upstream", "下游": "Downstream", "派生": "Spawn", "交接": "Handoff", "父子关系": "Parent relationship",
  "拖动节点调整位置；拖动画布平移；Ctrl / ⌘ + 滚轮缩放。": "Drag nodes to arrange; drag the canvas to pan; Ctrl / ⌘ + wheel to zoom.",
  "方向键平移，+ / − 缩放，0 适应画布。": "Arrow keys pan, + / − zoom, and 0 fits the canvas.",
  "缩小概览，放大查看完整节点信息": "Overview scale · zoom in for full node details", "包含循环关系": "Contains cyclic relationships",
  "全部状态": "All statuses", "搜索任务": "Search tasks", "清除筛选": "Clear filters", "没有匹配任务": "No matching tasks",
  "调整状态或搜索条件以查看其他任务。": "Change the status or search filter to see other tasks.",
  "显示更多任务": "Show more tasks", "显示": "Showing", "共": "of", "归属成员": "Owner", "请求成员": "Requested by",
  "父任务": "Parent task", "开始时间": "Started", "结束时间": "Ended", "最后事件": "Last event", "摘要": "Summary", "错误": "Error",
  "没有摘要": "No summary reported", "任务类型": "Task type", "轮次": "Turn", "委派": "Delegation", "会话 ID": "Session ID",
  "成员 ID": "Agent ID", "任务 ID": "Task ID", "事件 ID": "Event ID", "工作目录": "Workspace", "证据事件": "Evidence event",
  "创建时间": "Created", "父成员": "Parent agent", "查看成员事件": "View agent events", "查看任务事件": "View task events",
  "此快照中没有该记录": "This record is not present in the snapshot", "选择成员、任务、事件或资源以查看详情。": "Select an agent, task, event or resource to inspect it.",
  "所有成员": "All agents", "所有任务": "All tasks", "起始时间": "From", "截止时间": "Until", "本地时间": "Local time",
  "起始时间不能晚于截止时间。": "The start time must not be later than the end time.", "时间无效": "Invalid time",
  "事件": "Events", "尚无事件": "No events yet", "当前范围没有事件。": "There are no events in the selected range.",
  "加载更多事件": "Load more events", "已到最后一页": "End of event history", "有新事件，刷新列表": "New events available · refresh list",
  "事件读取失败，保留已加载记录。": "Events could not be read. Previously loaded records are retained.",
  "来源路径": "Source path", "来源行号": "Source line", "源时间戳": "Source timestamp", "采集时间": "Observed at",
  "关联 ID": "Correlation ID", "关联成员": "Related agent", "事件类型": "Event kind", "价格来源": "Price source", "价格版本": "Price version",
  "用量依据": "Usage basis", "用量记录键": "Usage record key", "在此时刻回放": "Replay at this time",
  "选择历史时刻": "Choose a point in history", "播放": "Play", "暂停": "Pause", "回放速度": "Replay speed",
  "请求时刻": "Requested time", "显示时刻": "Displayed time", "可用历史": "Available history", "暂无可回放历史": "No replay history available",
  "每一帧均由后端按事件投影；等待当前快照后继续播放。": "Each frame is projected by the backend from events. Playback waits for the current snapshot.",
  "历史已截断": "History is truncated", "保留天数": "Retention days", "原始文件不受影响": "Source files are unaffected",
  "资源与用量": "Resources and usage", "真实数据源": "Actual data sources", "按维度查看累计用量": "Cumulative usage by dimension",
  "维度": "Dimension", "按客户端": "By client", "按模型": "By model", "按团队": "By team", "按成员": "By agent",
  "按任务": "By task", "按会话": "By session", "名称": "Name", "没有该维度的用量": "No usage for this dimension",
  "显示更多": "Show more", "观测方式": "Observation mode", "最近检查": "Last checked", "文件数": "Files",
  "能力": "Capability", "可用性": "Availability", "说明": "Details", "未报告能力": "No capabilities reported",
  "生命周期": "Lifecycle", "通知通道": "Notification channel", "快照读取": "Snapshot reads", "轮询校准": "Periodic reconciliation",
  "刷新间隔 30 秒；事件通知触发增量刷新。": "Reconciles every 30 seconds; event notifications trigger refreshes.",
  "完整值": "Full value", "复制": "Copy", "已复制": "Copied", "复制失败": "Copy failed", "读取超时": "Read timed out",
  "拓扑画布": "Agent topology canvas", "正在恢复通知连接": "Reconnecting change notifications",
  "观测数据不包含 CPU / 内存采样。": "Observation data does not include CPU or memory samples.",
  "自定义层级": "Custom depth", "任务分配": "Assignment",
  "成员创建": "Agent created", "成员更新": "Agent updated", "成员派生": "Agent spawned", "成员状态": "Agent status",
  "成员关闭": "Agent closed", "成员开始": "Agent started", "成员结束": "Agent ended", "成员等待": "Agent waiting", "成员恢复": "Agent resumed",
  "任务开始": "Task started", "任务完成": "Task completed", "任务失败": "Task failed", "任务中断": "Task interrupted",
  "任务更新": "Task updated", "任务交接": "Task handoff", "工具开始": "Tool started", "工具返回": "Tool returned", "工具完成": "Tool completed",
  "消息": "Message", "上下文压缩": "Context compacted", "用量缺口": "Usage gap", "异常事件": "Exception",
  "成员失败": "Agent failed", "成员中断": "Agent interrupted", "工具失败": "Tool failed",
  "工具状态仅描述本次调用，不代表成员的执行状态。": "Tool status describes this invocation, not the agent's lifecycle state.",
} as const;

export type AgentText = keyof typeof english;
export type AgentTranslator = (text: AgentText) => string;
export function useAgentText() {
  const language = useLanguage();
  return useMemo(() => ({
    t: ((text: AgentText) => language === "en" ? english[text] : text) as AgentTranslator,
    locale: language === "en" ? "en-US" : "zh-CN",
  }), [language]);
}

export function statusText(status: AgentStatus, t: AgentTranslator) {
  return t(({ running: "运行中", waiting: "等待中", completed: "已完成", failed: "失败", interrupted: "已中断", unknown: "未知" } as const)[status] ?? "未知");
}
export function connectionText(connection: SourceConnection, t: AgentTranslator) {
  return t(({ connected: "已连接", disconnected: "已断开", unavailable: "不可用" } as const)[connection] ?? "未知");
}
export function modeText(mode: CapabilityMode, t: AgentTranslator) {
  return t(({ realtime: "实时", delayed: "延迟采集", history: "仅历史", unsupported: "未支持" } as const)[mode] ?? "未知");
}

const eventLabels: Record<string, AgentText> = {
  agent_created: "成员创建", agent_updated: "成员更新", agent_spawned: "成员派生", agent_status: "成员状态",
  agent_closed: "成员关闭", agent_started: "成员开始", agent_ended: "成员结束", agent_waiting: "成员等待", agent_resumed: "成员恢复",
  agent_failed: "成员失败", agent_interrupted: "成员中断",
  task_started: "任务开始", task_completed: "任务完成", task_failed: "任务失败", task_interrupted: "任务中断",
  task_updated: "任务更新", task_assigned: "任务分配", task_handoff: "任务交接",
  tool_started: "工具开始", tool_returned: "工具返回", tool_completed: "工具完成", tool_failed: "工具失败",
  message: "消息", usage: "用量记录", context: "上下文", context_compacted: "上下文压缩", usage_gap: "用量缺口", exception: "异常事件",
};
export function eventKindText(kind: string, t: AgentTranslator) { return eventLabels[kind] ? t(eventLabels[kind]) : kind; }
export function clientName(clientId: string) {
  const names: Record<string, string> = { codex: "Codex", deepseek_harness: "DeepSeek Harness" };
  return names[clientId] ?? clientId;
}
export function edgeKindText(kind: string, t: AgentTranslator) {
  const labels: Record<string, AgentText> = { spawn: "派生", assignment: "任务分配", handoff: "交接", parent: "父子关系" };
  return labels[kind] ? t(labels[kind]) : kind;
}
