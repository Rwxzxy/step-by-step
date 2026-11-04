//! Agent 进度：处理 Claude hook、Codex notify，增量读取 Codex rollout，向其他机器同步。

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime};

use chrono::{Datelike, Duration as ChronoDuration};
use steppy_core::agents::claude::{
    apply_hook, new_session, parse_hook, plan_from_transcript, wants_transcript,
};
use steppy_core::agents::codex::{
    apply_notify, parse_notify, thread_id_from_filename, CodexTracker,
};
use steppy_core::agents::{AgentKind, AgentSession, Cue, SessionStatus};
use steppy_core::net::wire::WireMsg;
use steppy_core::now_ms;
use steppy_core::pet::Anim;

use crate::integrate::codex_home;
use crate::notify;
use crate::pet_rt;
use crate::state::Shared;

pub enum AgentInput {
    Claude(String),
    CodexNotify(String),
}

/// 已结束的会话保留多久。
const ENDED_KEEP_MS: i64 = 30 * 60_000;
/// 不活跃的会话保留多久。
const STALE_KEEP_MS: i64 = 12 * 3_600_000;
/// Codex：第一次发现时超过这个时间没修改的文件按“回放”处理，不触发提醒。
const CODEX_LIVE_WINDOW: Duration = Duration::from_secs(90);
/// Codex：只跟踪最近修改过的 rollout。
const CODEX_RECENT: Duration = Duration::from_secs(12 * 3600);

pub fn start(state: Shared, rx: Receiver<AgentInput>) {
    let s = state.clone();
    std::thread::Builder::new()
        .name("agents".into())
        .spawn(move || worker(s, rx))
        .expect("spawn agents worker");
    std::thread::Builder::new()
        .name("codex-watch".into())
        .spawn(move || codex_watcher(state))
        .expect("spawn codex watcher");
}

fn worker(state: Shared, rx: Receiver<AgentInput>) {
    let mut last_prune = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(AgentInput::Claude(raw)) => handle_claude(&state, &raw),
            Ok(AgentInput::CodexNotify(raw)) => handle_codex_notify(&state, &raw),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if last_prune.elapsed() > Duration::from_secs(60) {
            last_prune = Instant::now();
            prune(&state);
        }
    }
}

fn expand_home(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        if let Some(h) = dirs::home_dir() {
            return h.join(rest);
        }
    }
    PathBuf::from(p)
}

fn handle_claude(state: &Shared, raw: &str) {
    let hook = match parse_hook(raw) {
        Ok(h) => h,
        Err(e) => {
            log::warn!("无法解析 Claude hook：{e}");
            return;
        }
    };
    let now = now_ms();
    let key = format!("claude:{}", hook.session_id);
    let (cue, need_transcript) = {
        let mut st = state.agents.lock().unwrap();
        let s = st
            .local
            .entry(key.clone())
            .or_insert_with(|| new_session(&hook, now));
        let cue = apply_hook(s, &hook, now);
        (cue, wants_transcript(&hook, s))
    };
    if need_transcript {
        if let Some(p) = &hook.transcript_path {
            let path = expand_home(p);
            if let Ok(f) = File::open(&path) {
                if let Some(plan) = plan_from_transcript(BufReader::new(f)) {
                    let mut st = state.agents.lock().unwrap();
                    if let Some(s) = st.local.get_mut(&key) {
                        s.plan = plan;
                    }
                }
            }
        }
    }
    let session = state.agents.lock().unwrap().local.get(&key).cloned();
    if let Some(s) = session {
        on_local_change(state, &s, cue);
    }
}

fn handle_codex_notify(state: &Shared, raw: &str) {
    let Some(n) = parse_notify(raw) else {
        log::warn!("无法解析 Codex notify");
        return;
    };
    let now = now_ms();
    let key = format!("codex:{}", n.thread_id);
    let (cue, session) = {
        let mut st = state.agents.lock().unwrap();
        let s = st.local.entry(key.clone()).or_insert_with(|| {
            AgentSession::new(AgentKind::Codex, n.thread_id.clone(), n.cwd.clone(), now)
        });
        let cue = apply_notify(s, &n, now);
        let snapshot = s.clone();
        // 同步到 tracker，避免随后读到 rollout 时重复提醒。
        for t in st.codex.values_mut() {
            if t.session.session_id == n.thread_id {
                t.session.status = snapshot.status;
            }
        }
        (cue, snapshot)
    };
    on_local_change(state, &session, cue);
}

