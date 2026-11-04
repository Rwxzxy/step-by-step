//! 宠物引擎：30 帧/秒推进运动逻辑、移动透明窗口、按宠物轮廓切换鼠标穿透、
//! 处理拖拽，以及串门交接（本机宠物出门 / 接待别人的宠物）。

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use steppy_core::handoff::{
    GuestLeaveReason, GuestSlot, HostAction, OwnerAction, OwnerHandoff, OwnerState, PetMsg,
    ReturnReason, Visitor,
};
use steppy_core::net::wire::WireMsg;
use steppy_core::net::NodeId;
use steppy_core::pet::{Anim, BrainEvent, PetBrain, Rect, Screen, Side};
use tauri::{Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::notify;
use crate::state::{AppState, Shared};
use crate::storage;

/// 宠物窗口的逻辑尺寸（宠物在底部中间，上方留给气泡）。
pub const PET_W: f64 = 240.0;
pub const PET_H: f64 = 210.0;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HitRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone)]
pub enum PetCmd {
    React(Anim, i64),
    ClearReaction(Anim),
    Poke,
    DragStart(String),
    DragEnd(String),
    /// 收到的交接消息和收到时的单调时间（不是处理时间：引擎线程可能因创建窗口等原因延迟处理，
    /// 用处理时间判断续约是否迟到会误判）。
    Msg(NodeId, PetMsg, i64),
    Recall,
    GoVisit(Side),
    SendGuestHome(NodeId),
    MemberGone(NodeId),
    LeftGroup,
}

#[derive(Serialize, Deserialize)]
struct PetFile {
    seq: u64,
    state: OwnerState,
}

/// 引擎与其他模块共享的状态。
pub struct PetShared {
    pub owner: OwnerHandoff,
    pub guests: BTreeMap<NodeId, GuestSlot>,
    pub queue: Vec<PetCmd>,
    pub hit: HashMap<String, Vec<HitRect>>,
    /// 正在来访的宠物：owner → 外观。
    pub visitors: BTreeMap<NodeId, Visitor>,
    /// 当前帧（供前端初始化时查询）。
    pub frames: HashMap<String, FramePayload>,
    /// 各宠物窗口当前的位置（物理像素，供诊断和端到端测试）。
    pub positions: HashMap<String, (i32, i32)>,
    restored: bool,
}

impl PetShared {
    pub fn load(me: &str) -> PetShared {
        let file: Option<PetFile> = storage::read_json(&storage::path("pet.json"));
        let (owner, restored) = match file {
            Some(f) => (
                OwnerHandoff::restore(me, f.seq, f.state.clone(), 0),
                !matches!(f.state, OwnerState::Home),
            ),
            None => (OwnerHandoff::new(me, 0), false),
        };
        PetShared {
            owner,
            guests: BTreeMap::new(),
            queue: Vec::new(),
            hit: HashMap::new(),
            visitors: BTreeMap::new(),
            frames: HashMap::new(),
            positions: HashMap::new(),
            restored,
        }
    }

