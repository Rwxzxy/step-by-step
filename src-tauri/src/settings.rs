//! 用户设置。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use steppy_core::quiet::QuietPeriod;

use crate::storage;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 这台机器的名字（显示给家庭组里的其他机器）。
    pub machine_name: String,
    /// 主人名字（宠物去别人屏幕上串门时显示）。
    pub owner_name: String,
    /// 宠物主色调，色相 0-360。
    pub pet_hue: u16,
    /// 是否显示宠物。
    pub pet_visible: bool,
    /// 同时发系统通知。
    pub system_notifications: bool,
    /// 气泡显示多少秒。
    pub bubble_seconds: u32,
    /// 免打扰时段。
    pub quiet_periods: Vec<QuietPeriod>,
    /// 其他机器上的 Agent 完成 / 等待时也提醒。
    pub remind_remote_agents: bool,
    /// 在这台机器上提醒日程。
    pub calendar_reminders: bool,
    /// 允许宠物串门（出门和接待）。
    pub allow_visits: bool,
    /// 宠物会自己溜达到邻居机器去。
    pub wander_visits: bool,
    /// 来访的宠物待多久后回家（分钟）。
    pub visit_minutes: u32,
    /// 是否把消息正文 / 提示词同步给某台机器（按 node id）。默认都不发。
    pub share_content_with: BTreeMap<String, bool>,
    /// 手动指定的其他机器地址（mDNS 不可用时），形如 `192.168.1.20:47900`。
    pub manual_peers: Vec<String>,
    /// 局域网监听端口。
    pub net_port: u16,
    /// 首次运行引导是否完成。
    pub onboarded: bool,
}

fn default_machine_name() -> String {
    gethostname::gethostname().to_string_lossy().to_string()
}

fn default_owner_name() -> String {
    let real = whoami::realname();
    if real.trim().is_empty() {
        whoami::username()
    } else {
        real
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            machine_name: default_machine_name(),
            owner_name: default_owner_name(),
            pet_hue: 32,
            pet_visible: true,
            system_notifications: false,
            bubble_seconds: 8,
            quiet_periods: vec![QuietPeriod {
                start: "22:30".into(),
                end: "08:00".into(),
                enabled: false,
            }],
            remind_remote_agents: true,
            calendar_reminders: true,
            allow_visits: true,
            wander_visits: false,
            visit_minutes: 10,
            share_content_with: BTreeMap::new(),
            manual_peers: Vec::new(),
            net_port: 47900,
            onboarded: false,
        }
    }
}

impl Settings {
    pub fn load() -> Settings {
        let mut s: Settings =
            storage::read_json(&storage::path("settings.json")).unwrap_or_default();
        s.sanitize();
        s
    }

    pub fn save(&self) {
        if let Err(e) = storage::write_json(&storage::path("settings.json"), self, false) {
            log::error!("保存设置失败：{e}");
        }
    }

    pub fn sanitize(&mut self) {
        if self.machine_name.trim().is_empty() {
            self.machine_name = default_machine_name();
        }
        if self.owner_name.trim().is_empty() {
            self.owner_name = default_owner_name();
        }
        self.machine_name = self.machine_name.trim().chars().take(40).collect();
        self.owner_name = self.owner_name.trim().chars().take(40).collect();
        self.pet_hue %= 360;
        self.bubble_seconds = self.bubble_seconds.clamp(2, 120);
        self.visit_minutes = self.visit_minutes.clamp(1, 240);
        if self.net_port < 1024 {
            self.net_port = 47900;
        }
        self.quiet_periods.retain(|p| p.is_valid());
    }

    pub fn shares_content_with(&self, node: &str) -> bool {
        self.share_content_with.get(node).copied().unwrap_or(false)
    }
}
