import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { ArrowLeft, Layers3, Pencil, Plus, RefreshCw } from "lucide-react";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { t, useLanguage } from "../../i18n/language";
import { listAccounts } from "../../lib/accounts-service";
import { channelGroupErrorText, channelGroupsNative, listChannelGroups, saveChannelGroup, setChannelGroupEnabled } from "../../lib/channel-group-service";
import type { AccountInfo } from "../../types/accounts";
import type { ChannelGroup, ChannelGroupsSnapshot } from "../../types/channel-group";
import type { Provider } from "../../types/provider";
import { Badge, Button, EmptyState, InlineError, LoadingState } from "../../ui";
import { TaskNavigation, useTaskDraft } from "../settings/TaskNavigation";
import type { ProviderNavigationGuard } from "./ProviderEditor";
import { ChannelGroupDetails } from "./ChannelGroupDetails";
import { ChannelGroupEditor } from "./ChannelGroupEditor";
import { ChannelGroupSimulation } from "./ChannelGroupSimulation";
import { ChannelGroupWarnings } from "./ChannelGroupStatus";
import { buildChannelGroupSave, editChannelGroupDraft, newChannelGroupDraft, type ChannelGroupDraft, type ChannelGroupFormErrors } from "./channel-group-form";
import { channelStrategyLabel, channelTime } from "./channel-group-presentation";
import "./channel-groups.css";

interface Props {
  providers: Provider[];
  metadataLoading?: boolean;
  metadataError?: string;
  onBack: () => void;
  onNavigationGuardChange?: (guard: ProviderNavigationGuard | null) => void;
}

export function ChannelGroupsPage(props: Props) {
  return <TaskNavigation onLeaveApproved={() => props.onNavigationGuardChange?.(null)}>{() => <ChannelGroupsWorkspace {...props} />}</TaskNavigation>;
}