    fn save(&self) {
        let f = PetFile {
            seq: self.owner.seq,
            state: self.owner.state.clone(),
        };
        let _ = storage::write_json(&storage::path("pet.json"), &f, false);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FramePayload {
    pub anim: Anim,
    pub facing: Side,
    pub visible: bool,
}

// ─────────────────────────── 对外接口 ───────────────────────────

fn push(state: &AppState, cmd: PetCmd) {
    state.pet.lock().unwrap().queue.push(cmd);
}

pub fn react(state: &AppState, anim: Anim, hold_ms: i64) {
    push(state, PetCmd::React(anim, hold_ms));
}

pub fn clear_reaction(state: &AppState, anim: Anim) {
    push(state, PetCmd::ClearReaction(anim));
}

pub fn pet_is_home(state: &AppState) -> bool {
    state.pet.lock().unwrap().owner.is_home()
}

pub fn on_pet_msg(state: &Shared, from: &str, msg: PetMsg) {
    let received = state.mono_ms();
    push(state, PetCmd::Msg(from.to_string(), msg, received));
}

pub fn on_peer_connected(state: &Shared, _peer: &str) {
    state.mark_dirty();
}

pub fn on_peer_disconnected(state: &Shared, _peer: &str) {
    state.mark_dirty();
}

pub fn on_member_removed(state: &Shared, node: &str) {
    push(state, PetCmd::MemberGone(node.to_string()));
}

pub fn on_left_group(state: &Shared) {
    push(state, PetCmd::LeftGroup);
}

pub fn poke(state: &Shared) {
    push(state, PetCmd::Poke);
}

pub fn drag_start(state: &Shared, label: &str) {
    push(state, PetCmd::DragStart(label.to_string()));
}

pub fn drag_end(state: &Shared, label: &str) {
    push(state, PetCmd::DragEnd(label.to_string()));
}

pub fn recall(state: &Shared) {
    push(state, PetCmd::Recall);
}

pub fn go_visit(state: &Shared, side: Side) {
    push(state, PetCmd::GoVisit(side));
}

pub fn send_guest_home(state: &Shared, owner: &str) {
    push(state, PetCmd::SendGuestHome(owner.to_string()));
}

pub fn set_hit_regions(state: &Shared, label: &str, rects: Vec<HitRect>) {
    state
        .pet
        .lock()
        .unwrap()
        .hit
        .insert(label.to_string(), rects);
}

/// 前端查询窗口信息（来访宠物的主人名字和颜色）。
#[derive(Debug, Clone, Serialize)]
pub struct WindowInfo {
    pub guest: bool,
    pub owner_name: String,
    pub machine_name: String,
    pub hue: u16,
    pub frame: Option<FramePayload>,
    /// 当前节日主题（来访的宠物也用这台机器的节日主题）。
    pub holiday: Option<steppy_core::holiday::ActiveHoliday>,
    /// 导入的形象 id（只用在自家宠物上；来访的宠物总是默认的步步）。
    pub skin: Option<String>,
}

pub fn window_info(state: &Shared, label: &str) -> WindowInfo {
    let shared = state.pet.lock().unwrap();
    let frame = shared.frames.get(label).cloned();
    if let Some(owner) = label.strip_prefix("guest-") {
        if let Some(v) = shared.visitors.get(owner) {
            return WindowInfo {
                guest: true,
                owner_name: v.owner_name.clone(),
                machine_name: v.machine_name.clone(),
                hue: v.hue,
                frame,
                holiday: crate::holiday_rt::current(state),
                skin: None,
            };
        }
    }
    let s = state.settings();
    WindowInfo {
        guest: false,
        owner_name: s.owner_name,
        machine_name: s.machine_name,
        hue: s.pet_hue,
        frame,
        holiday: crate::holiday_rt::current(state),
        skin: s.pet_skin.filter(|id| crate::skin_rt::get(id).is_some()),
    }
}

/// 面板上显示的宠物位置。
#[derive(Debug, Clone, Serialize)]
pub struct PetStatus {
    /// home / leaving / away / returning
    pub location: String,
    pub away_at: Option<NodeId>,
    pub away_at_name: Option<String>,
    pub guests: Vec<GuestView>,
    pub left_neighbor: Option<NodeId>,
    pub right_neighbor: Option<NodeId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GuestView {
    pub owner: NodeId,
    pub owner_name: String,
    pub machine_name: String,
}

pub fn status(state: &Shared) -> PetStatus {
    let (owner_state, guests) = {
        let p = state.pet.lock().unwrap();
        let guests = p
            .guests
            .iter()
            .filter(|(_, g)| g.is_visible())
            .filter_map(|(o, _)| {
                p.visitors.get(o).map(|v| GuestView {
                    owner: o.clone(),
                    owner_name: v.owner_name.clone(),
                    machine_name: v.machine_name.clone(),
                })
            })
            .collect();
        (p.owner.state.clone(), guests)
    };
    let (location, away_at) = match owner_state {
        OwnerState::Home => ("home", None),
        OwnerState::Offering { to, .. } => ("leaving", Some(to)),
        OwnerState::Away { at, .. } => ("away", Some(at)),
        OwnerState::Reclaiming { at, .. } => ("returning", Some(at)),
    };
    let (l, r) = neighbors(state);
    PetStatus {
        location: location.into(),
        away_at_name: away_at.as_ref().map(|n| state.machine_name(n)),
        away_at,
        guests,
        left_neighbor: l,
        right_neighbor: r,
    }
}

fn neighbors(state: &Shared) -> (Option<NodeId>, Option<NodeId>) {
    let me = state.me().clone();
    state
        .group
        .lock()
        .unwrap()
        .as_ref()
        .map(|g| g.neighbors(&me))
        .unwrap_or((None, None))
}

/// 某台机器在本机的哪一侧（只有直接相邻才算）。
fn side_of(state: &Shared, node: &str) -> Option<Side> {
    let (l, r) = neighbors(state);
    if l.as_deref() == Some(node) {
        Some(Side::Left)
    } else if r.as_deref() == Some(node) {
        Some(Side::Right)
    } else {
        None
    }
}

/// 这一侧的邻居是否可以接待。
fn neighbor_available(state: &Shared, side: Side) -> Option<NodeId> {
    if !state.settings().allow_visits {
        return None;
    }
    let (l, r) = neighbors(state);
    let n = match side {
        Side::Left => l,
        Side::Right => r,
    }?;
    if !state.net.is_connected(&n) {
        return None;
    }
    let accepts = state
        .peers
        .lock()
        .unwrap()
        .get(&n)
        .and_then(|p| p.info.as_ref().map(|i| i.accepts_visitors))
        .unwrap_or(false);
    accepts.then_some(n)
}

// ─────────────────────────── 引擎 ───────────────────────────

struct WinState {
    brain: PetBrain,
    last_pos: Option<(i32, i32)>,
    last_frame: Option<FramePayload>,
    ignore: Option<bool>,
    shown: bool,
    size: (f64, f64),
    scale: f64,
    size_checked: Instant,
}

/// 建造带窗口：(显示器 key, (x, y, 宽, 高) 物理像素)。
type StripGeo = (String, (i32, i32, u32, u32));

struct Engine {
    state: Shared,
    wins: HashMap<String, WinState>,
    screens: Vec<Screen>,
    /// 主显示器工作区的右侧（宠物第一次出现的位置）。
    home_x: Option<f64>,
    screens_at: Instant,
    /// 已应用的建造带版本（方块或建造模式变化时重新计算地形和窗口）。
    world_version: u64,
    /// 建造带窗口：label → (显示器 key, 物理位置和大小)。
    strips: HashMap<String, StripGeo>,
    drag: Option<(String, f64, f64)>,
    last_exit_side: Side,
    visit_started: HashMap<NodeId, i64>,
    last_tick: i64,
    saved_owner: Option<String>,
}

pub fn start(state: Shared) {
    std::thread::Builder::new()
        .name("pet-engine".into())
        .spawn(move || {
            let mut e = Engine {
                state: state.clone(),
                wins: HashMap::new(),
                screens: Vec::new(),
                home_x: None,
                world_version: 0,
                strips: HashMap::new(),
                screens_at: Instant::now() - Duration::from_secs(10),
                drag: None,
                last_exit_side: Side::Right,
                visit_started: HashMap::new(),
                last_tick: state.mono_ms(),
                saved_owner: None,
            };
            // 恢复：如果上次退出时宠物在别人那里，等租约过期后再出现。
            {
                let mut p = state.pet.lock().unwrap();
                if p.restored {
                    let st = p.owner.state.clone();
                    p.owner =
                        OwnerHandoff::restore(state.me().clone(), p.owner.seq, st, state.mono_ms());
                }
            }
            loop {
                let t0 = Instant::now();
                e.tick();
                let spent = t0.elapsed();
                if spent > Duration::from_millis(200) {
                    log::debug!("宠物引擎一帧耗时 {spent:?}");
                }
                std::thread::sleep(Duration::from_millis(33).saturating_sub(spent));
            }
        })
        .expect("spawn pet engine");
}

fn label_for_guest(owner: &str) -> String {
    format!("guest-{owner}")
}

impl Engine {
    fn refresh_screens(&mut self) {
        let world_version = crate::world_rt::version();
        let world_changed = world_version != self.world_version;
        if !world_changed
            && self.screens_at.elapsed() < Duration::from_secs(2)
            && !self.screens.is_empty()
        {
            return;
        }
        self.screens_at = Instant::now();
        self.world_version = world_version;
        let Ok(ms) = self.state.app.available_monitors() else {
            return;
        };
        if ms.is_empty() {
            return;
        }
        let keyed: Vec<(String, usize)> = ms
            .iter()
            .map(|m| {
                let key = match m.name() {
                    Some(n) if !n.is_empty() => {
                        format!("{n}@{}x{}", m.position().x, m.position().y)
                    }
                    _ => format!("{}x{}", m.position().x, m.position().y),
                };
                let cols = steppy_core::world::cols_for(
                    m.work_area().size.width as f64 / m.scale_factor(),
                );
                (key, cols)
            })
            .collect();
        crate::world_rt::adopt(&keyed);
        let mut mons: Vec<(String, Screen)> = ms
            .iter()
            .zip(keyed)
            .map(|(m, (key, cols))| {
                let wa = m.work_area();
                let scale = m.scale_factor();
                let world = crate::world_rt::world_for(&key, cols);
                (
                    key,
                    Screen {
                        work: Rect {
                            x: wa.position.x,
                            y: wa.position.y,
                            w: wa.size.width as i32,
                            h: wa.size.height as i32,
                        },
                        scale,
                        terrain: world.heights(),
                        cell: steppy_core::world::CELL * scale,
                    },
                )
            })
            .collect();
        mons.sort_by_key(|(_, s)| s.work.x);
        self.screens = mons.iter().map(|(_, s)| s.clone()).collect();
        if let Ok(Some(p)) = self.state.app.primary_monitor() {
            let wa = p.work_area();
            let w = PET_W * p.scale_factor();
            self.home_x = Some(wa.position.x as f64 + wa.size.width as f64 - w * 1.5);
        }
        self.sync_strips(&mons, world_changed);
    }

    /// 只给需要的显示器创建建造带窗口（有方块或在建造模式），不需要的销毁。
    fn sync_strips(&mut self, mons: &[(String, Screen)], world_changed: bool) {
        let build = crate::world_rt::build_mode();
        let mut wanted: HashMap<String, StripGeo> = HashMap::new();
        for (i, (key, s)) in mons.iter().enumerate() {
            if !crate::world_rt::needs_strip(key) {
                continue;
            }
            let h = (steppy_core::world::ROWS as f64 * steppy_core::world::CELL * s.scale).round()
                as u32;
            let geo = (s.work.x, s.work.bottom() - h as i32, s.work.w as u32, h);
            wanted.insert(format!("strip-{i}"), (key.clone(), geo));
        }
        // 销毁不再需要的。
        let stale: Vec<String> = self
            .strips
            .keys()
            .filter(|l| !wanted.contains_key(*l))
            .cloned()
            .collect();
        for label in stale {
            self.strips.remove(&label);
            crate::world_rt::unregister_strip(&label);
            if let Some(w) = self.window(&label) {
                let _ = w.destroy();
            }
        }
        let mut created = false;
        for (label, (key, geo)) in wanted {
            crate::world_rt::register_strip(&label, &key);
            let existing = self.strips.get(&label).cloned();
            if existing.as_ref().map(|e| e.1) != Some(geo) {
                let win = match self.window(&label) {
                    Some(w) => w,
                    None => {
                        let built = WebviewWindowBuilder::new(
                            &self.state.app,
                            &label,
                            WebviewUrl::App("index.html".into()),
                        )
                        .title("步步 · 建造")
                        .inner_size(400.0, 100.0)
                        .decorations(false)
                        .transparent(true)
                        .shadow(false)
                        .always_on_top(true)
                        .skip_taskbar(true)
                        .resizable(false)
                        .focused(false)
                        .visible(false)
                        .build();
                        match built {
                            Ok(w) => {
                                created = true;
                                w
                            }
                            Err(e) => {
                                log::error!("创建建造带窗口失败：{e}");
                                continue;
                            }
                        }
                    }
                };
                let _ = win.set_size(tauri::PhysicalSize::new(geo.2, geo.3));
                let _ = win.set_position(PhysicalPosition::new(geo.0, geo.1));
                let _ = win.show();
                self.strips.insert(label.clone(), (key, geo));
            }
            if let Some(w) = self.window(&label) {
                // 非建造模式下整条都鼠标穿透（必须在窗口显示之后设置）。
                let _ = w.set_ignore_cursor_events(!build);
                if world_changed {
                    let _ = w.emit("world://changed", build);
                }
            }
        }
        if created {
            // 新建的置顶窗口会盖在宠物上面，把宠物重新提到最上层。
            for label in self.wins.keys() {
                if let Some(w) = self.window(label) {
                    let _ = w.set_always_on_top(false);
                    let _ = w.set_always_on_top(true);
                }
            }
        }
    }

    fn window(&self, label: &str) -> Option<WebviewWindow> {
        self.state.app.get_webview_window(label)
    }

    fn ensure_win(&mut self, label: &str) -> bool {
        if self.wins.contains_key(label) {
            return true;
        }
        let Some(w) = self.window(label) else {
            return false;
        };
        let scale = w.scale_factor().unwrap_or(1.0);
        // 注意：Linux 上窗口显示（realize）之前不能设置鼠标穿透，否则 tao 会 panic；在第一次显示后再设置。
        let mut brain = PetBrain::new(rand::random::<u64>() | 1);
        brain.tuning.wander_exit_chance = 0.0;
        let size = (PET_W * scale, PET_H * scale);
        if label == "pet" {
            brain.place(&self.screens, self.home_x, size.0, size.1);
            brain.hide();
        } else {
            brain.hide();
        }
        self.wins.insert(
            label.to_string(),
            WinState {
                brain,
                last_pos: None,
                last_frame: None,
                ignore: None,
                shown: false,
                size,
                scale,
                size_checked: Instant::now() - Duration::from_secs(5),
            },
        );
        true
    }

    fn create_guest_window(&mut self, owner: &str) {
        let label = label_for_guest(owner);
        if self.window(&label).is_none() {
            let built = WebviewWindowBuilder::new(
                &self.state.app,
                &label,
                WebviewUrl::App("index.html".into()),
            )
            .title("步步 · 访客")
            .inner_size(PET_W, PET_H)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .focused(false)
            .visible(false)
            .build();
            if let Err(e) = built {
                log::error!("创建访客窗口失败：{e}");
                return;
            }
        }
        self.ensure_win(&label);
    }

    fn destroy_guest_window(&mut self, owner: &str) {
        let label = label_for_guest(owner);
        self.wins.remove(&label);
        if let Some(w) = self.window(&label) {
            let _ = w.destroy();
        }
        self.state.pet.lock().unwrap().frames.remove(&label);
    }

    fn send(&self, to: &str, msg: PetMsg) {
        self.state.net.send(to, WireMsg::Pet(msg));
    }

    fn apply_owner_actions(&mut self, actions: Vec<OwnerAction>) {
        for a in actions {
            log::debug!("主人动作：{a:?}");
            match a {
                OwnerAction::Send(to, m) => self.send(&to, m),
                OwnerAction::ShowAtHome { from, reason } => {
                    log::info!("宠物回家：{reason:?}（来自 {from:?}）");
                    let (w, h) = self.size_of("pet");
                    let side = from
                        .as_deref()
                        .and_then(|n| side_of(&self.state, n))
                        .unwrap_or(self.last_exit_side);
                    let screens = self.screens.clone();
                    if let Some(win) = self.wins.get_mut("pet") {
                        win.brain.enter_from(side, &screens, w, h);
                    }
                    let name = from
                        .as_deref()
                        .map(|n| self.state.machine_name(n))
                        .unwrap_or_default();
                    let text = match reason {
                        ReturnReason::Returned => format!("我从「{name}」回来啦！"),
                        ReturnReason::Reclaimed => format!("和「{name}」失去联系了，我先回家。"),
                        ReturnReason::NotAccepted => "邻居现在不方便，下次再去串门。".to_string(),
                    };
                    notify::info(&self.state, &text);
                    self.state.mark_dirty();
                }
            }
        }
    }

    fn apply_host_actions(&mut self, actions: Vec<HostAction>) {
        for a in actions {
            log::debug!("接待动作：{a:?}");
            match a {
                HostAction::Send(to, m) => self.send(&to, m),
                HostAction::ShowGuest { owner, visitor } => {
                    self.state
                        .pet
                        .lock()
                        .unwrap()
                        .visitors
                        .insert(owner.clone(), visitor.clone());
                    self.create_guest_window(&owner);
                    let label = label_for_guest(&owner);
                    let (w, h) = self.size_of(&label);
                    let screens = self.screens.clone();
                    if let Some(win) = self.wins.get_mut(&label) {
                        win.brain.enter_from(visitor.enter_side, &screens, w, h);
                    }
                    self.visit_started
                        .insert(owner.clone(), self.state.mono_ms());
                    // 新窗口的页面要过一会儿才加载完，稍等再打招呼。
                    let app = self.state.app.clone();
                    let greet = notify::Bubble {
                        id: uuid::Uuid::new_v4().to_string(),
                        text: format!("我是{}的步步，来串门啦！", visitor.owner_name),
                        kind: "info".into(),
                        actions: vec![],
                        ttl_ms: 5000,
                    };
                    let target = label.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(1500));
                        let _ = app.emit_to(target.as_str(), "pet://bubble", greet);
                    });
                    let _ = self.state.app.emit("steppy://guest", ());
                    self.state.mark_dirty();
                }
                HostAction::HideGuest { owner, reason } => {
                    let name = self
                        .state
                        .pet
                        .lock()
                        .unwrap()
                        .visitors
                        .get(&owner)
                        .map(|v| v.owner_name.clone())
                        .unwrap_or_default();
                    self.destroy_guest_window(&owner);
                    self.visit_started.remove(&owner);
                    if reason == GuestLeaveReason::LeaseExpired {
                        notify::info(&self.state, &format!("{name}的步步回家了。"));
                    }
                    self.state.mark_dirty();
                }
                HostAction::StartLeaving { owner } => self.guest_leave(&owner),
            }
        }
    }

