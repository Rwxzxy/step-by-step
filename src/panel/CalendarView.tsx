// 日程表：日视图 / 周视图，新建、编辑、删除，.ics 导入导出。
import { useCallback, useEffect, useMemo, useState } from "react";
import { api, on } from "../api";
import type { AgendaItem, EventData, Repeat } from "../types";
import {
  REPEAT_LABEL,
  WEEKDAYS,
  addDays,
  dateLabel,
  describeWhen,
  hhmm,
  parseLocalIso,
  startOfDay,
  startOfWeek,
  toLocalIso,
} from "../lib/format";

const HOUR_PX = 44;

export function CalendarView({ notify }: { notify: (msg: string) => void }) {
  const [mode, setMode] = useState<"day" | "week">("week");
  const [cursor, setCursor] = useState(() => startOfDay(new Date()));
  const [items, setItems] = useState<AgendaItem[]>([]);
  const [editing, setEditing] = useState<{ id: string | null; data: EventData } | null>(null);

  const range = useMemo(() => {
    const from = mode === "day" ? cursor : startOfWeek(cursor);
    return { from, to: addDays(from, mode === "day" ? 1 : 7) };
  }, [mode, cursor]);

  const load = useCallback(() => {
    api.listAgenda(toLocalIso(range.from), toLocalIso(range.to)).then(setItems).catch((e) => notify(String(e)));
  }, [range, notify]);

  useEffect(() => {
    load();
    const u = on("steppy://changed", load);
    return () => {
      u.then((f) => f());
    };
  }, [load]);

  const days = mode === "day" ? [range.from] : Array.from({ length: 7 }, (_, i) => addDays(range.from, i));

  const newAt = (day: Date, hour: number) => {
    const start = new Date(day.getFullYear(), day.getMonth(), day.getDate(), hour, 0, 0);
    setEditing({
      id: null,
      data: {
        title: "",
        notes: "",
        all_day: false,
        start: toLocalIso(start),
        end: toLocalIso(new Date(start.getTime() + 3600_000)),
        repeat: "none",
        remind_before_min: 10,
      },
    });
  };

  const edit = async (it: AgendaItem) => {
    const rec = await api.getEvent(it.event_id);
    if (rec?.data) setEditing({ id: rec.id, data: rec.data });
  };

  const shift = (n: number) => setCursor((c) => addDays(c, mode === "day" ? n : n * 7));
  const title = mode === "day" ? dateLabel(cursor) : `${dateLabel(range.from)} – ${dateLabel(addDays(range.to, -1))}`;

  return (
    <div className="calendar">
      <div className="toolbar">
        <div className="seg" role="tablist" aria-label="视图">
          <button role="tab" aria-selected={mode === "day"} className={mode === "day" ? "on" : ""} onClick={() => setMode("day")}>
            日
          </button>
          <button role="tab" aria-selected={mode === "week"} className={mode === "week" ? "on" : ""} onClick={() => setMode("week")}>
            周
          </button>
        </div>
        <button onClick={() => shift(-1)} aria-label="上一页">
          ‹
        </button>
        <button onClick={() => setCursor(startOfDay(new Date()))}>今天</button>
        <button onClick={() => shift(1)} aria-label="下一页">
          ›
        </button>
        <strong className="range-title">{title}</strong>
        <span className="spacer" />
        <button className="primary" onClick={() => newAt(mode === "day" ? cursor : startOfDay(new Date()), Math.min(new Date().getHours() + 1, 23))}>
          + 新建日程
        </button>
        <button onClick={() => api.importIcs().then((m) => m && notify(m)).catch((e) => notify(String(e)))}>导入 .ics</button>
        <button onClick={() => api.exportIcs().then((m) => m && notify(m)).catch((e) => notify(String(e)))}>导出 .ics</button>
      </div>

      <div className={`grid grid-${mode}`} style={{ ["--days" as string]: days.length }}>
        <div className="grid-head">
          <div className="gutter" />
          {days.map((d) => (
            <div key={d.toISOString()} className={`day-head${sameDay(d, new Date()) ? " today" : ""}`}>
              {mode === "week" ? `${WEEKDAYS[(d.getDay() + 6) % 7]} ${d.getMonth() + 1}/${d.getDate()}` : dateLabel(d)}
            </div>
          ))}
        </div>
        <div className="allday-row">
          <div className="gutter small muted">全天</div>
          {days.map((d) => (
            <div key={d.toISOString()} className="allday-cell">
              {items
                .filter((it) => it.data.all_day && sameDay(parseLocalIso(it.start), d))
                .map((it) => (
                  <button key={it.event_id + it.start} className="event allday" onClick={() => edit(it)}>
                    {it.data.title}
                  </button>
                ))}
            </div>
          ))}
        </div>
        <div className="grid-body">
          <div className="gutter">
            {Array.from({ length: 24 }, (_, h) => (
              <div key={h} className="hour-label" style={{ height: HOUR_PX }}>
                {String(h).padStart(2, "0")}:00
              </div>
            ))}
          </div>
          {days.map((d) => (
            <div key={d.toISOString()} className="day-col" style={{ height: HOUR_PX * 24 }}>
              {Array.from({ length: 24 }, (_, h) => (
                <div key={h} className="hour-slot" style={{ height: HOUR_PX }} onDoubleClick={() => newAt(d, h)} title="双击新建" />
              ))}
              {items
                .filter((it) => !it.data.all_day)
                .map((it) => layoutBlock(it, d))
                .filter((b): b is Block => b !== null)
                .map((b) => (
                  <button
                    key={b.it.event_id + b.it.start}
                    className={`event timed${b.it.conflict_of ? " conflict" : ""}`}
                    style={{ top: b.top, height: b.height }}
                    onClick={() => edit(b.it)}
                    title={`${b.it.data.title}\n${describeWhen(b.it)}`}
                  >
                    <span className="ev-time">{hhmm(b.it.start)}</span> {b.it.data.title}
                  </button>
                ))}
            </div>
          ))}
        </div>
      </div>

      {editing && (
        <EventEditor
          id={editing.id}
          initial={editing.data}
          onClose={() => setEditing(null)}
          onSaved={(m) => {
            setEditing(null);
            notify(m);
            load();
          }}
          onError={(m) => notify(m)}
        />
      )}
    </div>
  );
}

