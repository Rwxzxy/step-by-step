//! 宠物串门的交接协议：保证同一时刻一只宠物只显示在一台机器上。
//!
//! 每只宠物属于一台“主人机器”（owner），主人机器是这只宠物位置的唯一决策者。
//!
//! **出门（两阶段 + 租约）**
//!
//! 1. 宠物在主人机器上走出屏幕边缘后先隐藏，主人发 `Offer{seq}`（seq 单调递增并持久化）。
//! 2. 目标机器（host）若愿意接待，回 `Accept{seq}`，但**此时还不显示**。
//! 3. 主人收到 Accept，发 `Commit{seq}`；host 只有在发出 Accept 后 [`COMMIT_WINDOW_MS`] 内
//!    收到 Commit 才显示宠物，并开始一个 [`LEASE_MS`] 的租约。
//! 4. 主人每 [`RENEW_MS`] 发 `Lease{seq, elapsed}` 续约，host 回 `LeaseAck`。
//!    `elapsed` 是主人从发 Commit 起经过的时间，host 用自己从收到 Commit 起经过的时间和它比较，
//!    消息比 Commit 多延迟超过 [`MAX_EXTRA_DELAY_MS`] 就不续约（防止网络卡顿后迟到的续约）。
//! 5. host 租约到期就隐藏宠物（主人掉线 / 网络断了 → 宠物“自己回家了”）。
//! 6. 主人一直收不到 LeaseAck 时停止续约，等到 “最后一次发出续约 + 租约 + 余量
//!    [`MARGIN_MS`]” 之后才把宠物显示回家里。这时 host 的租约一定已经过期。
//!
//! **回家**：host 先隐藏宠物，再发 `Return{seq}`；主人收到后立即显示。主人召回时发 `Recall`，
//! host 播放走出去的动画后同样走 Return 流程。
//!
//! 任何时候主人只有在确定 host 不会（再）显示时才显示：
//! * Offer 超时 / 被拒：host 没收到 Commit，不会显示（迟到的 Commit 会被 [`COMMIT_WINDOW_MS`] 拒绝）。
//! * Commit 之后：要么收到 Return / Reject，要么等租约 + 余量过期。
//!
//! 时间都用各自机器的单调时钟，只比较“经过了多久”，不需要两台机器对时。

use serde::{Deserialize, Serialize};

use crate::pet::Side;

pub type NodeId = String;

/// host 端租约长度。
pub const LEASE_MS: i64 = 10_000;
/// 主人续约间隔。
pub const RENEW_MS: i64 = 2_000;
/// 主人判定“失联”前等待 LeaseAck 的时间。
pub const ACK_GRACE_MS: i64 = 6_000;
/// 主人收回宠物前在租约之外额外等待的时间。必须大于 COMMIT_WINDOW_MS + MAX_EXTRA_DELAY_MS。
pub const MARGIN_MS: i64 = 5_000;
/// host 发出 Accept 后多久之内必须收到 Commit。
pub const COMMIT_WINDOW_MS: i64 = 2_000;
/// 续约消息相对 Commit 允许多出的延迟。
pub const MAX_EXTRA_DELAY_MS: i64 = 1_000;
/// 主人等待 Accept 的时间。
pub const OFFER_TIMEOUT_MS: i64 = 4_000;

/// 宠物的外观信息，随 Offer 发给 host。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Visitor {
    pub owner_name: String,
    pub machine_name: String,
    /// 主色调（色相 0-360）。
    pub hue: u16,
    /// 在 host 屏幕的哪一侧出现。
    pub enter_side: Side,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PetMsgKind {
    Offer { visitor: Visitor },
    Accept,
    Reject { reason: String },
    Commit,
    Lease { elapsed_ms: i64 },
    LeaseAck,
    Abort,
    Recall,
    Return,
}

/// 线上的交接消息。`pet` 是主人机器 id（宠物的身份）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PetMsg {
    pub pet: NodeId,
    pub seq: u64,
    #[serde(flatten)]
    pub kind: PetMsgKind,
}

