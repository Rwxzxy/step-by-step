//! 日程：增删改查、同步、到点提醒（可稍后提醒）、.ics 导入导出。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Mutex;
use std::time::Duration;

use chrono::{Local, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use steppy_core::calendar::merge::merge_into;
use steppy_core::calendar::recur::{due_reminders, occurrences, DueReminder};
use steppy_core::calendar::{ics, new_event_id, EventData, EventRecord};
use steppy_core::net::wire::WireMsg;
use steppy_core::pet::Anim;

use crate::notify::{self, BubbleAction};
use crate::state::Shared;
use crate::storage;

/// 前端用的一次日程发生。
#[derive(Debug, Clone, Serialize)]
pub struct AgendaItem {
    pub event_id: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub data: EventData,
    pub version: u64,
    pub modified_by: String,
    pub conflict_of: Option<String>,
}

pub fn list(state: &Shared, from: NaiveDateTime, to: NaiveDateTime) -> Vec<AgendaItem> {
    let cal = state.calendar.lock().unwrap();
    let mut out = Vec::new();
    for r in cal.values() {
        let Some(d) = &r.data else { continue };
        for o in occurrences(&r.id, d, from, to) {
            out.push(AgendaItem {
                event_id: r.id.clone(),
                start: o.start,
                end: o.end,
                data: d.clone(),
                version: r.version(),
                modified_by: r.modified_by.clone(),
                conflict_of: r.conflict_of.clone(),
            });
        }
    }
    out.sort_by(|a, b| {
        b.data
            .all_day
            .cmp(&a.data.all_day)
            .then(a.start.cmp(&b.start))
            .then(a.event_id.cmp(&b.event_id))
    });
    out
}

pub fn today(state: &Shared) -> Vec<AgendaItem> {
    let d = Local::now().date_naive();
    let from = d.and_hms_opt(0, 0, 0).unwrap();
    list(state, from, from + chrono::Duration::days(1))
}

fn after_local_change(state: &Shared, changed: Vec<EventRecord>) {
    state.save_calendar();
    state.mark_dirty();
    if changed.is_empty() {
        return;
    }
    for peer in state.net.connected_peers() {
        state.net.send(
            &peer,
            WireMsg::Calendar {
                records: changed.clone(),
            },
        );
    }
}

/// 新建或修改。返回日程 id。
pub fn save(state: &Shared, id: Option<String>, data: EventData) -> anyhow::Result<String> {
    if data.title.trim().is_empty() {
        anyhow::bail!("标题不能为空");
    }
    let me = state.me().clone();
    let now = steppy_core::now_ms();
    let rec = {
        let mut cal = state.calendar.lock().unwrap();
        match id.and_then(|i| cal.get_mut(&i).map(|r| (i, r))) {
            Some((_, r)) => {
                r.edit(&me, data, now);
                // 编辑冲突副本即表示用户接受了它，不再标记为副本。
                r.conflict_of = None;
                r.clone()
            }
            None => {
                let r = EventRecord::create(new_event_id(), &me, data, now);
                cal.insert(r.id.clone(), r.clone());
                r
            }
        }
    };
    let id = rec.id.clone();
    after_local_change(state, vec![rec]);
    Ok(id)
}

pub fn delete(state: &Shared, id: &str) -> anyhow::Result<()> {
    let me = state.me().clone();
    let rec = {
        let mut cal = state.calendar.lock().unwrap();
        let r = cal
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("日程不存在"))?;
        r.delete(&me, steppy_core::now_ms());
        r.clone()
    };
    after_local_change(state, vec![rec]);
    Ok(())
}

pub fn get(state: &Shared, id: &str) -> Option<EventRecord> {
    state.calendar.lock().unwrap().get(id).cloned()
}

/// 收到其他机器的日程。合并后把有变化的记录转发给其他在线机器（帮助不直接相连的机器收敛）。
pub fn on_remote(state: &Shared, from: &str, records: Vec<EventRecord>) {
    let changed: Vec<EventRecord> = {
        let mut cal = state.calendar.lock().unwrap();
        let ids = merge_into(&mut cal, records);
        ids.iter().filter_map(|i| cal.get(i).cloned()).collect()
    };
    if changed.is_empty() {
        return;
    }
    state.save_calendar();
    state.mark_dirty();
    for peer in state.net.connected_peers() {
        if peer != from {
            state.net.send(
                &peer,
                WireMsg::Calendar {
                    records: changed.clone(),
                },
            );
        }
    }
}

pub fn all_records(state: &Shared) -> Vec<EventRecord> {
    state.calendar.lock().unwrap().values().cloned().collect()
}

pub fn to_local(t: chrono::DateTime<chrono::Utc>) -> NaiveDateTime {
    t.with_timezone(&Local).naive_local()
}

/// 导入 .ics 文本。已存在的同一 UID 日程会被更新。返回导入数量。
pub fn import_ics(state: &Shared, text: &str) -> anyhow::Result<usize> {
    let events = ics::import(text, &to_local).map_err(|e| anyhow::anyhow!("{e}"))?;
    let me = state.me().clone();
    let now = steppy_core::now_ms();
    let mut changed = Vec::new();
    {
        let mut cal = state.calendar.lock().unwrap();
        for ev in &events {
            match cal.get_mut(&ev.id) {
                Some(r) => {
                    if r.data.as_ref() != Some(&ev.data) {
                        r.edit(&me, ev.data.clone(), now);
                        changed.push(r.clone());
                    }
                }
                None => {
                    let r = EventRecord::create(ev.id.clone(), &me, ev.data.clone(), now);
                    cal.insert(r.id.clone(), r.clone());
                    changed.push(r);
                }
            }
        }
    }
    after_local_change(state, changed);
    Ok(events.len())
}

