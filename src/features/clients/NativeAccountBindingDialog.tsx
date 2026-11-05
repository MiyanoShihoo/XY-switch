import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { t, useLanguage } from "../../i18n/language";
import { Button, Field, InlineError, Input, Modal, Select } from "../../ui";
import { clientRuntimeAvailable } from "../../lib/clients-service";
import { accountErrorText } from "../../lib/accounts-service";
import type { ClientConnection, NativeLoginProgress } from "../../types/clients";
import { ACCOUNT_QUERY_KEYS, loadAccountProviders } from "../accounts/account-actions";
import { accountTime, nativeAccountScopeMessage, safeAccountLabel } from "../accounts/account-presentation";
import { usePageOperation } from "../shared/usePageOperation";
import { captureNativeBinding, nativeCandidateAvailable, nativeClientActionIssue, submitNativeBinding, type NativeBindingApproval } from "./native-account-actions";

export function NativeAccountBindingDialog({ row, rows, login, accountsReady, onClose, onBound }: {
  row: ClientConnection; rows: ClientConnection[] | undefined; login: NativeLoginProgress | null;
  accountsReady: boolean; onClose: () => void; onBound: () => void;
}) {
  const language = useLanguage();
  const client = useQueryClient();
  const operation = usePageOperation();
  const native = clientRuntimeAvailable();
  const providers = useQuery({ queryKey: ACCOUNT_QUERY_KEYS.providers, queryFn: loadAccountProviders, enabled: native, retry: false });
  const [timestamp, setTimestamp] = useState(() => Math.floor(Date.now() / 1000));
  const [candidateId, setCandidateId] = useState(() => row.account?.poolCandidates.find(candidate => nativeCandidateAvailable(candidate, timestamp))?.id ?? "");
  const [alias, setAlias] = useState("");
  const [selectedProvider, setSelectedProvider] = useState("");
  const [approval, setApproval] = useState<NativeBindingApproval | null>(null);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    const timer = setInterval(() => setTimestamp(Math.floor(Date.now() / 1000)), 1000);
    return () => { mounted.current = false; clearInterval(timer); };
  }, []);
  const providerId = selectedProvider || providers.data?.find(provider => provider.enabled)?.id || "";
  const provider = providers.data?.find(item => item.id === providerId);
  const destination = provider?.endpoint ?? "";
  const issue = nativeClientActionIssue(row.id, rows, login);
  useEffect(() => { setApproval(null); }, [candidateId, alias, providerId, destination, row.account, issue]);
  let current: NativeBindingApproval | null = null;
  try { current = captureNativeBinding(row, { candidateId, alias, providerId }, providers.data ?? [], timestamp); } catch { current = null; }
  const approved = Boolean(current && approval && current.clientId === approval.clientId && current.candidateId === approval.candidateId
    && current.alias === approval.alias && current.providerId === approval.providerId && current.approvedDestination === approval.approvedDestination);
  const disabled = !native || operation.busy || Boolean(issue) || !accountsReady || !providers.isSuccess;
  const candidate = row.account?.poolCandidates.find(item => item.id === candidateId);
  const submit = async () => {
    if (disabled || !approved || !approval) return;
    const completed = await operation.run(async () => { await submitNativeBinding(client, approval, true, () => mounted.current, login); });
    if (mounted.current) {
      setApproval(null);
      if (completed) onBound();
    }
  };
  return <Modal open title={t("绑定原生账号")} busy={operation.busy} onClose={() => { if (!operation.busy) onClose(); }} footer={<>
    <Button variant="secondary" disabled={operation.busy} onClick={onClose}>{t("取消")}</Button>
    <Button disabled={disabled || !approved} loading={operation.busy} onClick={() => void submit()}>{t("授权并绑定")}</Button>
  </>}>
    <div className="native-binding-form">
      <p>{nativeAccountScopeMessage()}</p>
      <Field label={t("原生账号来源")} htmlFor="native-binding-candidate" required>
        <Select id="native-binding-candidate" value={candidateId} disabled={disabled} onChange={event => { setCandidateId(event.currentTarget.value); setApproval(null); }}>
          <option value="">{t("请选择原生账号")}</option>
          {(row.account?.poolCandidates ?? []).map(item => <option key={item.id} value={item.id} disabled={!nativeCandidateAvailable(item, timestamp)}>
            {safeAccountLabel(item.label, t("原生账号"))}{item.boundAccountId !== null ? " · " + t("已绑定") : item.expiresAt <= timestamp ? " · " + t("已过期") : ""}
          </option>)}
        </Select>
      </Field>
      {candidate && <p className="muted">{t("本次绑定选择有效至：{time}", { time: accountTime(candidate.expiresAt, language) })}</p>}
      <Field label={t("账号别名")} htmlFor="native-binding-alias" description={t("使用不含邮箱或凭据的短名称。别名会保存在本地元数据中。")} required>
        <Input id="native-binding-alias" value={alias} maxLength={80} autoComplete="off" disabled={disabled} onChange={event => { setAlias(event.currentTarget.value); setApproval(null); }} />
      </Field>
      <Field label={t("绑定供应商")} htmlFor="native-binding-provider" required>
        <Select id="native-binding-provider" value={providerId} disabled={disabled} onChange={event => { setSelectedProvider(event.currentTarget.value); setApproval(null); }}>
          <option value="">{t("请选择供应商")}</option>{(providers.data ?? []).map(item => <option key={item.id} value={item.id} disabled={!item.enabled || !item.endpoint}>{safeAccountLabel(item.name, t("供应商"))}</option>)}
        </Select>
      </Field>
      <Field label={t("批准的完整绑定目的地")}>
        <code className="native-binding-destination">{destination || t("请求地址不可用于账号授权")}</code>
      </Field>
      <p>{t("授权绑定确认时的完整请求地址；供应商地址改变后需重新批准。")}</p>
      <label className="native-binding-confirm"><input type="checkbox" checked={approved} disabled={disabled || !current} onChange={event => { setApproval(event.currentTarget.checked ? current : null); }} />
        <span>{t("我已检查所选原生账号、别名、供应商与完整目的地，授权创建本地绑定。")}</span>
      </label>
      {issue && <p role="status">{issue}</p>}
      {!accountsReady && <p role="status">{t("请先刷新供应商和账号列表，再确认绑定。")}</p>}
      {!candidate || !nativeCandidateAvailable(candidate, timestamp) ? <p>{t("原生账号候选已过期、已绑定或已改变，请重新授权读取账号。")}</p> : null}
      <InlineError message={operation.error || (providers.error ? accountErrorText(providers.error) : "")} />
    </div>
  </Modal>;
}
