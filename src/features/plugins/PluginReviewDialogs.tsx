import { useEffect, useState } from "react";
import { useLanguage, t, translateMessage } from "../../i18n/language";
import { pluginStatusText } from "../../lib/plugins-service";
import type { InstallPreview, PluginClient, ToolOperation, TrustedKeyInput } from "../../types/plugins";
import { Button, Field, InlineError, Input, Modal } from "../../ui";
import { pluginClientLabels } from "./clients";
import { PermissionsSummary } from "./PermissionsSummary";
import type { InstallChoices, ToolReview, TrustedKeyReview, WithdrawalReview } from "./review";

function useExpired(expiresAt: number): boolean {
  const [expired, setExpired] = useState(() => expiresAt <= Date.now());
  useEffect(() => {
    const remaining = expiresAt - Date.now();
    setExpired(remaining <= 0);
    if (remaining <= 0) return;
    const timer = setTimeout(() => setExpired(true), remaining + 1);
    return () => clearTimeout(timer);
  }, [expiresAt]);
  return expired;
}

function ReviewCheckbox({ label, checked, disabled = false, onCheckedChange }: {
  label: string; checked: boolean; disabled?: boolean; onCheckedChange: (checked: boolean) => void;
}) {
  return <label className="plugins-check"><input type="checkbox" checked={checked} disabled={disabled}
    onChange={event => onCheckedChange(event.target.checked)} /><span>{label}</span></label>;
}

interface ReviewProps { busy: boolean; error: string; onClose: () => void }

export function TrustedKeyDialog({ input, review, busy, error, onChange, onPreview, onConfirm, onClose }: ReviewProps & {
  input: TrustedKeyInput; review: TrustedKeyReview | null; onChange: (input: TrustedKeyInput) => void;
  onPreview: () => void; onConfirm: (fingerprint: string, independentSource: boolean) => void;
}) {
  useLanguage();
  const [fingerprint, setFingerprint] = useState("");
  const [independentSource, setIndependentSource] = useState(false);
  useEffect(() => { setFingerprint(""); setIndependentSource(false); }, [review]);
  return <Modal open title={t("添加发布者信任密钥")} size="lg" busy={busy} onClose={onClose}
    description={t("请从独立可信来源取得公钥；插件包内的公钥不能建立信任。")}
    footer={<><Button variant="secondary" disabled={busy} onClick={onClose}>{t("取消")}</Button>
      {review ? <Button disabled={busy || !independentSource || fingerprint !== review.preview.fingerprint}
        onClick={() => onConfirm(fingerprint, independentSource)}>{t("确认信任此公钥")}</Button>
        : <Button disabled={busy || !input.id || !input.label || !input.publicKey} onClick={onPreview}>{t("预览公钥指纹")}</Button>}</>}>
    <Field label={t("密钥标识")} required><Input value={input.id} maxLength={96} disabled={busy} spellCheck={false}
      onChange={event => onChange({ ...input, id: event.target.value })} /></Field>
    <Field label={t("发布者标签")} required><Input value={input.label} maxLength={160} disabled={busy}
      onChange={event => onChange({ ...input, label: event.target.value })} /></Field>
    <Field label={t("Ed25519 公钥（小写 hex）")} required><Input value={input.publicKey} maxLength={64} disabled={busy} spellCheck={false}
      autoComplete="off" onChange={event => onChange({ ...input, publicKey: event.target.value })} /></Field>
    {review && <div className="plugins-review">
      <p>{t("宿主计算的公钥指纹")}</p><code className="plugins-digest">{review.preview.fingerprint}</code>
      <Field label={t("输入独立来源的精确指纹")} required><Input value={fingerprint} maxLength={64} disabled={busy} spellCheck={false}
        autoComplete="off" onChange={event => setFingerprint(event.target.value)} /></Field>
      <ReviewCheckbox checked={independentSource} disabled={busy} onCheckedChange={setIndependentSource}
        label={t("我已通过独立可信来源核对该公钥与指纹。")} />
    </div>}
    <InlineError message={translateMessage(error)} />
  </Modal>;
}

