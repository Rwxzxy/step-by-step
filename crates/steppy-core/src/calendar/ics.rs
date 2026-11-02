//! iCalendar (.ics, RFC 5545) 导入导出。
//!
//! 支持的子集：`VEVENT` 的 `UID`、`SUMMARY`、`DESCRIPTION`、`DTSTART`、`DTEND` / `DURATION`
//! （日期、浮动时间、UTC 时间、`TZID=` 时区）、`RRULE`（DAILY、WEEKLY、WEEKLY;BYDAY=MO..FR）、
//! `VALARM` 的 `TRIGGER`（相对开始时间的负偏移）。不支持的重复规则会降级为单次事件，
//! 并把原规则写进备注，不会悄悄丢失。

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};

use super::{EventData, EventRecord, Repeat};

/// 导出一组日程。墓碑不导出。
pub fn export(records: &[&EventRecord], now: DateTime<Utc>) -> String {
    let mut lines: Vec<String> = vec![
        "BEGIN:VCALENDAR".into(),
        "VERSION:2.0".into(),
        "PRODID:-//Steppy//Steppy 1.0//ZH".into(),
        "CALSCALE:GREGORIAN".into(),
        "X-WR-CALNAME:步步日程".into(),
    ];
    let stamp = now.format("%Y%m%dT%H%M%SZ").to_string();
    for r in records {
        let Some(d) = &r.data else { continue };
        lines.push("BEGIN:VEVENT".into());
        lines.push(format!("UID:{}@steppy", r.id));
        lines.push(format!("DTSTAMP:{stamp}"));
        if d.all_day {
            lines.push(format!("DTSTART;VALUE=DATE:{}", d.start.format("%Y%m%d")));
            lines.push(format!("DTEND;VALUE=DATE:{}", d.end.format("%Y%m%d")));
        } else {
            lines.push(format!("DTSTART:{}", d.start.format("%Y%m%dT%H%M%S")));
            lines.push(format!("DTEND:{}", d.end.format("%Y%m%dT%H%M%S")));
        }
        lines.push(format!("SUMMARY:{}", escape(&d.title)));
        if !d.notes.is_empty() {
            lines.push(format!("DESCRIPTION:{}", escape(&d.notes)));
        }
        match d.repeat {
            Repeat::None => {}
            Repeat::Daily => lines.push("RRULE:FREQ=DAILY".into()),
            Repeat::Weekly => lines.push("RRULE:FREQ=WEEKLY".into()),
            Repeat::Weekdays => lines.push("RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR".into()),
        }
        lines.push(format!("SEQUENCE:{}", r.version()));
        if let Some(m) = d.remind_before_min {
            lines.push("BEGIN:VALARM".into());
            lines.push("ACTION:DISPLAY".into());
            lines.push(format!("DESCRIPTION:{}", escape(&d.title)));
            lines.push(format!("TRIGGER:-PT{m}M"));
            lines.push("END:VALARM".into());
        }
        lines.push("END:VEVENT".into());
    }
    lines.push("END:VCALENDAR".into());
    let mut out = String::new();
    for l in lines {
        out.push_str(&fold(&l));
        out.push_str("\r\n");
    }
    out
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace("\r\n", "\\n").replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// 按 75 字节折行，不拆开 UTF-8 字符。
fn fold(line: &str) -> String {
    if line.len() <= 75 {
        return line.to_string();
    }
    let mut out = String::new();
    let mut count = 0;
    let mut first = true;
    for c in line.chars() {
        let limit = if first { 75 } else { 74 };
        if count + c.len_utf8() > limit {
            out.push_str("\r\n ");
            count = 0;
            first = false;
        }
        out.push(c);
        count += c.len_utf8();
    }
    out
}

/// 导入得到的一条日程。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedEvent {
    /// 日程 id：步步自己导出的文件保留原 id，其他来源由 UID 确定性生成。
    pub id: String,
    pub data: EventData,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum IcsError {
    #[error("不是 iCalendar 文件（缺少 BEGIN:VCALENDAR）")]
    NotCalendar,
}

struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

fn parse_prop(line: &str) -> Option<Prop> {
    // NAME;P1=V1;P2="V:2":VALUE —— 冒号可能出现在带引号的参数里。
    let mut in_quotes = false;
    let mut split = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                split = Some(i);
                break;
            }
            _ => {}
        }
    }
    let i = split?;
    let (head, value) = (&line[..i], &line[i + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_ascii_uppercase();
    let params = parts
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((k.trim().to_ascii_uppercase(), v.trim().trim_matches('"').to_string()))
        })
        .collect();
    Some(Prop { name, params, value: value.to_string() })
}

