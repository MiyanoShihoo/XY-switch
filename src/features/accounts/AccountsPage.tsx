import { useCallback, useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import { useLanguage, t, translateMessage } from "../../i18n/language";
import { Field, Button, Modal, Select, Input, InlineError } from "../../ui";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { usePageOperation } from "../shared/usePageOperation";
import { accountsNative, listAccounts, refreshAccount, revokeAccount, getAccountPolicy, saveAccountPolicy, DEFAULT_ACCOUNT_POLICY, accountErrorText } from "../../lib/accounts-service";
import { loadClientConnections } from "../../lib/clients-service";
import type { AccountClient, AccountInfo, AccountKind, AccountPolicy, SelectedAccountImport } from "../../types/accounts";
import { ACCOUNT_QUERY_KEYS, captureAccountDestination, clearCredentialField, loadAccountProviders, requireAccountAction, selectAccountImport, submitAccountSettings, submitApiKey, submitSelectedImport, type AccountProviderSummary } from "./account-actions";
import { accountCanRefresh, accountDestinationSnapshot, localRevocationMessage, nativeAccountScopeMessage, safeAccountLabel, validAccountAlias } from "./account-presentation";
import { AccountDetailsDialog } from "./AccountDetailsDialog";
import { AccountRow } from "./AccountRow";
import { CLIENT_CONNECTIONS_QUERY_KEY, nativeAccountCanSync, nativeAccountClientId, nativeAccountSyncCopy, submitNativeAccountSync } from "../clients/native-account-actions";
import { NativeAccountConfirmDialog } from "../clients/NativeAccountConfirmDialog";
import { accountParticipation, accountPolicyCandidates } from "./account-policy";
import "./accounts.css";

interface AccountEditor {
  mode: "add" | "import" | "edit" | "replace";
  id: string;
  expectedRevision: number;
  alias: string;
  providerId: string;
  kind: AccountKind;
  enabled: boolean;
  availableModels: string[];
  allowRefresh: boolean;
}
type PendingAction =
  | { kind: "editor"; editor: AccountEditor; approvedDestination: string }
  | { kind: "chooseImport" }
  | { kind: "policy"; providerId: string; policy: AccountPolicy }
  | { kind: "refresh"; id: string }
  | { kind: "nativeSync"; id: string; client: AccountClient | null }
  | { kind: "revoke"; id: string };

function editorTitle(mode: AccountEditor["mode"]): string {
  if (mode === "add") return t("添加 API key 账号");
  if (mode === "import") return t("导入指定账号文件");
  if (mode === "replace") return t("更新 API key");
  return t("账号设置");
}

export function accountActionConfirmation(action: PendingAction): { title: string; message: string; confirmLabel: string } {
  if (action.kind === "chooseImport") return { title: t("确认选择账号文件"), message: t("只使用你随后在文件对话框中选择的一个 JSON 文件。不扫描客户端目录，不预览文件内容或路径。"), confirmLabel: t("选择指定文件") };
  if (action.kind === "refresh") return { title: t("确认刷新凭据"), message: t("允许此账号已接入的授权刷新接口更新安全存储中的凭据。刷新不会创建新任务或预算。"), confirmLabel: t("授权刷新") };
  if (action.kind === "nativeSync") return nativeAccountSyncCopy(action);
  if (action.kind === "revoke") return { title: t("确认本地撤销"), message: t("撤销本应用的账号引用并清理本应用保存的凭据。不会撤销供应商授权、退出原生客户端或删除来源文件。"), confirmLabel: t("撤销本地引用") };
  if (action.kind === "policy") return { title: t("确认账号选择策略"), message: t("将保存此供应商的固定或轮换策略、会话亲和与冷却设置。已有任务继续使用原预算与发送账本。"), confirmLabel: t("确认保存策略") };
  const mode = action.editor.mode;
  const message = mode === "import"
    ? t("仅导入本次选定文件中的 API key 或 OAuth 凭据，绑定到所选供应商。原生客户端认证文件不能用于此导入。")
    : mode === "edit" ? t("将保存账号别名、供应商绑定与启停状态。OAuth 和原生账号保持原供应商绑定。")
    : t("允许本应用将表单中的 API key 保存到安全存储，并用于绑定供应商的请求。添加或更新本身不代表授权已验证。");
  return { title: editorTitle(mode), message: message + "\n\n"
    + t("授权请求地址：{destination}", { destination: accountDestinationSnapshot(action.approvedDestination) ?? t("请求地址不可用于账号授权") }) + "\n\n"
    + t("授权绑定确认时的完整请求地址；供应商地址改变后需重新批准。"),
    confirmLabel: mode === "edit" ? t("确认保存账号") : t("授权并保存") };
}

export function AccountPolicyFields({ policy, accounts, disabled, onChange }: {
  policy: AccountPolicy; accounts: AccountInfo[]; disabled: boolean; onChange: (policy: AccountPolicy) => void;
}) {
  useLanguage();
  return <div className="accounts-policy-fields">
    <section className="accounts-policy-section" aria-labelledby="accounts-policy-selection"><h3 id="accounts-policy-selection">{t("选择与会话")}</h3><div className="accounts-policy-grid">
    <Field label={t("选择方式")}><Select value={policy.mode} disabled={disabled} onChange={event => onChange({ ...policy, mode: event.currentTarget.value === "fixed" ? "fixed" : "rotate", fixedAccountId: null })}>
      <option value="rotate">{t("轮换账号")}</option><option value="fixed">{t("固定账号")}</option>
    </Select></Field>
    {policy.mode === "fixed" && <Field label={t("固定使用的账号")} required><Select value={policy.fixedAccountId ?? ""} disabled={disabled} onChange={event => onChange({ ...policy, fixedAccountId: event.currentTarget.value || null })}>
      <option value="">{t("请选择账号")}</option>{accounts.filter(account => account.kind !== "nativeManaged").map(account => <option key={account.id} value={account.id}>{safeAccountLabel(account.alias)}</option>)}
    </Select></Field>}
    </div>
    <label className="accounts-check"><input type="checkbox" checked={policy.affinity} disabled={disabled} onChange={event => onChange({ ...policy, affinity: event.currentTarget.checked })} /><span>{t("保持会话账号亲和")}</span></label>
    </section>
    <section className="accounts-policy-section" aria-labelledby="accounts-policy-cooldown"><h3 id="accounts-policy-cooldown">{t("失败与限流冷却")}</h3><div className="accounts-policy-grid accounts-policy-grid--cooldown">
    <Field label={t("失败冷却（秒）")}><Input type="number" min={1} max={86400} value={policy.failureCooldownSeconds} disabled={disabled} onChange={event => onChange({ ...policy, failureCooldownSeconds: Number(event.currentTarget.value) })} /></Field>
    <Field label={t("限流冷却（秒）")}><Input type="number" min={1} max={86400} value={policy.rateLimitCooldownSeconds} disabled={disabled} onChange={event => onChange({ ...policy, rateLimitCooldownSeconds: Number(event.currentTarget.value) })} /></Field>
    <Field label={t("最长冷却（秒）")}><Input type="number" min={1} max={86400} value={policy.maxCooldownSeconds} disabled={disabled} onChange={event => onChange({ ...policy, maxCooldownSeconds: Number(event.currentTarget.value) })} /></Field>
    </div></section>
  </div>;
}

export function accountPolicyValid(policy: AccountPolicy, accounts: AccountInfo[]): boolean {
  return [policy.failureCooldownSeconds, policy.rateLimitCooldownSeconds, policy.maxCooldownSeconds].every(value => Number.isSafeInteger(value) && value >= 1 && value <= 86400)
    && policy.failureCooldownSeconds <= policy.maxCooldownSeconds && policy.rateLimitCooldownSeconds <= policy.maxCooldownSeconds
    && ((policy.mode === "rotate" && policy.fixedAccountId === null) || (policy.mode === "fixed" && accounts.some(account => account.id === policy.fixedAccountId && account.kind !== "nativeManaged")));
}

export function AccountsPage() {
  useLanguage();
  const desktop = accountsNative();
  const client = useQueryClient();
  const op = usePageOperation();
  const [editor, setEditor] = useState<AccountEditor | null>(null);
  const [selectedAccountId, setSelectedAccountId] = useState<string | null>(null);
  const [policyOpen, setPolicyOpen] = useState(false);
  const [pending, setPending] = useState<PendingAction | null>(null);
  const [hasCredential, setHasCredential] = useState(false);
  const [hasSelectedSource, setHasSelectedSource] = useState(false);
  const [selectedPolicyProvider, setSelectedPolicyProvider] = useState<string | null>(null);
  const [policyDraft, setPolicyDraft] = useState<AccountPolicy | null>(null);
  const [timestamp, setTimestamp] = useState(() => Math.floor(Date.now() / 1000));
  const credential = useRef<HTMLInputElement | null>(null);
  const selection = useRef<SelectedAccountImport | null>(null);
  const mounted = useRef(true);
  const active = () => mounted.current;
  const bindCredential = useCallback((field: HTMLInputElement | null) => {
    if (credential.current !== field) clearCredentialField(credential.current);
    credential.current = field;
  }, []);
  useEffect(() => {
    mounted.current = true;
    const timer = setInterval(() => setTimestamp(Math.floor(Date.now() / 1000)), 15000);
    return () => { mounted.current = false; clearInterval(timer); clearCredentialField(credential.current); selection.current = null; };
  }, []);

  const catalog = useQuery({ queryKey: ACCOUNT_QUERY_KEYS.list, queryFn: listAccounts, enabled: desktop, retry: false });
  const providerQuery = useQuery({ queryKey: ACCOUNT_QUERY_KEYS.providers, queryFn: loadAccountProviders, enabled: desktop, retry: false });
  const accounts = catalog.data ?? [];
  const selectedAccount = accounts.find(account => account.id === selectedAccountId);
  const connections = useQuery({ queryKey: CLIENT_CONNECTIONS_QUERY_KEY, queryFn: loadClientConnections, enabled: desktop && accounts.some(account => account.kind === "nativeManaged"), retry: false });
  const nativeRows = connections.isSuccess ? connections.data : undefined;
  const providers = providerQuery.data ?? [];
  const policyProviderId = selectedPolicyProvider === null ? providers[0]?.id ?? "" : providers.some(provider => provider.id === selectedPolicyProvider) ? selectedPolicyProvider : "";
  const selectedProvider = providers.find(provider => provider.id === policyProviderId);
  const gatewayAccounts = accounts.filter(account => account.kind !== "nativeManaged");
  const nativeAccounts = accounts.filter(account => account.kind === "nativeManaged");
  const unboundAccounts = gatewayAccounts.filter(account => !providers.some(provider => provider.id === account.providerId));
  const providerAccounts = selectedProvider ? gatewayAccounts.filter(account => account.providerId === selectedProvider.id) : unboundAccounts;
  const policyQuery = useQuery({ queryKey: ACCOUNT_QUERY_KEYS.policy(policyProviderId), queryFn: () => getAccountPolicy(policyProviderId), enabled: desktop && providerQuery.isSuccess && Boolean(policyProviderId), retry: false });
  const policy = policyDraft ?? policyQuery.data ?? { ...DEFAULT_ACCOUNT_POLICY };
  const actualPolicy = catalog.isSuccess && providerQuery.isSuccess && policyQuery.isSuccess ? policyQuery.data : undefined;
  const fixedCandidates = accountPolicyCandidates(accounts, policyProviderId);
  const fixedAccount = actualPolicy?.fixedAccountId ? fixedCandidates.find(account => account.id === actualPolicy.fixedAccountId) : undefined;
  const unavailable = !desktop || op.busy || pending !== null;
  const accountWritesUnavailable = unavailable || !catalog.isSuccess;
  const settingsUnavailable = accountWritesUnavailable || !providerQuery.isSuccess;
  const providerLabel = (id: string) => safeAccountLabel(providers.find(provider => provider.id === id)?.name, t("供应商不可用"));
  const run = (work: () => Promise<void>) => op.run(async () => {
    try { await work(); } catch (error) { throw new Error(accountErrorText(error)); }
  });
  const clearEditor = () => {
    clearCredentialField(credential.current); selection.current = null;
    setHasCredential(false); setHasSelectedSource(false); setEditor(null);
  };
  const openEditor = (mode: AccountEditor["mode"], account?: AccountInfo) => {
    clearEditor(); op.setError(""); op.setMessage("");
    setEditor({ mode, id: account?.id ?? "", expectedRevision: account?.revision ?? 0, alias: account ? safeAccountLabel(account.alias) : "",
      providerId: account?.providerId ?? (selectedProvider?.enabled ? selectedProvider.id : providers.find(provider => provider.enabled)?.id ?? ""), kind: account?.kind ?? "apiKey",
      enabled: account?.enabled ?? true, availableModels: [...(account?.availableModels ?? [])], allowRefresh: false });
  };
  const remember = async (account: AccountInfo) => {
    client.setQueryData<AccountInfo[]>(ACCOUNT_QUERY_KEYS.list, previous => {
      const list = previous ?? [];
      return list.some(item => item.id === account.id) ? list.map(item => item.id === account.id ? account : item) : [...list, account];
    });
    await client.invalidateQueries({ queryKey: ACCOUNT_QUERY_KEYS.list });
  };
  const confirmPending = async () => {
    if (!pending || !active() || op.busy || pendingReadBlocked || nativeSyncBlocked) return;
    const action = pending;
    const succeeded = await (action.kind === "nativeSync" ? op.run : run)(async () => {
      requireAccountAction(true, active);
      if (action.kind === "chooseImport") {
        selection.current = null;
        if (active()) setHasSelectedSource(false);
        const selected = await selectAccountImport(true, active);
        if (active()) { selection.current = selected; setHasSelectedSource(selected !== null); }
        return;
      }
      if (action.kind === "policy") {
        const saved = await saveAccountPolicy(action.providerId, action.policy, true);
        client.setQueryData(ACCOUNT_QUERY_KEYS.policy(action.providerId), saved);
        if (active()) { setPolicyDraft(null); setPolicyOpen(false); op.setMessage(t("账号选择策略已保存。")); }
        return;
      }
      if (action.kind === "refresh") {
        try { await remember(await refreshAccount(action.id, true)); }
        catch (error) {
          await client.invalidateQueries({ queryKey: ACCOUNT_QUERY_KEYS.list });
          throw error;
        }
        if (active()) op.setMessage(t("凭据刷新已完成；额度仍以有效来源证据为准。"));
        return;
      }
      if (action.kind === "nativeSync") {
        await submitNativeAccountSync(client, action.id, true, active);
        if (active()) op.setMessage(t("原生账号状态与额度已同步；网关 HTTP 轮换不使用此引用。"));
        return;
      }
      if (action.kind === "revoke") {
        const result = await revokeAccount(action.id, true);
        await remember(result.account);
        if (active()) op.setMessage(localRevocationMessage(result));
        return;
      }
      const form = action.editor;
      let saved: AccountInfo;
      if (form.mode === "add" || form.mode === "replace") {
        try {
          saved = await submitApiKey(form.mode === "add" ? { mode: "add", alias: form.alias, providerId: form.providerId, approvedDestination: action.approvedDestination }
            : { mode: "replace", id: form.id, expectedRevision: form.expectedRevision, approvedDestination: action.approvedDestination }, credential.current, true, active);
        } finally { if (active()) setHasCredential(false); }
      } else if (form.mode === "import") {
        try { saved = await submitSelectedImport({ alias: form.alias, providerId: form.providerId, approvedDestination: action.approvedDestination, allowRefresh: form.allowRefresh }, selection, true, active); }
        finally { if (active()) setHasSelectedSource(false); }
      } else {
        saved = await submitAccountSettings({ id: form.id, expectedRevision: form.expectedRevision, alias: form.alias, providerId: form.providerId,
          approvedDestination: action.approvedDestination, enabled: form.enabled, availableModels: form.availableModels }, true, active);
      }
      await remember(saved);
      if (active()) op.setMessage(t("账号已保存；授权与额度状态以列表中的实际证据为准。"));
    });
    if (active()) {
      if (succeeded || action.kind === "editor") setPending(null);
      if (succeeded && action.kind === "editor") clearEditor();
    }
  };
  const requestImport = () => { openEditor("import"); setPending({ kind: "chooseImport" }); };
  const editorProvider = editor ? providers.find(provider => provider.id === editor.providerId) : undefined;
  const editorValid = editor !== null && validAccountAlias(editor.alias) && Boolean(editorProvider?.endpoint)
    && (editor.mode === "edit" || editorProvider!.enabled)
    && (editor.mode === "add" || editor.mode === "replace" ? hasCredential : editor.mode === "import" ? hasSelectedSource : true);
  const pendingCopy = pending ? accountActionConfirmation(pending) : null;
  const PendingDialog = pending?.kind === "nativeSync" ? NativeAccountConfirmDialog : ConfirmActionDialog;
  const pendingNativeAccount = pending?.kind === "nativeSync" ? accounts.find(account => account.id === pending.id) : undefined;
  const nativeSyncBlocked = pending?.kind === "nativeSync" && (!catalog.isSuccess || !pendingNativeAccount || !nativeAccountCanSync(pendingNativeAccount, nativeRows));
  const pendingReadBlocked = pending !== null && (!catalog.isSuccess
    || ((pending.kind === "editor" || pending.kind === "chooseImport" || pending.kind === "policy") && !providerQuery.isSuccess)
    || (pending.kind === "policy" && !policyQuery.isSuccess));
  const rowActions = (account: AccountInfo) => <>
    <Button variant="quiet" size="sm" aria-label={t("设置账号 {account}", { account: safeAccountLabel(account.alias) })} aria-haspopup="dialog"
      disabled={settingsUnavailable || account.status === "revoked"} onClick={() => openEditor("edit", account)}>{t("设置")}</Button>
    {account.kind === "oauth" && <Button variant="secondary" size="sm" aria-label={t("刷新账号 {account} 的凭据", { account: safeAccountLabel(account.alias) })} aria-haspopup="dialog"
      disabled={accountWritesUnavailable || !accountCanRefresh(account, timestamp)} onClick={() => { op.setError(""); setPending({ kind: "refresh", id: account.id }); }}>{t("刷新凭据")}</Button>}
    {account.kind === "nativeManaged" && nativeAccountClientId(account.client) && <Button variant="secondary" size="sm" aria-label={t("同步账号 {account} 的原生状态", { account: safeAccountLabel(account.alias) })} aria-haspopup="dialog"
      disabled={accountWritesUnavailable || !nativeAccountCanSync(account, nativeRows)} onClick={() => { op.setError(""); setPending({ kind: "nativeSync", id: account.id, client: account.client }); }}>{t("同步状态")}</Button>}
  </>;

  return <div className="workspace-page accounts-page">
    <header className="page-heading"><div className="page-heading__copy"><h1>{t("账号池")}</h1><p className="page-description">{t("按供应商核对实际策略、参与账号与额度；原生引用单独管理。")}</p></div>
      <div className="page-actions">
        <Button variant="quiet" aria-label={t("刷新账号列表")} title={t("刷新账号列表")} disabled={unavailable} loading={catalog.isFetching || providerQuery.isFetching || connections.isFetching} onClick={() => void run(async () => { await Promise.all([catalog.refetch({ throwOnError: true }), providerQuery.refetch({ throwOnError: true }), client.invalidateQueries({ queryKey: CLIENT_CONNECTIONS_QUERY_KEY }), client.invalidateQueries({ queryKey: ["accounts", "policy"] })]); })}><RefreshCw size={16} aria-hidden="true" /></Button>
        <Button variant="secondary" disabled={settingsUnavailable || !providers.some(provider => provider.enabled)} onClick={requestImport}>{t("导入指定文件")}</Button>
        <Button disabled={settingsUnavailable || !providers.some(provider => provider.enabled)} onClick={() => openEditor("add")}>{t("添加 API key")}</Button>
      </div>
    </header>
    {!desktop && <p className="workspace-route-notice" role="status">{t("账号池操作需要桌面应用。")}</p>}
    <InlineError message={op.error || (catalog.error ? accountErrorText(catalog.error) : providerQuery.error ? accountErrorText(providerQuery.error) : "")} />
    {op.message && <p className="accounts-message" role="status">{translateMessage(op.message)}</p>}
    <section aria-label={t("账号元数据列表")} className="accounts-catalog">
      {catalog.isError && <p className="accounts-notice" role="status">{t("账号读取失败，当前记录仅供查看。请刷新后再操作。")}</p>}
      {desktop && catalog.isPending && <p role="status">{t("正在读取账号元数据…")}</p>}
      {!catalog.isPending && !catalog.error && accounts.length === 0 && <p className="accounts-empty">{t("尚未添加账号。请授权添加 API key 或选择具体导入文件。")}</p>}
      <section className="accounts-group" data-kind="gateway" aria-labelledby="accounts-group-gateway">
        <div className="accounts-group__heading"><h2 id="accounts-group-gateway">{t("网关请求账号")}</h2><span>{t("{count} 个账号", { count: gatewayAccounts.length })}</span></div>
        <p className="accounts-scope-note">{t("参与关系按已保存策略显示；实际请求仍受授权、额度与冷却状态约束。")}</p>
        {!providerQuery.isSuccess ? <p className="accounts-empty" role="status">{t("供应商归属待确认")}</p> : <div className="accounts-provider-workspace" id="accounts-provider-workspace">
          <div className="accounts-provider-selector"><Field label={t("供应商与账号")}><Select aria-label={t("供应商与账号")} value={policyProviderId} disabled={unavailable || policyOpen || editor !== null} onChange={event => { setSelectedPolicyProvider(event.currentTarget.value); setPolicyDraft(null); }}>
            {providers.map(provider => <option key={provider.id} value={provider.id}>{safeAccountLabel(provider.name, t("供应商"))} · {t("{count} 个账号", { count: gatewayAccounts.filter(account => account.providerId === provider.id).length })}{!provider.enabled && " · " + t("已停用")}</option>)}
            <option value="">{t("未绑定供应商")} · {t("{count} 个账号", { count: unboundAccounts.length })}</option>
          </Select></Field></div>
          {selectedProvider ? <>
            <section className="accounts-actual-policy" aria-label={t("实际账号选择策略")} aria-busy={policyQuery.isFetching}>
              <p className="accounts-policy-summary"><span>{t("已保存策略")}</span>{actualPolicy ? <>
                <strong>{actualPolicy.mode === "rotate" ? t("轮换账号") : t("固定账号")}</strong>
                <span>{actualPolicy.mode === "rotate" ? t("{count} 个轮换候选", { count: fixedCandidates.length }) : fixedAccount ? safeAccountLabel(fixedAccount.alias) : t("固定账号已不可用")}</span>
              </> : <span role="status">{!catalog.isSuccess ? t("账号状态待刷新") : policyQuery.isError ? t("策略读取失败") : t("正在读取账号策略…")}</span>}</p>
              <div className="accounts-actions">
                {policyQuery.isError && <Button variant="quiet" size="sm" disabled={unavailable || policyQuery.isFetching} onClick={() => void policyQuery.refetch()}>{t("重试读取策略")}</Button>}
                <Button variant="quiet" size="sm" aria-label={t("账号选择策略")} aria-haspopup="dialog" disabled={settingsUnavailable || !actualPolicy} onClick={() => { op.setError(""); setPolicyDraft(null); setPolicyOpen(true); }}>{t("编辑策略")}</Button>
              </div>
            </section>
            {policyQuery.isError && <InlineError message={accountErrorText(policyQuery.error)} />}
            {actualPolicy?.mode === "fixed" && !fixedAccount && <p className="accounts-notice accounts-policy-notice">{t("固定账号归属不匹配、已停用或已不存在；请重新选择。")}</p>}
            {!selectedProvider.enabled && <p className="accounts-notice accounts-policy-notice">{t("供应商已停用；此处保留账号策略与额度记录。")}</p>}
          </> : <p className="accounts-scope-note">{t("未绑定或供应商已不存在的账号单独保留；不会推断归属或参与其他供应商策略。")}</p>}
            {providerAccounts.length > 0 ? <>
              <div className="accounts-catalog__columns" aria-hidden="true"><div className="accounts-catalog__labels"><span>{t("账号 / 供应商")}</span><span>{t("授权、状态与参与关系")}</span><span>{t("额度证据")}</span><span /></div><span className="accounts-catalog__actions">{t("常用操作")}</span></div>
              <ul className="accounts-list">{providerAccounts.map(account => <AccountRow key={account.id} account={account} providerLabel={providerLabel(account.providerId)} participation={catalog.isSuccess ? t(accountParticipation(account, policyProviderId, actualPolicy)) : t("账号状态待刷新")} nativeRows={nativeRows} timestamp={timestamp}
                busy={op.busy || pending !== null} selected={selectedAccountId === account.id} actions={rowActions(account)} onView={() => { op.setError(""); setSelectedAccountId(account.id); }} />)}</ul>
            </> : <div className="accounts-provider-empty"><p>{catalog.isSuccess ? selectedProvider ? t("此供应商尚无网关账号。") : t("暂无未绑定账号。") : t("账号状态待刷新")}</p>{selectedProvider?.enabled && <Button variant="secondary" disabled={settingsUnavailable} onClick={() => openEditor("add")}>{t("添加到此供应商")}</Button>}</div>}
        </div>}
      </section>
      {nativeAccounts.length > 0 && <section className="accounts-group accounts-native-group" data-kind="native" aria-labelledby="accounts-group-native">
        <div className="accounts-group__heading"><h2 id="accounts-group-native">{t("原生客户端引用")}</h2><span>{t("{count} 个账号", { count: nativeAccounts.length })}</span></div>
        <p className="accounts-scope-note">{nativeAccountScopeMessage()}</p>
        <div className="accounts-catalog__columns" aria-hidden="true"><div className="accounts-catalog__labels"><span>{t("账号 / 供应商")}</span><span>{t("授权与状态")}</span><span>{t("额度证据")}</span><span /></div><span className="accounts-catalog__actions">{t("常用操作")}</span></div>
        <ul className="accounts-list">{nativeAccounts.map(account => <AccountRow key={account.id} account={account} providerLabel={providerLabel(account.providerId)} nativeRows={nativeRows} timestamp={timestamp}
          busy={op.busy || pending !== null} selected={selectedAccountId === account.id} actions={rowActions(account)} onView={() => { op.setError(""); setSelectedAccountId(account.id); }} />)}</ul>
      </section>}
    </section>
    {selectedAccount && !editor && <AccountDetailsDialog key={selectedAccount.id} account={selectedAccount} providerLabel={providerLabel(selectedAccount.providerId)} nativeRows={nativeRows} timestamp={timestamp}
      disabled={accountWritesUnavailable} settingsDisabled={settingsUnavailable} accountsReady={catalog.isSuccess} busy={op.busy || pending !== null} error={op.error}
      onClose={() => setSelectedAccountId(null)} onEdit={mode => openEditor(mode, selectedAccount)}
      onRefresh={() => { op.setError(""); setPending({ kind: "refresh", id: selectedAccount.id }); }}
      onNativeSync={() => { op.setError(""); setPending({ kind: "nativeSync", id: selectedAccount.id, client: selectedAccount.client }); }}
      onRevoke={() => { op.setError(""); setPending({ kind: "revoke", id: selectedAccount.id }); }} />}
    {policyOpen && <div className="accounts-dialog"><Modal open size="lg" title={t("供应商账号选择策略")} busy={op.busy || pending !== null} onClose={() => { setPolicyOpen(false); setPolicyDraft(null); }} footer={<>
      <Button variant="secondary" disabled={unavailable} onClick={() => { setPolicyOpen(false); setPolicyDraft(null); }}>{t("取消")}</Button>
      <Button disabled={settingsUnavailable || !policyQuery.isSuccess || !accountPolicyValid(policy, fixedCandidates)} onClick={() => { op.setError(""); setPending({ kind: "policy", providerId: policyProviderId, policy: { ...policy } }); }}>{t("保存账号策略")}</Button>
    </>}>
      <div className="accounts-policy">
        <p className="accounts-detail-note">{t("会话亲和在账号可用时保持绑定；失败、限流或可信额度耗尽后按策略处理，轮换复用原任务预算。")}</p>
        <Field label={t("策略供应商")}><Select value={policyProviderId} disabled={settingsUnavailable || !providers.length} onChange={event => { setSelectedPolicyProvider(event.currentTarget.value); setPolicyDraft(null); }}>
          {!providers.length && <option value="">{t("暂无供应商")}</option>}{providers.map(provider => <option key={provider.id} value={provider.id}>{safeAccountLabel(provider.name, t("供应商"))}{provider.enabled ? "" : " · " + t("已停用")}</option>)}
        </Select></Field>
        {policyQuery.isFetching && <p role="status">{t("正在读取账号策略…")}</p>}
        <InlineError message={op.error || (policyQuery.error ? accountErrorText(policyQuery.error) : "")} />
        {(!catalog.isSuccess || !providerQuery.isSuccess) && <p className="accounts-notice" role="status">{t("账号或供应商读取失败，请关闭弹窗并刷新后再操作。")}</p>}
        <AccountPolicyFields policy={policy} accounts={fixedCandidates} disabled={settingsUnavailable || !policyQuery.isSuccess} onChange={setPolicyDraft} />
      </div>
    </Modal></div>}
    {editor && <div className="accounts-dialog"><Modal open title={editorTitle(editor.mode)} busy={op.busy} onClose={() => { if (!op.busy && !pending) clearEditor(); }} footer={<>
      <Button variant="secondary" disabled={op.busy || pending !== null} onClick={clearEditor}>{t("取消")}</Button>
      <Button disabled={settingsUnavailable || !editorValid} onClick={() => {
        op.setError("");
        try { setPending({ kind: "editor", editor: { ...editor, availableModels: [...editor.availableModels] }, approvedDestination: captureAccountDestination(editorProvider) }); }
        catch (error) { op.setError(accountErrorText(error)); }
      }}>{t("检查并确认")}</Button>
    </>}>
      <div className="accounts-editor">
        {(!catalog.isSuccess || !providerQuery.isSuccess) && <p className="accounts-notice" role="status">{t("账号或供应商读取失败，请关闭弹窗并刷新后再操作。")}</p>}
        <div className="accounts-editor-section">
        <Field label={t("账号别名")} description={t("使用不含邮箱或凭据的短名称。别名会保存在本地元数据中。")} required>
          <Input value={editor.alias} maxLength={80} disabled={settingsUnavailable || editor.mode === "replace"} onChange={event => setEditor({ ...editor, alias: event.currentTarget.value })} />
        </Field>
        <Field label={t("绑定供应商")} description={editor.kind !== "apiKey" ? t("OAuth 与原生账号不能跨供应商换绑。") : undefined} required>
          <Select value={editor.providerId} disabled={settingsUnavailable || editor.mode === "replace" || (editor.mode === "edit" && editor.kind !== "apiKey")} onChange={event => setEditor({ ...editor, providerId: event.currentTarget.value })}>
            <option value="">{t("请选择供应商")}</option>{providers.map((provider: AccountProviderSummary) => <option key={provider.id} value={provider.id} disabled={!provider.enabled && provider.id !== editor.providerId}>{safeAccountLabel(provider.name, t("供应商"))}{provider.enabled ? "" : " · " + t("已停用")}</option>)}
          </Select>
        </Field>
        <dl className="accounts-destination"><dt>{t("授权请求地址")}</dt><dd>{accountDestinationSnapshot(editorProvider?.endpoint) ?? t("请求地址不可用于账号授权")}</dd></dl>
        <p className="accounts-detail-note">{t("授权绑定确认时的完整请求地址；供应商地址改变后需重新批准。")}</p>
        </div>
        <div className="accounts-editor-section">
        {(editor.mode === "add" || editor.mode === "replace") && <Field label={t("API key")} description={t("凭据只保留在当前表单内存中，提交结束、取消或离开页面时清除，不提供预览。")} required>
          <Input ref={bindCredential} type="password" autoComplete="off" spellCheck={false} disabled={settingsUnavailable} onChange={event => setHasCredential(Boolean(event.currentTarget.value))} />
        </Field>}
        {editor.mode === "edit" && <label className="accounts-check"><input type="checkbox" checked={editor.enabled} disabled={settingsUnavailable} onChange={event => setEditor({ ...editor, enabled: event.currentTarget.checked })} /><span>{editor.kind === "nativeManaged" ? t("启用原生账号状态与额度同步") : t("允许账号参与选择")}</span></label>}
        {editor.kind === "nativeManaged" && <p className="accounts-detail-note">{nativeAccountScopeMessage()}</p>}
        {editor.mode === "import" && <>
          <p className="accounts-editor__source" role="status">{hasSelectedSource ? t("已选择一个文件；内容与路径不展示，票据仅可使用一次。") : t("尚未选择文件或票据已使用，请重新选择具体文件。")}</p>
          <p className="accounts-detail-note">{t("仅支持本应用 schemaVersion: 1 的 API key / OAuth JSON，文件上限 32 KiB。原生认证文件不支持。")}</p>
          <Button variant="secondary" disabled={settingsUnavailable} onClick={() => { op.setError(""); setPending({ kind: "chooseImport" }); }}>{t("重新选择文件")}</Button>
          <label className="accounts-check"><input type="checkbox" checked={editor.allowRefresh} disabled={settingsUnavailable} onChange={event => setEditor({ ...editor, allowRefresh: event.currentTarget.checked })} /><span>{t("允许已授权刷新接口更新导入的 OAuth 账号")}</span></label>
          <p className="accounts-detail-note">{t("此选项仅适用于含 refresh token 的 OAuth 文件；不会创建登录或推理授权。")}</p>
          <section className="accounts-authorization-note" aria-label={t("授权能力边界")}><h3>{t("授权能力边界")}</h3>
            <p>{t("OAuth 需要合法应用注册及推理授权。当前面板仅导入明确授权的 OAuth 凭据；PKCE 登录和 SIWC 订阅推理尚待接入。")}</p>
            <p>{t("刷新仅在已注册的授权接口可用时启用。原生客户端令牌不会导出或移用于订阅推理。")}</p>
          </section>
        </>}
        </div>
        <InlineError message={op.error} />
      </div>
    </Modal></div>}
    {pending && pendingCopy && <div className="accounts-dialog accounts-confirm"><PendingDialog title={pendingCopy.title} message={pendingCopy.message} confirmLabel={pendingCopy.confirmLabel} busy={op.busy} disabled={pendingReadBlocked || nativeSyncBlocked} error={pendingReadBlocked ? t("账号、供应商或策略尚未成功读取，请取消并刷新后再确认。") : nativeSyncBlocked ? t("原生账号暂不能同步，请检查登录进度、启用状态与目的地授权。") : op.error}
      onCancel={() => { clearCredentialField(credential.current); setHasCredential(false); op.setError(""); setPending(null); }} onConfirm={() => void confirmPending()} /></div>}
  </div>;
}
