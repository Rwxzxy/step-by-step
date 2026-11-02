//! 重复规则展开与提醒计算。

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use serde::{Deserialize, Serialize};

use super::{EventData, EventRecord, Repeat};

/// 一次具体发生。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Occurrence {
    pub event_id: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
}

fn occurs_on(data: &EventData, day: NaiveDate) -> bool {
    let first = data.start.date();
    if day < first {
        return false;
    }
    match data.repeat {
        Repeat::None => day == first,
        Repeat::Daily => true,
        Repeat::Weekly => day.weekday() == first.weekday(),
        Repeat::Weekdays => !matches!(day.weekday(), Weekday::Sat | Weekday::Sun),
    }
}

/// 列出与 `[from, to)` 有交集的所有发生。
pub fn occurrences(id: &str, data: &EventData, from: NaiveDateTime, to: NaiveDateTime) -> Vec<Occurrence> {
    let duration = data.end - data.start;
    let tod = data.start.time();
    let mut out = Vec::new();
    if to <= from {
        return out;
    }
    // 从 from 往前推一个事件长度，保证跨天的长事件也能被找到。
    let first_day = (from - duration - Duration::days(1)).date().max(data.start.date());
    let last_day = to.date();
    let mut day = first_day;
    let mut guard = 0;
    while day <= last_day && guard < 4000 {
        guard += 1;
        if occurs_on(data, day) {
            let start = day.and_time(tod);
            let end = start + duration;
            if start < to && end > from {
                out.push(Occurrence { event_id: id.to_string(), start, end });
            }
        }
        if data.repeat == Repeat::None && day >= data.start.date() {
            break;
        }
        day = match day.succ_opt() {
            Some(d) => d,
            None => break,
        };
    }
    out
}

/// 某天的所有日程发生，按开始时间排序（全天事件在前）。
pub fn day_agenda<'a>(records: impl IntoIterator<Item = &'a EventRecord>, day: NaiveDate) -> Vec<(Occurrence, &'a EventRecord)> {
    let from = day.and_time(NaiveTime::MIN);
    let to = from + Duration::days(1);
    let mut out: Vec<(Occurrence, &EventRecord)> = Vec::new();
    for r in records {
        if let Some(d) = &r.data {
            for o in occurrences(&r.id, d, from, to) {
                out.push((o, r));
            }
        }
    }
    out.sort_by(|a, b| {
        let ad = a.1.data.as_ref().map(|d| d.all_day).unwrap_or(false);
        let bd = b.1.data.as_ref().map(|d| d.all_day).unwrap_or(false);
        bd.cmp(&ad).then(a.0.start.cmp(&b.0.start)).then(a.0.event_id.cmp(&b.0.event_id))
    });
    out
}

/// 全天事件在当天几点提醒（再减去提前量）。
pub const ALL_DAY_REMIND_AT: (u32, u32) = (9, 0);

/// 一条到期的提醒。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DueReminder {
    pub event_id: String,
    pub title: String,
    pub occurrence_start: NaiveDateTime,
    pub remind_at: NaiveDateTime,
    pub all_day: bool,
}

impl DueReminder {
    /// 去重用的键：同一条日程的同一次发生只提醒一次。
    pub fn key(&self) -> String {
        format!("{}@{}", self.event_id, self.occurrence_start.format("%Y-%m-%dT%H:%M"))
    }
}

/// 某次发生的提醒时刻。
pub fn remind_time(data: &EventData, occ_start: NaiveDateTime) -> Option<NaiveDateTime> {
    let before = Duration::minutes(data.remind_before_min? as i64);
    let base = if data.all_day {
        occ_start.date().and_hms_opt(ALL_DAY_REMIND_AT.0, ALL_DAY_REMIND_AT.1, 0)?
    } else {
        occ_start
    };
    Some(base - before)
}