function ChannelGroupsWorkspace({ providers, metadataLoading = false, metadataError = "", onBack, onNavigationGuardChange }: Props) {
  useLanguage();
  const native = channelGroupsNative();
  const [snapshot, setSnapshot] = useState<ChannelGroupsSnapshot | null>(null);
  const [accounts, setAccounts] = useState<AccountInfo[]>([]);
  const [accountsError, setAccountsError] = useState("");
  const [readError, setReadError] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [loading, setLoading] = useState(native);
  const [action, setAction] = useState<string | null>(null);
  const [trialBusy, setTrialBusy] = useState(false);
  const [snapshotVersion, setSnapshotVersion] = useState(0);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<ChannelGroupDraft | null>(null);
  const [errors, setErrors] = useState<ChannelGroupFormErrors>({});
  const [discardOpen, setDiscardOpen] = useState(false);
  const baseline = useRef("");
  const alive = useRef(true);
  const lock = useRef(false);
  const trialLock = useRef(false);
  const pendingLeave = useRef<(() => void) | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const listPosition = useRef({ id: "", scrollTop: 0 });
  const busy = loading || action !== null || trialBusy;
  const dirty = draft !== null && JSON.stringify(draft) !== baseline.current;
  const selected = snapshot?.configuration.groups.find(group => group.id === selectedId);
  const blocked = !native || !snapshot || Boolean(readError) || metadataLoading || Boolean(metadataError);
  const screen = draft ? "editor" : selected ? "detail" : "directory";
  useTaskDraft(dirty, busy);

  const acceptSnapshot = useCallback((next: ChannelGroupsSnapshot) => {
    setSnapshot(next); setReadError(""); setSnapshotVersion(version => version + 1);
    setSelectedId(previous => next.configuration.groups.some(group => group.id === previous) ? previous : null);
  }, []);
  const load = useCallback(async () => {
    if (!native || lock.current || trialLock.current) return;
    lock.current = true; setLoading(true); setReadError(""); setError(""); setNotice("");
    try {
      const [groupsResult, accountsResult] = await Promise.allSettled([listChannelGroups(), listAccounts()]);
      if (!alive.current) return;
      if (groupsResult.status === "fulfilled") acceptSnapshot(groupsResult.value);
      else setReadError(channelGroupErrorText(groupsResult.reason));
      if (accountsResult.status === "fulfilled") { setAccounts(accountsResult.value); setAccountsError(""); }
      else setAccountsError(t("账号元数据读取失败；现有引用会保留，可刷新后重试。"));
    } finally { lock.current = false; if (alive.current) setLoading(false); }
  }, [native, acceptSnapshot]);
  useEffect(() => { alive.current = true; void load(); return () => { alive.current = false; }; }, [load]);
  useEffect(() => { if (readError || error) errorRef.current?.focus(); }, [readError, error]);
  useEffect(() => {
    if (screen === "editor" || !root.current) return;
    if (screen === "directory" && listPosition.current.id) {
      const button = [...root.current.querySelectorAll<HTMLButtonElement>("[data-group-open]")].find(item => item.dataset.groupOpen === listPosition.current.id);
      button?.focus({ preventScroll: true });
      const scroll = root.current.closest<HTMLElement>("#workspace-content");
      if (scroll) scroll.scrollTop = listPosition.current.scrollTop;
    } else root.current.querySelector<HTMLElement>("h1")?.focus({ preventScroll: true });
  }, [screen, selectedId]);
  const requestLeave = useCallback<ProviderNavigationGuard>(proceed => {
    if (busy || lock.current || trialLock.current) return;
    if (dirty) { pendingLeave.current = proceed; setDiscardOpen(true); }
    else proceed();
  }, [busy, dirty]);
  useLayoutEffect(() => {
    onNavigationGuardChange?.(requestLeave);
    return () => onNavigationGuardChange?.(null);
  }, [requestLeave, onNavigationGuardChange]);
  const onTrialBusyChange = useCallback((value: boolean) => { trialLock.current = value; setTrialBusy(value); }, []);
  const rememberGroup = (id: string) => { listPosition.current = { id, scrollTop: root.current?.closest<HTMLElement>("#workspace-content")?.scrollTop ?? 0 }; };
  const clearDraft = () => { setDraft(null); baseline.current = ""; setErrors({}); setError(""); };
  const startDraft = (group?: ChannelGroup) => requestLeave(() => {
    if (!snapshot || blocked) return;
    if (screen === "directory" && group) rememberGroup(group.id);
    const next = group ? editChannelGroupDraft(group, snapshot.configuration) : newChannelGroupDraft(`group-${crypto.randomUUID()}`, `channel-${crypto.randomUUID()}`);
    setSelectedId(group?.id ?? null); baseline.current = JSON.stringify(next); setDraft(next); setErrors({}); setError(""); setNotice("");
  });
  const openGroup = (group: ChannelGroup) => requestLeave(() => { rememberGroup(group.id); clearDraft(); setSelectedId(group.id); setNotice(""); });
  const back = () => requestLeave(() => { if (draft) clearDraft(); else if (selected) { setSelectedId(null); setError(""); } else { onNavigationGuardChange?.(null); onBack(); } });
  const refresh = () => requestLeave(() => { clearDraft(); void load(); });
  const save = async () => {
    if (!draft || !snapshot || blocked || busy || lock.current || trialLock.current) return;
    const built = buildChannelGroupSave(draft, snapshot.configuration);
    if (built.errors) { setErrors(built.errors); return; }
    lock.current = true; setAction("save"); setError(""); setNotice("");
    try {
      const next = await saveChannelGroup(built.input);
      if (!alive.current) return;
      acceptSnapshot(next); clearDraft(); setSelectedId(built.input.group.id); setNotice(t("离线配置已保存。"));
    } catch (cause) { if (alive.current) setError(channelGroupErrorText(cause)); }
    finally { lock.current = false; if (alive.current) setAction(null); }
  };
  const toggle = async (group: ChannelGroup) => {
    if (blocked || busy || lock.current || trialLock.current) return;
    lock.current = true; setAction(group.id); setError(""); setNotice("");
    try {
      const next = await setChannelGroupEnabled(group.id, !group.enabled, group.revision);
      if (alive.current) { acceptSnapshot(next); setNotice(group.enabled ? t("渠道组已在离线试点中停用。") : t("渠道组已在离线试点中启用。")); }
    } catch (cause) { if (alive.current) setError(channelGroupErrorText(cause)); }
    finally { lock.current = false; if (alive.current) setAction(null); }
  };
  return <div ref={root} className="workspace-page channel-groups-page">
    <header className="page-heading channel-group-heading">
      <div className="page-heading__copy">
        <Button variant="quiet" size="sm" icon={<ArrowLeft size={15} />} disabled={busy} onClick={back}>{draft ? selected ? t("返回组详情") : t("返回渠道组") : selected ? t("返回渠道组") : t("返回供应商")}</Button>
        <h1 tabIndex={-1}>{draft ? selected ? t("编辑渠道组") : t("新建渠道组") : selected?.name ?? t("渠道组")}</h1>
        <p className="page-description">{t("将供应商、账号与上游模型组合为虚拟模型，在本地验证选择策略。")}</p>
      </div>
      <div className="page-actions">
        {native && <Button variant="quiet" icon={<RefreshCw size={15} />} loading={loading} disabled={busy} onClick={refresh}>{t("刷新快照")}</Button>}
        {!draft && selected && <>
          <Button variant="secondary" loading={action === selected.id} disabled={blocked || busy} onClick={() => void toggle(selected)}>{selected.enabled ? t("停用组（离线）") : t("启用组（离线）")}</Button>
          <Button icon={<Pencil size={15} />} disabled={blocked || busy} onClick={() => startDraft(selected)}>{t("编辑配置")}</Button>
        </>}
        {!draft && !selected && native && <Button icon={<Plus size={15} />} disabled={blocked || busy} onClick={() => startDraft()}>{t("新建渠道组")}</Button>}
      </div>
    </header>
    <aside className="channel-group-offline" aria-label={t("离线试点状态")}>
      <Layers3 size={19} aria-hidden="true" /><div><strong>{t("离线试点")}</strong><p>{t("启用仅参与本地选择与模拟，尚未接入实时路由。superapi 计费测试保持暂停。")}</p></div>
      <Badge tone="warning">{t("实时派发关闭")}</Badge>
    </aside>
    <div ref={errorRef} tabIndex={-1}><InlineError message={readError} onRetry={busy ? undefined : refresh} /><InlineError message={error} /></div>
    {notice && <p className="channel-group-notice" role="status">{notice}</p>}
    {!native ? <EmptyState title={t("请在桌面端打开渠道组")} description={t("浏览器预览未连接本地引擎，无法读取组配置、保存或运行模拟。这里不会生成示例引擎结果。")}/>
      : loading && !snapshot ? <LoadingState label={t("读取渠道组本地快照…")} />
      : snapshot && <>
        <div className="channel-group-snapshot-meta"><span>{t("本地离线快照：{time}", { time: channelTime(snapshot.generatedAtMs) })}</span>{selected && <Badge>{selected.enabled ? t("离线已启用") : t("离线已停用")}</Badge>}</div>
        <ChannelGroupWarnings warnings={snapshot.warnings} />
        {metadataError && <InlineError message={t("供应商目录读取失败；刷新供应商目录后再编辑渠道引用。")}/>}
        {draft ? <ChannelGroupEditor key={draft.id} draft={draft} configuration={snapshot.configuration} providers={providers} accounts={accounts} accountsError={accountsError}
          errors={errors} busy={busy} blocked={blocked} dirty={dirty} onChange={next => { setDraft(next); setErrors({}); }} onSave={() => void save()} onCancel={back} />
          : selected ? <>
            <ChannelGroupDetails group={selected} snapshot={snapshot} providers={providers} accounts={accounts} />
            <ChannelGroupSimulation key={`${selected.id}:${snapshotVersion}`} group={selected} snapshotAtMs={snapshot.generatedAtMs} providers={providers} accounts={accounts} disabled={blocked || loading || action !== null} onBusyChange={onTrialBusyChange} />
          </> : <section className="channel-group-directory" aria-label={t("渠道组列表")}>
            {!snapshot.configuration.groups.length ? <EmptyState title={t("尚未创建渠道组")} description={t("从轮询开始，添加已有供应商的模型引用，再运行固定输入的离线模拟。")}
              action={<Button variant="secondary" disabled={blocked || busy} onClick={() => startDraft()}>{t("创建第一个渠道组")}</Button>} />
              : <ul className="channel-group-list">{snapshot.configuration.groups.map(group => <li key={group.id}>
                <button type="button" className="channel-group-identity" data-group-open={group.id} disabled={busy} onClick={() => openGroup(group)}><strong>{group.name}</strong><code>{group.virtualModel}</code></button>
                <div className="channel-group-list-meta"><span>{channelStrategyLabel(group.strategy)} · {t("{count} 个渠道", { count: group.channels.length })}</span><span>{group.projectId ? t("共享项目：{id}", { id: group.projectId }) : t("未关联项目预算")}</span></div>
                <Badge>{group.enabled ? t("离线已启用") : t("离线已停用")}</Badge>
                <div className="channel-group-row-actions"><Button variant="quiet" size="sm" icon={<Pencil size={14} />} aria-label={t("编辑渠道组 {name}", { name: group.name })} disabled={blocked || busy} onClick={() => startDraft(group)}>{t("编辑")}</Button>
                  <Button variant="secondary" size="sm" loading={action === group.id} aria-label={group.enabled ? t("停用渠道组 {name}（离线）", { name: group.name }) : t("启用渠道组 {name}（离线）", { name: group.name })} disabled={blocked || busy} onClick={() => void toggle(group)}>{group.enabled ? t("停用") : t("启用")}</Button></div>
              </li>)}</ul>}
          </section>}
      </>}
    {discardOpen && <ConfirmActionDialog title={t("放弃渠道组草稿？")} message={t("未保存的渠道与共享项目预算修改将丢弃，已保存配置不会改变。")}
      confirmLabel={t("放弃修改")} busy={busy} onCancel={() => { pendingLeave.current = null; setDiscardOpen(false); }} onConfirm={() => {
        if (busy || lock.current || trialLock.current) return;
        const proceed = pendingLeave.current; pendingLeave.current = null; setDiscardOpen(false); proceed?.();
      }} />}
  </div>;
}
