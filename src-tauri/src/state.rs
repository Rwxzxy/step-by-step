//! 全局状态。

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use steppy_core::agents::codex::CodexTracker;
use steppy_core::agents::AgentSession;
use steppy_core::calendar::EventRecord;
use steppy_core::net::identity::IdentityFile;
use steppy_core::net::wire::PeerInfo;
use steppy_core::net::{GroupState, Identity, NodeId};
use tauri::{AppHandle, Emitter};

use crate::net_rt::NetHandle;
use crate::pet_rt::PetShared;
use crate::settings::Settings;
use crate::storage;

/// 本机的 Agent 会话和从其他机器同步来的会话。
#[derive(Default)]
pub struct AgentStore {
    pub local: BTreeMap<String, AgentSession>,
    pub codex: HashMap<PathBuf, CodexTracker>,
    pub remote: BTreeMap<NodeId, BTreeMap<String, AgentSession>>,
}

/// 在线的其他机器。
#[derive(Clone, Debug)]
pub struct PeerEntry {
    pub info: Option<PeerInfo>,
}

/// 正在进行的配对（本机显示配对码）。
#[derive(Clone, Debug)]
pub struct PairingWindow {
    pub code: String,
    pub expires: Instant,
    pub failures: u32,
}

/// mDNS 发现的实例。
#[derive(Clone, Debug)]
pub struct Discovered {
    pub addrs: Vec<std::net::SocketAddr>,
    pub pairing_open: bool,
    pub seen: Instant,
}

pub struct AppState {
    pub app: AppHandle,
    pub settings: RwLock<Settings>,
    pub identity: Identity,
    pub group: Mutex<Option<GroupState>>,
    pub agents: Mutex<AgentStore>,
    pub calendar: Mutex<BTreeMap<String, EventRecord>>,
    pub peers: Mutex<BTreeMap<NodeId, PeerEntry>>,
    pub pairing: Mutex<Option<PairingWindow>>,
    pub discovered: Mutex<HashMap<NodeId, Discovered>>,
    pub pet: Mutex<PetShared>,
    pub net: NetHandle,
    pub started: Instant,
    dirty: AtomicBool,
}

pub type Shared = Arc<AppState>;

fn load_identity() -> Identity {
    let p = storage::path("identity.json");
    if let Some(f) = storage::read_json::<IdentityFile>(&p) {
        if let Some(id) = Identity::from_file(&f) {
            return id;
        }
    }
    let id = Identity::generate();
    if let Err(e) = storage::write_json(&p, &id.to_file(), true) {
        log::error!("保存身份失败：{e}");
    }
    id
}

impl AppState {
    pub fn new(app: AppHandle, net: NetHandle) -> Shared {
        let settings = Settings::load();
        let identity = load_identity();
        let group: Option<GroupState> = storage::read_json(&storage::path("group.json"));
        let calendar: BTreeMap<String, EventRecord> =
            storage::read_json(&storage::path("calendar.json")).unwrap_or_default();
        let pet = PetShared::load(&identity.node_id);
        Arc::new(AppState {
            app,
            settings: RwLock::new(settings),
            identity,
            group: Mutex::new(group),
            agents: Mutex::new(AgentStore::default()),
            calendar: Mutex::new(calendar),
            peers: Mutex::new(BTreeMap::new()),
            pairing: Mutex::new(None),
            discovered: Mutex::new(HashMap::new()),
            pet: Mutex::new(pet),
            net,
            started: Instant::now(),
            dirty: AtomicBool::new(true),
        })
    }

    pub fn me(&self) -> &NodeId {
        &self.identity.node_id
    }

    /// 单调毫秒时钟（交接协议只比较时间差）。
    pub fn mono_ms(&self) -> i64 {
        self.started.elapsed().as_millis() as i64
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    /// 标记界面需要刷新（由 [`crate::run_ui_notifier`] 合并后发给前端）。
    pub fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Release);
    }

    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    pub fn emit<S: serde::Serialize + Clone>(&self, event: &str, payload: S) {
        if let Err(e) = self.app.emit(event, payload) {
            log::warn!("发送事件 {event} 失败：{e}");
        }
    }

    pub fn save_group(&self) {
        let g = self.group.lock().unwrap().clone();
        let p = storage::path("group.json");
        match g {
            Some(g) => {
                if let Err(e) = storage::write_json(&p, &g, true) {
                    log::error!("保存家庭组失败：{e}");
                }
            }
            None => {
                let _ = std::fs::remove_file(p);
            }
        }
    }

    pub fn save_calendar(&self) {
        let c = self.calendar.lock().unwrap().clone();
        if let Err(e) = storage::write_json(&storage::path("calendar.json"), &c, false) {
            log::error!("保存日程失败：{e}");
        }
    }

    pub fn peer_info(&self) -> PeerInfo {
        let s = self.settings();
        PeerInfo {
            machine_name: s.machine_name.clone(),
            owner_name: s.owner_name.clone(),
            hue: s.pet_hue,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            shares_content: false,
            accepts_visitors: s.allow_visits,
        }
    }

    /// 某台机器的显示名。
    pub fn machine_name(&self, node: &str) -> String {
        if node == self.me() {
            return self.settings().machine_name;
        }
        if let Some(p) = self.peers.lock().unwrap().get(node) {
            if let Some(i) = &p.info {
                return i.machine_name.clone();
            }
        }
        if let Some(g) = self.group.lock().unwrap().as_ref() {
            if let Some(m) = g.members.get(node) {
                return m.machine_name.clone();
            }
        }
        format!("设备 {}", &node[..node.len().min(6)])
    }
}
