//! 前端调用的命令。

use serde::{Deserialize, Serialize};
use steppy_core::agents::AgentSession;
use steppy_core::calendar::EventData;
use steppy_core::net::NodeId;
use steppy_core::pet::Side;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;

use crate::calendar_rt::{self, AgendaItem};
use crate::integrate::{self, IntegrationStatus};
use crate::net_rt;
use crate::notify;
use crate::pet_rt::{self, HitRect, PetStatus, WindowInfo};
use crate::settings::Settings;
use crate::state::Shared;

type R<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct MeView {
    pub node_id: NodeId,
    pub machine_name: String,
    pub owner_name: String,
    pub hue: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct MachineView {
    pub node_id: NodeId,
    pub name: String,
    pub owner_name: String,
    pub is_me: bool,
    pub online: bool,
    /// 对方是否把消息正文发给了我。
    pub shares_content_with_me: bool,
    /// 我是否把消息正文发给对方。
    pub i_share_content: bool,
    pub sessions: Vec<AgentSession>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairingView {
    pub code: String,
    pub expires_in_sec: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupView {
    pub group_id: String,
    pub epoch: u32,
    pub layout: Vec<NodeId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub me: MeView,
    pub machines: Vec<MachineView>,
    pub today: Vec<AgendaItem>,
    pub group: Option<GroupView>,
    pub pairing: Option<PairingView>,
    pub pet: PetStatus,
    pub quiet_now: bool,
    /// 局域网里发现的、还没配对的步步数量。
    pub discovered_unpaired: usize,
    pub net_port: u16,
    /// 当前节日主题。
    pub holiday: Option<steppy_core::holiday::ActiveHoliday>,
}

fn sorted(mut v: Vec<AgentSession>) -> Vec<AgentSession> {
    v.sort_by_key(|s| std::cmp::Reverse(s.updated_ms));
    v
}

pub fn build_snapshot(state: &Shared) -> Snapshot {
    let s = state.settings();
    let me = state.me().clone();
    let group = state.group.lock().unwrap().clone();
    let agents = state.agents.lock().unwrap();
    let peers = state.peers.lock().unwrap().clone();
    let mut machines = vec![MachineView {
        node_id: me.clone(),
        name: s.machine_name.clone(),
        owner_name: s.owner_name.clone(),
        is_me: true,
        online: true,
        shares_content_with_me: true,
        i_share_content: true,
        sessions: sorted(agents.local.values().cloned().collect()),
    }];
    if let Some(g) = &group {
        let order: Vec<NodeId> = g.layout.order.clone();
        let mut members: Vec<_> = g
            .active_members()
            .filter(|m| m.node_id != me)
            .cloned()
            .collect();
        members.sort_by_key(|m| {
            order
                .iter()
                .position(|n| n == &m.node_id)
                .unwrap_or(usize::MAX)
        });
        for m in members {
            let info = peers.get(&m.node_id).and_then(|p| p.info.clone());
            machines.push(MachineView {
                node_id: m.node_id.clone(),
                name: info
                    .as_ref()
                    .map(|i| i.machine_name.clone())
                    .unwrap_or(m.machine_name.clone()),
                owner_name: info
                    .as_ref()
                    .map(|i| i.owner_name.clone())
                    .unwrap_or(m.owner_name.clone()),
                is_me: false,
                online: state.net.is_connected(&m.node_id),
                shares_content_with_me: info.map(|i| i.shares_content).unwrap_or(false),
                i_share_content: s.shares_content_with(&m.node_id),
                sessions: sorted(
                    agents
                        .remote
                        .get(&m.node_id)
                        .map(|x| x.values().cloned().collect())
                        .unwrap_or_default(),
                ),
            });
        }
    }
    drop(agents);
    let member_ids: Vec<NodeId> = group
        .as_ref()
        .map(|g| g.members.keys().cloned().collect())
        .unwrap_or_default();
    let discovered_unpaired = state
        .discovered
        .lock()
        .unwrap()
        .keys()
        .filter(|n| !member_ids.contains(n))
        .count();
    let pairing = state.pairing.lock().unwrap().as_ref().and_then(|p| {
        let left = p
            .expires
            .saturating_duration_since(std::time::Instant::now())
            .as_secs();
        (left > 0).then(|| PairingView {
            code: p.code.clone(),
            expires_in_sec: left,
        })
    });
    Snapshot {
        me: MeView {
            node_id: me,
            machine_name: s.machine_name.clone(),
            owner_name: s.owner_name.clone(),
            hue: s.pet_hue,
        },
        machines,
        today: calendar_rt::today(state),
        group: group.map(|g| GroupView {
            group_id: g.group_id.clone(),
            epoch: g.epoch(),
            layout: g.layout.order.clone(),
        }),
        pairing,
        pet: pet_rt::status(state),
        quiet_now: notify::quiet_now(state),
        discovered_unpaired,
        net_port: state.net.port(),
        holiday: crate::holiday_rt::current(state),
    }
}

#[tauri::command]
pub fn get_snapshot(state: State<'_, Shared>) -> Snapshot {
    build_snapshot(&state)
}

#[tauri::command]
pub fn get_settings(state: State<'_, Shared>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(state: State<'_, Shared>, settings: Settings) -> R<Settings> {
    let mut s = settings;
    s.sanitize();
    *state.settings.write().unwrap() = s.clone();
    s.save();
    net_rt::update_my_info(&state);
    crate::holiday_rt::refresh(&state);
    if let Some(w) = state.app.get_webview_window("pet") {
        let _ = w.emit("pet://appearance", ());
    }
    state.mark_dirty();
    Ok(s)
}

#[tauri::command]
pub fn integration_status() -> IntegrationStatus {
    integrate::status()
}

#[tauri::command]
pub fn set_integration(agent: String, enable: bool) -> R<String> {
    integrate::set(&agent, enable).map_err(err)
}

// ─────────────── 日程 ───────────────

#[tauri::command]
pub fn list_agenda(state: State<'_, Shared>, from: String, to: String) -> R<Vec<AgendaItem>> {
    let from = calendar_rt::parse_local(&from).map_err(err)?;
    let to = calendar_rt::parse_local(&to).map_err(err)?;
    Ok(calendar_rt::list(&state, from, to))
}

#[derive(Debug, Deserialize)]
pub struct EventInput {
    pub title: String,
    #[serde(default)]
    pub notes: String,
    pub all_day: bool,
    pub start: String,
    pub end: String,
    pub repeat: steppy_core::calendar::Repeat,
    pub remind_before_min: Option<u32>,
}

#[tauri::command]
pub fn save_event(state: State<'_, Shared>, id: Option<String>, event: EventInput) -> R<String> {
    let data = EventData {
        title: event.title,
        notes: event.notes,
        all_day: event.all_day,
        start: calendar_rt::parse_local(&event.start).map_err(err)?,
        end: calendar_rt::parse_local(&event.end).map_err(err)?,
        repeat: event.repeat,
        remind_before_min: event.remind_before_min,
    };
    calendar_rt::save(&state, id, data).map_err(err)
}

#[tauri::command]
pub fn delete_event(state: State<'_, Shared>, id: String) -> R<()> {
    calendar_rt::delete(&state, &id).map_err(err)
}

#[tauri::command]
pub fn get_event(
    state: State<'_, Shared>,
    id: String,
) -> Option<steppy_core::calendar::EventRecord> {
    calendar_rt::get(&state, &id)
}

#[tauri::command]
pub async fn import_ics(app: AppHandle, state: State<'_, Shared>) -> R<String> {
    let picked = app
        .dialog()
        .file()
        .add_filter("iCalendar", &["ics"])
        .blocking_pick_file();
    let Some(p) = picked else {
        return Ok(String::new());
    };
    let path = p.into_path().map_err(err)?;
    let text = std::fs::read_to_string(&path).map_err(err)?;
    let n = calendar_rt::import_ics(&state, &text).map_err(err)?;
    Ok(format!("导入了 {n} 条日程"))
}

#[tauri::command]
pub async fn export_ics(app: AppHandle, state: State<'_, Shared>) -> R<String> {
    let picked = app
        .dialog()
        .file()
        .add_filter("iCalendar", &["ics"])
        .set_file_name("步步日程.ics")
        .blocking_save_file();
    let Some(p) = picked else {
        return Ok(String::new());
    };
    let path = p.into_path().map_err(err)?;
    std::fs::write(&path, calendar_rt::export_ics(&state)).map_err(err)?;
    Ok(format!("已导出到 {}", path.display()))
}

// ─────────────── 家庭组 ───────────────

#[tauri::command]
pub fn create_group(state: State<'_, Shared>) -> R<()> {
    net_rt::create_group(&state).map_err(err)
}

#[tauri::command]
pub fn start_pairing(state: State<'_, Shared>) -> R<String> {
    net_rt::start_pairing(&state).map_err(err)
}

#[tauri::command]
pub fn cancel_pairing(state: State<'_, Shared>) {
    net_rt::cancel_pairing(&state)
}

#[tauri::command]
pub async fn join_group(state: State<'_, Shared>, code: String) -> R<String> {
    net_rt::join_group(state.inner().clone(), code)
        .await
        .map_err(err)
}

#[tauri::command]
pub fn kick_member(state: State<'_, Shared>, node: String) -> R<()> {
    net_rt::kick(&state, &node).map_err(err)
}

#[tauri::command]
pub fn leave_group(state: State<'_, Shared>) -> R<()> {
    net_rt::leave_group(&state).map_err(err)
}

#[tauri::command]
pub fn set_layout(state: State<'_, Shared>, order: Vec<NodeId>) -> R<()> {
    net_rt::set_layout(&state, order).map_err(err)
}

#[tauri::command]
pub fn set_share_content(state: State<'_, Shared>, node: String, share: bool) -> R<()> {
    {
        let mut s = state.settings.write().unwrap();
        s.share_content_with.insert(node, share);
        s.save();
    }
    net_rt::update_my_info(&state);
    state.mark_dirty();
    Ok(())
}

// ─────────────── 宠物 ───────────────

#[tauri::command]
pub fn pet_window_info(window: WebviewWindow, state: State<'_, Shared>) -> WindowInfo {
    pet_rt::window_info(&state, window.label())
}

#[tauri::command]
pub fn pet_hit_regions(window: WebviewWindow, state: State<'_, Shared>, rects: Vec<HitRect>) {
    pet_rt::set_hit_regions(&state, window.label(), rects);
}

#[tauri::command]
pub fn pet_drag_start(window: WebviewWindow, state: State<'_, Shared>) {
    pet_rt::drag_start(&state, window.label());
}

#[tauri::command]
pub fn pet_drag_end(window: WebviewWindow, state: State<'_, Shared>) {
    pet_rt::drag_end(&state, window.label());
}

#[tauri::command]
pub fn pet_clicked(window: WebviewWindow, state: State<'_, Shared>) {
    if window.label() == "pet" {
        pet_rt::poke(&state);
    }
    open_panel_window(&state.app, "overview");
}

#[tauri::command]
pub fn pet_recall(state: State<'_, Shared>) {
    pet_rt::recall(&state);
}

#[tauri::command]
pub fn pet_go_visit(state: State<'_, Shared>, side: Side) {
    pet_rt::go_visit(&state, side);
}

#[tauri::command]
pub fn pet_send_guest_home(state: State<'_, Shared>, owner: String) {
    pet_rt::send_guest_home(&state, &owner);
}

#[tauri::command]
pub fn bubble_action(bubble_id: String, action: String) {
    calendar_rt::bubble_action(&bubble_id, &action);
}

#[tauri::command]
pub fn open_panel(app: AppHandle, tab: Option<String>) {
    open_panel_window(&app, tab.as_deref().unwrap_or("overview"));
}

pub fn open_panel_window(app: &AppHandle, tab: &str) {
    if let Some(w) = app.get_webview_window("panel") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        let _ = w.emit("steppy://navigate", tab.to_string());
    }
}

// ─────────────── 开机自启 ───────────────

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enable: bool) -> R<bool> {
    let al = app.autolaunch();
    if enable {
        al.enable().map_err(err)?;
    } else {
        al.disable().map_err(err)?;
    }
    crate::tray::sync_autostart(&app);
    Ok(al.is_enabled().unwrap_or(enable))
}

#[tauri::command]
pub fn list_holidays() -> Vec<crate::holiday_rt::HolidayInfo> {
    crate::holiday_rt::list()
}
