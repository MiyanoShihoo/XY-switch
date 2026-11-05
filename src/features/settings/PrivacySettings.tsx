import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { t, useLanguage } from "../../i18n/language";
import { errorText } from "../../lib/error";
import { defaultPrivacySettings, loadPrivacySettings, previewPrivacy, privacyBlockMessage, savePrivacySettings } from "../../lib/privacy-service";
import type { PrivacyInspection, PrivacySettings as Policy } from "../../types/privacy";
import { Badge, Button, Field, InlineError, LoadingState, Textarea, Toast, Toggle } from "../../ui";
import { usePageOperation } from "../shared/usePageOperation";

export function PrivacySettings({ desktop }: { desktop: boolean }) {
  useLanguage();
  const client = useQueryClient();
  const query = useQuery({ queryKey: ["privacy-settings"], queryFn: loadPrivacySettings, enabled: desktop, retry: false, refetchOnWindowFocus: false });
  const operation = usePageOperation();
  const [draft, setDraft] = useState<Policy>({ ...defaultPrivacySettings });
  const [text, setText] = useState("");
  const [preview, setPreview] = useState<PrivacyInspection | null>(null);
  const revision = useRef(0);
  useEffect(() => { if (query.data) { setDraft(query.data); setPreview(null); revision.current += 1; } }, [query.data]);
  useEffect(() => () => { revision.current += 1; }, []);
  const ready = desktop && query.isSuccess && !query.isFetching;
  const dirty = query.data && (draft.redactPersonalInformation !== query.data.redactPersonalInformation || draft.blockUninspectedAttachments !== query.data.blockUninspectedAttachments);
  const changePolicy = (change: Partial<Policy>) => { setDraft(value => ({ ...value, ...change })); setPreview(null); revision.current += 1; };
  const changeText = (value: string) => { setText(value); setPreview(null); revision.current += 1; operation.setError(""); };
  const inspect = () => {
    const expected = revision.current;
    setPreview(null);
    void operation.run(async () => {
      const result = await previewPrivacy(text, draft);
      if (expected === revision.current) setPreview(result);
    });
  };
  return <div className="settings-sections">
    <section className="settings-section">
      <div className="settings-section__heading"><h2>{t("隐私保护")}</h2><p>{t("策略保存在本机，每次代理发送前读取并执行。")}</p></div>
      <div className="form-stack">
        {!desktop && <p className="notice">{t("浏览器展示默认策略；保存和本地检查需要桌面应用。")}</p>}
        {desktop && query.isPending && <LoadingState label={t("正在读取隐私策略…")} />}
        {query.error && <InlineError message={t("无法读取隐私策略，发送已阻止。") + " " + errorText(query.error)} onRetry={() => void query.refetch()} retrying={query.isFetching} />}
        <div className="form-switch"><div className="form-switch__copy"><strong>{t("凭据保护")}</strong><small>{t("识别 API Key、令牌、密码和私钥；当前供应商密钥同时参与检查。")}</small></div><Badge>{t("始终启用")}</Badge></div>
        <div className="form-switch"><div className="form-switch__copy"><strong>{t("个人信息脱敏")}</strong><small>{t("对已识别的邮箱和电话号码进行替换。检测范围有限，请结合本地预览审阅。")}</small></div><Toggle aria-label={t("个人信息脱敏")} checked={draft.redactPersonalInformation} onCheckedChange={value => changePolicy({ redactPersonalInformation: value })} disabled={!ready || operation.busy} /></div>
        <div className="form-switch"><div className="form-switch__copy"><strong>{t("阻止未检查附件")}</strong><small>{t("开启时阻止无法检查内容的图片、音频和文件；关闭后这些附件可原样发送，可能包含私人内容。")}</small></div><Toggle aria-label={t("阻止未检查附件")} checked={draft.blockUninspectedAttachments} onCheckedChange={value => changePolicy({ blockUninspectedAttachments: value })} disabled={!ready || operation.busy} /></div>
        <p className="muted">{t("安全的工具名称、调用标识、参数和结构保持原样；检测到敏感结构时阻止发送，避免改变工具执行含义。")}</p>
        <div className="inline-actions">
          <Button loading={operation.busy} disabled={!ready || !dirty} onClick={() => void operation.run(async () => { client.setQueryData(["privacy-settings"], await savePrivacySettings(draft)); }, t("隐私策略已保存"))}>{t("保存隐私策略")}</Button>
          <Button variant="secondary" disabled={!ready || !dirty || operation.busy} onClick={() => { if (query.data) changePolicy(query.data); }}>{t("撤销修改")}</Button>
        </div>
      </div>
    </section>
    <section className="settings-section">
      <div className="settings-section__heading"><h2>{t("本地脱敏预览")}</h2><p>{t("使用当前草稿策略检查；预览文本仅保留在此页面内存中。")}</p></div>
      <div className="form-stack">
        <Field label={t("待检查文本或 JSON")} description={t("最多 256 KiB。预览在本机运行，不保存正文、不发送到供应商。")}> <Textarea rows={6} value={text} onChange={event => changeText(event.target.value)} disabled={!ready} spellCheck={false} autoComplete="off" /></Field>
        <div className="inline-actions"><Button variant="secondary" disabled={!ready || operation.busy || !text.trim()} loading={operation.busy} onClick={inspect}>{t("检查并预览")}</Button><Button variant="quiet" disabled={!text && !preview} onClick={() => changeText("")}>{t("清除文本")}</Button></div>
        {preview && <>
          {preview.blockedCode ? <InlineError message={privacyBlockMessage(preview.blockedCode)} /> : <Badge>{t("检查完成")}</Badge>}
          <dl className="key-values"><dt>{t("凭据替换数")}</dt><dd>{preview.report.secretRedactions}</dd><dt>{t("个人信息替换数")}</dt><dd>{preview.report.personalRedactions}</dd><dt>{t("敏感结构字段数")}</dt><dd>{preview.report.protectedFields}</dd><dt>{t("未检查附件数")}</dt><dd>{preview.report.uninspectedAttachments}</dd></dl>
          <Field label={t("脱敏后的预览")}><Textarea rows={7} readOnly value={typeof preview.redacted === "string" ? preview.redacted : JSON.stringify(preview.redacted, null, 2)} spellCheck={false} /></Field>
          <p className="muted">{t("实际发送还会检查当前供应商密钥和正文覆盖项，结果可能与预览不同。请求诊断只记录计数和原因。")}</p>
        </>}
        <InlineError message={operation.error} />
        {operation.message && <Toast tone="success" title={operation.message} onDismiss={() => operation.setMessage("")} />}
      </div>
    </section>
  </div>;
}
