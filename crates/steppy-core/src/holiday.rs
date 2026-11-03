//! 节日主题：根据日期算出今天该用哪个节日的外观。
//!
//! * 公历节日：元旦、情人节、劳动节、儿童节、国庆、万圣节、圣诞。
//! * 农历节日（用 tyme4rs 换算）：春节（除夕到正月初七）、元宵、端午、七夕、中秋、重阳。
//! * 节气：清明。
//! * 用户自定义的公历日期（比如家人生日）。
//!
//! 每个节日有一个“正日子”和前后的生效范围。几个节日的范围重叠时（比如中秋和国庆），
//! 正日子优先，其次按列表里的优先级。

use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use tyme4rs::tyme::lunar::LunarDay;
use tyme4rs::tyme::solar::{SolarDay, SolarTerm};

/// 节日日期规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// 公历月、日。
    Solar(u32, u32),
    /// 农历月、日（非闰月）。
    Lunar(u32, u32),
    /// 节气（名称）。
    Term(&'static str),
}

/// 内置节日。
#[derive(Debug, Clone, Copy)]
pub struct HolidayDef {
    pub id: &'static str,
    pub name: &'static str,
    pub greeting: &'static str,
    pub rule: Rule,
    /// 正日子之前几天开始生效。
    pub before: i64,
    /// 正日子之后几天仍然生效。
    pub after: i64,
}

/// 内置节日列表，越靠前优先级越高。
pub const HOLIDAYS: &[HolidayDef] = &[
    HolidayDef {
        id: "spring_festival",
        name: "春节",
        greeting: "新春快乐，万事顺意！",
        rule: Rule::Lunar(1, 1),
        before: 1,
        after: 6,
    },
    HolidayDef {
        id: "lantern",
        name: "元宵节",
        greeting: "元宵节快乐，记得吃汤圆！",
        rule: Rule::Lunar(1, 15),
        before: 0,
        after: 0,
    },
    HolidayDef {
        id: "dragon_boat",
        name: "端午节",
        greeting: "端午安康！",
        rule: Rule::Lunar(5, 5),
        before: 0,
        after: 0,
    },
    HolidayDef {
        id: "qixi",
        name: "七夕",
        greeting: "七夕快乐！",
        rule: Rule::Lunar(7, 7),
        before: 0,
        after: 0,
    },
    HolidayDef {
        id: "mid_autumn",
        name: "中秋节",
        greeting: "中秋快乐，月圆人团圆！",
        rule: Rule::Lunar(8, 15),
        before: 1,
        after: 1,
    },
    HolidayDef {
        id: "double_ninth",
        name: "重阳节",
        greeting: "重阳节快乐！",
        rule: Rule::Lunar(9, 9),
        before: 0,
        after: 0,
    },
    HolidayDef {
        id: "qingming",
        name: "清明",
        greeting: "清明时节，出门走走吧。",
        rule: Rule::Term("清明"),
        before: 0,
        after: 0,
    },
    HolidayDef {
        id: "new_year",
        name: "元旦",
        greeting: "新年快乐！",
        rule: Rule::Solar(1, 1),
        before: 1,
        after: 0,
    },
    HolidayDef {
        id: "valentine",
        name: "情人节",
        greeting: "情人节快乐！",
        rule: Rule::Solar(2, 14),
        before: 0,
        after: 0,
    },
    HolidayDef {
        id: "labor_day",
        name: "劳动节",
        greeting: "劳动节快乐，好好休息！",
        rule: Rule::Solar(5, 1),
        before: 0,
        after: 4,
    },
    HolidayDef {
        id: "children",
        name: "儿童节",
        greeting: "儿童节快乐！",
        rule: Rule::Solar(6, 1),
        before: 0,
        after: 0,
    },
    HolidayDef {
        id: "national_day",
        name: "国庆节",
        greeting: "国庆快乐！",
        rule: Rule::Solar(10, 1),
        before: 0,
        after: 6,
    },
    HolidayDef {
        id: "halloween",
        name: "万圣节",
        greeting: "不给糖就捣蛋！",
        rule: Rule::Solar(10, 31),
        before: 0,
        after: 0,
    },
    HolidayDef {
        id: "christmas",
        name: "圣诞节",
        greeting: "圣诞快乐！",
        rule: Rule::Solar(12, 25),
        before: 1,
        after: 0,
    },
];

/// 自定义节日（公历，每年重复）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomHoliday {
    pub name: String,
    pub month: u32,
    pub day: u32,
    /// 气泡里说的话；为空时用“{name}快乐！”。
    #[serde(default)]
    pub greeting: String,
}

/// 自定义节日的主题 id（共用一套占位外观）。
pub const CUSTOM_ID: &str = "custom";

