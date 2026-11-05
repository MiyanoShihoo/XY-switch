import { useCallback, useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useLanguage, t, translateMessage } from "../../i18n/language";
import { Card, Field, Button, Modal, Select, Input, InlineError } from "../../ui";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { usePageOperation } from "../shared/usePageOperation";
import { accountsNative, listAccounts, refreshAccount, revokeAccount, getAccountPolicy, saveAccountPolicy, DEFAULT_ACCOUNT_POLICY, accountErrorText } from "../../lib/accounts-service";
import { loadClientConnections } from "../../lib/clients-service";
import type { AccountClient, AccountInfo, AccountKind, AccountPolicy, SelectedAccountImport } from "../../types/accounts";
import { ACCOUNT_QUERY_KEYS, captureAccountDestination, clearCredentialField, loadAccountProviders, requireAccountAction, selectAccountImport, submitAccountSettings, submitApiKey, submitSelectedImport, type AccountProviderSummary } from "./account-actions";
import { AccountQuotaView, accountDestinationLabel, accountStatusLabel, authorizationLabel, accountKindLabel, accountTime, accountCanRefresh, localRevocationMessage, nativeAccountScopeMessage, safeAccountLabel, validAccountAlias } from "./account-presentation";
import { CLIENT_CONNECTIONS_QUERY_KEY, nativeAccountCanSync, nativeAccountClientId, nativeAccountSyncCopy, nativeClientActionIssue, submitNativeAccountSync } from "../clients/native-account-actions";
import { NativeAccountConfirmDialog } from "../clients/NativeAccountConfirmDialog";
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

function confirmation(action: PendingAction): { title: string; message: string; confirmLabel: string } {
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
  return { title: editorTitle(mode), message: message + " "
    + t("授权请求地址：{destination}", { destination: action.editor.kind === "nativeManaged" ? action.approvedDestination : accountDestinationLabel(action.approvedDestination) }) + " "
    + t("授权绑定确认时的完整请求地址；供应商地址改变后需重新批准。"),
    confirmLabel: mode === "edit" ? t("确认保存账号") : t("授权并保存") };
}

export function AccountPolicyFields({ policy, accounts, disabled, onChange }: {
  policy: AccountPolicy; accounts: AccountInfo[]; disabled: boolean; onChange: (policy: AccountPolicy) => void;
}) {
  useLanguage();
  return <div className="accounts-policy-fields">
    <Field label={t("选择方式")}><Select value={policy.mode} disabled={disabled} onChange={event => onChange({ ...policy, mode: event.currentTarget.value === "fixed" ? "fixed" : "rotate", fixedAccountId: null })}>
      <option value="rotate">{t("轮换账号")}</option><option value="fixed">{t("固定账号")}</option>
    </Select></Field>
    {policy.mode === "fixed" && <Field label={t("固定使用的账号")} required><Select value={policy.fixedAccountId ?? ""} disabled={disabled} onChange={event => onChange({ ...policy, fixedAccountId: event.currentTarget.value || null })}>
      <option value="">{t("请选择账号")}</option>{accounts.filter(account => account.kind !== "nativeManaged").map(account => <option key={account.id} value={account.id}>{safeAccountLabel(account.alias)}</option>)}
    </Select></Field>}
    <label className="accounts-check"><input type="checkbox" checked={policy.affinity} disabled={disabled} onChange={event => onChange({ ...policy, affinity: event.currentTarget.checked })} /><span>{t("保持会话账号亲和")}</span></label>
    <Field label={t("失败冷却（秒）")}><Input type="number" min={1} max={86400} value={policy.failureCooldownSeconds} disabled={disabled} onChange={event => onChange({ ...policy, failureCooldownSeconds: Number(event.currentTarget.value) })} /></Field>
    <Field label={t("限流冷却（秒）")}><Input type="number" min={1} max={86400} value={policy.rateLimitCooldownSeconds} disabled={disabled} onChange={event => onChange({ ...policy, rateLimitCooldownSeconds: Number(event.currentTarget.value) })} /></Field>
    <Field label={t("最长冷却（秒）")}><Input type="number" min={1} max={86400} value={policy.maxCooldownSeconds} disabled={disabled} onChange={event => onChange({ ...policy, maxCooldownSeconds: Number(event.currentTarget.value) })} /></Field>
  </div>;
}