/// 本机会话变化：同步给其他机器、刷新界面、宠物做反应。
pub fn on_local_change(state: &Shared, s: &AgentSession, cue: Option<Cue>) {
    broadcast_session(state, s);
    state.mark_dirty();
    if let Some(c) = cue {
        react(state, s, c, None);
    }
    if s.status != SessionStatus::Waiting {
        maybe_lower_hand(state);
    }
}

/// 向每台在线机器发送会话（按设置决定是否带消息正文）。
pub fn broadcast_session(state: &Shared, s: &AgentSession) {
    let settings = state.settings();
    for peer in state.net.connected_peers() {
        let session = if settings.shares_content_with(&peer) {
            s.clone()
        } else {
            s.redacted()
        };
        state.net.send(&peer, WireMsg::AgentUpsert { session });
    }
}

/// 发给某台机器的完整快照。
pub fn snapshot_for(state: &Shared, peer: &str) -> WireMsg {
    let share = state.settings().shares_content_with(peer);
    let st = state.agents.lock().unwrap();
    let sessions = st
        .local
        .values()
        .map(|s| if share { s.clone() } else { s.redacted() })
        .collect();
    WireMsg::AgentSnapshot { sessions }
}

fn maybe_lower_hand(state: &Shared) {
    let st = state.agents.lock().unwrap();
    let anyone_waiting = st
        .local
        .values()
        .any(|s| s.status == SessionStatus::Waiting)
        || st
            .remote
            .values()
            .flat_map(|m| m.values())
            .any(|s| s.status == SessionStatus::Waiting);
    drop(st);
    if !anyone_waiting {
        pet_rt::clear_reaction(state, Anim::RaiseHand);
    }
}

fn describe(s: &AgentSession, cue: Cue) -> (String, String) {
    let agent = s.agent.label();
    let project = s.project_name();
    let (done, total) = s.plan_progress();
    let progress = if total > 0 {
        format!("（计划 {done}/{total}）")
    } else {
        String::new()
    };
    match cue {
        Cue::Done => (
            format!("{agent} 完成了"),
            format!("{agent} 在「{project}」完成了这一轮{progress}"),
        ),
        Cue::Waiting => {
            let what = match s.detail.as_deref() {
                Some(d) if d.starts_with("等待批准：") => {
                    format!("等你批准 {}", d.trim_start_matches("等待批准："))
                }
                Some("等待输入") => "等你输入".to_string(),
                _ => "等你回复".to_string(),
            };
            (
                format!("{agent} 在等你"),
                format!("{agent} 在「{project}」{what}"),
            )
        }
        Cue::Error => {
            let d = s.detail.clone().unwrap_or_default();
            (
                format!("{agent} 出错了"),
                format!(
                    "{agent} 在「{project}」出错了{}",
                    if d.is_empty() {
                        String::new()
                    } else {
                        format!("：{d}")
                    }
                ),
            )
        }
    }
}

/// 宠物做出反应并冒气泡。`machine` 为 Some 表示是其他机器上的会话。
pub fn react(state: &Shared, s: &AgentSession, cue: Cue, machine: Option<String>) {
    let (title, mut text) = describe(s, cue);
    if let Some(m) = machine {
        text = format!("【{m}】{text}");
    }
    let (anim, hold) = match cue {
        Cue::Done => (Anim::Happy, 4_000),
        Cue::Waiting => (Anim::RaiseHand, 60_000),
        Cue::Error => (Anim::Anxious, 7_000),
    };
    let kind = match cue {
        Cue::Done => "done",
        Cue::Waiting => "waiting",
        Cue::Error => "error",
    };
    notify::alert(state, kind, &title, &text, Some(anim), hold, vec![]);
}

/// 收到其他机器的会话更新。
pub fn on_remote_upsert(state: &Shared, peer: &str, session: AgentSession) {
    let before = {
        let mut st = state.agents.lock().unwrap();
        let map = st.remote.entry(peer.to_string()).or_default();
        map.insert(session.key(), session.clone())
    };
    let cue = steppy_core::agents::cue_for_transition(
        before.map(|b| b.status).unwrap_or(SessionStatus::Idle),
        session.status,
    );
    if let Some(c) = cue {
        if state.settings().remind_remote_agents {
            react(state, &session, c, Some(state.machine_name(peer)));
        }
    }
    if session.status != SessionStatus::Waiting {
        maybe_lower_hand(state);
    }
    state.mark_dirty();
}

pub fn on_remote_snapshot(state: &Shared, peer: &str, sessions: Vec<AgentSession>) {
    let mut st = state.agents.lock().unwrap();
    st.remote.insert(
        peer.to_string(),
        sessions.into_iter().map(|s| (s.key(), s)).collect(),
    );
    drop(st);
    state.mark_dirty();
}

