// 面板第二块：今天的日程。
import type { AgendaItem } from "../types";
import { REPEAT_LABEL, describeWhen, parseLocalIso } from "../lib/format";

export function TodaySection({ items, onOpenCalendar, now }: { items: AgendaItem[]; onOpenCalendar: () => void; now?: Date }) {
  const current = now ?? new Date();
  return (
    <section className="card" aria-labelledby="today-title">
      <header className="card-head">
        <h2 id="today-title">今天的日程</h2>
        <button className="link" onClick={onOpenCalendar}>
          打开日程表 →
        </button>
      </header>
      {items.length === 0 ? (
        <p className="empty">今天没有日程。</p>
      ) : (
        <ul className="agenda">
          {items.map((it) => {
            const past = !it.data.all_day && parseLocalIso(it.end) < current;
            const ongoing = !it.data.all_day && parseLocalIso(it.start) <= current && !past;
            return (
              <li key={`${it.event_id}@${it.start}`} className={`agenda-item${past ? " past" : ""}${ongoing ? " ongoing" : ""}`}>
                <span className="when">{describeWhen(it)}</span>
                <span className="what">
                  {it.data.title}
                  {it.conflict_of && <span className="tag warn">冲突副本</span>}
                </span>
                <span className="muted small">
                  {it.data.repeat !== "none" && REPEAT_LABEL[it.data.repeat]}
                  {it.data.remind_before_min !== null && ` · 提前 ${it.data.remind_before_min} 分钟提醒`}
                </span>
                {ongoing && <span className="tag">进行中</span>}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
