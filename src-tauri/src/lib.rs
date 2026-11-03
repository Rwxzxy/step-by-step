//! 步步 Steppy 桌面应用。

mod agents_rt;
mod calendar_rt;
mod commands;
mod e2e;
mod holiday_rt;
mod integrate;
mod local_api;
mod net_rt;
mod notify;
mod pet_rt;
mod settings;
mod state;
mod storage;
mod tray;
mod world_rt;

use std::time::Duration;

use tauri::{Manager, RunEvent, WindowEvent};

use crate::state::{AppState, Shared};

/// 把“界面需要刷新”合并成每 250 毫秒最多一次的事件。
fn run_ui_notifier(state: Shared) {
    std::thread::Builder::new()
        .name("ui-notifier".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_millis(250));
            if state.take_dirty() {
                state.emit("steppy://changed", ());
                tray::sync_toggle(&state);
            }
        })
        .expect("spawn ui notifier");
}

pub fn run() {
    // Linux：Wayland 不允许应用自己摆放窗口位置、也拿不到全局光标位置，宠物没法走动。
    // 默认改用 XWayland（X11 后端）。用户显式设置了 GDK_BACKEND 时尊重用户。
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("GDK_BACKEND").is_none()
            && std::env::var_os("WAYLAND_DISPLAY").is_some()
        {
            std::env::set_var("GDK_BACKEND", "x11");
        }
    }

    let (agent_tx, agent_rx) = std::sync::mpsc::channel();

    let mut builder = tauri::Builder::default();
    // 端到端测试时要在同一台机器上跑两个实例，跳过单实例限制。
    if !e2e::enabled() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            commands::open_panel_window(app, "overview");
        }));
    }
    let app = builder
        .plugin(
            tauri_plugin_log::Builder::new()
                .clear_targets()
                .level(
                    std::env::var("STEPPY_LOG")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(log::LevelFilter::Info),
                )
                .level_for("mdns_sd", log::LevelFilter::Warn)
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("steppy".into()),
                    },
                ))
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ))
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let state = AppState::new(app.handle().clone(), net_rt::NetHandle::new());
            app.manage(state.clone());
            integrate::refresh_on_startup();
            if let Err(e) = local_api::start(agent_tx.clone(), state.clone()) {
                log::error!("hook 接口启动失败：{e}");
            }
            agents_rt::start(state.clone(), agent_rx);
            calendar_rt::start_reminders(state.clone());
            holiday_rt::start(state.clone());
            net_rt::start(state.clone());
            pet_rt::start(state.clone());
            // 没有系统托盘的环境（比如某些 Linux 桌面）：不影响其他功能。
            if let Err(e) = tray::create(app, state.clone()) {
                log::warn!("创建托盘图标失败：{e}");
            }
            run_ui_notifier(state.clone());
            let first_run = !state.settings().onboarded && !e2e::enabled();
            if first_run {
                commands::open_panel_window(app.handle(), "welcome");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // 宠物窗口不能被关掉（比如 Alt+F4）；面板关闭时直接销毁以释放内存，程序继续在托盘里运行。
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "pet" || window.label().starts_with("strip-") {
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::get_settings,
            commands::save_settings,
            commands::integration_status,
            commands::set_integration,
            commands::list_agenda,
            commands::save_event,
            commands::delete_event,
            commands::get_event,
            commands::import_ics,
            commands::export_ics,
            commands::create_group,
            commands::start_pairing,
            commands::cancel_pairing,
            commands::join_group,
            commands::kick_member,
            commands::leave_group,
            commands::set_layout,
            commands::set_share_content,
            commands::pet_window_info,
            commands::pet_hit_regions,
            commands::pet_drag_start,
            commands::pet_drag_end,
            commands::pet_clicked,
            commands::pet_recall,
            commands::pet_go_visit,
            commands::pet_send_guest_home,
            commands::bubble_action,
            commands::open_panel,
            commands::get_autostart,
            commands::set_autostart,
            commands::list_holidays,
            commands::world_view,
            commands::world_set,
            commands::world_clear,
            commands::get_build_mode,
            commands::set_build_mode,
            commands::import_texture_pack,
            commands::clear_texture_pack,
            commands::get_textures,
        ])
        .build(tauri::generate_context!())
        .expect("启动步步失败");

    app.run(|_app, event| match event {
        // 所有窗口都隐藏时不退出；只有托盘里的“退出”（带退出码）才真正退出。
        RunEvent::ExitRequested { code, api, .. } => {
            if code.is_none() {
                api.prevent_exit();
            }
        }
        RunEvent::Exit => local_api::remove_endpoint(),
        _ => {}
    });
}
