import { useQuery } from "@tanstack/react-query";
import { Badge, Button, EmptyState, InlineError, LoadingState } from "../../ui";
import { hasProviderApiKey } from "../../lib/credential-service";
import { errorText } from "../../lib/error";
import type { ProvidersController } from "../providers/useProviders";

export function CredentialsSettings({ desktop, providers: controller }: { desktop: boolean; providers: ProvidersController }) {
  const ids = controller.providers.map(provider => provider.id);
  const query = useQuery({
    queryKey: ["credential-status", ids],
    enabled: desktop && !controller.loading && !controller.error,
    queryFn: () => Promise.all(ids.map(async id => {
      try { return { id, configured: await hasProviderApiKey(id), error: "" }; }
      catch (cause) { return { id, configured: null, error: errorText(cause) }; }
    })),
    retry: false,
    refetchOnWindowFocus: false,
  });
  const current = controller.providers.find(provider => provider.id === controller.currentId);
  return <div className="settings-sections"><section className="settings-section"><div className="settings-section__heading"><h2>凭据状态</h2><p>只查询凭据是否存在，不读取或展示实际密钥。</p></div><div className="form-stack">
    <div className="section-heading"><p>当前配置：<strong>{controller.loading ? "读取中" : controller.error ? "未知" : current?.name ?? "未设置当前供应商"}</strong></p><Button variant="secondary" loading={query.isFetching || controller.loading} disabled={!desktop} onClick={() => { void controller.refresh(); void query.refetch(); }}>刷新状态</Button></div>
    <p className="notice">供应商密钥使用系统安全存储：Windows Credential Manager、Linux Secret Service、Apple Keychain 或 Android Keystore（AES-GCM 加密记录）。目标名为 <code>XY Switch/provider/&lt;供应商 ID&gt;</code>。修改或清除密钥请使用供应商编辑器。</p>
    {controller.error && <InlineError message={controller.error} onRetry={() => void controller.refresh()} />}
    {(controller.loading || (desktop && query.isPending && !controller.error)) && <LoadingState label="正在查询凭据状态…" />}
    {query.error && <InlineError message={"凭据查询失败：" + errorText(query.error)} onRetry={() => void query.refetch()} />}
    {!controller.loading && !controller.error && !controller.providers.length && <EmptyState title="尚未添加供应商" />}
    {controller.providers.length > 0 && <div className="record-list">{controller.providers.map(provider => {
      const record = query.data?.find(item => item.id === provider.id);
      const unknown = !desktop || !record || record.configured === null || query.isError;
      return <article className="record-row" key={provider.id}><div className="record-row__main"><div className="record-row__title">{provider.name}{provider.id === controller.currentId && <Badge tone="accent">当前</Badge>}</div><code className="record-row__detail">{provider.id}</code>{record?.error && <p className="settings-credential-error" role="alert">查询失败：{record.error}</p>}</div><Badge tone={unknown ? "warning" : record.configured ? "success" : "neutral"}>{!desktop ? "预览模式 · 未查询" : unknown ? "状态未知" : record.configured ? "已配置" : "未配置"}</Badge></article>;
    })}</div>}
  </div></section></div>;
}