// ───────────────────────────── 主人一侧 ─────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum OwnerState {
    /// 在家（显示在本机）。
    Home,
    /// 已隐藏，等待对方接受。
    Offering { to: NodeId, seq: u64, since: i64 },
    /// 在对方机器上。
    Away {
        at: NodeId,
        seq: u64,
        commit_sent: i64,
        last_lease_sent: i64,
        last_ack: i64,
    },
    /// 和对方失联，等对方租约必然过期后收回。
    Reclaiming { at: NodeId, seq: u64, until: i64 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum OwnerAction {
    Send(NodeId, PetMsg),
    /// 在本机显示宠物。`from` 为 Some 表示从那台机器回来（从对应一侧走进来）。
    ShowAtHome {
        from: Option<NodeId>,
        reason: ReturnReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReturnReason {
    /// 对方拒绝或没有响应，宠物没出门。
    NotAccepted,
    /// 正常回家。
    Returned,
    /// 失联后收回。
    Reclaimed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnerHandoff {
    pub me: NodeId,
    /// 已经用过的最大 seq（需要持久化）。
    pub seq: u64,
    pub state: OwnerState,
}

impl OwnerHandoff {
    pub fn new(me: impl Into<NodeId>, persisted_seq: u64) -> Self {
        OwnerHandoff {
            me: me.into(),
            seq: persisted_seq,
            state: OwnerState::Home,
        }
    }

    /// 程序重启时：如果上次退出时宠物不在家，就保守地等一个完整的租约 + 余量再显示。
    pub fn restore(
        me: impl Into<NodeId>,
        persisted_seq: u64,
        persisted: OwnerState,
        now: i64,
    ) -> Self {
        let state = match persisted {
            OwnerState::Home => OwnerState::Home,
            OwnerState::Offering { to, seq, .. } => OwnerState::Reclaiming {
                at: to,
                seq,
                until: now + LEASE_MS + MARGIN_MS,
            },
            OwnerState::Away { at, seq, .. } | OwnerState::Reclaiming { at, seq, .. } => {
                OwnerState::Reclaiming {
                    at,
                    seq,
                    until: now + LEASE_MS + MARGIN_MS,
                }
            }
        };
        OwnerHandoff {
            me: me.into(),
            seq: persisted_seq,
            state,
        }
    }

    pub fn is_home(&self) -> bool {
        matches!(self.state, OwnerState::Home)
    }

    /// 宠物现在在哪台机器（Some(host)）或在家（None）。
    pub fn away_at(&self) -> Option<&NodeId> {
        match &self.state {
            OwnerState::Away { at, .. }
            | OwnerState::Reclaiming { at, .. }
            | OwnerState::Offering { to: at, .. } => Some(at),
            OwnerState::Home => None,
        }
    }

    fn msg(&self, seq: u64, kind: PetMsgKind) -> PetMsg {
        PetMsg {
            pet: self.me.clone(),
            seq,
            kind,
        }
    }

    /// 宠物已经在本机隐藏（走出了屏幕边缘），开始交接。只能在家时调用。
    pub fn begin_visit(
        &mut self,
        to: NodeId,
        visitor: Visitor,
        now: i64,
    ) -> Option<Vec<OwnerAction>> {
        if !self.is_home() || to == self.me {
            return None;
        }
        self.seq += 1;
        let seq = self.seq;
        self.state = OwnerState::Offering {
            to: to.clone(),
            seq,
            since: now,
        };
        Some(vec![OwnerAction::Send(
            to,
            self.msg(seq, PetMsgKind::Offer { visitor }),
        )])
    }

    /// 召回。
    pub fn recall(&mut self) -> Vec<OwnerAction> {
        match &self.state {
            OwnerState::Away { at, seq, .. } => vec![OwnerAction::Send(
                at.clone(),
                self.msg(*seq, PetMsgKind::Recall),
            )],
            _ => vec![],
        }
    }

    pub fn on_msg(&mut self, from: &NodeId, msg: &PetMsg, now: i64) -> Vec<OwnerAction> {
        if msg.pet != self.me {
            return vec![];
        }
        let mut out = vec![];
        match (&self.state, &msg.kind) {
            (OwnerState::Offering { to, seq, .. }, PetMsgKind::Accept)
                if to == from && *seq == msg.seq =>
            {
                let (at, seq) = (to.clone(), *seq);
                out.push(OwnerAction::Send(
                    at.clone(),
                    self.msg(seq, PetMsgKind::Commit),
                ));
                self.state = OwnerState::Away {
                    at,
                    seq,
                    commit_sent: now,
                    last_lease_sent: now,
                    last_ack: now,
                };
            }
            (OwnerState::Offering { to, seq, .. }, PetMsgKind::Reject { .. })
                if to == from && *seq == msg.seq =>
            {
                self.state = OwnerState::Home;
                out.push(OwnerAction::ShowAtHome {
                    from: None,
                    reason: ReturnReason::NotAccepted,
                });
            }
            (
                OwnerState::Away { at, seq, .. } | OwnerState::Reclaiming { at, seq, .. },
                PetMsgKind::Return | PetMsgKind::Reject { .. },
            ) if at == from && *seq == msg.seq => {
                let at = at.clone();
                let reason = if matches!(msg.kind, PetMsgKind::Return) {
                    ReturnReason::Returned
                } else {
                    ReturnReason::NotAccepted
                };
                self.state = OwnerState::Home;
                out.push(OwnerAction::ShowAtHome {
                    from: Some(at),
                    reason,
                });
            }
            (
                OwnerState::Away {
                    at,
                    seq,
                    commit_sent,
                    last_lease_sent,
                    ..
                },
                PetMsgKind::LeaseAck,
            ) if at == from && *seq == msg.seq => {
                self.state = OwnerState::Away {
                    at: at.clone(),
                    seq: *seq,
                    commit_sent: *commit_sent,
                    last_lease_sent: *last_lease_sent,
                    last_ack: now,
                };
            }
            _ => {}
        }
        out
    }

    pub fn tick(&mut self, now: i64) -> Vec<OwnerAction> {
        let mut out = vec![];
        match self.state.clone() {
            OwnerState::Offering { to, seq, since } if now - since > OFFER_TIMEOUT_MS => {
                // 没发过 Commit，对方不可能显示。告诉对方作废。
                out.push(OwnerAction::Send(to, self.msg(seq, PetMsgKind::Abort)));
                self.state = OwnerState::Home;
                out.push(OwnerAction::ShowAtHome {
                    from: None,
                    reason: ReturnReason::NotAccepted,
                });
            }
            OwnerState::Away {
                at,
                seq,
                commit_sent,
                last_lease_sent,
                last_ack,
            } => {
                if now - last_ack > ACK_GRACE_MS {
                    // 失联：停止续约，等对方租约必然过期。
                    self.state = OwnerState::Reclaiming {
                        at,
                        seq,
                        until: last_lease_sent + LEASE_MS + MARGIN_MS,
                    };
                } else if now - last_lease_sent >= RENEW_MS {
                    out.push(OwnerAction::Send(
                        at.clone(),
                        self.msg(
                            seq,
                            PetMsgKind::Lease {
                                elapsed_ms: now - commit_sent,
                            },
                        ),
                    ));
                    self.state = OwnerState::Away {
                        at,
                        seq,
                        commit_sent,
                        last_lease_sent: now,
                        last_ack,
                    };
                }
            }
            OwnerState::Reclaiming { at, until, .. } if now >= until => {
                self.state = OwnerState::Home;
                out.push(OwnerAction::ShowAtHome {
                    from: Some(at),
                    reason: ReturnReason::Reclaimed,
                });
            }
            _ => {}
        }
        out
    }
}

// ───────────────────────────── 接待一侧 ─────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum GuestState {
    None,
    Accepted {
        seq: u64,
        accepted_at: i64,
        visitor: Visitor,
    },
    Visiting {
        seq: u64,
        commit_at: i64,
        lease_until: i64,
        visitor: Visitor,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum HostAction {
    Send(NodeId, PetMsg),
    /// 显示来访的宠物。
    ShowGuest {
        owner: NodeId,
        visitor: Visitor,
    },
    /// 立即隐藏来访的宠物。
    HideGuest {
        owner: NodeId,
        reason: GuestLeaveReason,
    },
    /// 请播放走出去的动画，播完调用 [`GuestSlot::leave`]。
    StartLeaving {
        owner: NodeId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestLeaveReason {
    /// 正常离开（召回 / 时间到 / 走回去了）。
    Left,
    /// 主人失联，租约过期。
    LeaseExpired,
}

/// 接待某一只宠物（一个 owner）的状态。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuestSlot {
    pub owner: NodeId,
    pub state: GuestState,
    /// 见过的最大 seq，拒绝旧消息。
    pub max_seq: u64,
}

impl GuestSlot {
    pub fn new(owner: impl Into<NodeId>) -> Self {
        GuestSlot {
            owner: owner.into(),
            state: GuestState::None,
            max_seq: 0,
        }
    }

    pub fn is_visible(&self) -> bool {
        matches!(self.state, GuestState::Visiting { .. })
    }

    fn reply(&self, seq: u64, kind: PetMsgKind) -> HostAction {
        HostAction::Send(
            self.owner.clone(),
            PetMsg {
                pet: self.owner.clone(),
                seq,
                kind,
            },
        )
    }

    /// `willing` 表示本机当前是否愿意接待（设置里允许、没有在免打扰等）。
    pub fn on_msg(
        &mut self,
        from: &NodeId,
        msg: &PetMsg,
        now: i64,
        willing: bool,
    ) -> Vec<HostAction> {
        // 只接受主人机器本人发来的、关于它自己宠物的消息。
        if from != &self.owner || msg.pet != self.owner {
            return vec![];
        }
        let mut out = vec![];
        match &msg.kind {
            PetMsgKind::Offer { visitor } => {
                if msg.seq <= self.max_seq {
                    out.push(self.reply(
                        msg.seq,
                        PetMsgKind::Reject {
                            reason: "过期的请求".into(),
                        },
                    ));
                    return out;
                }
                self.max_seq = msg.seq;
                // 主人已经开始新一轮交接，说明它认为宠物在家；旧的来访必须结束（正常情况下早已过期）。
                if let GuestState::Visiting { .. } = self.state {
                    self.state = GuestState::None;
                    out.push(HostAction::HideGuest {
                        owner: self.owner.clone(),
                        reason: GuestLeaveReason::LeaseExpired,
                    });
                }
                if willing {
                    self.state = GuestState::Accepted {
                        seq: msg.seq,
                        accepted_at: now,
                        visitor: visitor.clone(),
                    };
                    out.push(self.reply(msg.seq, PetMsgKind::Accept));
                } else {
                    self.state = GuestState::None;
                    out.push(self.reply(
                        msg.seq,
                        PetMsgKind::Reject {
                            reason: "对方暂时不接待".into(),
                        },
                    ));
                }
            }
            PetMsgKind::Commit => match &self.state {
                GuestState::Accepted {
                    seq,
                    accepted_at,
                    visitor,
                } if *seq == msg.seq => {
                    if now - accepted_at <= COMMIT_WINDOW_MS {
                        let visitor = visitor.clone();
                        self.state = GuestState::Visiting {
                            seq: *seq,
                            commit_at: now,
                            lease_until: now + LEASE_MS,
                            visitor: visitor.clone(),
                        };
                        out.push(HostAction::ShowGuest {
                            owner: self.owner.clone(),
                            visitor,
                        });
                        out.push(self.reply(msg.seq, PetMsgKind::LeaseAck));
                    } else {
                        // Commit 来得太晚：不显示，并明确告诉主人。
                        self.state = GuestState::None;
                        out.push(self.reply(
                            msg.seq,
                            PetMsgKind::Reject {
                                reason: "确认超时".into(),
                            },
                        ));
                    }
                }
                _ => {
                    if msg.seq <= self.max_seq && !self.is_visible() {
                        out.push(self.reply(
                            msg.seq,
                            PetMsgKind::Reject {
                                reason: "没有待确认的来访".into(),
                            },
                        ));
                    }
                }
            },
            PetMsgKind::Lease { elapsed_ms } => {
                if let GuestState::Visiting {
                    seq,
                    commit_at,
                    visitor,
                    ..
                } = &self.state
                {
                    if *seq == msg.seq {
                        let host_elapsed = now - commit_at;
                        if host_elapsed - elapsed_ms <= MAX_EXTRA_DELAY_MS {
                            self.state = GuestState::Visiting {
                                seq: *seq,
                                commit_at: *commit_at,
                                lease_until: now + LEASE_MS,
                                visitor: visitor.clone(),
                            };
                            out.push(self.reply(msg.seq, PetMsgKind::LeaseAck));
                        }
                    }
                }
            }
            PetMsgKind::Abort => {
                if let GuestState::Accepted { seq, .. } = &self.state {
                    if *seq == msg.seq {
                        self.state = GuestState::None;
                    }
                }
            }
            PetMsgKind::Recall => match &self.state {
                GuestState::Visiting { seq, .. } if *seq == msg.seq => {
                    out.push(HostAction::StartLeaving {
                        owner: self.owner.clone(),
                    });
                }
                GuestState::Visiting { .. } => {}
                // 还没显示（Commit 还在路上）或者已经不在这里了：作废这一轮，告诉主人宠物不在这儿。
                // 之后迟到的 Commit 会因为 seq 已作废而被拒绝。
                _ => {
                    if let GuestState::Accepted { seq, .. } = &self.state {
                        if *seq == msg.seq {
                            self.state = GuestState::None;
                        }
                    }
                    self.max_seq = self.max_seq.max(msg.seq);
                    out.push(self.reply(msg.seq, PetMsgKind::Return));
                }
            },
            _ => {}
        }
        out
    }

    /// 宠物离开（走出屏幕 / 时间到 / 被送回家）：先隐藏再通知主人。
    pub fn leave(&mut self) -> Vec<HostAction> {
        match &self.state {
            GuestState::Visiting { seq, .. } => {
                let seq = *seq;
                self.state = GuestState::None;
                vec![
                    HostAction::HideGuest {
                        owner: self.owner.clone(),
                        reason: GuestLeaveReason::Left,
                    },
                    self.reply(seq, PetMsgKind::Return),
                ]
            }
            _ => vec![],
        }
    }

    pub fn tick(&mut self, now: i64) -> Vec<HostAction> {
        match &self.state {
            GuestState::Visiting { lease_until, .. } if now > *lease_until => {
                self.state = GuestState::None;
                vec![HostAction::HideGuest {
                    owner: self.owner.clone(),
                    reason: GuestLeaveReason::LeaseExpired,
                }]
            }
            GuestState::Accepted { accepted_at, .. } if now - accepted_at > COMMIT_WINDOW_MS => {
                self.state = GuestState::None;
                vec![]
            }
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visitor() -> Visitor {
        Visitor {
            owner_name: "小明".into(),
            machine_name: "台式机".into(),
            hue: 30,
            enter_side: Side::Left,
        }
    }

    /// 带延迟和丢包的模拟网络。
    struct Net {
        queue: Vec<(i64, NodeId, NodeId, PetMsg)>,
        rng: u64,
        drop_pct: u64,
        max_delay: i64,
        /// 网络断开期间所有消息丢失。
        partitioned: bool,
    }

    impl Net {
        fn rand(&mut self) -> u64 {
            let mut x = self.rng;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.rng = x;
            x
        }
        fn send(&mut self, now: i64, from: &str, to: NodeId, msg: PetMsg) {
            if self.partitioned || self.rand() % 100 < self.drop_pct {
                return;
            }
            let delay = 1 + (self.rand() % self.max_delay as u64) as i64;
            self.queue.push((now + delay, from.to_string(), to, msg));
        }
        fn due(&mut self, now: i64) -> Vec<(NodeId, NodeId, PetMsg)> {
            let mut due = vec![];
            let mut rest = vec![];
            for item in self.queue.drain(..) {
                if item.0 <= now {
                    due.push((item.1, item.2, item.3));
                } else {
                    rest.push(item);
                }
            }
            self.queue = rest;
            due
        }
    }

    struct World {
        owner: OwnerHandoff,
        guest: GuestSlot,
        owner_visible: bool,
        guest_visible: bool,
        net: Net,
        now: i64,
        willing: bool,
        shown_home: usize,
        shown_guest: usize,
    }

    impl World {
        fn new(seed: u64, drop_pct: u64, max_delay: i64) -> Self {
            World {
                owner: OwnerHandoff::new("O", 0),
                guest: GuestSlot::new("O"),
                owner_visible: true,
                guest_visible: false,
                net: Net {
                    queue: vec![],
                    rng: seed | 1,
                    drop_pct,
                    max_delay,
                    partitioned: false,
                },
                now: 0,
                willing: true,
                shown_home: 0,
                shown_guest: 0,
            }
        }

        fn apply_owner(&mut self, actions: Vec<OwnerAction>) {
            for a in actions {
                match a {
                    OwnerAction::Send(to, m) => self.net.send(self.now, "O", to, m),
                    OwnerAction::ShowAtHome { .. } => {
                        self.owner_visible = true;
                        self.shown_home += 1;
                    }
                }
            }
        }

        fn apply_host(&mut self, actions: Vec<HostAction>) {
            for a in actions {
                match a {
                    HostAction::Send(to, m) => self.net.send(self.now, "H", to, m),
                    HostAction::ShowGuest { .. } => {
                        self.guest_visible = true;
                        self.shown_guest += 1;
                    }
                    HostAction::HideGuest { .. } => self.guest_visible = false,
                    HostAction::StartLeaving { .. } => {
                        let acts = self.guest.leave();
                        self.apply_host(acts);
                    }
                }
            }
        }

        fn depart(&mut self) {
            if self.owner.is_home() && self.owner_visible {
                self.owner_visible = false; // 先走出屏幕、隐藏
                let acts = self
                    .owner
                    .begin_visit("H".into(), visitor(), self.now)
                    .unwrap();
                self.apply_owner(acts);
            }
        }

        fn step(&mut self, ms: i64) {
            for _ in 0..ms / 10 {
                self.now += 10;
                for (from, to, msg) in self.net.due(self.now) {
                    if to == "O" {
                        let a = self.owner.on_msg(&from, &msg, self.now);
                        self.apply_owner(a);
                    } else {
                        let a = self.guest.on_msg(&from, &msg, self.now, self.willing);
                        self.apply_host(a);
                    }
                }
                let a = self.owner.tick(self.now);
                self.apply_owner(a);
                let a = self.guest.tick(self.now);
                self.apply_host(a);
                assert!(
                    !(self.owner_visible && self.guest_visible),
                    "t={} 宠物同时出现在两台机器上：owner={:?} guest={:?}",
                    self.now,
                    self.owner.state,
                    self.guest.state
                );
            }
        }
    }

    #[test]
    fn happy_path_visit_and_return() {
        let mut w = World::new(1, 0, 20);
        w.depart();
        w.step(500);
        assert!(w.guest_visible && !w.owner_visible);
        assert!(matches!(w.owner.state, OwnerState::Away { .. }));
        // 待一会儿，租约一直被续上。
        w.step(60_000);
        assert!(w.guest_visible);
        // 主人召回。
        let a = w.owner.recall();
        w.apply_owner(a);
        w.step(500);
        assert!(w.owner_visible && !w.guest_visible);
        assert!(w.owner.is_home());
        assert_eq!(w.owner.seq, 1);
    }

    #[test]
    fn host_refuses() {
        let mut w = World::new(2, 0, 20);
        w.willing = false;
        w.depart();
        w.step(500);
        assert!(w.owner_visible && !w.guest_visible);
        assert_eq!(w.shown_guest, 0);
    }

    #[test]
    fn host_offline_offer_times_out() {
        let mut w = World::new(3, 100, 20);
        w.depart();
        w.step(OFFER_TIMEOUT_MS + 100);
        assert!(w.owner_visible);
        assert!(w.owner.is_home());
    }

    #[test]
    fn partition_while_away_never_duplicates() {
        let mut w = World::new(4, 0, 20);
        w.depart();
        w.step(1_000);
        assert!(w.guest_visible);
        w.net.partitioned = true;
        w.step(LEASE_MS + 1_000);
        // host 租约到期先隐藏…
        assert!(!w.guest_visible);
        // …主人在更晚的时候才收回。
        w.step(ACK_GRACE_MS + MARGIN_MS + LEASE_MS);
        assert!(w.owner_visible);
        assert!(w.owner.is_home());
        // 网络恢复后，迟到的消息不会让宠物再次出现在 host。
        w.net.partitioned = false;
        w.step(5_000);
        assert!(!w.guest_visible);
    }

    #[test]
    fn late_commit_is_rejected() {
        let mut g = GuestSlot::new("O");
        let offer = PetMsg {
            pet: "O".into(),
            seq: 1,
            kind: PetMsgKind::Offer { visitor: visitor() },
        };
        let a = g.on_msg(&"O".into(), &offer, 0, true);
        assert!(matches!(
            &a[0],
            HostAction::Send(
                _,
                PetMsg {
                    kind: PetMsgKind::Accept,
                    ..
                }
            )
        ));
        let commit = PetMsg {
            pet: "O".into(),
            seq: 1,
            kind: PetMsgKind::Commit,
        };
        let a = g.on_msg(&"O".into(), &commit, COMMIT_WINDOW_MS + 1, true);
        assert!(!g.is_visible());
        assert!(matches!(
            &a[0],
            HostAction::Send(
                _,
                PetMsg {
                    kind: PetMsgKind::Reject { .. },
                    ..
                }
            )
        ));
    }

    #[test]
    fn stale_lease_is_ignored() {
        let mut g = GuestSlot::new("O");
        g.on_msg(
            &"O".into(),
            &PetMsg {
                pet: "O".into(),
                seq: 1,
                kind: PetMsgKind::Offer { visitor: visitor() },
            },
            0,
            true,
        );
        g.on_msg(
            &"O".into(),
            &PetMsg {
                pet: "O".into(),
                seq: 1,
                kind: PetMsgKind::Commit,
            },
            10,
            true,
        );
        assert!(g.is_visible());
        // 一条在主人那边 elapsed=2000 时发出、但 8 秒后才到的续约：不续。
        let a = g.on_msg(
            &"O".into(),
            &PetMsg {
                pet: "O".into(),
                seq: 1,
                kind: PetMsgKind::Lease { elapsed_ms: 2_000 },
            },
            10_010,
            true,
        );
        assert!(a.is_empty());
        // 租约照常过期。
        let a = g.tick(10 + LEASE_MS + 1);
        assert!(matches!(
            a[0],
            HostAction::HideGuest {
                reason: GuestLeaveReason::LeaseExpired,
                ..
            }
        ));
    }

    #[test]
    fn messages_from_impostors_are_ignored() {
        let mut g = GuestSlot::new("O");
        let offer = PetMsg {
            pet: "O".into(),
            seq: 1,
            kind: PetMsgKind::Offer { visitor: visitor() },
        };
        assert!(g.on_msg(&"X".into(), &offer, 0, true).is_empty());
        let mut o = OwnerHandoff::new("O", 0);
        o.begin_visit("H".into(), visitor(), 0).unwrap();
        let accept = PetMsg {
            pet: "O".into(),
            seq: 1,
            kind: PetMsgKind::Accept,
        };
        assert!(o.on_msg(&"X".into(), &accept, 1).is_empty());
        assert!(matches!(o.state, OwnerState::Offering { .. }));
        // 旧 seq 的 Accept 也无效。
        let old = PetMsg {
            pet: "O".into(),
            seq: 0,
            kind: PetMsgKind::Accept,
        };
        assert!(o.on_msg(&"H".into(), &old, 1).is_empty());
    }

    #[test]
    fn cannot_depart_twice() {
        let mut o = OwnerHandoff::new("O", 5);
        assert!(o.begin_visit("H".into(), visitor(), 0).is_some());
        assert_eq!(o.seq, 6);
        assert!(o.begin_visit("H2".into(), visitor(), 1).is_none());
        assert!(o.begin_visit("O".into(), visitor(), 1).is_none());
    }

    #[test]
    fn restart_while_away_waits_before_showing() {
        let away = OwnerState::Away {
            at: "H".into(),
            seq: 3,
            commit_sent: 0,
            last_lease_sent: 0,
            last_ack: 0,
        };
        let mut o = OwnerHandoff::restore("O", 3, away, 100_000);
        assert!(o.tick(100_000 + LEASE_MS).is_empty());
        let a = o.tick(100_000 + LEASE_MS + MARGIN_MS);
        assert!(matches!(
            a[0],
            OwnerAction::ShowAtHome {
                reason: ReturnReason::Reclaimed,
                ..
            }
        ));
    }

    #[test]
    fn randomized_lossy_network_keeps_single_location() {
        // 随机丢包、随机延迟（不超过允许的额外延迟）、随机断网和召回：任何时刻都不能两边同时显示。
        for seed in 1..60u64 {
            let mut w = World::new(seed * 7919, (seed % 5) * 8, 1 + (seed as i64 % 8) * 100);
            let mut r = seed.wrapping_mul(6364136223846793005) | 1;
            for _ in 0..80 {
                r ^= r << 13;
                r ^= r >> 7;
                r ^= r << 17;
                match r % 7 {
                    0 | 1 => w.depart(),
                    2 => {
                        let a = w.owner.recall();
                        w.apply_owner(a);
                    }
                    3 => w.net.partitioned = !w.net.partitioned,
                    4 => {
                        let a = w.guest.leave();
                        w.apply_host(a);
                    }
                    5 => w.willing = !w.willing,
                    _ => {}
                }
                w.step(500 + (r % 4000) as i64);
            }
            // 最后恢复网络，系统收敛：宠物一定在某一处可见。
            w.net.partitioned = false;
            w.net.drop_pct = 0;
            w.step(LEASE_MS + ACK_GRACE_MS + MARGIN_MS + OFFER_TIMEOUT_MS + 2_000);
            assert!(
                w.owner_visible ^ w.guest_visible,
                "seed {seed}: 最终应恰好在一处"
            );
        }
    }
}
