import { t } from "../../i18n/language";
import type { AccountInfo } from "../../types/accounts";
import type { ChannelGroup, ChannelGroupsSnapshot } from "../../types/channel-group";
import type { Provider } from "../../types/provider";
import { channelMoney, channelStrategyLabel } from "./channel-group-presentation";
import { ChannelGroupRecordedUsage, ChannelGroupStatus } from "./ChannelGroupStatus";

interface Props { group: ChannelGroup; snapshot: ChannelGroupsSnapshot; providers: Provider[]; accounts: AccountInfo[] }

export function ChannelGroupDetails({ group, snapshot, providers, accounts }: Props) {
  const project = snapshot.configuration.projects.find(item => item.id === group.projectId);
  const relatedGroups = snapshot.configuration.groups.filter(item => item.projectId && item.projectId === group.projectId);
  return <>
    <section className="channel-group-section channel-group-overview" aria-label={t("已保存的组配置")}>
      <dl className="channel-group-facts">
        <div><dt>{t("虚拟模型")}</dt><dd><code>{group.virtualModel}</code></dd></div>
        <div><dt>{t("选择策略")}</dt><dd>{channelStrategyLabel(group.strategy)}</dd></div>
        <div><dt>{t("共享项目 ID")}</dt><dd><code>{group.projectId ?? t("未关联")}</code></dd></div>
        <div><dt>{t("项目日预算")}</dt><dd>{group.projectId ? project ? project.dailyMicrousd === null ? t("未设置上限") : channelMoney(project.dailyMicrousd) : t("未知") : t("未关联")}</dd></div>
        <div><dt>{t("会话亲和有效期（秒）")}</dt><dd>{group.affinityTtlSeconds}</dd></div>
        <div><dt>{t("组 ID / 修订号")}</dt><dd><code>{group.id}</code> · {group.revision}</dd></div>
      </dl>
      {project && <p>{t("共享此项目预算的组：{groups}", { groups: relatedGroups.map(item => item.name).join(" · ") })}</p>}
    </section>
    <ChannelGroupStatus group={group} channels={snapshot.channels} providers={providers} accounts={accounts} source="snapshot" />
    <ChannelGroupRecordedUsage group={group} channels={snapshot.channels} providers={providers} />
  </>;
}