export function InstallReviewDialog({ preview, initialClients = [], busy, error, onConfirm, onClose }: ReviewProps & {
  preview: InstallPreview; initialClients?: PluginClient[]; onConfirm: (choices: InstallChoices) => void;
}) {
  useLanguage();
  const [clients, setClients] = useState<PluginClient[]>(() => initialClients.filter(client => preview.validation.manifest.clients.includes(client)));
  const [digest, setDigest] = useState("");
  const [accepted, setAccepted] = useState(false);
  const [enable, setEnable] = useState(false);
  const expired = useExpired(preview.expiresAt);
  const manifest = preview.validation.manifest;
  return <Modal open title={t("审核插件安装与客户端授权")} size="lg" busy={busy} onClose={onClose}
    description={t("本次确认会停用旧客户端授权，仅按下方选择重新授权。")}
    footer={<><Button variant="secondary" disabled={busy} onClick={onClose}>{t("取消")}</Button>
      <Button disabled={busy || expired || !accepted || !clients.length || digest !== preview.validation.digest}
        onClick={() => onConfirm({ clients, digest, accepted, enable })}>{t("确认保存并授权")}</Button></>}>
    <dl className="key-values"><dt>{t("插件")}</dt><dd>{manifest.name} · {manifest.id}</dd>
      <dt>{t("版本")}</dt><dd>{preview.previousVersion ? preview.previousVersion + " → " + manifest.version : manifest.version}</dd>
      <dt>{t("发布者")}</dt><dd>{manifest.author}</dd><dt>{t("签名密钥")}</dt><dd>{preview.signingKeyId}</dd>
      <dt>{t("公钥指纹")}</dt><dd><code className="plugins-digest">{preview.signingKeyFingerprint}</code></dd>
      <dt>{t("文件数量 / 文本字节")}</dt><dd>{preview.validation.fileCount} / {preview.validation.canonicalSize}</dd>
      <dt>{t("固定版本依赖")}</dt><dd>{manifest.dependencies.length ? manifest.dependencies.join(" · ") : t("无")}</dd></dl>
    <p>{manifest.description}</p>
    <PermissionsSummary permissions={manifest.permissions} />
    <fieldset disabled={busy} className="plugins-client-choices"><legend>{t("本次授权客户端")}</legend>
      {manifest.clients.map(client => <ReviewCheckbox key={client} label={pluginClientLabels[client]} checked={clients.includes(client)}
        onCheckedChange={checked => setClients(previous => checked ? [...previous, client] : previous.filter(item => item !== client))} />)}
    </fieldset>
    {preview.permissionChanges.length > 0 && <details><summary>{t("各客户端新增权限")}</summary>
      {preview.permissionChanges.map(change => <section className="plugins-review" key={change.client}>
        <h4>{pluginClientLabels[change.client]}</h4><PermissionsSummary permissions={change.added} /></section>)}</details>}
    {preview.validation.warnings.map(code => <p className="plugins-notice" key={code}>{translateMessage(pluginStatusText(code))}</p>)}
    <ReviewCheckbox checked={enable} disabled={busy || !preview.validation.runtimeSupported} onCheckedChange={setEnable}
      label={t("确认后启用所选客户端")} />
    <p className="plugins-notice">{t("默认保持停用。进程、凭据、写文件和外部文件读取当前无法授权运行。")}</p>
    <p>{t("插件包精确摘要")}</p><code className="plugins-digest">{preview.validation.digest}</code>
    <Field label={t("输入本次插件包的精确摘要")} required><Input value={digest} maxLength={64} disabled={busy} spellCheck={false}
      autoComplete="off" onChange={event => setDigest(event.target.value)} /></Field>
    <ReviewCheckbox checked={accepted} disabled={busy} onCheckedChange={setAccepted}
      label={t("我已审核完整权限、客户端、版本与精确摘要。")} />
    {expired && <InlineError message={t("预览已过期或本地状态已变化，请重新审核并预览。")} />}
    <InlineError message={translateMessage(error)} />
  </Modal>;
}