/// 当前生效的节日。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveHoliday {
    /// 主题 id，前端据此选择外观。
    pub id: String,
    pub name: String,
    pub greeting: String,
    /// 今天是不是正日子。
    pub main_day: bool,
    /// 今年的正日子。
    pub date: NaiveDate,
}

fn to_naive(d: &SolarDay) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(
        d.get_year() as i32,
        d.get_month() as u32,
        d.get_day() as u32,
    )
}

/// 某个规则在某个公历年的正日子。农历节日按“落在这个公历年里的那一次”计算。
pub fn date_in_year(rule: Rule, year: i32) -> Option<NaiveDate> {
    match rule {
        Rule::Solar(m, d) => NaiveDate::from_ymd_opt(year, m, d),
        Rule::Lunar(m, d) => {
            // 农历正月初一总在公历 1 月下旬到 2 月中旬，农历日期与公历年基本一一对应；
            // 农历年 Y 的节日落在公历年 Y（腊月的节日不在列表里）。
            let lunar = LunarDay::new(year as isize, m as isize, d as usize).ok()?;
            to_naive(&lunar.get_solar_day())
        }
        Rule::Term(name) => {
            let term = SolarTerm::new(year as isize, name).ok()?;
            to_naive(&term.get_julian_day().get_solar_day())
        }
    }
}

fn window_hit(main: NaiveDate, before: i64, after: i64, today: NaiveDate) -> bool {
    today >= main - Duration::days(before) && today <= main + Duration::days(after)
}

/// 今天（`today`）生效的节日。自定义节日只在当天生效，并且优先于内置节日的“前后几天”，
/// 但不优先于内置节日的正日子。
pub fn active_on(today: NaiveDate, custom: &[CustomHoliday]) -> Option<ActiveHoliday> {
    let mut window_match: Option<ActiveHoliday> = None;
    for h in HOLIDAYS {
        // 跨年的生效范围（比如元旦前一天、春节后几天）需要看前后两年。
        for year in [today.year() - 1, today.year(), today.year() + 1] {
            let Some(main) = date_in_year(h.rule, year) else {
                continue;
            };
            if !window_hit(main, h.before, h.after, today) {
                continue;
            }
            let active = ActiveHoliday {
                id: h.id.to_string(),
                name: h.name.to_string(),
                greeting: h.greeting.to_string(),
                main_day: main == today,
                date: main,
            };
            if active.main_day {
                return Some(active);
            }
            if window_match.is_none() {
                window_match = Some(active);
            }
        }
    }
    for c in custom {
        if c.month == today.month() && c.day == today.day() {
            return Some(ActiveHoliday {
                id: CUSTOM_ID.to_string(),
                name: c.name.clone(),
                greeting: if c.greeting.trim().is_empty() {
                    format!("{}快乐！", c.name)
                } else {
                    c.greeting.clone()
                },
                main_day: true,
                date: today,
            });
        }
    }
    window_match
}

