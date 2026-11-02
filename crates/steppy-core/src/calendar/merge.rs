//! 日程合并规则（纯函数）。
//!
//! 每条日程带一个版本向量（每台机器的修改计数）。两份副本合并时：
//!
//! 1. 一方的版本向量支配另一方（包含了对方见过的所有修改）→ 取新的那份。
//! 2. 版本向量相同 → 是同一个版本，内容若意外不同按下面的确定性规则选。
//! 3. 并发修改（两台机器离线时各改了一次）：版本向量取逐项最大值，然后
//!    * 一边删除、一边修改 → 保留修改（删除让位于数据，不丢数据）；
//!    * 两边都删除 → 墓碑；
//!    * 两边内容相同 → 直接合并；
//!    * 两边改成了不同内容 → 按 (修改时间, 修改机器, 内容哈希) 选出胜者，
//!      落败的内容另存为一条“冲突副本”日程（id 由原 id 与内容哈希确定性地生成），
//!      这样任何一方的修改都不会丢，用户可以自己删掉不要的那条。
//!
//! 规则只依赖两份输入本身，所以无论哪台机器、以什么顺序合并，结果都相同（交换律、幂等）。

use std::cmp::Ordering;
use std::collections::BTreeMap;

use super::{EventData, EventRecord};

/// 版本向量的偏序关系。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Causality {
    Equal,
    /// 左边更新（支配右边）。
    After,
    /// 右边更新。
    Before,
    Concurrent,
}

pub fn compare_vv(a: &BTreeMap<String, u64>, b: &BTreeMap<String, u64>) -> Causality {
    let mut a_bigger = false;
    let mut b_bigger = false;
    for k in a.keys().chain(b.keys()) {
        let x = a.get(k).copied().unwrap_or(0);
        let y = b.get(k).copied().unwrap_or(0);
        match x.cmp(&y) {
            Ordering::Greater => a_bigger = true,
            Ordering::Less => b_bigger = true,
            Ordering::Equal => {}
        }
    }
    match (a_bigger, b_bigger) {
        (false, false) => Causality::Equal,
        (true, false) => Causality::After,
        (false, true) => Causality::Before,
        (true, true) => Causality::Concurrent,
    }
}

fn vv_max(a: &BTreeMap<String, u64>, b: &BTreeMap<String, u64>) -> BTreeMap<String, u64> {
    let mut out = a.clone();
    for (k, v) in b {
        let e = out.entry(k.clone()).or_insert(0);
        if *v > *e {
            *e = *v;
        }
    }
    out
}

/// 确定性的全序：用于并发冲突时挑胜者。
fn tiebreak_key(r: &EventRecord) -> (i64, &str, [u8; 32], bool) {
    let hash = r.data.as_ref().map(|d| d.content_hash()).unwrap_or([0; 32]);
    (
        r.modified_ms,
        r.modified_by.as_str(),
        hash,
        r.data.is_some(),
    )
}

fn pick<'a>(a: &'a EventRecord, b: &'a EventRecord) -> (&'a EventRecord, &'a EventRecord) {
    if tiebreak_key(a) >= tiebreak_key(b) {
        (a, b)
    } else {
        (b, a)
    }
}

/// 冲突副本的确定性 id。
pub fn conflict_copy_id(original_id: &str, loser: &EventData) -> String {
    let hash = hex::encode(&loser.content_hash()[..8]);
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("steppy-conflict:{original_id}:{hash}").as_bytes(),
    )
    .to_string()
}

/// 冲突副本的标题前缀。
pub const CONFLICT_PREFIX: &str = "（冲突副本）";

fn conflict_copy(original: &EventRecord, loser: &EventRecord) -> Option<EventRecord> {
    let data = loser.data.as_ref()?;
    let mut copy_data = data.clone();
    if !copy_data.title.starts_with(CONFLICT_PREFIX) {
        copy_data.title = format!("{CONFLICT_PREFIX}{}", copy_data.title);
    }
    Some(EventRecord {
        id: conflict_copy_id(&original.id, data),
        // 空版本向量：之后在任何机器上编辑这条副本都会自然地支配它。
        vv: BTreeMap::new(),
        data: Some(copy_data),
        modified_ms: loser.modified_ms,
        modified_by: loser.modified_by.clone(),
        conflict_of: Some(original.id.clone()),
    })
}

/// 两份同一 id 的记录的合并结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    pub record: EventRecord,
    /// 并发且内容冲突时，落败一方的内容。
    pub conflict_copy: Option<EventRecord>,
}

