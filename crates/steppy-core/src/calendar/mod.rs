//! 日程：模型、合并、重复规则、提醒、.ics。
//!
//! 时间一律使用“本地浮动时间”（`NaiveDateTime`，即 iCalendar 里不带时区的 floating time）：
//! 同一个家庭组的电脑通常在同一个时区，每周重复的事件跨夏令时也能保持“每周一 9 点”。

pub mod ics;
pub mod merge;
pub mod recur;

use std::collections::BTreeMap;

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 机器标识（家庭组里的 node id）。
pub type NodeId = String;

/// 重复规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Repeat {
    #[default]
    None,
    Daily,
    Weekly,
    /// 工作日（周一到周五）。
    Weekdays,
}

/// 日程内容。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventData {
    pub title: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub all_day: bool,
    /// 开始时间（全天事件为当天 00:00）。
    pub start: NaiveDateTime,
    /// 结束时间，不含（全天事件为最后一天的次日 00:00）。
    pub end: NaiveDateTime,
    #[serde(default)]
    pub repeat: Repeat,
    /// 提前多少分钟提醒；None 表示不提醒。
    #[serde(default)]
    pub remind_before_min: Option<u32>,
}

impl EventData {
    /// 内容哈希，用于确定性地比较两份内容。
    pub fn content_hash(&self) -> [u8; 32] {
        let json = serde_json::to_vec(self).expect("serialize event");
        Sha256::digest(&json).into()
    }

    /// 修正明显不合法的输入：结束不早于开始；全天事件对齐到整天。
    pub fn normalized(mut self) -> Self {
        if self.all_day {
            let sd = self.start.date();
            let mut ed = self.end.date();
            if self.end.time() != chrono::NaiveTime::MIN {
                ed = ed.succ_opt().unwrap_or(ed);
            }
            if ed <= sd {
                ed = sd.succ_opt().unwrap_or(sd);
            }
            self.start = sd.and_time(chrono::NaiveTime::MIN);
            self.end = ed.and_time(chrono::NaiveTime::MIN);
        } else if self.end <= self.start {
            self.end = self.start + chrono::Duration::minutes(30);
        }
        self.title = self.title.trim().to_string();
        self
    }
}

/// 一条带版本的日程记录。`data == None` 表示已删除（墓碑）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventRecord {
    pub id: String,
    /// 版本向量：每台机器对这条日程做过几次修改。
    pub vv: BTreeMap<NodeId, u64>,
    pub data: Option<EventData>,
    /// 最后修改时间（毫秒），只用于并发冲突时确定性地挑选胜者。
    pub modified_ms: i64,
    pub modified_by: NodeId,
    /// 如果这是一条冲突副本，记录原日程 id。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict_of: Option<String>,
}

impl EventRecord {
    /// 新建。
    pub fn create(id: impl Into<String>, node: &str, data: EventData, now_ms: i64) -> Self {
        let mut vv = BTreeMap::new();
        vv.insert(node.to_string(), 1);
        EventRecord {
            id: id.into(),
            vv,
            data: Some(data.normalized()),
            modified_ms: now_ms,
            modified_by: node.to_string(),
            conflict_of: None,
        }
    }

    fn bump(&mut self, node: &str, now_ms: i64) {
        *self.vv.entry(node.to_string()).or_insert(0) += 1;
        self.modified_ms = now_ms;
        self.modified_by = node.to_string();
    }

    /// 本机修改。
    pub fn edit(&mut self, node: &str, data: EventData, now_ms: i64) {
        self.data = Some(data.normalized());
        self.bump(node, now_ms);
    }

    /// 本机删除：留下墓碑。
    pub fn delete(&mut self, node: &str, now_ms: i64) {
        self.data = None;
        self.bump(node, now_ms);
    }

    pub fn is_deleted(&self) -> bool {
        self.data.is_none()
    }

    /// 标量版本号（所有机器修改次数之和），用于显示。
    pub fn version(&self) -> u64 {
        self.vv.values().sum()
    }
}

/// 生成新的日程 id。
pub fn new_event_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn dt(y: i32, m: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
    }

    #[test]
    fn normalization() {
        let e = EventData {
            title: "  会议 ".into(),
            notes: String::new(),
            all_day: false,
            start: dt(2026, 11, 2, 10, 0),
            end: dt(2026, 11, 2, 9, 0),
            repeat: Repeat::None,
            remind_before_min: None,
        }
        .normalized();
        assert_eq!(e.title, "会议");
        assert_eq!(e.end, dt(2026, 11, 2, 10, 30));

        let all = EventData {
            title: "假期".into(),
            notes: String::new(),
            all_day: true,
            start: dt(2026, 11, 2, 13, 0),
            end: dt(2026, 11, 3, 8, 0),
            repeat: Repeat::None,
            remind_before_min: None,
        }
        .normalized();
        assert_eq!(all.start, dt(2026, 11, 2, 0, 0));
        assert_eq!(all.end, dt(2026, 11, 4, 0, 0));
    }

    #[test]
    fn edit_and_delete_bump_version() {
        let data = EventData {
            title: "a".into(),
            notes: String::new(),
            all_day: false,
            start: dt(2026, 1, 1, 9, 0),
            end: dt(2026, 1, 1, 10, 0),
            repeat: Repeat::None,
            remind_before_min: Some(10),
        };
        let mut r = EventRecord::create("e1", "A", data.clone(), 1);
        assert_eq!(r.version(), 1);
        r.edit("B", data, 2);
        assert_eq!(r.version(), 2);
        assert_eq!(r.vv.get("B"), Some(&1));
        r.delete("A", 3);
        assert!(r.is_deleted());
        assert_eq!(r.vv.get("A"), Some(&2));
    }
}
