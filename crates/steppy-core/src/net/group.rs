//! 家庭组：成员名单、组密钥（按 epoch 轮换）、各机器的相对位置。
//!
//! 组状态在成员之间同步，合并规则是确定性的：
//! * 成员：并集；一旦被移除（墓碑）永远是移除状态；成员自己的资料按 `info_version` 取新。
//! * 组密钥：按 epoch 并集；同一 epoch 出现两把不同的密钥（两台机器同时踢人）时取哈希较大者。
//!   如果当前密钥的持有者里有已被移除的成员，就需要再轮换一次（[`GroupState::needs_rekey`]）。
//! * 位置排列：按 (version, 修改者) 取新。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{random_bytes, Identity, NodeId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub node_id: NodeId,
    /// Ed25519 公钥（十六进制）。
    pub pubkey: String,
    pub machine_name: String,
    pub owner_name: String,
    /// 宠物主色调。
    #[serde(default)]
    pub hue: u16,
    #[serde(default)]
    pub info_version: u64,
    pub joined_ms: i64,
    #[serde(default)]
    pub removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupKey {
    pub epoch: u32,
    /// 32 字节密钥（十六进制）。
    pub key: String,
    /// 生成这把密钥时的成员（知道这把密钥的机器）。
    pub holders: Vec<NodeId>,
}

impl GroupKey {
    pub fn bytes(&self) -> [u8; 32] {
        let v = hex::decode(&self.key).unwrap_or_default();
        let mut out = [0u8; 32];
        if v.len() == 32 {
            out.copy_from_slice(&v);
        }
        out
    }

    fn rank(&self) -> [u8; 32] {
        Sha256::digest(self.key.as_bytes()).into()
    }
}

/// 机器的左右排列（从左到右）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Layout {
    pub order: Vec<NodeId>,
    pub version: u64,
    pub modified_by: NodeId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupState {
    pub group_id: String,
    pub members: BTreeMap<NodeId, Member>,
    /// 按 epoch 升序，只保留最近几把（旧密钥用于离线成员追赶）。
    pub keys: Vec<GroupKey>,
    #[serde(default)]
    pub layout: Layout,
}

/// 保留的历史密钥数量。
const KEEP_KEYS: usize = 3;

fn new_key(epoch: u32, holders: Vec<NodeId>) -> GroupKey {
    GroupKey { epoch, key: hex::encode(random_bytes::<32>()), holders }
}

impl GroupState {
    /// 第一台机器创建家庭组。
    pub fn create(me: &Identity, machine_name: &str, owner_name: &str, hue: u16, now: i64) -> Self {
        let mut members = BTreeMap::new();
        members.insert(
            me.node_id.clone(),
            Member {
                node_id: me.node_id.clone(),
                pubkey: me.public_hex(),
                machine_name: machine_name.into(),
                owner_name: owner_name.into(),
                hue,
                info_version: 1,
                joined_ms: now,
                removed: false,
            },
        );
        GroupState {
            group_id: hex::encode(random_bytes::<8>()),
            members,
            keys: vec![new_key(0, vec![me.node_id.clone()])],
            layout: Layout { order: vec![me.node_id.clone()], version: 1, modified_by: me.node_id.clone() },
        }
    }

    pub fn current_key(&self) -> &GroupKey {
        self.keys.last().expect("group has a key")
    }

    pub fn epoch(&self) -> u32 {
        self.current_key().epoch
    }

    pub fn key_for(&self, epoch: u32) -> Option<&GroupKey> {
        self.keys.iter().find(|k| k.epoch == epoch)
    }

    pub fn is_active_member(&self, node: &str) -> bool {
        self.members.get(node).map(|m| !m.removed).unwrap_or(false)
    }

    pub fn active_members(&self) -> impl Iterator<Item = &Member> {
        self.members.values().filter(|m| !m.removed)
    }

    /// 新成员加入（配对成功后由接待它的成员调用）。
    pub fn add_member(&mut self, m: Member) {
        let id = m.node_id.clone();
        match self.members.get_mut(&id) {
            Some(existing) if existing.removed => {
                // 被踢出的机器不能用同一个身份重新加入；需要重新生成身份。
            }
            Some(existing) => {
                if m.info_version > existing.info_version {
                    *existing = Member { removed: false, ..m };
                }
            }
            None => {
                self.members.insert(id.clone(), m);
            }
        }
        if !self.layout.order.contains(&id) && self.is_active_member(&id) {
            self.layout.order.push(id);
            self.layout.version += 1;
        }
    }

