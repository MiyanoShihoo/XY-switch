import { renderToStaticMarkup as render } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setLanguagePreference, translate } from "../../i18n/language";
import { en } from "../../i18n/en";
import { ChannelGroupDetails } from "./ChannelGroupDetails";
import { ChannelGroupEditor } from "./ChannelGroupEditor";
import { ChannelGroupSimulation } from "./ChannelGroupSimulation";
import { ChannelGroupRecordedUsage, ChannelGroupStatus, ChannelGroupWarnings } from "./ChannelGroupStatus";
import { ChannelGroupsPage } from "./ChannelGroupsPage";
import { ProvidersPage } from "./ProvidersPage";
import type { ProvidersController } from "./useProviders";
import { channelMoney, channelNumber, channelReason, channelTime, channelWarning, recordedProviderUsage } from "./channel-group-presentation";
import { channel, draft, group, provider, snapshot, view } from "./channel-group-test-fixtures";
import translations from "./channel-groups-en.json";

const { invoke, network } = vi.hoisted(() => ({ invoke: vi.fn(), network: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

beforeEach(async () => {
  vi.unstubAllGlobals(); await setLanguagePreference("zh-CN"); vi.resetAllMocks();
  vi.stubGlobal("fetch", network.mockRejectedValue(new Error("Network is forbidden in this test")));
});
afterEach(() => { expect(invoke).not.toHaveBeenCalled(); expect(network).not.toHaveBeenCalled(); vi.unstubAllGlobals(); });

const editorProps = () => ({ draft: draft(), configuration: { groups: [], projects: [] }, providers: [provider()], accounts: [],
  accountsError: "", errors: {}, busy: false, blocked: false, dirty: true, onChange: vi.fn(), onSave: vi.fn(), onCancel: vi.fn() });
const text = (html: string) => html.replace(/<[^>]*>/g, " ").replace(/\s+/g, " ");

describe("channel group offline presentation", () => {
  it("exposes a provider-directory entry without triggering a provider action", () => {
    const controller: ProvidersController = { providers: [provider()], currentId: "relay", loading: false, busy: false, testing: null,
      error: "", readError: "", message: "", health: {}, refresh: vi.fn(async () => true), activate: vi.fn(async () => {}),
      copy: vi.fn(async () => {}), remove: vi.fn(async () => true), reorder: vi.fn(async () => {}), test: vi.fn(async () => {}), saved: vi.fn(), dismissMessage: vi.fn() };
    const html = render(<ProvidersPage controller={controller} onRuntimeChanged={vi.fn(async () => {})} onConfigure={vi.fn()} onDiagnostics={vi.fn()} />);
    expect(html).toContain('data-directory-action="channel-groups"'); expect(html).toContain("渠道组");
    expect(controller.activate).not.toHaveBeenCalled(); expect(controller.test).not.toHaveBeenCalled(); expect(controller.refresh).not.toHaveBeenCalled();
  });

  it("shows an explicit desktop-only browser fallback with no invented engine or history result", () => {
    const html = render(<ChannelGroupsPage providers={[provider()]} onBack={vi.fn()} />);
    expect(html).toContain("请在桌面端打开渠道组");
    expect(html).toContain("这里不会生成示例引擎结果");
    expect(html).toContain("superapi 计费测试保持暂停");
    expect(html).not.toContain("运行固定模拟"); expect(html).not.toContain("离线配置已保存");
    expect(html).not.toContain("DO_NOT_RENDER");
  });

  it("offers all five strategy choices and separates optional channel settings", () => {
    const html = render(<ChannelGroupEditor {...editorProps()} />);
    expect(html.match(/type="radio"/g)).toHaveLength(5);
    for (const label of ["轮询", "额度感知", "加权轮询", "最小负载", "延迟优先"]) expect(html).toContain(label);
    expect(html.match(/data-primary="true"/g)).toHaveLength(2);
    expect(html).toContain("xy-group/research"); expect(html).toContain("限制、权重、优先级与额度共享");
    expect(html).toContain("未验证独立性");
    expect(html).not.toContain("DO_NOT_RENDER");
  });

  it("keeps missing account/provider references editable and visibly marks their source", () => {
    const props = editorProps(); props.draft.channels[0].providerId = "missing-provider"; props.draft.channels[0].accountId = "missing-account";
    props.accountsError = "账号元数据读取失败";
    const html = render(<ChannelGroupEditor {...props} />);
    expect(html).toContain("引用缺失：missing-provider");
    expect(html).toContain("未读取的账号引用：missing-account");
    expect(html).toContain("账号元数据读取失败");
  });

  it("opens invalid advanced settings and provides accessible validation errors", () => {
    const html = render(<ChannelGroupEditor {...editorProps()} errors={{ "channels.0.rpm": "RATE_ERROR", form: "FORM_ERROR" }} />);
    expect(html).toContain('class="channel-group-advanced" open=""');
    expect(html).toContain('aria-invalid="true"'); expect(html).toContain("RATE_ERROR"); expect(html).toContain("FORM_ERROR");
  });

  it("shows declarations and unknown quota alongside explicit replay counters", () => {
    const html = render(<ChannelGroupStatus group={group()} channels={[view({ poolDeclared: true, tpmUnknownRequests: 2 })]} providers={[provider()]} accounts={[]} source="snapshot" />);
    expect(html).toContain("用户声明，未经独立性验证");
    expect(html).toContain("路由计数来自本地回放或模拟");
    expect(text(html)).toContain("剩余请求额度 未知");
    expect(text(html)).toContain("窗口 Token 未知请求数 2");
    expect(html).not.toContain("DO_NOT_RENDER");
    expect(channelNumber(null)).toBe("未知"); expect(channelNumber(undefined)).toBe("未知"); expect(channelNumber(0)).toBe("0");
    expect(channelMoney(null)).toBe("未知"); expect(channelMoney(0)).toBe("0 μUSD");
    expect(channelTime(null)).toBe("未知"); expect(channelTime(NaN)).toBe("未知");
  });

  it("does not double-count provider history when several channels reference the same provider", () => {
    const saved = group({ channels: [channel(), channel({ id: "channel-b" }), channel({ id: "channel-c", providerId: "other" })] });
    const views = [view(), view({ channelId: "channel-b" }), view({ groupId: "different-group", providerId: "other" })];
    expect(recordedProviderUsage(saved, views)).toEqual([{ providerId: "relay", usage: view().recordedUsage }, { providerId: "other", usage: undefined }]);
    const html = render(<ChannelGroupRecordedUsage group={saved} channels={views} providers={[provider()]} />);
    expect(html.match(/<tr>/g)).toHaveLength(3);
    expect(html).toContain("它们不属于某个渠道或渠道组");
    expect(html).toContain("已知 0 / 3 条"); expect(html).toContain("已知 1 / 3 条");
    expect(html).not.toContain("0 μUSD");
  });

  it("displays a shared zero budget as zero, independently of unknown quota", () => {
    const saved = group({ projectId: "shared" });
    const data = snapshot({ configuration: { groups: [saved], projects: [{ id: "shared", dailyMicrousd: 0, revision: 4 }] } });
    const html = render(<ChannelGroupDetails group={saved} snapshot={data} providers={[provider()]} accounts={[]} />);
    expect(text(html)).toContain("项目日预算 0 μUSD"); expect(text(html)).toContain("剩余请求额度 未知");
  });

  it("makes deterministic simulation and optional provider event replay explicit before any result", () => {
    const html = render(<ChannelGroupSimulation group={group()} snapshotAtMs={1800000000000} providers={[provider()]} accounts={[]} onBusyChange={vi.fn()} />);
    expect(html).toContain("运行固定模拟"); expect(html).toContain("读取本地记录");
    expect(html).toContain("6 条合成预留"); expect(html).toContain("不生成模型完成结果");
    expect(html).toContain("不发送网络、模型或探测请求");
    expect(html).not.toContain("模拟结束时的渠道状态");
  });

  it("renders all current engine warnings and reason codes with useful labels", () => {
    const warnings = ["routing_metrics_are_replay_and_simulation_not_live_counters", "health_history_limited_to_latest_2048_events", "recorded_headers_unavailable_configured_cooldown_used"];
    const html = render(<ChannelGroupWarnings warnings={warnings} />);
    for (const warning of warnings) { expect(channelWarning(warning)).not.toBe(warning); expect(html).not.toContain(warning); }
    expect(html).toContain("不是实时请求计数"); expect(html).toContain("2,048"); expect(html).toContain("历史记录不含响应头");
    for (const reason of ["authentication_disabled", "cost_reservation_required", "unresolved_ledger_cost"]) expect(channelReason(reason)).not.toBe(reason);
  });
});

describe("channel group English translations", () => {
  it("registers every feature message and preserves interpolation placeholders", () => {
    const placeholders = (value: string) => [...value.matchAll(/\{([A-Za-z][A-Za-z0-9_]*)\}/g)].map(match => match[1]).sort();
    for (const [source, translated] of Object.entries(translations)) {
      const registered = en[source];
      expect(registered, source).toBeTypeOf("string");
      expect(registered?.trim(), source).not.toBe("");
      expect(translated, source).not.toMatch(/[\u3400-\u9fff]/u);
      expect(placeholders(translated), source).toEqual(placeholders(source));
      expect(registered, source).not.toMatch(/[\u3400-\u9fff]/u);
      expect(placeholders(registered), source).toEqual(placeholders(source));
      expect(translate(source, {}, "en"), source).toBe(registered);
    }
  });

  it("renders the editor and browser fallback in English without leaking untranslated controls", async () => {
    await setLanguagePreference("en");
    const editor = render(<ChannelGroupEditor {...editorProps()} />);
    const page = render(<ChannelGroupsPage providers={[]} onBack={vi.fn()} />);
    expect(text(editor)).not.toMatch(/[\u3400-\u9fff]/u); expect(text(page)).not.toMatch(/[\u3400-\u9fff]/u);
    expect(editor).toContain("Round robin"); expect(editor).toContain("Quota aware");
  });
});
