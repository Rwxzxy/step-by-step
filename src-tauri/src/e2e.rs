//! 端到端测试用的自动化接口：只有设置了环境变量 `STEPPY_E2E=1` 时才启用，
//! 仍然只监听 127.0.0.1 且需要 endpoint.json 里的 token。正常使用时完全不存在。
//!
//! * `GET  /v1/debug/snapshot`：面板快照 + 宠物交接状态。
//! * `POST /v1/debug/call`：`{"cmd": "...", "args": {...}}`，调用和界面按钮相同的函数。

use serde_json::{json, Value};

use crate::state::Shared;
use crate::{calendar_rt, commands, net_rt, pet_rt};

pub fn enabled() -> bool {
    std::env::var("STEPPY_E2E")
        .map(|v| v == "1")
        .unwrap_or(false)
}

pub fn handle(state: &Shared, method: &str, url: &str, body: &str) -> Option<(u16, String)> {
    if !enabled() || !url.starts_with("/v1/debug/") {
        return None;
    }
    let result: Result<Value, String> = match (method, url) {
        ("GET", "/v1/debug/snapshot") => Ok(snapshot(state)),
        ("POST", "/v1/debug/call") => {
            let v: Value = serde_json::from_str(body)
                .map_err(|e| e.to_string())
                .unwrap_or(Value::Null);
            call(
                state,
                v.get("cmd").and_then(|c| c.as_str()).unwrap_or(""),
                v.get("args").cloned().unwrap_or(Value::Null),
            )
        }
        _ => Err("unknown".into()),
    };
    Some(match result {
        Ok(v) => (200, v.to_string()),
        Err(e) => (400, json!({ "error": e }).to_string()),
    })
}

fn snapshot(state: &Shared) -> Value {
    let snap = commands::build_snapshot(state);
    let (owner, guests) = {
        let p = state.pet.lock().unwrap();
        let guests: Vec<String> = p
            .guests
            .iter()
            .filter(|(_, g)| g.is_visible())
            .map(|(o, _)| o.clone())
            .collect();
        (
            serde_json::to_value(&p.owner.state).unwrap_or(Value::Null),
            guests,
        )
    };
    let frames: Value = {
        let p = state.pet.lock().unwrap();
        p.frames
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap_or(Value::Null)))
            .collect::<serde_json::Map<_, _>>()
            .into()
    };
    json!({
        "snapshot": snap,
        "owner_state": owner,
        "visible_guests": guests,
        "frames": frames,
        "calendar": calendar_rt::all_records(state),
        "peers": state.net.connected_peers(),
        "peer_info": state.peers.lock().unwrap().iter().map(|(k, v)| (k.clone(), serde_json::to_value(&v.info).unwrap_or(Value::Null))).collect::<serde_json::Map<_, _>>(),
    })
}

fn s(args: &Value, k: &str) -> String {
    args.get(k)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

fn call(state: &Shared, cmd: &str, args: Value) -> Result<Value, String> {
    let e = |e: anyhow::Error| e.to_string();
    match cmd {
        "create_group" => net_rt::create_group(state).map(|_| Value::Null).map_err(e),
        "start_pairing" => net_rt::start_pairing(state).map(Value::String).map_err(e),
        "join_group" => {
            let code = s(&args, "code");
            tauri::async_runtime::block_on(net_rt::join_group(state.clone(), code))
                .map(Value::String)
                .map_err(e)
        }
        "set_layout" => {
            let order: Vec<String> =
                serde_json::from_value(args.get("order").cloned().unwrap_or_default())
                    .map_err(|e| e.to_string())?;
            net_rt::set_layout(state, order)
                .map(|_| Value::Null)
                .map_err(e)
        }
        "kick" => net_rt::kick(state, &s(&args, "node"))
            .map(|_| Value::Null)
            .map_err(e),
        "save_event" => {
            let input: commands::EventInput =
                serde_json::from_value(args).map_err(|e| e.to_string())?;
            let data = steppy_core::calendar::EventData {
                title: input.title,
                notes: input.notes,
                all_day: input.all_day,
                start: calendar_rt::parse_local(&input.start).map_err(e)?,
                end: calendar_rt::parse_local(&input.end).map_err(e)?,
                repeat: input.repeat,
                remind_before_min: input.remind_before_min,
            };
            calendar_rt::save(state, None, data)
                .map(Value::String)
                .map_err(e)
        }
        "go_visit" => {
            let side = if s(&args, "side") == "left" {
                steppy_core::pet::Side::Left
            } else {
                steppy_core::pet::Side::Right
            };
            pet_rt::go_visit(state, side);
            Ok(Value::Null)
        }
        "open_panel" => {
            commands::open_panel_window(&state.app, &s(&args, "tab"));
            Ok(Value::Null)
        }
        "recall" => {
            pet_rt::recall(state);
            Ok(Value::Null)
        }
        "patch_settings" => {
            let mut cur = serde_json::to_value(state.settings()).map_err(|e| e.to_string())?;
            if let (Some(obj), Some(patch)) = (cur.as_object_mut(), args.as_object()) {
                for (k, v) in patch {
                    obj.insert(k.clone(), v.clone());
                }
            }
            let mut new: crate::settings::Settings =
                serde_json::from_value(cur).map_err(|e| e.to_string())?;
            new.sanitize();
            *state.settings.write().unwrap() = new.clone();
            new.save();
            net_rt::update_my_info(state);
            state.mark_dirty();
            Ok(Value::Null)
        }
        _ => Err(format!("unknown cmd {cmd}")),
    }
}