pub fn export_ics(state: &Shared) -> String {
    let cal = state.calendar.lock().unwrap();
    let recs: Vec<&EventRecord> = cal.values().collect();
    ics::export(&recs, chrono::Utc::now())
}

// ─────────────────────────── 提醒 ───────────────────────────

#[derive(Default)]
pub struct ReminderState {
    /// 已经提醒过的 key → 发生时间（用于清理）。
    fired: BTreeMap<String, NaiveDateTime>,
    /// 稍后提醒。
    snoozed: Vec<(NaiveDateTime, DueReminder)>,
    /// 气泡 id → 提醒（处理“稍后提醒”按钮）。
    bubbles: HashMap<String, DueReminder>,
}

#[derive(Serialize, Deserialize, Default)]
struct FiredFile {
    fired: BTreeMap<String, NaiveDateTime>,
}

pub static REMINDERS: Mutex<Option<ReminderState>> = Mutex::new(None);

fn fire(state: &Shared, r: &DueReminder, snoozed: bool) {
    let when = if r.all_day {
        "今天".to_string()
    } else {
        let mins = (r.occurrence_start - Local::now().naive_local()).num_minutes();
        if mins <= 0 {
            format!("{} 开始了", r.occurrence_start.format("%H:%M"))
        } else {
            format!(
                "{} 分钟后（{}）开始",
                mins,
                r.occurrence_start.format("%H:%M")
            )
        }
    };
    let text = format!(
        "{}「{}」{}",
        if snoozed { "再提醒一次：" } else { "" },
        r.title,
        when
    );
    let actions = vec![
        BubbleAction {
            id: "snooze:5".into(),
            label: "5 分钟后".into(),
        },
        BubbleAction {
            id: "snooze:15".into(),
            label: "15 分钟后".into(),
        },
        BubbleAction {
            id: "dismiss".into(),
            label: "知道了".into(),
        },
    ];
    if let Some(id) = notify::alert(
        state,
        "calendar",
        "日程提醒",
        &text,
        Some(Anim::Calendar),
        10_000,
        actions,
    ) {
        if let Some(rs) = REMINDERS.lock().unwrap().as_mut() {
            rs.bubbles.insert(id, r.clone());
        }
    }
}

/// 气泡按钮。
pub fn bubble_action(bubble_id: &str, action: &str) {
    let mut guard = REMINDERS.lock().unwrap();
    let Some(rs) = guard.as_mut() else { return };
    let Some(r) = rs.bubbles.remove(bubble_id) else {
        return;
    };
    if let Some(m) = action
        .strip_prefix("snooze:")
        .and_then(|m| m.parse::<i64>().ok())
    {
        let at = Local::now().naive_local() + chrono::Duration::minutes(m);
        rs.snoozed.push((at, r));
    }
}

pub fn start_reminders(state: Shared) {
    let fired = storage::read_json::<FiredFile>(&storage::path("reminders.json"))
        .unwrap_or_default()
        .fired;
    *REMINDERS.lock().unwrap() = Some(ReminderState {
        fired,
        ..Default::default()
    });
    std::thread::Builder::new()
        .name("reminders".into())
        .spawn(move || {
            // 启动时只回看 1 分钟，避免补发很久以前的提醒。
            let mut last = Local::now().naive_local() - chrono::Duration::minutes(1);
            loop {
                let now = Local::now().naive_local();
                if now > last {
                    check(&state, last, now);
                    last = now;
                } else {
                    // 系统时间被调回去：重置窗口。
                    last = now;
                }
                std::thread::sleep(Duration::from_secs(10));
            }
        })
        .expect("spawn reminders");
}

fn check(state: &Shared, after: NaiveDateTime, until: NaiveDateTime) {
    let enabled = state.settings().calendar_reminders;
    let due: Vec<DueReminder> = {
        let cal = state.calendar.lock().unwrap();
        due_reminders(cal.values(), after, until)
    };
    let mut to_fire = Vec::new();
    let snoozed_due: Vec<DueReminder>;
    {
        let mut guard = REMINDERS.lock().unwrap();
        let Some(rs) = guard.as_mut() else { return };
        for d in due {
            let key = d.key();
            if rs.fired.contains_key(&key) {
                continue;
            }
            rs.fired.insert(key, d.occurrence_start);
            to_fire.push(d);
        }
        let (ready, later): (Vec<_>, Vec<_>) =
            rs.snoozed.drain(..).partition(|(at, _)| *at <= until);
        rs.snoozed = later;
        snoozed_due = ready.into_iter().map(|(_, r)| r).collect();
        // 清理两天前的记录。
        let cutoff = until - chrono::Duration::days(2);
        rs.fired.retain(|_, t| *t >= cutoff);
        if !to_fire.is_empty() {
            let _ = storage::write_json(
                &storage::path("reminders.json"),
                &FiredFile {
                    fired: rs.fired.clone(),
                },
                false,
            );
        }
    }
    if !enabled {
        return;
    }
    // 已被删除的日程不再提醒（稍后提醒期间被删除）。
    let alive: BTreeSet<String> = state
        .calendar
        .lock()
        .unwrap()
        .values()
        .filter(|r| !r.is_deleted())
        .map(|r| r.id.clone())
        .collect();
    for r in to_fire {
        fire(state, &r, false);
    }
    for r in snoozed_due
        .into_iter()
        .filter(|r| alive.contains(&r.event_id))
    {
        fire(state, &r, true);
    }
}

/// 解析前端传来的本地时间字符串。
pub fn parse_local(s: &str) -> anyhow::Result<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M"))
        .or_else(|_| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d").map(|d| d.and_hms_opt(0, 0, 0).unwrap())
        })
        .map_err(|e| anyhow::anyhow!("时间格式不正确：{s}（{e}）"))
}
