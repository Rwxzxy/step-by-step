//! 托盘菜单：显示 / 隐藏、打开面板、设置、召回宠物、开机自启、退出。

use std::sync::Mutex;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

use crate::commands::open_panel_window;
use crate::state::Shared;

static TOGGLE: Mutex<Option<MenuItem<Wry>>> = Mutex::new(None);
static AUTOSTART: Mutex<Option<CheckMenuItem<Wry>>> = Mutex::new(None);
static BUILD: Mutex<Option<CheckMenuItem<Wry>>> = Mutex::new(None);

pub fn create(app: &App, state: Shared) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", toggle_label(&state), true, None::<&str>)?;
    let panel = MenuItem::with_id(app, "panel", "打开面板", true, None::<&str>)?;
    let calendar = MenuItem::with_id(app, "calendar", "日程表", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?;
    let recall = MenuItem::with_id(app, "recall", "召回宠物", true, None::<&str>)?;
    let build = CheckMenuItem::with_id(app, "build", "建造模式", true, false, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "开机自启",
        true,
        app.autolaunch().is_enabled().unwrap_or(false),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &toggle, &panel, &calendar, &settings, &sep1, &build, &recall, &autostart, &sep2, &quit,
        ],
    )?;
    *TOGGLE.lock().unwrap() = Some(toggle);
    *AUTOSTART.lock().unwrap() = Some(autostart);
    *BUILD.lock().unwrap() = Some(build);

    let icon = app.default_window_icon().cloned();
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("步步 Steppy")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "toggle" => {
                let st = app.state::<Shared>().inner().clone();
                let visible = {
                    let mut s = st.settings.write().unwrap();
                    s.pet_visible = !s.pet_visible;
                    s.save();
                    s.pet_visible
                };
                if let Some(t) = TOGGLE.lock().unwrap().as_ref() {
                    let _ = t.set_text(if visible {
                        "隐藏宠物"
                    } else {
                        "显示宠物"
                    });
                }
                st.mark_dirty();
            }
            "panel" => open_panel_window(app, "overview"),
            "calendar" => open_panel_window(app, "calendar"),
            "settings" => open_panel_window(app, "settings"),
            "recall" => {
                let st = app.state::<Shared>().inner().clone();
                crate::pet_rt::recall(&st);
            }
            "autostart" => {
                let al = app.autolaunch();
                let enabled = al.is_enabled().unwrap_or(false);
                let _ = if enabled { al.disable() } else { al.enable() };
                sync_autostart(app);
            }
            "build" => {
                let on = !crate::world_rt::build_mode();
                crate::world_rt::set_build_mode(on);
                sync_build(on);
                let _ = tauri::Emitter::emit(app, "world://build", on);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                open_panel_window(tray.app_handle(), "overview");
            }
        });
    if let Some(icon) = icon {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

fn toggle_label(state: &Shared) -> &'static str {
    if state.settings().pet_visible {
        "隐藏宠物"
    } else {
        "显示宠物"
    }
}

pub fn sync_autostart(app: &AppHandle) {
    let enabled = app.autolaunch().is_enabled().unwrap_or(false);
    if let Some(item) = AUTOSTART.lock().unwrap().as_ref() {
        let _ = item.set_checked(enabled);
    }
}

pub fn sync_toggle(state: &Shared) {
    if let Some(t) = TOGGLE.lock().unwrap().as_ref() {
        let _ = t.set_text(toggle_label(state));
    }
}

pub fn sync_build(on: bool) {
    if let Some(item) = BUILD.lock().unwrap().as_ref() {
        let _ = item.set_checked(on);
    }
}
