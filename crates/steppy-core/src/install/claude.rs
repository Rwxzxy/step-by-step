//! 合并 / 撤销 `~/.claude/settings.json` 里的 hooks。
//!
//! 结构（见 hooks 文档 Configuration 一节）：
//! `{ "hooks": { "<Event>": [ { "matcher"?: "...", "hooks": [ { "type": "command", "command": "...", "args": [...], "timeout": n } ] } ] } }`
//!
//! 我们用 exec 形式（`command` + `args`），这样路径里有空格、Windows 上没有 Git Bash 时也不需要处理 shell 引号。

use serde_json::{json, Map, Value};

use super::{is_our_program, HookCommand, InstallError};

/// 需要接入的事件及 hook 超时（秒）。`None` 表示用 Claude Code 的默认值。
///
/// * `SessionStart`、`SessionEnd`：会话开始 / 结束。SessionEnd 共享 1.5 秒预算，不设 timeout 以免拉长退出时间。
/// * `UserPromptSubmit`：用户发了新消息。
/// * `PostToolUse`：工具执行完（包括 TodoWrite / TaskCreate / TaskUpdate 计划变化，以及批准后恢复运行）。
/// * `TaskCreated`、`TaskCompleted`：任务工具的任务创建 / 完成。
/// * `PermissionRequest`、`Notification`：等待批准 / 输入。
/// * `Stop`、`StopFailure`：一轮结束 / 因 API 错误结束。
pub const EVENTS: [(&str, Option<u64>); 10] = [
    ("SessionStart", Some(5)),
    ("UserPromptSubmit", Some(5)),
    ("PostToolUse", Some(5)),
    ("TaskCreated", Some(5)),
    ("TaskCompleted", Some(5)),
    ("PermissionRequest", Some(5)),
    ("Notification", Some(5)),
    ("Stop", Some(5)),
    ("StopFailure", Some(5)),
    ("SessionEnd", None),
];

fn handler(cmd: &HookCommand, timeout: Option<u64>) -> Value {
    let mut h = json!({
        "type": "command",
        "command": cmd.program,
        "args": ["claude", "--endpoint", cmd.endpoint_file],
    });
    if let Some(t) = timeout {
        h["timeout"] = json!(t);
    }
    h
}

fn handler_is_ours(h: &Value) -> bool {
    h.get("command")
        .and_then(|c| c.as_str())
        .map(is_our_program)
        .unwrap_or(false)
        || h.get("command")
            .and_then(|c| c.as_str())
            .map(|c| c.contains("steppy-hook"))
            .unwrap_or(false)
}

fn parse(text: &str) -> Result<Value, InstallError> {
    if text.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    let v: Value = serde_json::from_str(text).map_err(|e| InstallError::BadJson(e.to_string()))?;
    if !v.is_object() {
        return Err(InstallError::Shape("settings.json 顶层不是对象".into()));
    }
    Ok(v)
}

fn render(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(v).expect("serialize settings");
    s.push('\n');
    s
}

/// 从 settings 里删掉我们的 hook，返回删掉的数量。用户自己的 hook 原样保留。
fn strip_ours(v: &mut Value) -> Result<usize, InstallError> {
    let Some(hooks) = v.get_mut("hooks") else {
        return Ok(0);
    };
    let Some(events) = hooks.as_object_mut() else {
        return Err(InstallError::Shape("\"hooks\" 不是对象".into()));
    };
    let mut removed = 0;
    let names: Vec<String> = events.keys().cloned().collect();
    for name in names {
        let Some(groups) = events.get_mut(&name).and_then(|g| g.as_array_mut()) else {
            continue;
        };
        for group in groups.iter_mut() {
            if let Some(list) = group.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                let before = list.len();
                list.retain(|h| !handler_is_ours(h));
                removed += before - list.len();
            }
        }
        // 删除因此变空的 matcher 组（用户本来就空的组也保留原样，只删我们清空的）。
        groups.retain(|g| {
            g.get("hooks")
                .and_then(|h| h.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(true)
        });
        if groups.is_empty() {
            events.remove(&name);
        }
    }
    if events.is_empty() {
        v.as_object_mut().unwrap().remove("hooks");
    }
    Ok(removed)
}

/// 接入：先移除旧的步步 hook（比如路径变了），再给每个事件追加一个新的 matcher 组。
pub fn install(text: &str, cmd: &HookCommand) -> Result<String, InstallError> {
    let mut v = parse(text)?;
    strip_ours(&mut v)?;
    let obj = v.as_object_mut().unwrap();
    let hooks = obj
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let events = hooks
        .as_object_mut()
        .ok_or_else(|| InstallError::Shape("\"hooks\" 不是对象".into()))?;
    for (event, timeout) in EVENTS {
        let groups = events
            .entry(event.to_string())
            .or_insert_with(|| Value::Array(vec![]));
        let arr = groups
            .as_array_mut()
            .ok_or_else(|| InstallError::Shape(format!("hooks.{event} 不是数组")))?;
        arr.push(json!({ "hooks": [handler(cmd, timeout)] }));
    }
    Ok(render(&v))
}

/// 撤销：只删除步步自己的条目。没有我们的条目时原文不变。
pub fn uninstall(text: &str) -> Result<String, InstallError> {
    let mut v = parse(text)?;
    let removed = strip_ours(&mut v)?;
    if removed == 0 {
        return Ok(text.to_string());
    }
    Ok(render(&v))
}

