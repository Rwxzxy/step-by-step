//! 提醒：宠物头上冒气泡，可选同时发系统通知；免打扰时段内都不打扰。

use serde::Serialize;
use steppy_core::pet::Anim;
use steppy_core::quiet::is_quiet;
use tauri::Emitter;
use tauri_plugin_notification::NotificationExt;

use crate::pet_rt;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct BubbleAction {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Bubble {
    pub id: String,
    pub text: String,
    /// done / waiting / error / calendar / info
    pub kind: String,
    pub actions: Vec<BubbleAction>,
    pub ttl_ms: u64,
}

pub fn quiet_now(state: &AppState) -> bool {
    let s = state.settings();
    is_quiet(&s.quiet_periods, chrono::Local::now().time())
}

/// 发出一条提醒。`anim` 是宠物配合做的动作，`hold_ms` 是动作持续时间。
pub fn alert(
    state: &AppState,
    kind: &str,
    title: &str,
    text: &str,
    anim: Option<Anim>,
    hold_ms: i64,
    actions: Vec<BubbleAction>,
) -> Option<String> {
    let settings = state.settings();
    if quiet_now(state) {
        log::info!("免打扰中，跳过提醒：{text}");
        return None;
    }
    if let Some(a) = anim {
        pet_rt::react(state, a, hold_ms);
    }
    let id = uuid::Uuid::new_v4().to_string();
    let pet_home = pet_rt::pet_is_home(state) && settings.pet_visible;
    if pet_home {
        let ttl = if actions.is_empty() {
            settings.bubble_seconds as u64 * 1000
        } else {
            (settings.bubble_seconds as u64 * 1000).max(30_000)
        };
        let b = Bubble {
            id: id.clone(),
            text: text.to_string(),
            kind: kind.into(),
            actions,
            ttl_ms: ttl,
        };
        if let Err(e) = state.app.emit_to("pet", "pet://bubble", b) {
            log::warn!("发送气泡失败：{e}");
        }
    }
    // 宠物不在家（去串门了 / 被隐藏）时没有气泡可冒，用系统通知代替。
    if settings.system_notifications || !pet_home {
        if let Err(e) = state
            .app
            .notification()
            .builder()
            .title(title)
            .body(text)
            .show()
        {
            log::warn!("系统通知失败：{e}");
        }
    }
    Some(id)
}

/// 普通信息气泡（不受通知设置影响、不发系统通知）。
pub fn info(state: &AppState, text: &str) {
    if quiet_now(state) || !pet_rt::pet_is_home(state) {
        return;
    }
    let b = Bubble {
        id: uuid::Uuid::new_v4().to_string(),
        text: text.into(),
        kind: "info".into(),
        actions: vec![],
        ttl_ms: 5_000,
    };
    let _ = state.app.emit_to("pet", "pet://bubble", b);
}