    /// 更新自己的资料（机器名、主人名字、颜色）。
    pub fn update_member_info(&mut self, node: &str, machine_name: &str, owner_name: &str, hue: u16) -> bool {
        if let Some(m) = self.members.get_mut(node) {
            if m.machine_name != machine_name || m.owner_name != owner_name || m.hue != hue {
                m.machine_name = machine_name.into();
                m.owner_name = owner_name.into();
                m.hue = hue;
                m.info_version += 1;
                return true;
            }
        }
        false
    }

    /// 踢出成员并轮换组密钥。返回新的 epoch。
    pub fn remove_member(&mut self, node: &str, by: &str) -> Option<u32> {
        let m = self.members.get_mut(node)?;
        if m.removed {
            return None;
        }
        m.removed = true;
        self.layout.order.retain(|n| n != node);
        self.layout.version += 1;
        self.layout.modified_by = by.to_string();
        Some(self.rekey())
    }

    /// 生成新组密钥，只有当前在组里的成员持有。
    pub fn rekey(&mut self) -> u32 {
        let epoch = self.epoch() + 1;
        let holders = self.active_members().map(|m| m.node_id.clone()).collect();
        self.keys.push(new_key(epoch, holders));
        self.trim_keys();
        epoch
    }

    fn trim_keys(&mut self) {
        self.keys.sort_by_key(|k| k.epoch);
        if self.keys.len() > KEEP_KEYS {
            let cut = self.keys.len() - KEEP_KEYS;
            self.keys.drain(..cut);
        }
    }

    /// 当前密钥是否可能被已移除的成员知道。
    pub fn needs_rekey(&self) -> bool {
        self.current_key().holders.iter().any(|h| !self.is_active_member(h))
    }

    /// 设置左右排列。
    pub fn set_layout(&mut self, order: Vec<NodeId>, by: &str) {
        let order: Vec<NodeId> = order.into_iter().filter(|n| self.is_active_member(n)).collect();
        if order != self.layout.order {
            self.layout = Layout { order, version: self.layout.version + 1, modified_by: by.to_string() };
        }
    }

    /// 排列里某台机器左右两边的邻居。
    pub fn neighbors(&self, node: &str) -> (Option<NodeId>, Option<NodeId>) {
        let order = &self.layout.order;
        match order.iter().position(|n| n == node) {
            Some(i) => (
                if i > 0 { Some(order[i - 1].clone()) } else { None },
                order.get(i + 1).cloned(),
            ),
            None => (None, None),
        }
    }

    /// 合并另一台机器发来的组状态。返回是否有变化。
    pub fn merge(&mut self, other: &GroupState) -> bool {
        if other.group_id != self.group_id {
            return false;
        }
        let before = self.clone();
        for (id, m) in &other.members {
            match self.members.get_mut(id) {
                None => {
                    self.members.insert(id.clone(), m.clone());
                }
                Some(mine) => {
                    let removed = mine.removed || m.removed;
                    if m.info_version > mine.info_version {
                        *mine = m.clone();
                    }
                    mine.removed = removed;
                }
            }
        }
        for k in &other.keys {
            match self.keys.iter_mut().find(|x| x.epoch == k.epoch) {
                None => self.keys.push(k.clone()),
                Some(mine) => {
                    if mine.key != k.key && k.rank() > mine.rank() {
                        *mine = k.clone();
                    }
                }
            }
        }
        self.trim_keys();
        let l = &other.layout;
        if (l.version, &l.modified_by) > (self.layout.version, &self.layout.modified_by) {
            self.layout = l.clone();
        }
        // 排列里去掉已移除的成员、补上新成员。
        let removed: Vec<NodeId> = self.members.values().filter(|m| m.removed).map(|m| m.node_id.clone()).collect();
        self.layout.order.retain(|n| !removed.contains(n));
        for m in self.members.values() {
            if !m.removed && !self.layout.order.contains(&m.node_id) {
                self.layout.order.push(m.node_id.clone());
            }
        }
        *self != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &Identity, name: &str) -> Member {
        Member {
            node_id: id.node_id.clone(),
            pubkey: id.public_hex(),
            machine_name: name.into(),
            owner_name: "主人".into(),
            hue: 0,
            info_version: 1,
            joined_ms: 0,
            removed: false,
        }
    }