/// 合并同一条日程的两份副本。
pub fn merge_record(a: &EventRecord, b: &EventRecord) -> Merged {
    assert_eq!(a.id, b.id, "只能合并同一条日程");
    match compare_vv(&a.vv, &b.vv) {
        Causality::After => Merged {
            record: a.clone(),
            conflict_copy: None,
        },
        Causality::Before => Merged {
            record: b.clone(),
            conflict_copy: None,
        },
        Causality::Equal => {
            let (w, _) = pick(a, b);
            Merged {
                record: w.clone(),
                conflict_copy: None,
            }
        }
        Causality::Concurrent => {
            let vv = vv_max(&a.vv, &b.vv);
            let (winner, loser, copy) = match (&a.data, &b.data) {
                (None, None) => {
                    let (w, l) = pick(a, b);
                    (w, l, None)
                }
                (Some(_), None) => (a, b, None),
                (None, Some(_)) => (b, a, None),
                (Some(x), Some(y)) if x == y => {
                    let (w, l) = pick(a, b);
                    (w, l, None)
                }
                (Some(_), Some(_)) => {
                    let (w, l) = pick(a, b);
                    (w, l, conflict_copy(w, l))
                }
            };
            let _ = loser;
            let mut record = winner.clone();
            record.vv = vv;
            Merged {
                record,
                conflict_copy: copy,
            }
        }
    }
}

/// 把一批收到的记录合并进本地日程表。返回发生变化的 id（包括新产生的冲突副本）。
///
/// 冲突副本本身也走同样的合并：两台机器各自生成的同一个副本完全相同，合并后不变。
pub fn merge_into(
    local: &mut BTreeMap<String, EventRecord>,
    incoming: impl IntoIterator<Item = EventRecord>,
) -> Vec<String> {
    let mut changed = Vec::new();
    let mut queue: Vec<EventRecord> = incoming.into_iter().collect();
    while let Some(rec) = queue.pop() {
        match local.get(&rec.id) {
            None => {
                changed.push(rec.id.clone());
                local.insert(rec.id.clone(), rec);
            }
            Some(existing) => {
                let m = merge_record(existing, &rec);
                if let Some(copy) = m.conflict_copy {
                    queue.push(copy);
                }
                if &m.record != existing {
                    changed.push(rec.id.clone());
                    local.insert(rec.id.clone(), m.record);
                }
            }
        }
    }
    changed.sort();
    changed.dedup();
    changed
}

#[cfg(test)]
mod tests {
    use super::super::Repeat;
    use super::*;
    use chrono::NaiveDate;

    fn data(title: &str) -> EventData {
        EventData {
            title: title.into(),
            notes: String::new(),
            all_day: false,
            start: NaiveDate::from_ymd_opt(2026, 11, 3)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap(),
            end: NaiveDate::from_ymd_opt(2026, 11, 3)
                .unwrap()
                .and_hms_opt(10, 0, 0)
                .unwrap(),
            repeat: Repeat::None,
            remind_before_min: Some(10),
        }
    }

    fn base() -> EventRecord {
        EventRecord::create("e1", "A", data("周会"), 1_000)
    }