pub fn on_remote_remove(state: &Shared, peer: &str, key: &str) {
    let mut st = state.agents.lock().unwrap();
    if let Some(m) = st.remote.get_mut(peer) {
        m.remove(key);
    }
    drop(st);
    state.mark_dirty();
}

fn prune(state: &Shared) {
    let now = now_ms();
    let mut removed = Vec::new();
    {
        let mut st = state.agents.lock().unwrap();
        st.local.retain(|k, s| {
            let keep = !((s.status == SessionStatus::Ended && now - s.updated_ms > ENDED_KEEP_MS)
                || now - s.updated_ms > STALE_KEEP_MS);
            if !keep {
                removed.push(k.clone());
            }
            keep
        });
        for m in st.remote.values_mut() {
            m.retain(|_, s| now - s.updated_ms <= STALE_KEEP_MS);
        }
    }
    for key in &removed {
        for peer in state.net.connected_peers() {
            state
                .net
                .send(&peer, WireMsg::AgentRemove { key: key.clone() });
        }
    }
    if !removed.is_empty() {
        state.mark_dirty();
    }
}

// ─────────────────────────── Codex rollout ───────────────────────────

/// 需要扫描的日期目录：本地和 UTC 的今天、昨天（rollout 按创建日期分目录）。
fn candidate_dirs(sessions: &Path) -> Vec<PathBuf> {
    let mut dates = Vec::new();
    let local = chrono::Local::now().date_naive();
    let utc = chrono::Utc::now().date_naive();
    for d in [
        local,
        local - ChronoDuration::days(1),
        utc,
        utc - ChronoDuration::days(1),
    ] {
        if !dates.contains(&d) {
            dates.push(d);
        }
    }
    dates
        .into_iter()
        .map(|d| {
            sessions
                .join(format!("{:04}", d.year()))
                .join(format!("{:02}", d.month()))
                .join(format!("{:02}", d.day()))
        })
        .filter(|p| p.is_dir())
        .collect()
}

fn read_from(path: &Path, offset: u64, max: u64) -> std::io::Result<Vec<u8>> {
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::new();
    f.take(max).read_to_end(&mut buf)?;
    Ok(buf)
}

fn codex_watcher(state: Shared) {
    let mut first_scan = true;
    loop {
        let sessions_dir = codex_home().join("sessions");
        if sessions_dir.is_dir() {
            scan_codex(&state, &sessions_dir, first_scan);
            first_scan = false;
        }
        std::thread::sleep(Duration::from_millis(1500));
    }
}

fn scan_codex(state: &Shared, sessions_dir: &Path, first_scan: bool) {
    let now_sys = SystemTime::now();
    for dir in candidate_dirs(sessions_dir) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.starts_with("rollout-") || !name.ends_with(".jsonl") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let age = meta
                .modified()
                .ok()
                .and_then(|m| now_sys.duration_since(m).ok())
                .unwrap_or_default();
            let len = meta.len();
            let mut st = state.agents.lock().unwrap();
            let known = st.codex.contains_key(&path);
            if !known && age > CODEX_RECENT {
                continue;
            }
            let replay = !known && (first_scan || age > CODEX_LIVE_WINDOW);
            let tracker = st.codex.entry(path.clone()).or_insert_with(|| {
                CodexTracker::new(
                    thread_id_from_filename(name).unwrap_or_else(|| name.to_string()),
                    steppy_core::now_ms(),
                )
            });
            if len < tracker.offset {
                // 文件被截断或替换：从头读。
                *tracker = CodexTracker::new(
                    thread_id_from_filename(name).unwrap_or_else(|| name.to_string()),
                    steppy_core::now_ms(),
                );
            }
            if len == tracker.offset {
                continue;
            }
            let offset = tracker.offset;
            drop(st);
            let Ok(bytes) = read_from(&path, offset, 32 * 1024 * 1024) else {
                continue;
            };
            let mut st = state.agents.lock().unwrap();
            let Some(tracker) = st.codex.get_mut(&path) else {
                continue;
            };
            let mut cue = tracker.feed(&bytes, steppy_core::now_ms(), replay);
            if replay {
                cue = None;
            }
            let visible = tracker.has_meta && !tracker.hidden;
            let mut session = tracker.session.clone();
            if !visible {
                continue;
            }
            let key = session.key();
            // notify 可能已经把状态推进到“完成”，不要重复提醒。
            if let Some(existing) = st.local.get(&key) {
                if cue.is_some() && existing.status == session.status {
                    cue = None;
                }
                if session.last_prompt.is_none() {
                    session.last_prompt = existing.last_prompt.clone();
                }
            }
            st.local.insert(key, session.clone());
            drop(st);
            on_local_change(state, &session, cue);
        }
    }
}