    /// 让来访的宠物往主人那一侧走出去；主人不在旁边就直接离开。
    fn guest_leave(&mut self, owner: &str) {
        let label = label_for_guest(owner);
        match side_of(&self.state, owner) {
            Some(side) if self.wins.contains_key(&label) => {
                if let Some(w) = self.wins.get_mut(&label) {
                    w.brain.request_exit(side);
                }
            }
            _ => {
                let acts = self
                    .state
                    .pet
                    .lock()
                    .unwrap()
                    .guests
                    .get_mut(owner)
                    .map(|g| g.leave())
                    .unwrap_or_default();
                self.apply_host_actions(acts);
            }
        }
    }

    fn size_of(&self, label: &str) -> (f64, f64) {
        self.wins
            .get(label)
            .map(|w| w.size)
            .unwrap_or((PET_W, PET_H))
    }

    fn process_queue(&mut self) {
        let cmds: Vec<PetCmd> = std::mem::take(&mut self.state.pet.lock().unwrap().queue);
        let now_wall = steppy_core::now_ms();
        for c in cmds {
            match c {
                PetCmd::React(anim, hold) => {
                    if let Some(w) = self.wins.get_mut("pet") {
                        w.brain.react(anim, now_wall, hold);
                    }
                }
                PetCmd::ClearReaction(anim) => {
                    if let Some(w) = self.wins.get_mut("pet") {
                        w.brain.clear_reaction(anim, now_wall);
                    }
                }
                PetCmd::Poke => {
                    if let Some(w) = self.wins.get_mut("pet") {
                        w.brain.poke(now_wall);
                    }
                }
                PetCmd::DragStart(label) => {
                    if let (Some(w), Ok(c)) =
                        (self.wins.get_mut(&label), self.state.app.cursor_position())
                    {
                        if !w.brain.is_hidden() {
                            w.brain.start_drag(now_wall);
                            self.drag = Some((label.clone(), c.x - w.brain.x, c.y - w.brain.y));
                        }
                    }
                }
                PetCmd::DragEnd(label) => {
                    if self.drag.as_ref().map(|d| d.0 == label).unwrap_or(false) {
                        self.drag = None;
                    }
                    let screens = self.screens.clone();
                    let st = self.state.clone();
                    let is_guest = label.starts_with("guest-");
                    let owner = label.strip_prefix("guest-").map(|s| s.to_string());
                    if let Some(w) = self.wins.get_mut(&label) {
                        let width = w.size.0;
                        if is_guest {
                            let home = owner.as_deref().and_then(|o| side_of(&st, o));
                            w.brain.end_drag(&screens, width, |s| Some(s) == home);
                        } else {
                            let home = st.pet.lock().unwrap().owner.is_home();
                            w.brain.end_drag(&screens, width, |s| {
                                home && neighbor_available(&st, s).is_some()
                            });
                        }
                    }
                }
                PetCmd::Msg(from, msg, received) => {
                    log::debug!(
                        "交接消息 来自 {from}：{:?}（seq {}，收到于 {received}）",
                        msg.kind,
                        msg.seq
                    );
                    if &msg.pet == self.state.me() {
                        let acts = self
                            .state
                            .pet
                            .lock()
                            .unwrap()
                            .owner
                            .on_msg(&from, &msg, received);
                        self.apply_owner_actions(acts);
                    } else {
                        let willing =
                            self.state.settings().allow_visits && self.state.settings().pet_visible;
                        let acts = {
                            let mut p = self.state.pet.lock().unwrap();
                            let slot = p
                                .guests
                                .entry(msg.pet.clone())
                                .or_insert_with(|| GuestSlot::new(msg.pet.clone()));
                            slot.on_msg(&from, &msg, received, willing)
                        };
                        self.apply_host_actions(acts);
                    }
                }
                PetCmd::Recall => {
                    let acts = self.state.pet.lock().unwrap().owner.recall();
                    self.apply_owner_actions(acts);
                }
                PetCmd::GoVisit(side) => {
                    let home = self.state.pet.lock().unwrap().owner.is_home();
                    log::info!(
                        "去串门：{side:?}，在家={home}，邻居可用={:?}",
                        neighbor_available(&self.state, side)
                    );
                    if home && neighbor_available(&self.state, side).is_some() {
                        if let Some(w) = self.wins.get_mut("pet") {
                            w.brain.request_exit(side);
                        }
                    }
                }
                PetCmd::SendGuestHome(owner) => self.guest_leave(&owner),
                PetCmd::MemberGone(node) => {
                    // 被移除的机器的宠物立即离开（无法再通知它）。
                    let visible = self
                        .state
                        .pet
                        .lock()
                        .unwrap()
                        .guests
                        .remove(&node)
                        .map(|g| g.is_visible())
                        .unwrap_or(false);
                    if visible || self.wins.contains_key(&label_for_guest(&node)) {
                        self.destroy_guest_window(&node);
                    }
                }
                PetCmd::LeftGroup => {
                    let owners: Vec<NodeId> =
                        std::mem::take(&mut self.state.pet.lock().unwrap().guests)
                            .into_keys()
                            .collect();
                    for o in owners {
                        self.destroy_guest_window(&o);
                    }
                }
            }
        }
    }

