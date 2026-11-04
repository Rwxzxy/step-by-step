//! 节日主题：每分钟检查一次日期，节日开始 / 结束时通知所有窗口切换外观；
//! 节日正日子（或生效期的第一天）宠物会开心地冒一个祝福气泡，每个节日每年只说一次。

use std::sync::Mutex;
use std::time::Duration;

use chrono::Datelike;
use serde::Serialize;
use steppy_core::holiday::{self, ActiveHoliday, HOLIDAYS};
use steppy_core::pet::Anim;

use crate::notify;
use crate::state::Shared;

static LAST: Mutex<Option<Option<ActiveHoliday>>> = Mutex::new(None);

fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

/// 当前应该显示的节日（考虑“预览”和“关闭节日主题”设置）。
pub fn current(state: &Shared) -> Option<ActiveHoliday> {
    let s = state.settings();
    if let Some(p) = s.holiday_preview.as_deref().filter(|p| !p.is_empty()) {
        return holiday::preview(p, today(), &s.custom_holidays);
    }
    if !s.holiday_theme {
        return None;
    }
    holiday::active_on(today(), &s.custom_holidays)
}

/// 重新计算；有变化就通知窗口，需要时说祝福语。
pub fn refresh(state: &Shared) {
    let now = current(state);
    let changed = {
        let mut last = LAST.lock().unwrap();
        let changed = last.as_ref() != Some(&now);
        *last = Some(now.clone());
        changed
    };
    if changed {
        state.emit("steppy://theme", now.clone());
        state.mark_dirty();
    }
    greet_once(state, now.as_ref());
}

fn greet_once(state: &Shared, h: Option<&ActiveHoliday>) {
    let Some(h) = h else { return };
    let s = state.settings();
    // 预览时不说祝福语。
    if s.holiday_preview.as_deref().is_some_and(|p| !p.is_empty()) {
        return;
    }
    let key = format!("{}:{}", h.date, h.id);
    if s.last_holiday_greeting.as_deref() == Some(key.as_str()) {
        return;
    }
    // 免打扰时段不消耗这次祝福，等时段结束后再说。
    if notify::quiet_now(state) {
        return;
    }
    if notify::alert(
        state,
        "holiday",
        &h.name,
        &h.greeting,
        Some(Anim::Happy),
        5_000,
        vec![],
    )
    .is_some()
    {
        let mut w = state.settings.write().unwrap();
        w.last_holiday_greeting = Some(key);
        w.save();
    }
}

pub fn start(state: Shared) {
    std::thread::Builder::new()
        .name("holiday".into())
        .spawn(move || {
            // 等宠物窗口加载好再第一次检查，否则祝福气泡会丢。
            std::thread::sleep(Duration::from_secs(5));
            loop {
                refresh(&state);
                std::thread::sleep(Duration::from_secs(60));
            }
        })
        .expect("spawn holiday thread");
}

/// 设置页“预览节日”下拉框的选项：内置节日和它今年（或明年）的日期。
#[derive(Debug, Clone, Serialize)]
pub struct HolidayInfo {
    pub id: String,
    pub name: String,
    /// 下一次正日子（今天或以后）。
    pub next_date: Option<chrono::NaiveDate>,
}

pub fn list() -> Vec<HolidayInfo> {
    let t = today();
    let mut out: Vec<HolidayInfo> = HOLIDAYS
        .iter()
        .map(|h| {
            let next = [t.year(), t.year() + 1]
                .into_iter()
                .filter_map(|y| holiday::date_in_year(h.rule, y))
                .find(|d| *d >= t);
            HolidayInfo {
                id: h.id.into(),
                name: h.name.into(),
                next_date: next,
            }
        })
        .collect();
    out.sort_by_key(|h| h.next_date);
    out.push(HolidayInfo {
        id: holiday::CUSTOM_ID.into(),
        name: "自定义纪念日".into(),
        next_date: None,
    });
    out
}
