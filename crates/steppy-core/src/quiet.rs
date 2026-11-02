//! 免打扰时段。

use chrono::NaiveTime;
use serde::{Deserialize, Serialize};

/// 一个每天重复的免打扰时段，例如 22:00–08:00（可以跨午夜）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuietPeriod {
    /// "HH:MM"
    pub start: String,
    /// "HH:MM"
    pub end: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

fn parse_hm(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

impl QuietPeriod {
    pub fn contains(&self, t: NaiveTime) -> bool {
        if !self.enabled {
            return false;
        }
        let (Some(s), Some(e)) = (parse_hm(&self.start), parse_hm(&self.end)) else { return false };
        if s == e {
            return false;
        }
        if s < e {
            t >= s && t < e
        } else {
            t >= s || t < e
        }
    }

    pub fn is_valid(&self) -> bool {
        parse_hm(&self.start).is_some() && parse_hm(&self.end).is_some()
    }
}

/// 当前是否处于任一免打扰时段。
pub fn is_quiet(periods: &[QuietPeriod], t: NaiveTime) -> bool {
    periods.iter().any(|p| p.contains(t))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str, e: &str) -> QuietPeriod {
        QuietPeriod { start: s.into(), end: e.into(), enabled: true }
    }
    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn same_day_period() {
        let q = p("12:00", "13:30");
        assert!(q.contains(t(12, 0)));
        assert!(q.contains(t(13, 29)));
        assert!(!q.contains(t(13, 30)));
        assert!(!q.contains(t(11, 59)));
    }

    #[test]
    fn overnight_period() {
        let q = p("22:00", "08:00");
        assert!(q.contains(t(23, 0)));
        assert!(q.contains(t(0, 0)));
        assert!(q.contains(t(7, 59)));
        assert!(!q.contains(t(8, 0)));
        assert!(!q.contains(t(21, 59)));
    }

    #[test]
    fn disabled_and_invalid() {
        let mut q = p("22:00", "08:00");
        q.enabled = false;
        assert!(!q.contains(t(23, 0)));
        assert!(!p("bad", "08:00").contains(t(7, 0)));
        assert!(!p("08:00", "08:00").contains(t(8, 0)));
        assert!(is_quiet(&[p("01:00", "02:00"), p("22:00", "23:00")], t(22, 30)));
        assert!(!is_quiet(&[], t(22, 30)));
    }
}