fn param<'a>(p: &'a Prop, key: &str) -> Option<&'a str> {
    p.params.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// 解析日期时间属性，返回 (本地时间, 是否全天)。
fn parse_dt(p: &Prop, to_local: &dyn Fn(DateTime<Utc>) -> NaiveDateTime) -> Option<(NaiveDateTime, bool)> {
    let v = p.value.trim();
    if param(p, "VALUE") == Some("DATE") || (v.len() == 8 && v.chars().all(|c| c.is_ascii_digit())) {
        let d = NaiveDate::parse_from_str(v, "%Y%m%d").ok()?;
        return Some((d.and_time(NaiveTime::MIN), true));
    }
    if let Some(utc) = v.strip_suffix('Z') {
        let n = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some((to_local(Utc.from_utc_datetime(&n)), false));
    }
    let n = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()?;
    if let Some(tzid) = param(p, "TZID") {
        if let Ok(tz) = tzid.parse::<chrono_tz::Tz>() {
            if let Some(t) = tz.from_local_datetime(&n).earliest() {
                return Some((to_local(t.with_timezone(&Utc)), false));
            }
        }
        // 未知时区：按浮动时间处理。
    }
    Some((n, false))
}

/// 解析 RFC 5545 duration（如 `PT1H30M`、`P1D`、`-PT15M`），返回分钟数（带符号）。
pub fn parse_duration_min(s: &str) -> Option<i64> {
    let s = s.trim();
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let rest = rest.strip_prefix('P')?;
    let mut total = 0i64;
    let mut num = String::new();
    let mut in_time = false;
    for c in rest.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => num.push(c),
            'W' | 'D' | 'H' | 'M' | 'S' => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += match (c, in_time) {
                    ('W', _) => n * 7 * 24 * 60,
                    ('D', _) => n * 24 * 60,
                    ('H', true) => n * 60,
                    ('M', true) => n,
                    ('S', true) => n / 60,
                    _ => return None,
                };
            }
            _ => return None,
        }
    }
    Some(if neg { -total } else { total })
}

fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
            out.last_mut().unwrap().push_str(&line[1..]);
        } else if !line.is_empty() {
            out.push(line.to_string());
        }
    }
    out
}

/// 导入 .ics。`to_local` 把 UTC 时间换成本地浮动时间。
pub fn import(text: &str, to_local: &dyn Fn(DateTime<Utc>) -> NaiveDateTime) -> Result<Vec<ImportedEvent>, IcsError> {
    let lines = unfold(text.trim_start_matches('\u{feff}'));
    if !lines.iter().any(|l| l.eq_ignore_ascii_case("BEGIN:VCALENDAR")) {
        return Err(IcsError::NotCalendar);
    }
    let mut out = Vec::new();
    let mut in_event = false;
    let mut in_alarm = false;
    let mut cur: Vec<Prop> = Vec::new();
    let mut alarm_trigger: Option<i64> = None;
    for line in lines {
        let upper = line.to_ascii_uppercase();
        match upper.as_str() {
            "BEGIN:VEVENT" => {
                in_event = true;
                cur.clear();
                alarm_trigger = None;
                continue;
            }
            "END:VEVENT" => {
                if in_event {
                    if let Some(ev) = build_event(&cur, alarm_trigger, to_local) {
                        out.push(ev);
                    }
                }
                in_event = false;
                continue;
            }
            "BEGIN:VALARM" => {
                in_alarm = true;
                continue;
            }
            "END:VALARM" => {
                in_alarm = false;
                continue;
            }
            _ => {}
        }
        if !in_event {
            continue;
        }
        let Some(p) = parse_prop(&line) else { continue };
        if in_alarm {
            if p.name == "TRIGGER" && param(&p, "VALUE") != Some("DATE-TIME") && param(&p, "RELATED") != Some("END") {
                if let Some(m) = parse_duration_min(&p.value) {
                    // 取最早的一个提醒。
                    let before = (-m).max(0);
                    alarm_trigger = Some(alarm_trigger.map_or(before, |x| x.max(before)));
                }
            }
        } else {
            cur.push(p);
        }
    }
    Ok(out)
}

