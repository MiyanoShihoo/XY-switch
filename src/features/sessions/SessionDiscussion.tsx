import { t } from "../../i18n/language";
import type { SessionMessage } from "../../types/session";

export function sessionDiscussion(messages: SessionMessage[]) {
  return messages.flatMap((message, index) => message.role === "user"
    ? [{ index, title: message.content.trim().replace(/\s+/g, " ").slice(0, 88) }]
    : []);
}

interface SessionDiscussionProps {
  items: ReturnType<typeof sessionDiscussion>;
  activeIndex?: number;
  onJump: (index: number) => void;
}

export function SessionDiscussion({ items, activeIndex, onJump }: SessionDiscussionProps) {
  return <section className="session-discussion">
    <h3 className="session-discussion__heading">{t("本次讨论")} <span>{items.length}</span></h3>
    <nav className="session-discussion__items" aria-label={t("本次讨论")}>
      {items.length ? <ol>{items.map((item, position) => <li key={item.index}>
        <button type="button" aria-current={activeIndex === item.index ? "location" : undefined} onClick={() => onJump(item.index)}>
          <span className="session-discussion__number" aria-hidden="true">{String(position + 1).padStart(2, "0")}</span>
          <span>{item.title || t("第 {number} 段讨论", { number: position + 1 })}</span>
        </button>
      </li>)}</ol> : <p>{t("已加载消息中没有用户讨论。")}</p>}
    </nav>
  </section>;
}