    #[test]
    fn vv_comparison() {
        let m = |xs: &[(&str, u64)]| {
            xs.iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(
            compare_vv(&m(&[("A", 1)]), &m(&[("A", 1)])),
            Causality::Equal
        );
        assert_eq!(
            compare_vv(&m(&[("A", 2)]), &m(&[("A", 1)])),
            Causality::After
        );
        assert_eq!(
            compare_vv(&m(&[("A", 1)]), &m(&[("A", 1), ("B", 1)])),
            Causality::Before
        );
        assert_eq!(
            compare_vv(&m(&[("A", 2)]), &m(&[("A", 1), ("B", 1)])),
            Causality::Concurrent
        );
        assert_eq!(compare_vv(&m(&[]), &m(&[("A", 0)])), Causality::Equal);
    }

    #[test]
    fn sequential_edit_wins() {
        let a = base();
        let mut b = a.clone();
        b.edit("B", data("周会（改到 10 点）"), 2_000);
        let m = merge_record(&a, &b);
        assert_eq!(m.record, b);
        assert!(m.conflict_copy.is_none());
        assert_eq!(merge_record(&b, &a).record, b);
    }

    #[test]
    fn newer_edit_wins_even_with_older_clock() {
        // B 的电脑时钟慢，但它是在看到 A 的修改之后再改的：版本向量说了算，与时间无关。
        let a = base();
        let mut b = a.clone();
        b.edit("B", data("B 改的"), 10);
        assert_eq!(merge_record(&a, &b).record.data.unwrap().title, "B 改的");
    }

    #[test]
    fn concurrent_edits_are_deterministic_and_keep_both() {
        let orig = base();
        let mut a = orig.clone();
        a.edit("A", data("A 的标题"), 5_000);
        let mut b = orig.clone();
        b.edit("B", data("B 的标题"), 6_000);

        let ab = merge_record(&a, &b);
        let ba = merge_record(&b, &a);
        assert_eq!(ab, ba, "合并必须满足交换律");
        // 时间更晚的 B 胜出。
        assert_eq!(ab.record.data.as_ref().unwrap().title, "B 的标题");
        assert_eq!(ab.record.vv.get("A"), Some(&2));
        assert_eq!(ab.record.vv.get("B"), Some(&1));
        // A 的内容作为冲突副本保留。
        let copy = ab.conflict_copy.unwrap();
        assert_eq!(copy.data.as_ref().unwrap().title, "（冲突副本）A 的标题");
        assert_eq!(copy.conflict_of.as_deref(), Some("e1"));
        assert_eq!(copy.id, conflict_copy_id("e1", a.data.as_ref().unwrap()));
    }

    #[test]
    fn concurrent_same_time_uses_node_then_hash() {
        let orig = base();
        let mut a = orig.clone();
        a.edit("A", data("甲"), 5_000);
        let mut b = orig.clone();
        b.edit("B", data("乙"), 5_000);
        let m = merge_record(&a, &b);
        assert_eq!(m.record.modified_by, "B");
        assert_eq!(m, merge_record(&b, &a));
    }

    #[test]
    fn concurrent_delete_vs_edit_keeps_edit() {
        let orig = base();
        let mut a = orig.clone();
        a.delete("A", 9_000);
        let mut b = orig.clone();
        b.edit("B", data("还要开"), 5_000);
        let m = merge_record(&a, &b);
        assert_eq!(m, merge_record(&b, &a));
        assert_eq!(m.record.data.as_ref().unwrap().title, "还要开");
        assert!(m.conflict_copy.is_none());
        // 之后任一方再删除，删除会生效。
        let mut later = m.record.clone();
        later.delete("A", 10_000);
        assert!(merge_record(&m.record, &later).record.is_deleted());
    }

    #[test]
    fn tombstone_beats_older_version() {
        let orig = base();
        let mut a = orig.clone();
        a.delete("A", 2_000);
        let m = merge_record(&orig, &a);
        assert!(m.record.is_deleted());
        // 墓碑不会被旧数据复活。
        let mut local = BTreeMap::new();
        local.insert("e1".to_string(), a.clone());
        let changed = merge_into(&mut local, vec![orig]);
        assert!(changed.is_empty());
        assert!(local["e1"].is_deleted());
    }

    #[test]
    fn merge_is_idempotent_and_set_level_converges() {
        let orig = base();
        let mut a = orig.clone();
        a.edit("A", data("A 版"), 5_000);
        let mut b = orig.clone();
        b.edit("B", data("B 版"), 6_000);
        let other = EventRecord::create("e2", "B", data("新日程"), 7_000);

        // 机器 A 的视角：收到 B 的两条。
        let mut on_a = BTreeMap::new();
        on_a.insert(a.id.clone(), a.clone());
        merge_into(&mut on_a, vec![b.clone(), other.clone()]);

        // 机器 B 的视角：收到 A 的一条。
        let mut on_b = BTreeMap::new();
        on_b.insert(b.id.clone(), b.clone());
        on_b.insert(other.id.clone(), other.clone());
        merge_into(&mut on_b, vec![a.clone()]);

        assert_eq!(on_a, on_b, "两边最终状态必须一致");
        assert_eq!(on_a.len(), 3, "原日程 + 新日程 + 冲突副本");

        // 再互相同步一次（包括副本），不再变化。
        let snapshot = on_a.clone();
        let changed = merge_into(&mut on_a, on_b.values().cloned().collect::<Vec<_>>());
        assert!(changed.is_empty());
        assert_eq!(on_a, snapshot);
    }

    #[test]
    fn three_way_concurrent_converges_regardless_of_order() {
        let orig = base();
        let mut a = orig.clone();
        a.edit("A", data("A"), 1_100);
        let mut b = orig.clone();
        b.edit("B", data("B"), 1_200);
        let mut c = orig.clone();
        c.edit("C", data("C"), 1_300);

        let orders = [
            vec![a.clone(), b.clone(), c.clone()],
            vec![c.clone(), a.clone(), b.clone()],
            vec![b.clone(), c.clone(), a.clone()],
        ];
        let mut results = Vec::new();
        for order in orders {
            let mut m = BTreeMap::new();
            merge_into(&mut m, order);
            results.push(m);
        }
        assert_eq!(results[0]["e1"], results[1]["e1"]);
        assert_eq!(results[1]["e1"], results[2]["e1"]);
        assert_eq!(results[0]["e1"].data.as_ref().unwrap().title, "C");
        // 主记录相同；每种顺序下 A、B 的内容都至少以冲突副本保留（不丢数据）。
        for r in &results {
            let titles: Vec<_> = r
                .values()
                .filter_map(|x| x.data.as_ref())
                .map(|d| d.title.clone())
                .collect();
            assert!(titles.contains(&"（冲突副本）A".to_string()), "{titles:?}");
            assert!(titles.contains(&"（冲突副本）B".to_string()), "{titles:?}");
        }
    }

    #[test]
    fn identical_concurrent_edits_do_not_create_copies() {
        let orig = base();
        let mut a = orig.clone();
        a.edit("A", data("同样的修改"), 5_000);
        let mut b = orig.clone();
        b.edit("B", data("同样的修改"), 6_000);
        let m = merge_record(&a, &b);
        assert!(m.conflict_copy.is_none());
        assert_eq!(m.record.data.unwrap().title, "同样的修改");
    }
}