fn build_event(props: &[Prop], alarm: Option<i64>, to_local: &dyn Fn(DateTime<Utc>) -> NaiveDateTime) -> Option<ImportedEvent> {
    let get = |n: &str| props.iter().find(|p| p.name == n);
    let uid = get("UID").map(|p| p.value.trim().to_string()).unwrap_or_default();
    let (start, all_day) = parse_dt(get("DTSTART")?, to_local)?;
    let end = match get("DTEND").and_then(|p| parse_dt(p, to_local)) {
        Some((e, _)) => e,
        None => match get("DURATION").and_then(|p| parse_duration_min(&p.value)) {
            Some(m) => start + Duration::minutes(m),
            None if all_day => start + Duration::days(1),
            None => start + Duration::minutes(30),
        },
    };
    let title = get("SUMMARY").map(|p| unescape(&p.value)).unwrap_or_else(|| "（无标题）".into());
    let mut notes = get("DESCRIPTION").map(|p| unescape(&p.value)).unwrap_or_default();
    let repeat = match get("RRULE") {
        None => Repeat::None,
        Some(p) => match parse_rrule(&p.value) {
            Some(r) => r,
            None => {
                if !notes.is_empty() {
                    notes.push('\n');
                }
                notes.push_str(&format!("（原重复规则未支持，已按单次导入：{}）", p.value));
                Repeat::None
            }
        },
    };
    let id = match uid.strip_suffix("@steppy") {
        Some(own) if !own.is_empty() => own.to_string(),
        _ if !uid.is_empty() => uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, format!("ics:{uid}").as_bytes()).to_string(),
        _ => uuid::Uuid::new_v4().to_string(),
    };
    Some(ImportedEvent {
        id,
        data: EventData {
            title,
            notes,
            all_day,
            start,
            end,
            repeat,
            remind_before_min: alarm.map(|m| m.min(u32::MAX as i64) as u32),
        }
        .normalized(),
    })
}