export function accountPolicyValid(policy: AccountPolicy, accounts: AccountInfo[]): boolean {
  return [policy.failureCooldownSeconds, policy.rateLimitCooldownSeconds, policy.maxCooldownSeconds].every(value => Number.isSafeInteger(value) && value >= 1 && value <= 86400)
    && policy.failureCooldownSeconds <= policy.maxCooldownSeconds && policy.rateLimitCooldownSeconds <= policy.maxCooldownSeconds
    && ((policy.mode === "rotate" && policy.fixedAccountId === null) || (policy.mode === "fixed" && accounts.some(account => account.id === policy.fixedAccountId && account.kind !== "nativeManaged")));
}

export function AccountsPage() {
  const language = useLanguage();
  const desktop = accountsNative();
  const client = useQueryClient();
  const op = usePageOperation();
  const [editor, setEditor] = useState<AccountEditor | null>(null);
  const [pending, setPending] = useState<PendingAction | null>(null);
  const [hasCredential, setHasCredential] = useState(false);
  const [hasSelectedSource, setHasSelectedSource] = useState(false);
  const [selectedPolicyProvider, setSelectedPolicyProvider] = useState("");
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
  const connections = useQuery({ queryKey: CLIENT_CONNECTIONS_QUERY_KEY, queryFn: loadClientConnections, enabled: desktop && accounts.some(account => account.kind === "nativeManaged"), retry: false });
  const nativeRows = connections.isSuccess ? connections.data : undefined;
  const providers = providerQuery.data ?? [];
  const policyProviderId = selectedPolicyProvider || providers[0]?.id || "";
  const policyQuery = useQuery({ queryKey: ACCOUNT_QUERY_KEYS.policy(policyProviderId), queryFn: () => getAccountPolicy(policyProviderId), enabled: desktop && Boolean(policyProviderId), retry: false });
  const policy = policyDraft ?? policyQuery.data ?? { ...DEFAULT_ACCOUNT_POLICY };
  const fixedCandidates = accounts.filter(account => account.providerId === policyProviderId && account.kind !== "nativeManaged" && account.enabled && account.status !== "revoked");
  const unavailable = !desktop || op.busy || pending !== null;
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
      providerId: account?.providerId ?? providers.find(provider => provider.enabled)?.id ?? "", kind: account?.kind ?? "apiKey",
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
    if (!pending || !active()) return;
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
        if (active()) { setPolicyDraft(null); op.setMessage(t("账号选择策略已保存。")); }
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
  const pendingCopy = pending ? confirmation(pending) : null;
  const PendingDialog = pending?.kind === "nativeSync" ? NativeAccountConfirmDialog : ConfirmActionDialog;
  const pendingNativeAccount = pending?.kind === "nativeSync" ? accounts.find(account => account.id === pending.id) : undefined;
  const nativeSyncBlocked = pending?.kind === "nativeSync" && (!catalog.isSuccess || !pendingNativeAccount || !nativeAccountCanSync(pendingNativeAccount, nativeRows));

  return <div className="workspace-page accounts-page">
    <header className="page-heading"><div className="page-heading__copy"><h1>{t("账号池")}</h1><p className="page-description">{t("管理明确授权的账号引用、供应商绑定与选择策略。列表仅展示元数据。")}</p></div>
      <div className="page-actions">
        <Button variant="secondary" disabled={unavailable} loading={catalog.isFetching || providerQuery.isFetching || connections.isFetching} onClick={() => void run(async () => { await Promise.all([catalog.refetch({ throwOnError: true }), providerQuery.refetch({ throwOnError: true }), client.invalidateQueries({ queryKey: CLIENT_CONNECTIONS_QUERY_KEY })]); })}>{t("刷新账号列表")}</Button>
        <Button variant="secondary" disabled={unavailable || !providers.some(provider => provider.enabled)} onClick={requestImport}>{t("导入指定文件")}</Button>
        <Button disabled={unavailable || !providers.some(provider => provider.enabled)} onClick={() => openEditor("add")}>{t("添加 API key")}</Button>
      </div>
    </header>
    {!desktop && <Card role="status">{t("账号池操作需要桌面应用。")}</Card>}
    <InlineError message={op.error || (catalog.error ? accountErrorText(catalog.error) : providerQuery.error ? accountErrorText(providerQuery.error) : "")} />
    {op.message && <p className="accounts-message" role="status">{translateMessage(op.message)}</p>}
    <Card className="accounts-boundaries"><h2>{t("授权能力边界")}</h2>
      <p>{nativeAccountScopeMessage()}</p>
      <p>{t("OAuth 需要合法应用注册及推理授权。当前面板仅导入明确授权的 OAuth 凭据；PKCE 登录和 SIWC 订阅推理尚待接入。")}</p>
      <p>{t("刷新仅在已注册的授权接口可用时启用。原生客户端令牌不会导出或移用于订阅推理。")}</p>
    </Card>
    <section aria-label={t("账号元数据列表")} className="accounts-catalog">
      {desktop && catalog.isPending && <p role="status">{t("正在读取账号元数据…")}</p>}
      {!catalog.isPending && !catalog.error && accounts.length === 0 && <Card>{t("尚未添加账号。请授权添加 API key 或选择具体导入文件。")}</Card>}
      {accounts.map(account => <Card key={account.id} className="accounts-account">
        <div className="accounts-account__heading"><h2>{safeAccountLabel(account.alias)}</h2><span className="accounts-status" data-status={account.status}>{accountStatusLabel(account.status, account.kind)}</span></div>
        <dl className="accounts-metadata">
          <div><dt>{t("凭据类型")}</dt><dd>{accountKindLabel(account.kind)}</dd></div>
          <div><dt>{t("绑定供应商")}</dt><dd>{providerLabel(account.providerId)}</dd></div>
          <div><dt>{t("授权状态")}</dt><dd>{authorizationLabel(account.authorizationStatus)}</dd></div>
          <div><dt>{t("请求地址授权")}</dt><dd>{account.destinationApproved === true ? t("当前请求地址已批准") : t("请求地址待批准")}</dd></div>
          <div><dt>{t("来源")}</dt><dd>{account.source === "selectedFile" ? t("用户指定文件") : account.source === "nativeClient" ? t("原生客户端") : t("手动添加")}</dd></div>
          <div><dt>{t("凭据有效期")}</dt><dd>{accountTime(account.expiresAt, language)}</dd></div>
          <div><dt>{t("冷却截止")}</dt><dd>{account.cooldownUntil !== null && account.cooldownUntil > timestamp ? accountTime(account.cooldownUntil, language) : t("无有效冷却")}</dd></div>
        </dl>
        <AccountQuotaView quota={account.quota} timestamp={timestamp} />
        {account.destinationApproved !== true && account.status !== "revoked" && <p>{t("请求地址待批准；请在账号设置中检查供应商并重新确认授权。")}</p>}
        {account.kind === "nativeManaged" && <>
          <p>{nativeAccountScopeMessage()}</p>
          <p>{t("请在对应原生客户端完成登录或更新授权。")}</p>
          {nativeAccountClientId(account.client) && nativeClientActionIssue(nativeAccountClientId(account.client)!, nativeRows) && <p role="status">{nativeClientActionIssue(nativeAccountClientId(account.client)!, nativeRows)}</p>}
        </>}
        {account.kind === "oauth" && !account.canRefresh && account.status !== "revoked" && <p>{t("此账号尚无可用的授权刷新接口，请重新授权或等待客户端接入。")}</p>}
        {account.cleanupPending && <p role="status">{account.status === "revoked"
          ? t("安全存储清理待完成；本地引用保持撤销，可重试清理。")
          : t("旧凭据的安全存储清理待完成；当前账号引用仍有效。")}</p>}
        <div className="accounts-actions">
          <Button variant="secondary" size="sm" disabled={unavailable || account.status === "revoked"} onClick={() => openEditor("edit", account)}>{account.destinationApproved === true ? t("账号设置") : t("重新批准请求地址")}</Button>
          {account.kind === "apiKey" && <Button variant="secondary" size="sm" disabled={unavailable || account.status === "revoked"} onClick={() => openEditor("replace", account)}>{t("更新 API key")}</Button>}
          {account.kind === "oauth" && <Button variant="secondary" size="sm" disabled={unavailable || !accountCanRefresh(account, timestamp)} onClick={() => { op.setError(""); setPending({ kind: "refresh", id: account.id }); }}>{t("刷新凭据")}</Button>}
          {account.kind === "nativeManaged" && nativeAccountClientId(account.client) && <Button variant="secondary" size="sm" disabled={unavailable || !catalog.isSuccess || !nativeAccountCanSync(account, nativeRows)} onClick={() => { op.setError(""); setPending({ kind: "nativeSync", id: account.id, client: account.client }); }}>{account.client === "codexDesktop" || account.client === "codexCli" ? t("通过 Codex 刷新") : t("同步 OpenCode 状态")}</Button>}
          <Button variant="danger" size="sm" disabled={unavailable || (account.status === "revoked" && !account.cleanupPending)} onClick={() => { op.setError(""); setPending({ kind: "revoke", id: account.id }); }}>{account.status === "revoked" && account.cleanupPending ? t("重试本地清理") : t("撤销本地引用")}</Button>
        </div>
      </Card>)}
    </section>
    <Card className="accounts-policy"><h2>{t("供应商账号选择策略")}</h2><p>{t("会话亲和在账号可用时保持绑定；失败、限流或可信额度耗尽后按策略处理，轮换复用原任务预算。")}</p>
      <Field label={t("策略供应商")}><Select value={policyProviderId} disabled={unavailable || !providers.length} onChange={event => { setSelectedPolicyProvider(event.currentTarget.value); setPolicyDraft(null); }}>
        {!providers.length && <option value="">{t("暂无供应商")}</option>}{providers.map(provider => <option key={provider.id} value={provider.id}>{safeAccountLabel(provider.name, t("供应商"))}{provider.enabled ? "" : " · " + t("已停用")}</option>)}
      </Select></Field>
      {policyQuery.error && <InlineError message={accountErrorText(policyQuery.error)} />}
      <AccountPolicyFields policy={policy} accounts={fixedCandidates} disabled={unavailable || !policyQuery.isSuccess} onChange={setPolicyDraft} />
      <Button disabled={unavailable || !policyQuery.isSuccess || !accountPolicyValid(policy, fixedCandidates)} onClick={() => { op.setError(""); setPending({ kind: "policy", providerId: policyProviderId, policy: { ...policy } }); }}>{t("保存账号策略")}</Button>
    </Card>
    {editor && <Modal open title={editorTitle(editor.mode)} busy={op.busy} onClose={() => { if (!op.busy && !pending) clearEditor(); }} footer={<>
      <Button variant="secondary" disabled={op.busy || pending !== null} onClick={clearEditor}>{t("取消")}</Button>
      <Button disabled={unavailable || !editorValid} onClick={() => {
        op.setError("");
        try { setPending({ kind: "editor", editor: { ...editor, availableModels: [...editor.availableModels] }, approvedDestination: captureAccountDestination(editorProvider) }); }
        catch (error) { op.setError(accountErrorText(error)); }
      }}>{t("检查并确认")}</Button>
    </>}>
      <div className="accounts-editor">
        <Field label={t("账号别名")} description={t("使用不含邮箱或凭据的短名称。别名会保存在本地元数据中。")} required>
          <Input value={editor.alias} maxLength={80} disabled={unavailable || editor.mode === "replace"} onChange={event => setEditor({ ...editor, alias: event.currentTarget.value })} />
        </Field>
        <Field label={t("绑定供应商")} description={editor.kind !== "apiKey" ? t("OAuth 与原生账号不能跨供应商换绑。") : undefined} required>
          <Select value={editor.providerId} disabled={unavailable || editor.mode === "replace" || (editor.mode === "edit" && editor.kind !== "apiKey")} onChange={event => setEditor({ ...editor, providerId: event.currentTarget.value })}>
            <option value="">{t("请选择供应商")}</option>{providers.map((provider: AccountProviderSummary) => <option key={provider.id} value={provider.id} disabled={!provider.enabled && provider.id !== editor.providerId}>{safeAccountLabel(provider.name, t("供应商"))}{provider.enabled ? "" : " · " + t("已停用")}</option>)}
          </Select>
        </Field>
        <p>{t("授权请求地址：{destination}", { destination: editor.kind === "nativeManaged" ? editorProvider?.endpoint || t("请求地址不可用于账号授权") : accountDestinationLabel(editorProvider?.endpoint) })}</p>
        <p>{t("授权绑定确认时的完整请求地址；供应商地址改变后需重新批准。")}</p>
        {(editor.mode === "add" || editor.mode === "replace") && <Field label={t("API key")} description={t("凭据只保留在当前表单内存中，提交结束、取消或离开页面时清除，不提供预览。")} required>
          <Input ref={bindCredential} type="password" autoComplete="off" spellCheck={false} disabled={unavailable} onChange={event => setHasCredential(Boolean(event.currentTarget.value))} />
        </Field>}
        {editor.mode === "edit" && <label className="accounts-check"><input type="checkbox" checked={editor.enabled} disabled={unavailable} onChange={event => setEditor({ ...editor, enabled: event.currentTarget.checked })} /><span>{editor.kind === "nativeManaged" ? t("启用原生账号状态与额度同步") : t("允许账号参与选择")}</span></label>}
        {editor.kind === "nativeManaged" && <p>{nativeAccountScopeMessage()}</p>}
        {editor.mode === "import" && <>
          <p>{hasSelectedSource ? t("已选择一个文件；内容与路径不展示，票据仅可使用一次。") : t("尚未选择文件或票据已使用，请重新选择具体文件。")}</p>
          <p>{t("仅支持本应用 schemaVersion: 1 的 API key / OAuth JSON，文件上限 32 KiB。原生认证文件不支持。")}</p>
          <Button variant="secondary" disabled={unavailable} onClick={() => { op.setError(""); setPending({ kind: "chooseImport" }); }}>{t("重新选择文件")}</Button>
          <label className="accounts-check"><input type="checkbox" checked={editor.allowRefresh} disabled={unavailable} onChange={event => setEditor({ ...editor, allowRefresh: event.currentTarget.checked })} /><span>{t("允许已授权刷新接口更新导入的 OAuth 账号")}</span></label>
          <p>{t("此选项仅适用于含 refresh token 的 OAuth 文件；不会创建登录或推理授权。")}</p>
        </>}
        <InlineError message={op.error} />
      </div>
    </Modal>}
    {pending && pendingCopy && <PendingDialog title={pendingCopy.title} message={pendingCopy.message} confirmLabel={pendingCopy.confirmLabel} busy={op.busy} disabled={nativeSyncBlocked} error={nativeSyncBlocked ? t("原生账号暂不能同步，请检查登录进度、启用状态与目的地授权。") : op.error}
      onCancel={() => { clearCredentialField(credential.current); setHasCredential(false); op.setError(""); setPending(null); }} onConfirm={() => void confirmPending()} />}
  </div>;
}