export function ToolReviewDialog({ review, operation, busy, error, onConfirm, onClose }: ReviewProps & {
  review: ToolReview; operation: ToolOperation["type"]; onConfirm: (argumentsDigest: string, accepted: boolean) => void;
}) {
  useLanguage();
  const [argumentsDigest, setArgumentsDigest] = useState("");
  const [accepted, setAccepted] = useState(false);
  const expired = useExpired(review.preview.expiresAt);
  const preview = review.preview;
  const effect = operation === "json_passthrough" ? t("返回本次明确确认的 JSON 输入。")
    : operation === "read_text" ? t("读取已签名插件包内的文本。") : t("向已授权公网 HTTPS 站点发送一次 GET 请求。");
  return <Modal open title={t("确认这一次工具调用")} size="lg" busy={busy} onClose={onClose}
    description={t("调用确认仅使用一次。修改参数、客户端、插件版本或权限后必须重新预览。")}
    footer={<><Button variant="secondary" disabled={busy} onClick={onClose}>{t("取消")}</Button>
      <Button disabled={busy || expired || !accepted || argumentsDigest !== preview.argumentsDigest}
        onClick={() => onConfirm(argumentsDigest, accepted)}>{t("确认并执行这一次调用")}</Button></>}>
    <dl className="key-values"><dt>{t("插件 / 工具")}</dt><dd>{preview.pluginId} / {preview.tool}</dd>
      <dt>{t("版本")}</dt><dd>{preview.version}</dd><dt>{t("客户端")}</dt><dd>{pluginClientLabels[preview.client]}</dd>
      <dt>{t("插件包精确摘要")}</dt><dd><code className="plugins-digest">{preview.pluginDigest}</code></dd></dl>
    <p>{effect}</p><PermissionsSummary permissions={preview.requiredPermissions} />
    <p>{t("本次工具参数")}</p><pre className="plugins-text-preview">{review.draft.argumentsJson}</pre>
    <p>{t("宿主计算的参数摘要")}</p><code className="plugins-digest">{preview.argumentsDigest}</code>
    <Field label={t("输入本次参数的精确摘要")} required><Input value={argumentsDigest} maxLength={64} disabled={busy} spellCheck={false}
      autoComplete="off" onChange={event => setArgumentsDigest(event.target.value)} /></Field>
    <ReviewCheckbox checked={accepted} disabled={busy} onCheckedChange={setAccepted}
      label={t("我确认此客户端、本次参数及列出的最小权限。")} />
    {expired && <InlineError message={t("预览已过期或本地状态已变化，请重新审核并预览。")} />}
    <InlineError message={translateMessage(error)} />
  </Modal>;
}

export function WithdrawalReviewDialog({ review, busy, error, onConfirm, onClose }: ReviewProps & {
  review: WithdrawalReview; onConfirm: (accepted: boolean) => void;
}) {
  useLanguage();
  const [accepted, setAccepted] = useState(false);
  const notice = review.notice;
  const reason = notice.reason === "security" ? t("安全问题") : notice.reason === "privacy" ? t("隐私问题") : t("发布者撤回");
  return <Modal open title={t("应用签名撤回通知")} size="lg" busy={busy} onClose={onClose}
    description={t("签名将在确认后由宿主验证。验证成功后，此版本将禁止安装、运行和回滚。")}
    footer={<><Button variant="secondary" disabled={busy} onClick={onClose}>{t("取消")}</Button>
      <Button variant="danger" disabled={busy || !accepted} onClick={() => onConfirm(accepted)}>{t("确认验证并应用撤回")}</Button></>}>
    <dl className="key-values"><dt>{t("插件")}</dt><dd>{notice.id}</dd><dt>{t("版本")}</dt><dd>{notice.version}</dd>
      <dt>{t("插件包精确摘要")}</dt><dd><code className="plugins-digest">{notice.digest}</code></dd>
      <dt>{t("签名密钥")}</dt><dd>{notice.signingKeyId}</dd><dt>{t("撤回原因")}</dt><dd>{reason}</dd></dl>
    <p className="plugins-notice">{t("撤回标记不可撤销；已卸载版本的历史缓存也会受到约束。")}</p>
    <ReviewCheckbox checked={accepted} disabled={busy} onCheckedChange={setAccepted}
      label={t("我确认应用此签名撤回通知并停用对应版本。")}/>
    <InlineError message={translateMessage(error)} />
  </Modal>;
}