fn parse_rrule(v: &str) -> Option<Repeat> {
    let mut freq = None;
    let mut byday: Option<String> = None;
    for part in v.split(';') {
        let (k, val) = part.split_once('=')?;
        match k.to_ascii_uppercase().as_str() {
            "FREQ" => freq = Some(val.to_ascii_uppercase()),
            "BYDAY" => byday = Some(val.to_ascii_uppercase()),
            "INTERVAL" if val != "1" => return None,
            // 有结束条件的重复我们表达不了，交给调用方降级处理。
            "COUNT" | "UNTIL" => return None,
            "WKST" => {}
            _ => return None,
        }
    }
    match (freq.as_deref()?, byday.as_deref()) {
        ("DAILY", None) => Some(Repeat::Daily),
        ("WEEKLY", None) => Some(Repeat::Weekly),
        ("WEEKLY", Some(days)) => {
            let mut d: Vec<&str> = days.split(',').collect();
            d.sort();
            if d == ["FR", "MO", "TH", "TU", "WE"] {
                Some(Repeat::Weekdays)
            } else if d.len() == 1 {
                Some(Repeat::Weekly)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc8(t: DateTime<Utc>) -> NaiveDateTime {
        (t + Duration::hours(8)).naive_utc()
    }

    fn dt(y: i32, m: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, mi, 0).unwrap()
    }

    #[test]
    fn roundtrip() {
        let mut long = EventData {
            title: "周会; 讨论, 季度计划 \\ 很长很长很长很长很长很长很长很长很长很长很长很长很长的标题".into(),
            notes: "第一行\n第二行".into(),
            all_day: false,
            start: dt(2026, 11, 2, 9, 0),
            end: dt(2026, 11, 2, 10, 0),
            repeat: Repeat::Weekdays,
            remind_before_min: Some(15),
        };
        let a = EventRecord::create("id-a", "A", long.clone(), 0);
        long.all_day = true;
        long.repeat = Repeat::Weekly;
        long.remind_before_min = None;
        long.title = "全天".into();
        let b = EventRecord::create("id-b", "A", long, 0);
        let mut c = EventRecord::create("id-c", "A", a.data.clone().unwrap(), 0);
        c.delete("A", 1);
        let text = export(&[&a, &b, &c], Utc::now());
        assert!(text.lines().all(|l| l.len() <= 76), "折行后每行不超过 75 字节");
        assert!(text.contains("\r\n"));
        let imported = import(&text, &utc8).unwrap();
        assert_eq!(imported.len(), 2, "墓碑不导出");
        assert_eq!(imported[0].id, "id-a");
        assert_eq!(&imported[0].data, a.data.as_ref().unwrap());
        assert_eq!(imported[1].id, "id-b");
        assert_eq!(&imported[1].data, b.data.as_ref().unwrap());
    }

    #[test]
    fn import_foreign_calendar() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Other//EN\r\nBEGIN:VTIMEZONE\r\nTZID:Asia/Shanghai\r\nEND:VTIMEZONE\r\n\
BEGIN:VEVENT\r\nUID:abc-123@example.com\r\nDTSTART:20261103T010000Z\r\nDURATION:PT1H30M\r\nSUMMARY:UTC 会议\r\n\
BEGIN:VALARM\r\nTRIGGER:-PT10M\r\nACTION:DISPLAY\r\nEND:VALARM\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:tz@example.com\r\nDTSTART;TZID=America/New_York:20261103T090000\r\nDTEND;TZID=America/New_York:20261103T100000\r\nSUMMARY:纽约时间\r\nRRULE:FREQ=MONTHLY;BYMONTHDAY=3\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:allday@example.com\r\nDTSTART;VALUE=DATE:20261105\r\nSUMMARY:全天\r\n  折行的标题\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR\r\nEND:VEVENT\r\n\
END:VCALENDAR\r\n";
        let evs = import(ics, &utc8).unwrap();
        assert_eq!(evs.len(), 3);
        assert_eq!(evs[0].data.start, dt(2026, 11, 3, 9, 0));
        assert_eq!(evs[0].data.end, dt(2026, 11, 3, 10, 30));
        assert_eq!(evs[0].data.remind_before_min, Some(10));
        // 同一 UID 每次导入得到同一 id（重复导入会更新而不是重复添加）。
        assert_eq!(evs[0].id, import(ics, &utc8).unwrap()[0].id);
        // 纽约 9:00（EST，UTC-5）= 北京时间 22:00。
        assert_eq!(evs[1].data.start, dt(2026, 11, 3, 22, 0));
        assert_eq!(evs[1].data.repeat, Repeat::None);
        assert!(evs[1].data.notes.contains("FREQ=MONTHLY"));
        assert!(evs[2].data.all_day);
        assert_eq!(evs[2].data.title, "全天 折行的标题");
        assert_eq!(evs[2].data.end, dt(2026, 11, 6, 0, 0));
        assert_eq!(evs[2].data.repeat, Repeat::Weekdays);
    }

    #[test]
    fn rejects_non_calendar() {
        assert_eq!(import("hello", &utc8), Err(IcsError::NotCalendar));
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration_min("-PT15M"), Some(-15));
        assert_eq!(parse_duration_min("PT1H30M"), Some(90));
        assert_eq!(parse_duration_min("-P1D"), Some(-1440));
        assert_eq!(parse_duration_min("P1W"), Some(10080));
        assert_eq!(parse_duration_min("P1DT2H"), Some(1560));
        assert_eq!(parse_duration_min("junk"), None);
    }
}