interface Block {
  it: AgendaItem;
  top: number;
  height: number;
}

function sameDay(a: Date, b: Date) {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
}

function layoutBlock(it: AgendaItem, day: Date): Block | null {
  const s = parseLocalIso(it.start);
  const e = parseLocalIso(it.end);
  const dayStart = startOfDay(day);
  const dayEnd = addDays(dayStart, 1);
  if (e <= dayStart || s >= dayEnd) return null;
  const from = Math.max(s.getTime(), dayStart.getTime());
  const to = Math.min(e.getTime(), dayEnd.getTime());
  const top = ((from - dayStart.getTime()) / 3600_000) * HOUR_PX;
  const height = Math.max(((to - from) / 3600_000) * HOUR_PX, 20);
  return { it, top, height };
}

const REMIND_OPTIONS: { v: number | null; label: string }[] = [
  { v: null, label: "不提醒" },
  { v: 0, label: "开始时" },
  { v: 5, label: "提前 5 分钟" },
  { v: 10, label: "提前 10 分钟" },
  { v: 15, label: "提前 15 分钟" },
  { v: 30, label: "提前 30 分钟" },
  { v: 60, label: "提前 1 小时" },
  { v: 1440, label: "提前 1 天" },
];

export function EventEditor({
  id,
  initial,
  onClose,
  onSaved,
  onError,
}: {
  id: string | null;
  initial: EventData;
  onClose: () => void;
  onSaved: (msg: string) => void;
  onError: (msg: string) => void;
}) {
  // 编辑时全天事件的结束日期按“包含当天”显示和保存在状态里，提交时再换成次日 00:00。
  const [d, setD] = useState<EventData>(() =>
    initial.all_day ? { ...initial, end: toLocalIso(addDays(parseLocalIso(initial.end), -1)) } : initial,
  );
  const date = d.start.slice(0, 10);
  const endDate = d.end.slice(0, 10);
  const set = (p: Partial<EventData>) => setD((x) => ({ ...x, ...p }));

  const setDate = (v: string) => {
    if (!v) return;
    const lenMs = parseLocalIso(d.end).getTime() - parseLocalIso(d.start).getTime();
    const start = `${v}T${d.start.slice(11)}`;
    set({ start, end: toLocalIso(new Date(parseLocalIso(start).getTime() + lenMs)) });
  };

  const save = async () => {
    if (!d.title.trim()) {
      onError("请填写标题");
      return;
    }
    let data = d;
    if (d.all_day) {
      // 全天：结束日期包含在内，传给后端时换成次日 00:00。
      const end = addDays(parseLocalIso(`${endDate}T00:00:00`), 1);
      data = { ...d, start: `${date}T00:00:00`, end: toLocalIso(end) };
    }
    try {
      await api.saveEvent(id, data);
      onSaved(id ? "日程已更新" : "日程已添加");
    } catch (e) {
      onError(String(e));
    }
  };

  const remove = async () => {
    if (!id) return;
    if (!window.confirm(d.repeat !== "none" ? "删除这个重复日程的所有发生？" : "删除这个日程？")) return;
    try {
      await api.deleteEvent(id);
      onSaved("日程已删除");
    } catch (e) {
      onError(String(e));
    }
  };

  const toggleAllDay = (on: boolean) => {
    if (on) {
      set({ all_day: true, start: `${date}T00:00:00`, end: `${endDate < date ? date : endDate}T00:00:00` });
    } else {
      set({ all_day: false, start: `${date}T09:00:00`, end: `${date}T10:00:00` });
    }
  };

  return (
    <div className="modal-back" role="dialog" aria-modal="true" aria-label={id ? "编辑日程" : "新建日程"}>
      <div className="modal">
        <h3>{id ? "编辑日程" : "新建日程"}</h3>
        <label>
          标题
          <input autoFocus value={d.title} onChange={(e) => set({ title: e.target.value })} placeholder="比如：周会" />
        </label>
        <label className="row">
          <input type="checkbox" checked={d.all_day} onChange={(e) => toggleAllDay(e.target.checked)} /> 全天
        </label>
        <div className="row">
          <label>
            日期
            <input type="date" value={date} onChange={(e) => setDate(e.target.value)} />
          </label>
          {d.all_day ? (
            <label>
              结束日期
              <input
                type="date"
                value={endDate}
                min={date}
                onChange={(e) => e.target.value && set({ end: `${e.target.value}T00:00:00` })}
              />
            </label>
          ) : (
            <>
              <label>
                开始
                <input type="time" value={d.start.slice(11, 16)} onChange={(e) => e.target.value && set({ start: `${date}T${e.target.value}:00` })} />
              </label>
              <label>
                结束
                <input type="time" value={d.end.slice(11, 16)} onChange={(e) => e.target.value && set({ end: `${endDate}T${e.target.value}:00` })} />
              </label>
            </>
          )}
        </div>
        <div className="row">
          <label>
            重复
            <select value={d.repeat} onChange={(e) => set({ repeat: e.target.value as Repeat })}>
              {(Object.keys(REPEAT_LABEL) as Repeat[]).map((r) => (
                <option key={r} value={r}>
                  {REPEAT_LABEL[r]}
                </option>
              ))}
            </select>
          </label>
          <label>
            提醒
            <select
              value={d.remind_before_min === null ? "none" : String(d.remind_before_min)}
              onChange={(e) => set({ remind_before_min: e.target.value === "none" ? null : Number(e.target.value) })}
            >
              {REMIND_OPTIONS.map((o) => (
                <option key={o.label} value={o.v === null ? "none" : String(o.v)}>
                  {o.label}
                </option>
              ))}
            </select>
          </label>
        </div>
        <label>
          备注
          <textarea rows={3} value={d.notes} onChange={(e) => set({ notes: e.target.value })} />
        </label>
        {d.repeat !== "none" && id && <p className="muted small">修改会作用于这个重复日程的所有发生。</p>}
        <div className="modal-actions">
          {id && (
            <button className="danger" onClick={remove}>
              删除
            </button>
          )}
          <span className="spacer" />
          <button onClick={onClose}>取消</button>
          <button className="primary" onClick={save}>
            保存
          </button>
        </div>
      </div>
    </div>
  );
}