    fn handoff_ticks(&mut self) {
        let now = self.state.mono_ms();
        let (oa, ha) = {
            let mut p = self.state.pet.lock().unwrap();
            let oa = p.owner.tick(now);
            let mut ha = Vec::new();
            for g in p.guests.values_mut() {
                ha.extend(g.tick(now));
            }
            (oa, ha)
        };
        self.apply_owner_actions(oa);
        self.apply_host_actions(ha);
        // 来访时间到了：送回家。
        let minutes = self.state.settings().visit_minutes as i64;
        let expired: Vec<NodeId> = self
            .visit_started
            .iter()
            .filter(|(_, t)| now - **t > minutes * 60_000)
            .map(|(o, _)| o.clone())
            .collect();
        for o in expired {
            self.visit_started.insert(o.clone(), now + 3_600_000); // 避免重复触发
            self.guest_leave(&o);
        }
        // 持久化主人状态。
        let p = self.state.pet.lock().unwrap();
        let ser = serde_json::to_string(&(&p.owner.seq, &p.owner.state)).unwrap_or_default();
        if self.saved_owner.as_deref() != Some(ser.as_str()) {
            p.save();
            drop(p);
            self.saved_owner = Some(ser);
            self.state.mark_dirty();
        }
    }

    fn tick(&mut self) {
        self.refresh_screens();
        if self.screens.is_empty() {
            return;
        }
        self.ensure_win("pet");
        self.process_queue();
        self.handoff_ticks();

        let now_wall = steppy_core::now_ms();
        let dt = (now_wall - self.last_tick).clamp(0, 200);
        self.last_tick = now_wall;
        let settings = self.state.settings();
        let cursor = self.state.app.cursor_position().ok();
        let owner_home = self.state.pet.lock().unwrap().owner.is_home();

        // 本机宠物在家且可见时才显示。
        let labels: Vec<String> = self.wins.keys().cloned().collect();
        let mut events: Vec<(String, BrainEvent)> = Vec::new();
        for label in labels {
            let is_guest = label.starts_with("guest-");
            let owner = label.strip_prefix("guest-").map(|s| s.to_string());
            let st = self.state.clone();
            let screens = self.screens.clone();
            let home_x = self.home_x;
            let drag = self.drag.clone();
            let Some(w) = self.wins.get_mut(&label) else {
                continue;
            };
            // 偶尔刷新窗口实际尺寸（跨 DPI 显示器时会变）。
            if w.size_checked.elapsed() > Duration::from_millis(500) {
                w.size_checked = Instant::now();
                if let Some(win) = st.app.get_webview_window(&label) {
                    if let Ok(s) = win.outer_size() {
                        if s.width > 0 && s.height > 0 {
                            w.size = (s.width as f64, s.height as f64);
                        }
                    }
                    if let Ok(sf) = win.scale_factor() {
                        w.scale = sf;
                    }
                }
            }
            if !is_guest {
                w.brain.tuning.wander_exit_chance = if settings.wander_visits { 0.08 } else { 0.0 };
                // 主人状态从“不在家”变回“在家”的那一刻由 ShowAtHome 处理；这里只处理首次显示。
                if owner_home && w.brain.is_hidden() && !w.shown && w.last_frame.is_none() {
                    w.brain.place(&screens, home_x, w.size.0, w.size.1);
                }
            }
            if let (Some((dl, dx, dy)), Some(c)) = (drag.as_ref(), cursor) {
                if dl == &label {
                    w.brain.drag_to(c.x - dx, c.y - dy);
                }
            }
            let allow: Box<dyn Fn(Side) -> bool> = if is_guest {
                let home = owner.as_deref().and_then(|o| side_of(&st, o));
                Box::new(move |s| Some(s) == home)
            } else {
                let st2 = st.clone();
                Box::new(move |s| owner_home && neighbor_available(&st2, s).is_some())
            };
            let frame = w
                .brain
                .tick(now_wall, dt, &screens, w.size.0, w.size.1, &*allow);
            if let Some(ev) = frame.event {
                events.push((label.clone(), ev));
            }
            let visible = frame.visible && (is_guest || settings.pet_visible);
            let win = st.app.get_webview_window(&label);
            if let Some(win) = &win {
                if visible {
                    if w.last_pos != Some((frame.x, frame.y)) {
                        let _ = win.set_position(PhysicalPosition::new(frame.x, frame.y));
                        w.last_pos = Some((frame.x, frame.y));
                    }
                    if !w.shown {
                        let _ = win.show();
                        w.shown = true;
                    }
                } else if w.shown {
                    let _ = win.hide();
                    w.shown = false;
                }
            }
            st.pet
                .lock()
                .unwrap()
                .positions
                .insert(label.clone(), (frame.x, frame.y));
            let payload = FramePayload {
                anim: frame.anim,
                facing: frame.facing,
                visible,
            };
            if w.last_frame.as_ref() != Some(&payload) {
                w.last_frame = Some(payload.clone());
                let _ = st
                    .app
                    .emit_to(label.as_str(), "pet://frame", payload.clone());
                st.pet.lock().unwrap().frames.insert(label.clone(), payload);
            }
            // 鼠标穿透：只有光标在宠物（和气泡）上时才接收鼠标。
            if let (Some(win), Some(c), true) = (&win, cursor, w.shown) {
                let dragging = drag.as_ref().map(|d| d.0 == label).unwrap_or(false);
                let inside = dragging
                    || (visible && {
                        let hits = st
                            .pet
                            .lock()
                            .unwrap()
                            .hit
                            .get(&label)
                            .cloned()
                            .unwrap_or_default();
                        hits.iter().any(|r| {
                            let x0 = frame.x as f64 + r.x * w.scale;
                            let y0 = frame.y as f64 + r.y * w.scale;
                            c.x >= x0
                                && c.x < x0 + r.w * w.scale
                                && c.y >= y0
                                && c.y < y0 + r.h * w.scale
                        })
                    });
                let ignore = !inside;
                if w.ignore != Some(ignore) {
                    let _ = win.set_ignore_cursor_events(ignore);
                    w.ignore = Some(ignore);
                }
            }
        }

        for (label, ev) in events {
            match (label.as_str(), ev) {
                ("pet", BrainEvent::Exited(side)) => {
                    self.last_exit_side = side;
                    log::info!("宠物走出了{side:?}边缘");
                    match neighbor_available(&self.state, side) {
                        Some(target) => {
                            let visitor = Visitor {
                                owner_name: settings.owner_name.clone(),
                                machine_name: settings.machine_name.clone(),
                                hue: settings.pet_hue,
                                enter_side: side.opposite(),
                            };
                            let now = self.state.mono_ms();
                            let acts = self
                                .state
                                .pet
                                .lock()
                                .unwrap()
                                .owner
                                .begin_visit(target, visitor, now);
                            match acts {
                                Some(a) => self.apply_owner_actions(a),
                                None => self.reenter("pet", side),
                            }
                        }
                        None => self.reenter("pet", side),
                    }
                }
                (l, BrainEvent::Exited(_)) if l.starts_with("guest-") => {
                    let owner = l.trim_start_matches("guest-").to_string();
                    let acts = self
                        .state
                        .pet
                        .lock()
                        .unwrap()
                        .guests
                        .get_mut(&owner)
                        .map(|g| g.leave())
                        .unwrap_or_default();
                    self.apply_host_actions(acts);
                }
                _ => {}
            }
        }
    }

    fn reenter(&mut self, label: &str, side: Side) {
        let screens = self.screens.clone();
        if let Some(w) = self.wins.get_mut(label) {
            let (ww, hh) = w.size;
            w.brain.enter_from(side, &screens, ww, hh);
        }
    }
}