    #[test]
    fn create_add_remove_rekey() {
        let a = Identity::generate();
        let b = Identity::generate();
        let c = Identity::generate();
        let mut g = GroupState::create(&a, "A", "甲", 10, 0);
        assert_eq!(g.epoch(), 0);
        g.add_member(member(&b, "B"));
        g.add_member(member(&c, "C"));
        assert_eq!(g.layout.order, vec![a.node_id.clone(), b.node_id.clone(), c.node_id.clone()]);
        assert_eq!(g.neighbors(&b.node_id), (Some(a.node_id.clone()), Some(c.node_id.clone())));
        let old_key = g.current_key().key.clone();
        let e = g.remove_member(&b.node_id, &a.node_id).unwrap();
        assert_eq!(e, 1);
        assert_ne!(g.current_key().key, old_key);
        assert!(!g.current_key().holders.contains(&b.node_id));
        assert!(!g.is_active_member(&b.node_id));
        assert!(!g.needs_rekey());
        assert_eq!(g.neighbors(&a.node_id), (None, Some(c.node_id.clone())));
        // 被踢出的身份不能重新加入。
        g.add_member(member(&b, "B again"));
        assert!(!g.is_active_member(&b.node_id));
    }

    #[test]
    fn keys_are_trimmed() {
        let a = Identity::generate();
        let mut g = GroupState::create(&a, "A", "甲", 0, 0);
        for _ in 0..5 {
            g.rekey();
        }
        assert_eq!(g.keys.len(), 3);
        assert_eq!(g.epoch(), 5);
        assert!(g.key_for(2).is_none());
        assert!(g.key_for(3).is_some());
    }

    #[test]
    fn merge_converges_and_removal_sticks() {
        let a = Identity::generate();
        let b = Identity::generate();
        let c = Identity::generate();
        let mut base = GroupState::create(&a, "A", "甲", 0, 0);
        base.add_member(member(&b, "B"));
        base.add_member(member(&c, "C"));

        // 两台机器离线时分别踢了不同的人。
        let mut on_a = base.clone();
        on_a.remove_member(&b.node_id, &a.node_id);
        let mut on_c = base.clone();
        on_c.remove_member(&a.node_id, &c.node_id); // 极端情况：C 踢了 A

        let mut x = on_a.clone();
        x.merge(&on_c);
        let mut y = on_c.clone();
        y.merge(&on_a);
        assert_eq!(x, y, "合并结果与顺序无关");
        assert!(!x.is_active_member(&b.node_id));
        assert!(!x.is_active_member(&a.node_id));
        // 同一 epoch 的两把密钥里选出了一把，而它的持有者包含被移除的成员 → 需要再轮换。
        assert!(x.needs_rekey());
        x.rekey();
        assert!(!x.needs_rekey());
        assert_eq!(x.current_key().holders, vec![c.node_id.clone()]);
        // 幂等。
        let snapshot = x.clone();
        assert!(!x.merge(&snapshot));
    }

    #[test]
    fn member_info_and_layout_merge() {
        let a = Identity::generate();
        let b = Identity::generate();
        let mut g1 = GroupState::create(&a, "A", "甲", 0, 0);
        g1.add_member(member(&b, "B"));
        let mut g2 = g1.clone();
        assert!(g2.update_member_info(&b.node_id, "B 的新名字", "乙", 200));
        g2.set_layout(vec![b.node_id.clone(), a.node_id.clone()], &b.node_id);
        assert!(g1.merge(&g2));
        assert_eq!(g1.members[&b.node_id].machine_name, "B 的新名字");
        assert_eq!(g1.layout.order, vec![b.node_id.clone(), a.node_id.clone()]);
        // 不同组的状态不合并。
        let other = GroupState::create(&Identity::generate(), "X", "x", 0, 0);
        assert!(!g1.merge(&other));
    }
}