/// 是否已接入（所有事件都有我们的 hook）。
pub fn is_installed(text: &str) -> bool {
    let Ok(v) = parse(text) else { return false };
    EVENTS.iter().all(|(event, _)| {
        v.get("hooks")
            .and_then(|h| h.get(*event))
            .and_then(|g| g.as_array())
            .map(|groups| {
                groups.iter().any(|g| {
                    g.get("hooks")
                        .and_then(|h| h.as_array())
                        .map(|a| a.iter().any(handler_is_ours))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd() -> HookCommand {
        HookCommand {
            program: r"C:\Users\Me Too\AppData\Local\Steppy\bin\steppy-hook.exe".into(),
            endpoint_file: r"C:\Users\Me Too\AppData\Local\Steppy\endpoint.json".into(),
        }
    }

    const USER_SETTINGS: &str = r#"{
  "model": "opus",
  "permissions": { "allow": ["Bash(npm test)"] },
  "hooks": {
    "PreToolUse": [
      { "matcher": "Bash", "hooks": [ { "type": "command", "command": "/home/me/check.sh" } ] }
    ],
    "Stop": [
      { "hooks": [ { "type": "command", "command": "say done", "timeout": 3 } ] }
    ],
    "Notification": [
      { "matcher": "idle_prompt", "hooks": [ { "type": "command", "command": "notify-send idle" } ] }
    ]
  },
  "env": { "FOO": "1" }
}"#;

    #[test]
    fn install_into_empty_file() {
        let out = install("", &cmd()).unwrap();
        assert!(is_installed(&out));
        let v: Value = serde_json::from_str(&out).unwrap();
        let start = &v["hooks"]["SessionStart"][0]["hooks"][0];
        assert_eq!(start["type"], "command");
        assert_eq!(start["command"], cmd().program);
        assert_eq!(
            start["args"],
            json!(["claude", "--endpoint", cmd().endpoint_file])
        );
        assert_eq!(start["timeout"], 5);
        // SessionEnd 不设 timeout。
        assert!(v["hooks"]["SessionEnd"][0]["hooks"][0]
            .get("timeout")
            .is_none());
        // 不带 matcher：匹配所有。
        assert!(v["hooks"]["PostToolUse"][0].get("matcher").is_none());
    }

    #[test]
    fn install_preserves_user_hooks_and_settings() {
        let out = install(USER_SETTINGS, &cmd()).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["model"], "opus");
        assert_eq!(v["env"]["FOO"], "1");
        assert_eq!(v["permissions"]["allow"][0], "Bash(npm test)");
        // 用户的 PreToolUse 不受影响。
        assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        // Stop：用户的组在前，我们的追加在后。
        let stop = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["hooks"][0]["command"], "say done");
        assert!(is_our_program(
            stop[1]["hooks"][0]["command"].as_str().unwrap()
        ));
        let notif = v["hooks"]["Notification"].as_array().unwrap();
        assert_eq!(notif[0]["matcher"], "idle_prompt");
        assert_eq!(notif.len(), 2);
        // 原有键顺序保留。
        let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, vec!["model", "permissions", "hooks", "env"]);
    }

    #[test]
    fn install_is_idempotent_and_updates_path() {
        let once = install(USER_SETTINGS, &cmd()).unwrap();
        let twice = install(&once, &cmd()).unwrap();
        assert_eq!(once, twice);
        let mut other = cmd();
        other.program = "/opt/steppy/steppy-hook".into();
        let moved = install(&once, &other).unwrap();
        assert!(!moved.contains("Me Too\\\\AppData\\\\Local\\\\Steppy\\\\bin"));
        assert_eq!(
            moved.matches("/opt/steppy/steppy-hook").count(),
            EVENTS.len()
        );
    }

    #[test]
    fn uninstall_restores_user_config_exactly() {
        let installed = install(USER_SETTINGS, &cmd()).unwrap();
        let removed = uninstall(&installed).unwrap();
        let a: Value = serde_json::from_str(USER_SETTINGS).unwrap();
        let b: Value = serde_json::from_str(&removed).unwrap();
        assert_eq!(a, b);
        assert!(!is_installed(&removed));
    }

    #[test]
    fn uninstall_removes_hooks_key_when_only_ours() {
        let installed = install("{\n  \"theme\": \"dark\"\n}", &cmd()).unwrap();
        let removed = uninstall(&installed).unwrap();
        let v: Value = serde_json::from_str(&removed).unwrap();
        assert_eq!(v, json!({"theme": "dark"}));
    }

    #[test]
    fn uninstall_keeps_mixed_group() {
        // 用户把自己的 hook 和我们的放在同一个组里：只删我们的那一个。
        let text = json!({"hooks": {"Stop": [{"hooks": [
            {"type": "command", "command": "say hi"},
            {"type": "command", "command": "/x/steppy-hook", "args": ["claude"]}
        ]}]}})
        .to_string();
        let out = uninstall(&text).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v,
            json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "say hi"}]}]}})
        );
    }

    #[test]
    fn uninstall_without_our_hooks_is_noop() {
        assert_eq!(uninstall(USER_SETTINGS).unwrap(), USER_SETTINGS);
        assert_eq!(uninstall("").unwrap(), "");
    }

    #[test]
    fn refuses_to_touch_invalid_json() {
        assert!(matches!(
            install("{ not json", &cmd()),
            Err(InstallError::BadJson(_))
        ));
        assert!(matches!(
            install("[1,2]", &cmd()),
            Err(InstallError::Shape(_))
        ));
        assert!(matches!(
            install(r#"{"hooks": []}"#, &cmd()),
            Err(InstallError::Shape(_))
        ));
    }
}