/// 用于“预览节日”：不管今天是几号，直接返回某个节日。
pub fn preview(id: &str, today: NaiveDate, custom: &[CustomHoliday]) -> Option<ActiveHoliday> {
    if id == CUSTOM_ID {
        let c = custom.first();
        let name = c
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "纪念日".to_string());
        return Some(ActiveHoliday {
            id: CUSTOM_ID.into(),
            greeting: format!("{name}快乐！"),
            name,
            main_day: true,
            date: today,
        });
    }
    let h = HOLIDAYS.iter().find(|h| h.id == id)?;
    Some(ActiveHoliday {
        id: h.id.into(),
        name: h.name.into(),
        greeting: h.greeting.into(),
        main_day: true,
        date: date_in_year(h.rule, today.year()).unwrap_or(today),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn id_on(date: NaiveDate) -> Option<String> {
        active_on(date, &[]).map(|a| a.id)
    }

    #[test]
    fn lunar_and_term_dates_match_the_real_calendar() {
        // 2026 年各节日的实际日期。
        assert_eq!(date_in_year(Rule::Lunar(1, 1), 2026), Some(d(2026, 2, 17)));
        assert_eq!(date_in_year(Rule::Lunar(1, 15), 2026), Some(d(2026, 3, 3)));
        assert_eq!(date_in_year(Rule::Term("清明"), 2026), Some(d(2026, 4, 5)));
        assert_eq!(date_in_year(Rule::Lunar(5, 5), 2026), Some(d(2026, 6, 19)));
        assert_eq!(date_in_year(Rule::Lunar(7, 7), 2026), Some(d(2026, 8, 19)));
        assert_eq!(date_in_year(Rule::Lunar(8, 15), 2026), Some(d(2026, 9, 25)));
        assert_eq!(date_in_year(Rule::Lunar(9, 9), 2026), Some(d(2026, 10, 18)));
        // 其他年份抽查。
        assert_eq!(date_in_year(Rule::Lunar(1, 1), 2027), Some(d(2027, 2, 6)));
        assert_eq!(date_in_year(Rule::Lunar(1, 1), 2025), Some(d(2025, 1, 29)));
        assert_eq!(date_in_year(Rule::Lunar(8, 15), 2025), Some(d(2025, 10, 6)));
        assert_eq!(date_in_year(Rule::Term("清明"), 2025), Some(d(2025, 4, 4)));
    }

    #[test]
    fn spring_festival_window_from_eve_to_seventh_day() {
        assert_eq!(id_on(d(2026, 2, 15)), None);
        let eve = active_on(d(2026, 2, 16), &[]).unwrap();
        assert_eq!(eve.id, "spring_festival");
        assert!(!eve.main_day);
        assert!(active_on(d(2026, 2, 17), &[]).unwrap().main_day);
        assert_eq!(id_on(d(2026, 2, 23)), Some("spring_festival".into()));
        assert_eq!(id_on(d(2026, 2, 24)), None);
    }

    #[test]
    fn overlapping_holidays_prefer_the_main_day() {
        // 2025 年中秋（10 月 6 日）落在国庆假期里：中秋前后一天显示中秋（优先级更高），
        // 国庆假期的其余日子显示国庆。
        assert_eq!(id_on(d(2025, 10, 6)), Some("mid_autumn".into()));
        assert_eq!(id_on(d(2025, 10, 5)), Some("mid_autumn".into()));
        assert_eq!(id_on(d(2025, 10, 1)), Some("national_day".into()));
        assert_eq!(id_on(d(2025, 10, 3)), Some("national_day".into()));
        // 国庆正日子永远是国庆。
        assert!(active_on(d(2025, 10, 1), &[]).unwrap().main_day);
        // 中秋前一天不在国庆范围内时显示中秋。
        assert_eq!(id_on(d(2026, 9, 24)), Some("mid_autumn".into()));
    }

    #[test]
    fn year_boundaries() {
        assert_eq!(id_on(d(2026, 12, 31)), Some("new_year".into()));
        assert_eq!(id_on(d(2027, 1, 1)), Some("new_year".into()));
        assert_eq!(id_on(d(2026, 12, 24)), Some("christmas".into()));
        assert_eq!(id_on(d(2026, 12, 26)), None);
        // 2025 年春节（1 月 29 日）前一天是除夕。
        assert_eq!(id_on(d(2025, 1, 28)), Some("spring_festival".into()));
    }

    #[test]
    fn ordinary_days_have_no_theme() {
        for date in [d(2026, 3, 15), d(2026, 7, 20), d(2026, 11, 3)] {
            assert_eq!(id_on(date), None, "{date}");
        }
    }

    #[test]
    fn custom_holidays() {
        let custom = vec![CustomHoliday {
            name: "妈妈生日".into(),
            month: 11,
            day: 3,
            greeting: String::new(),
        }];
        let a = active_on(d(2026, 11, 3), &custom).unwrap();
        assert_eq!(a.id, CUSTOM_ID);
        assert_eq!(a.greeting, "妈妈生日快乐！");
        // 内置节日正日子优先。
        let clash = vec![CustomHoliday {
            name: "结婚纪念日".into(),
            month: 12,
            day: 25,
            greeting: "纪念日快乐".into(),
        }];
        assert_eq!(active_on(d(2026, 12, 25), &clash).unwrap().id, "christmas");
        // 但优先于内置节日的“前后几天”。
        let near = vec![CustomHoliday {
            name: "生日".into(),
            month: 10,
            day: 3,
            greeting: String::new(),
        }];
        assert_eq!(active_on(d(2026, 10, 3), &near).unwrap().id, CUSTOM_ID);
    }

    #[test]
    fn preview_any_holiday() {
        let p = preview("mid_autumn", d(2026, 3, 1), &[]).unwrap();
        assert_eq!(p.name, "中秋节");
        assert_eq!(p.date, d(2026, 9, 25));
        assert_eq!(
            preview("custom", d(2026, 3, 1), &[]).unwrap().name,
            "纪念日"
        );
        assert!(preview("nope", d(2026, 3, 1), &[]).is_none());
        // 每个内置节日都能算出日期。
        for h in HOLIDAYS {
            for y in 2024..=2035 {
                assert!(date_in_year(h.rule, y).is_some(), "{} {y}", h.id);
            }
        }
    }
}