/// 找出提醒时刻落在 `(after, until]` 里的提醒。
pub fn due_reminders<'a>(records: impl IntoIterator<Item = &'a EventRecord>, after: NaiveDateTime, until: NaiveDateTime) -> Vec<DueReminder> {
    let mut out = Vec::new();
    if until <= after {
        return out;
    }
    for r in records {
        let Some(d) = &r.data else { continue };
        let Some(before) = d.remind_before_min else { continue };
        // 提醒时刻在 (after, until] → 发生时刻在 (after + before, until + before + 一天的余量]
        let pad = Duration::minutes(before as i64) + Duration::days(1);
        for o in occurrences(&r.id, d, after - Duration::days(1), until + pad) {
            if let Some(at) = remind_time(d, o.start) {
                if at > after && at <= until {
                    out.push(DueReminder {
                        event_id: r.id.clone(),
                        title: d.title.clone(),
                        occurrence_start: o.start,
                        remind_at: at,
                        all_day: d.all_day,
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| a.remind_at.cmp(&b.remind_at).then(a.event_id.cmp(&b.event_id)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(y: i32, m: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, mi, 0).unwrap()
    }

    fn ev(start: NaiveDateTime, end: NaiveDateTime, repeat: Repeat, remind: Option<u32>) -> EventData {
        EventData { title: "t".into(), notes: String::new(), all_day: false, start, end, repeat, remind_before_min: remind }
    }

    #[test]
    fn single_event() {
        let e = ev(dt(2026, 11, 2, 9, 0), dt(2026, 11, 2, 10, 0), Repeat::None, None);
        assert_eq!(occurrences("e", &e, dt(2026, 11, 1, 0, 0), dt(2026, 11, 8, 0, 0)).len(), 1);
        assert!(occurrences("e", &e, dt(2026, 11, 3, 0, 0), dt(2026, 11, 8, 0, 0)).is_empty());
        // 结束边界不含。
        assert!(occurrences("e", &e, dt(2026, 11, 2, 10, 0), dt(2026, 11, 2, 11, 0)).is_empty());
    }

    #[test]
    fn daily_weekly_weekdays() {
        // 2026-11-02 是周一。
        let start = dt(2026, 11, 2, 9, 0);
        let end = dt(2026, 11, 2, 9, 30);
        let week = (dt(2026, 11, 2, 0, 0), dt(2026, 11, 9, 0, 0));
        assert_eq!(occurrences("e", &ev(start, end, Repeat::Daily, None), week.0, week.1).len(), 7);
        let weekly = occurrences("e", &ev(start, end, Repeat::Weekly, None), dt(2026, 11, 1, 0, 0), dt(2026, 11, 30, 0, 0));
        assert_eq!(weekly.iter().map(|o| o.start.day()).collect::<Vec<_>>(), vec![2, 9, 16, 23]);
        let wd = occurrences("e", &ev(start, end, Repeat::Weekdays, None), week.0, week.1);
        assert_eq!(wd.len(), 5);
        assert!(wd.iter().all(|o| o.start.weekday().num_days_from_monday() < 5));
        // 不早于第一次。
        assert!(occurrences("e", &ev(start, end, Repeat::Daily, None), dt(2026, 10, 1, 0, 0), dt(2026, 11, 1, 0, 0)).is_empty());
    }

    #[test]
    fn overnight_event_seen_next_morning() {
        let e = ev(dt(2026, 11, 2, 23, 0), dt(2026, 11, 3, 1, 0), Repeat::None, None);
        assert_eq!(occurrences("e", &e, dt(2026, 11, 3, 0, 0), dt(2026, 11, 4, 0, 0)).len(), 1);
    }

    #[test]
    fn agenda_sorting() {
        let mk = |id: &str, data: EventData| EventRecord::create(id, "A", data, 0);
        let mut all_day = ev(dt(2026, 11, 2, 0, 0), dt(2026, 11, 3, 0, 0), Repeat::None, None);
        all_day.all_day = true;
        let recs = [
            mk("late", ev(dt(2026, 11, 2, 15, 0), dt(2026, 11, 2, 16, 0), Repeat::None, None)),
            mk("early", ev(dt(2026, 11, 2, 8, 0), dt(2026, 11, 2, 9, 0), Repeat::None, None)),
            mk("allday", all_day),
        ];
        let ids: Vec<_> = day_agenda(recs.iter(), NaiveDate::from_ymd_opt(2026, 11, 2).unwrap())
            .into_iter()
            .map(|(o, _)| o.event_id)
            .collect();
        assert_eq!(ids, vec!["allday", "early", "late"]);
    }

    #[test]
    fn reminders_fire_once_in_window() {
        let r = EventRecord::create("e", "A", ev(dt(2026, 11, 2, 9, 0), dt(2026, 11, 2, 10, 0), Repeat::Weekdays, Some(15)), 0);
        let due = due_reminders([&r], dt(2026, 11, 2, 8, 44), dt(2026, 11, 2, 8, 45));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].remind_at, dt(2026, 11, 2, 8, 45));
        assert_eq!(due[0].key(), "e@2026-11-02T09:00");
        // 下一个窗口不会重复。
        assert!(due_reminders([&r], dt(2026, 11, 2, 8, 45), dt(2026, 11, 2, 8, 46)).is_empty());
        // 周六不提醒。
        assert!(due_reminders([&r], dt(2026, 11, 7, 0, 0), dt(2026, 11, 8, 0, 0)).is_empty());
        // 跨午夜的窗口：00:10 的日程提前 30 分钟，在前一天 23:40 提醒。
        let mid = EventRecord::create("m", "A", ev(dt(2026, 11, 3, 0, 10), dt(2026, 11, 3, 0, 40), Repeat::None, Some(30)), 0);
        assert_eq!(due_reminders([&mid], dt(2026, 11, 2, 23, 0), dt(2026, 11, 2, 23, 59)).len(), 1);
    }

    #[test]
    fn all_day_reminder_time() {
        let mut d = ev(dt(2026, 11, 2, 0, 0), dt(2026, 11, 3, 0, 0), Repeat::None, Some(60));
        d.all_day = true;
        assert_eq!(remind_time(&d, dt(2026, 11, 2, 0, 0)), Some(dt(2026, 11, 2, 8, 0)));
        d.remind_before_min = None;
        assert_eq!(remind_time(&d, dt(2026, 11, 2, 0, 0)), None);
    }

    #[test]
    fn deleted_events_never_remind() {
        let mut r = EventRecord::create("e", "A", ev(dt(2026, 11, 2, 9, 0), dt(2026, 11, 2, 10, 0), Repeat::Daily, Some(5)), 0);
        r.delete("A", 1);
        assert!(due_reminders([&r], dt(2026, 11, 2, 0, 0), dt(2026, 11, 9, 0, 0)).is_empty());
    }
}
