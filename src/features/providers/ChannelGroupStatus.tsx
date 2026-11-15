import { useId } from "react";
import { t, useLanguage } from "../../i18n/language";
import type { AccountInfo } from "../../types/accounts";
import type { ChannelGroup, ChannelView } from "../../types/channel-group";
import type { Provider } from "../../types/provider";
import { Badge } from "../../ui";
import { channelMoney, channelNumber, channelReason, channelTime, channelWarning, recordedProviderUsage } from "./channel-group-presentation";

export function ChannelGroupWarnings({ warnings }: { warnings: string[] }) {
  if (!warnings.length) return null;
  return <details className="channel-group-warnings"><summary>{t("引擎说明（{count}）", { count: warnings.length })}</summary>
    <ul>{[...new Set(warnings)].map(warning => <li key={warning}>{channelWarning(warning)}</li>)}</ul>
  </details>;
}

interface ChannelStatusProps {
  group: ChannelGroup;
  channels: ChannelView[];
  providers: Provider[];
  accounts: AccountInfo[];
  source: "snapshot" | "simulation";
}

export function ChannelGroupStatus({ group, channels, providers, accounts, source }: ChannelStatusProps) {
  useLanguage();
  const id = useId();
  const configuredNumber = (value: number | null) => value === null ? t("未设置") : channelNumber(value);
  return <section className="channel-group-section" aria-labelledby={id}>
    <h2 id={id}>{source === "snapshot" ? t("渠道与本地回放状态") : t("模拟结束时的渠道状态")}</h2>
    <p>{t("健康与路由计数来自本地回放或模拟；供应商历史可影响多个引用渠道，不能据此判断实时可用性。")}</p>
    <div className="channel-group-status-list">{group.channels.map((channel, index) => {
      const view = channels.find(item => item.groupId === group.id && item.channelId === channel.id);
      const provider = providers.find(item => item.id === channel.providerId);
      const account = accounts.find(item => item.id === channel.accountId && item.providerId === channel.providerId);
      return <article key={channel.id} className="channel-group-readout" data-channel-id={channel.id}>
        <header><div><span className="channel-group-eyebrow">{t("渠道 {index}", { index: index + 1 })}</span><h3><code>{channel.upstreamModel}</code></h3></div>
          <Badge tone={view?.health === "cooling_down" || view?.health === "unknown_result" ? "warning" : "neutral"}>{view ? channelReason(view.health) : t("状态未读取")}</Badge>
        </header>
        <p className="channel-group-reference"><span>{provider?.name ?? channel.providerId}</span><span>{channel.accountId ? account?.alias ?? channel.accountId : t("供应商默认引用")}</span><span>{channel.enabled ? t("参与离线选择") : t("渠道已停用")}</span></p>
        <dl className="channel-group-metrics">
          <div><dt>{t("剩余请求额度")}</dt><dd>{channelNumber(view?.quota.remainingRequests)}</dd></div>
          <div><dt>{t("剩余 Token 额度")}</dt><dd>{channelNumber(view?.quota.remainingTokens)}</dd></div>
          <div><dt>{t("额度来源")}</dt><dd>{view?.quota.source ? channelReason(view.quota.source) : t("未知")}</dd></div>
          <div><dt>{t("预计冷却恢复")}</dt><dd>{view?.recoveryAtMs != null ? channelTime(view.recoveryAtMs) : view ? t("无已记录冷却") : t("未知")}</dd></div>
        </dl>
        {view?.cooldownReason && <p className="channel-group-cooldown">{channelReason(view.cooldownReason)}</p>}
        <details className="channel-group-advanced">
          <summary>{t("查看引用、限制与回放计数")}</summary>
          <dl className="channel-group-facts">
            <div><dt>{t("渠道 ID")}</dt><dd><code>{channel.id}</code></dd></div>
            <div><dt>{t("供应商 ID")}</dt><dd><code>{channel.providerId}</code></dd></div>
            <div><dt>{t("账号 ID")}</dt><dd><code>{channel.accountId ?? t("供应商默认引用")}</code></dd></div>
            <div><dt>{t("优先级 / 权重")}</dt><dd>{channel.priority} / {channel.weight}</dd></div>
            <div><dt>{t("RPM / TPM 上限")}</dt><dd>{configuredNumber(channel.limits.rpm)} / {configuredNumber(channel.limits.tpm)}</dd></div>
            <div><dt>{t("并发上限")}</dt><dd>{configuredNumber(channel.limits.concurrency)}</dd></div>
            <div><dt>{t("渠道日预算（μUSD）")}</dt><dd>{configuredNumber(channel.limits.dailyMicrousd)}</dd></div>
            <div><dt>{t("限流 / 失败 / 最大退避（秒）")}</dt><dd>{channel.cooldown.rateLimitSeconds} / {channel.cooldown.failureSeconds} / {channel.cooldown.maxBackoffSeconds}</dd></div>
            <div><dt>{t("额度共享方式")}</dt><dd>{channel.quotaPoolId ? t("声明共享额度池") : channel.quotaIndependent ? t("声明独立额度") : t("保守共享（默认）")}</dd></div>
            <div><dt>{t("引擎额度池 ID")}</dt><dd><code>{view?.poolId ?? t("未知")}</code></dd></div>
            <div><dt>{t("额度池关系来源")}</dt><dd>{view ? view.poolDeclared ? t("用户声明，未经独立性验证") : t("未知，保守共享") : t("未知")}</dd></div>
            <div><dt>{t("请求 / Token 额度重置")}</dt><dd>{channelTime(view?.quota.requestsResetAtMs)} / {channelTime(view?.quota.tokensResetAtMs)}</dd></div>
          </dl>
          <h4>{t("回放 / 模拟计数")}</h4>
          <p>{t("以下窗口计数由回放和合成预留产生，不代表实时吞吐。共享池计数不能按渠道相加。")}</p>
          <dl className="channel-group-metrics">
            <div><dt>{t("窗口请求数（RPM）")}</dt><dd>{channelNumber(view?.rpm)}</dd></div>
            <div><dt>{t("窗口已知 / 预留 Token")}</dt><dd>{channelNumber(view?.tpmReserved)}</dd></div>
            <div><dt>{t("窗口 Token 未知请求数")}</dt><dd>{channelNumber(view?.tpmUnknownRequests)}</dd></div>
            <div><dt>{t("预留并发")}</dt><dd>{channelNumber(view?.concurrent)}</dd></div>
            <div><dt>{t("观测错误率")}</dt><dd>{view?.errorRate == null ? t("未知") : `${channelNumber(Math.round(view.errorRate * 10000) / 100)}%`}</dd></div>
            <div><dt>{t("观测延迟（毫秒）")}</dt><dd>{channelNumber(view?.latencyMs)}</dd></div>
            <div><dt>{t("结果未知数")}</dt><dd>{channelNumber(view?.unknownResults)}</dd></div>
            <div><dt>{t("本次合成预留数")}</dt><dd>{channelNumber(view?.simulatedUsage.requests)}</dd></div>
          </dl>
        </details>
      </article>;
    })}</div>
  </section>;
}

export function ChannelGroupRecordedUsage({ group, channels, providers }: Pick<ChannelStatusProps, "group" | "channels" | "providers">) {
  const id = useId();
  return <section className="channel-group-section" aria-labelledby={id}>
    <h2 id={id}>{t("供应商级已记录用量")}</h2>
    <p>{t("每个供应商只展示一次，包含其全部已记录请求。它们不属于某个渠道或渠道组；未知用量与未知金额仍保留。")}</p>
    <div className="channel-group-table-scroll"><table className="channel-group-table">
      <caption className="sr-only">{t("供应商级账本覆盖范围")}</caption>
      <thead><tr><th scope="col">{t("供应商")}</th><th scope="col">{t("记录请求数")}</th><th scope="col">{t("已知 Token 小计")}</th><th scope="col">{t("已知金额小计")}</th></tr></thead>
      <tbody>{recordedProviderUsage(group, channels).map(({ providerId, usage }) => <tr key={providerId}>
        <th scope="row">{providers.find(provider => provider.id === providerId)?.name ?? providerId}<code>{providerId}</code></th>
        <td>{channelNumber(usage?.requests)}</td>
        <td>{channelNumber(usage && (usage.tokenKnownRequests > 0 || usage.requests === 0) ? usage.knownTokens : null)}<small>{t("已知 {known} / {total} 条", { known: channelNumber(usage?.tokenKnownRequests), total: channelNumber(usage?.requests) })}</small></td>
        <td>{channelMoney(usage && (usage.costKnownRequests > 0 || usage.requests === 0) ? usage.knownMicrousd : null)}<small>{t("已知 {known} / {total} 条", { known: channelNumber(usage?.costKnownRequests), total: channelNumber(usage?.requests) })}</small></td>
      </tr>)}</tbody>
    </table></div>
  </section>;
}
