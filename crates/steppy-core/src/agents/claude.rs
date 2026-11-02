//! Claude Code hooks 的 payload 解析和会话状态归约。
//!
//! 字段名以 <https://code.claude.com/docs/en/hooks> 为准：
//!
//! * 公共字段：`session_id`、`transcript_path`、`cwd`、`permission_mode`、`hook_event_name`。
//! * `SessionStart`：`source`、可选 `model`、`session_title`。
//! * `UserPromptSubmit`：`prompt`、可选 `session_title`。
//! * `PostToolUse`：`tool_name`、`tool_input`、`tool_response`、`tool_use_id`。
//! * `PermissionRequest`：`tool_name`、`tool_input`。
//! * `Notification`：`message`、可选 `title`、`notification_type`。
//! * `TaskCreated` / `TaskCompleted`：`task_id`、`task_subject`、可选 `task_description`。
//! * `Stop`：`stop_hook_active`、`last_assistant_message`、`background_tasks`、`session_crons`。
//! * `StopFailure`：`error`、可选 `error_details`、`last_assistant_message`。
//! * `SessionEnd`：`reason`。
//!
//! 计划清单来自 `TodoWrite`（`tool_input.todos[]`），或者当前版本默认启用的任务工具
//! `TaskCreate` / `TaskUpdate`（以及 `TaskCreated` / `TaskCompleted` 事件）。

use std::io::BufRead;

use serde_json::Value;

use super::{cue_for_transition, excerpt, AgentKind, AgentSession, Cue, PlanItem, PlanStatus, SessionStatus, EXCERPT_CHARS};

/// 解析后的 hook 事件。
#[derive(Debug, Clone, PartialEq)]
pub enum ClaudeEvent {
    SessionStart {
        source: String,
        model: Option<String>,
        session_title: Option<String>,
    },
    UserPromptSubmit {
        prompt: String,
        session_title: Option<String>,
    },
    PostToolUse {
        tool_name: String,
        tool_input: Value,
        tool_response: Value,
    },
    PermissionRequest {
        tool_name: String,
    },
    Notification {
        notification_type: String,
        message: String,
        title: Option<String>,
    },
    TaskCreated {
        task_id: String,
        subject: String,
    },
    TaskCompleted {
        task_id: String,
        subject: String,
    },
    Stop {
        last_assistant_message: Option<String>,
        background_tasks: usize,
    },
    StopFailure {
        error: String,
        error_details: Option<String>,
    },
    SessionEnd {
        reason: String,
    },
    /// 其他我们不关心的事件。
    Other(String),
}

/// 一次 hook 调用。
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeHook {
    pub session_id: String,
    pub transcript_path: Option<String>,
    pub cwd: String,
    pub permission_mode: Option<String>,
    pub event: ClaudeEvent,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ParseError {
    #[error("不是合法的 JSON：{0}")]
    Json(String),
    #[error("缺少字段 {0}")]
    Missing(&'static str),
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|x| x.to_string())
}

/// 解析 hook 的 stdin JSON。
pub fn parse_hook(raw: &str) -> Result<ClaudeHook, ParseError> {
    let v: Value = serde_json::from_str(raw).map_err(|e| ParseError::Json(e.to_string()))?;
    parse_hook_value(&v)
}

pub fn parse_hook_value(v: &Value) -> Result<ClaudeHook, ParseError> {
    let session_id = s(v, "session_id").ok_or(ParseError::Missing("session_id"))?;
    let name = s(v, "hook_event_name").ok_or(ParseError::Missing("hook_event_name"))?;
    let cwd = s(v, "cwd").unwrap_or_default();
    let event = match name.as_str() {
        "SessionStart" => ClaudeEvent::SessionStart {
            source: s(v, "source").unwrap_or_else(|| "startup".into()),
            model: s(v, "model"),
            session_title: s(v, "session_title"),
        },
        "UserPromptSubmit" => ClaudeEvent::UserPromptSubmit {
            prompt: s(v, "prompt").unwrap_or_default(),
            session_title: s(v, "session_title"),
        },
        "PostToolUse" => ClaudeEvent::PostToolUse {
            tool_name: s(v, "tool_name").unwrap_or_default(),
            tool_input: v.get("tool_input").cloned().unwrap_or(Value::Null),
            tool_response: v.get("tool_response").cloned().unwrap_or(Value::Null),
        },
        "PermissionRequest" => ClaudeEvent::PermissionRequest {
            tool_name: s(v, "tool_name").unwrap_or_default(),
        },
        "Notification" => ClaudeEvent::Notification {
            notification_type: s(v, "notification_type").unwrap_or_default(),
            message: s(v, "message").unwrap_or_default(),
            title: s(v, "title"),
        },
        "TaskCreated" => ClaudeEvent::TaskCreated {
            task_id: s(v, "task_id").unwrap_or_default(),
            subject: s(v, "task_subject").unwrap_or_default(),
        },
        "TaskCompleted" => ClaudeEvent::TaskCompleted {
            task_id: s(v, "task_id").unwrap_or_default(),
            subject: s(v, "task_subject").unwrap_or_default(),
        },
        "Stop" => ClaudeEvent::Stop {
            last_assistant_message: s(v, "last_assistant_message"),
            background_tasks: v
                .get("background_tasks")
                .and_then(|b| b.as_array())
                .map(|a| a.len())
                .unwrap_or(0),
        },
        "StopFailure" => ClaudeEvent::StopFailure {
            error: s(v, "error").unwrap_or_else(|| "unknown".into()),
            error_details: s(v, "error_details"),
        },
        "SessionEnd" => ClaudeEvent::SessionEnd {
            reason: s(v, "reason").unwrap_or_else(|| "other".into()),
        },
        other => ClaudeEvent::Other(other.to_string()),
    };
    Ok(ClaudeHook {
        session_id,
        transcript_path: s(v, "transcript_path"),
        cwd,
        permission_mode: s(v, "permission_mode"),
        event,
    })
}

/// 会让计划变化的工具。
pub const PLAN_TOOLS: [&str; 4] = ["TodoWrite", "TaskCreate", "TaskUpdate", "ExitPlanMode"];

/// 这个事件之后是否值得读 transcript 来补全计划。
pub fn wants_transcript(hook: &ClaudeHook, session: &AgentSession) -> bool {
    match &hook.event {
        ClaudeEvent::SessionStart { source, .. } => source != "startup" && source != "clear",
        // TaskUpdate 引用了我们还不认识的 id（例如宠物启动前创建的任务）。
        ClaudeEvent::PostToolUse { tool_name, tool_input, .. } if tool_name == "TaskUpdate" => {
            match task_id_of(tool_input) {
                Some(id) => !session.plan.iter().any(|p| p.id.as_deref() == Some(id.as_str())),
                None => false,
            }
        }
        ClaudeEvent::Stop { .. } => session.plan.is_empty(),
        _ => false,
    }
}

fn task_id_of(input: &Value) -> Option<String> {
    for key in ["taskId", "id", "task_id"] {
        if let Some(v) = input.get(key) {
            if let Some(s) = v.as_str() {
                return Some(s.to_string());
            }
            if let Some(n) = v.as_i64() {
                return Some(n.to_string());
            }
        }
    }
    None
}

fn str_field(input: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| input.get(*k).and_then(|v| v.as_str()))
        .map(|s| s.to_string())
}

/// 处理 TodoWrite：整张清单替换。
fn plan_from_todos(input: &Value) -> Option<Vec<PlanItem>> {
    let todos = input.get("todos")?.as_array()?;
    Some(
        todos
            .iter()
            .filter_map(|t| {
                let text = str_field(t, &["content", "subject"])?;
                let status = t
                    .get("status")
                    .and_then(|s| s.as_str())
                    .and_then(PlanStatus::parse)
                    .unwrap_or(PlanStatus::Pending);
                Some(PlanItem {
                    id: t.get("id").and_then(|i| i.as_str()).map(|s| s.to_string()),
                    text,
                    status,
                    active_text: str_field(t, &["activeForm", "active_form"]),
                })
            })
            .collect(),
    )
}

/// 从 ExitPlanMode 的 markdown 计划里提取列表项。
pub fn plan_from_markdown(md: &str) -> Vec<PlanItem> {
    let mut items = Vec::new();
    for line in md.lines() {
        // 只取顶层列表项（不缩进）。
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let t = line.trim();
        let rest = if let Some(r) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            Some(r)
        } else {
            let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
            if digits > 0 {
                t[digits..].strip_prefix(". ").or_else(|| t[digits..].strip_prefix(") "))
            } else {
                None
            }
        };
        if let Some(r) = rest {
            let (status, text) = if let Some(x) = r.strip_prefix("[x] ").or_else(|| r.strip_prefix("[X] ")) {
                (PlanStatus::Completed, x)
            } else if let Some(x) = r.strip_prefix("[ ] ") {
                (PlanStatus::Pending, x)
            } else {
                (PlanStatus::Pending, r)
            };
            let text = text.trim();
            if !text.is_empty() {
                items.push(PlanItem::new(text, status));
            }
        }
    }
    items
}

/// 任务工具的增量更新。
fn upsert_task(plan: &mut Vec<PlanItem>, id: &str, subject: Option<String>, status: Option<PlanStatus>, active: Option<String>) {
    if let Some(item) = plan.iter_mut().find(|p| p.id.as_deref() == Some(id)) {
        if let Some(sub) = subject {
            if !sub.is_empty() {
                item.text = sub;
            }
        }
        if let Some(st) = status {
            item.status = st;
        }
        if active.is_some() {
            item.active_text = active;
        }
    } else if let Some(sub) = subject {
        plan.push(PlanItem {
            id: Some(id.to_string()),
            text: sub,
            status: status.unwrap_or(PlanStatus::Pending),
            active_text: active,
        });
    }
}

fn apply_task_update(plan: &mut Vec<PlanItem>, input: &Value) {
    let Some(id) = task_id_of(input) else { return };
    let status_str = input.get("status").and_then(|s| s.as_str());
    if status_str == Some("deleted") {
        plan.retain(|p| p.id.as_deref() != Some(id.as_str()));
        return;
    }
    upsert_task(
        plan,
        &id,
        str_field(input, &["subject"]),
        status_str.and_then(PlanStatus::parse),
        str_field(input, &["activeForm", "active_form"]),
    );
}

fn apply_plan_tool(plan: &mut Vec<PlanItem>, tool_name: &str, input: &Value, response: &Value) -> bool {
    match tool_name {
        "TodoWrite" => {
            if let Some(p) = plan_from_todos(input) {
                *plan = p;
                return true;
            }
            false
        }
        "TaskCreate" => {
            let subject = str_field(input, &["subject"]).or_else(|| {
                response.get("task").and_then(|t| str_field(t, &["subject"]))
            });
            let id = response
                .get("task")
                .and_then(|t| t.get("id"))
                .and_then(|i| i.as_str().map(|s| s.to_string()).or_else(|| i.as_i64().map(|n| n.to_string())));
            let active = str_field(input, &["activeForm", "active_form"]);
            match (id, subject) {
                (Some(id), sub) => upsert_task(plan, &id, sub, None, active),
                (None, Some(sub)) => {
                    // 没有 id（例如 tool_response 被截断）：按标题去重。
                    if !plan.iter().any(|p| p.text == sub) {
                        let mut item = PlanItem::new(sub, PlanStatus::Pending);
                        item.active_text = active;
                        plan.push(item);
                    }
                }
                (None, None) => return false,
            }
            true
        }
        "TaskUpdate" => {
            apply_task_update(plan, input);
            true
        }
        "ExitPlanMode" => {
            if !plan.is_empty() {
                return false;
            }
            if let Some(md) = input.get("plan").and_then(|p| p.as_str()) {
                let items = plan_from_markdown(md);
                if !items.is_empty() {
                    *plan = items;
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

/// 把一次 hook 应用到会话上，返回宠物应有的反应。
pub fn apply_hook(session: &mut AgentSession, hook: &ClaudeHook, now: i64) -> Option<Cue> {
    let before = session.status;
    if !hook.cwd.is_empty() {
        session.cwd = hook.cwd.clone();
    }
    session.updated_ms = now;
    match &hook.event {
        ClaudeEvent::SessionStart { source, session_title, .. } => {
            if session_title.is_some() {
                session.title = session_title.clone();
            }
            if source == "clear" || source == "startup" {
                session.plan.clear();
            }
            // compact / resume 时如果正在跑就保持，否则空闲。
            if !matches!(session.status, SessionStatus::Running) {
                session.set_status(SessionStatus::Idle, None);
            }
        }
        ClaudeEvent::UserPromptSubmit { prompt, session_title } => {
            if session_title.is_some() {
                session.title = session_title.clone();
            }
            if !prompt.is_empty() {
                session.last_prompt = Some(excerpt(prompt, EXCERPT_CHARS));
            }
            session.set_status(SessionStatus::Running, None);
        }
        ClaudeEvent::PostToolUse { tool_name, tool_input, tool_response } => {
            apply_plan_tool(&mut session.plan, tool_name, tool_input, tool_response);
            session.set_status(SessionStatus::Running, None);
        }
        ClaudeEvent::PermissionRequest { tool_name } => {
            session.set_status(SessionStatus::Waiting, Some(format!("等待批准：{tool_name}")));
        }
        ClaudeEvent::Notification { notification_type, .. } => match notification_type.as_str() {
            "permission_prompt" => {
                let detail = session.detail.clone().filter(|_| session.status == SessionStatus::Waiting);
                session.set_status(SessionStatus::Waiting, detail.or_else(|| Some("等待批准".into())));
            }
            "elicitation_dialog" | "elicitation_url_dialog" | "agent_needs_input" => {
                session.set_status(SessionStatus::Waiting, Some("等待输入".into()));
            }
            "idle_prompt" => {
                // 一轮已结束且我一直没回复：保持“完成”，不重复提醒。
            }
            _ => {}
        },
        ClaudeEvent::TaskCreated { task_id, subject } => {
            if !task_id.is_empty() {
                upsert_task(&mut session.plan, task_id, Some(subject.clone()), None, None);
            }
        }
        ClaudeEvent::TaskCompleted { task_id, subject } => {
            if !task_id.is_empty() {
                upsert_task(&mut session.plan, task_id, Some(subject.clone()), Some(PlanStatus::Completed), None);
            }
        }
        ClaudeEvent::Stop { last_assistant_message, background_tasks } => {
            if let Some(m) = last_assistant_message {
                session.last_reply = Some(excerpt(m, EXCERPT_CHARS));
            }
            let detail = (*background_tasks > 0).then(|| format!("还有 {background_tasks} 个后台任务"));
            session.set_status(SessionStatus::Done, detail);
        }
        ClaudeEvent::StopFailure { error, error_details } => {
            let detail = match error_details {
                Some(d) if !d.is_empty() => format!("{error}：{}", excerpt(d, 80)),
                _ => error.clone(),
            };
            session.set_status(SessionStatus::Error, Some(detail));
        }
        ClaudeEvent::SessionEnd { reason } => {
            session.set_status(SessionStatus::Ended, Some(reason.clone()));
        }
        ClaudeEvent::Other(_) => {}
    }
    cue_for_transition(before, session.status)
}

/// 为新的 hook 创建会话。
pub fn new_session(hook: &ClaudeHook, now: i64) -> AgentSession {
    AgentSession::new(AgentKind::Claude, hook.session_id.clone(), hook.cwd.clone(), now)
}

/// 从 transcript（JSONL）重建计划清单。
///
/// transcript 每行是一条消息：助手消息的 `message.content[]` 里有 `tool_use` 块
/// （`id`、`name`、`input`），工具结果在用户消息里，结构化输出放在 `toolUseResult`
/// （SDK 流里叫 `tool_use_result`），`TaskCreate` 的结果形如 `{ "task": { "id", "subject" } }`。
/// 返回 `None` 表示 transcript 里没有任何计划工具调用。
pub fn plan_from_transcript<R: BufRead>(reader: R) -> Option<Vec<PlanItem>> {
    let mut plan: Vec<PlanItem> = Vec::new();
    let mut seen = false;
    let mut pending_creates: std::collections::HashMap<String, Value> = Default::default();
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(content) = v.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_array()) else {
            continue;
        };
        for block in content {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("tool_use") => {
                    let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let input = block.get("input").cloned().unwrap_or(Value::Null);
                    match name {
                        "TodoWrite" | "TaskUpdate" | "ExitPlanMode" => {
                            seen |= apply_plan_tool(&mut plan, name, &input, &Value::Null);
                        }
                        "TaskCreate" => {
                            if let Some(id) = block.get("id").and_then(|i| i.as_str()) {
                                pending_creates.insert(id.to_string(), input);
                            }
                        }
                        _ => {}
                    }
                }
                Some("tool_result") => {
                    let Some(use_id) = block.get("tool_use_id").and_then(|i| i.as_str()) else { continue };
                    if let Some(input) = pending_creates.remove(use_id) {
                        if block.get("is_error").and_then(|e| e.as_bool()) == Some(true) {
                            continue;
                        }
                        let result = v
                            .get("toolUseResult")
                            .or_else(|| v.get("tool_use_result"))
                            .cloned()
                            .unwrap_or(Value::Null);
                        seen |= apply_plan_tool(&mut plan, "TaskCreate", &input, &result);
                    }
                }
                _ => {}
            }
        }
    }
    seen.then_some(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION_START: &str = include_str!("../../tests/fixtures/claude/session_start.json");
    const USER_PROMPT: &str = include_str!("../../tests/fixtures/claude/user_prompt_submit.json");
    const POST_TOOL_USE: &str = include_str!("../../tests/fixtures/claude/post_tool_use.json");
    const PERMISSION: &str = include_str!("../../tests/fixtures/claude/permission_request.json");
    const NOTIFICATION: &str = include_str!("../../tests/fixtures/claude/notification.json");
    const TASK_CREATED: &str = include_str!("../../tests/fixtures/claude/task_created.json");
    const TASK_COMPLETED: &str = include_str!("../../tests/fixtures/claude/task_completed.json");
    const STOP: &str = include_str!("../../tests/fixtures/claude/stop.json");
    const STOP_FAILURE: &str = include_str!("../../tests/fixtures/claude/stop_failure.json");
    const SESSION_END: &str = include_str!("../../tests/fixtures/claude/session_end.json");
    const TODO_WRITE: &str = include_str!("../../tests/fixtures/claude/post_tool_use_todowrite.json");
    const TASK_CREATE: &str = include_str!("../../tests/fixtures/claude/post_tool_use_task_create.json");
    const TASK_UPDATE: &str = include_str!("../../tests/fixtures/claude/post_tool_use_task_update.json");
    const TRANSCRIPT: &str = include_str!("../../tests/fixtures/claude/transcript.jsonl");

    #[test]
    fn parses_documented_samples() {
        let h = parse_hook(SESSION_START).unwrap();
        assert_eq!(h.session_id, "abc123");
        assert_eq!(
            h.event,
            ClaudeEvent::SessionStart { source: "resume".into(), model: Some("claude-opus-5".into()), session_title: None }
        );
        assert!(h.transcript_path.unwrap().ends_with(".jsonl"));

        let h = parse_hook(USER_PROMPT).unwrap();
        assert_eq!(h.permission_mode.as_deref(), Some("default"));
        match h.event {
            ClaudeEvent::UserPromptSubmit { prompt, .. } => {
                assert_eq!(prompt, "Write a function to calculate the factorial of a number")
            }
            e => panic!("{e:?}"),
        }

        let h = parse_hook(POST_TOOL_USE).unwrap();
        match h.event {
            ClaudeEvent::PostToolUse { tool_name, tool_response, .. } => {
                assert_eq!(tool_name, "Write");
                assert_eq!(tool_response["type"], "create");
            }
            e => panic!("{e:?}"),
        }

        let h = parse_hook(PERMISSION).unwrap();
        assert_eq!(h.event, ClaudeEvent::PermissionRequest { tool_name: "Bash".into() });

        let h = parse_hook(NOTIFICATION).unwrap();
        assert_eq!(
            h.event,
            ClaudeEvent::Notification {
                notification_type: "permission_prompt".into(),
                message: "Claude needs your permission".into(),
                title: Some("Permission needed".into())
            }
        );

        let h = parse_hook(TASK_CREATED).unwrap();
        assert_eq!(h.event, ClaudeEvent::TaskCreated { task_id: "task-001".into(), subject: "Implement user authentication".into() });
        let h = parse_hook(TASK_COMPLETED).unwrap();
        assert_eq!(h.event, ClaudeEvent::TaskCompleted { task_id: "task-001".into(), subject: "Implement user authentication".into() });

        let h = parse_hook(STOP).unwrap();
        assert_eq!(
            h.event,
            ClaudeEvent::Stop {
                last_assistant_message: Some("I've completed the refactoring. Here's a summary...".into()),
                background_tasks: 1
            }
        );

        let h = parse_hook(STOP_FAILURE).unwrap();
        assert_eq!(h.event, ClaudeEvent::StopFailure { error: "rate_limit".into(), error_details: Some("429 Too Many Requests".into()) });

        let h = parse_hook(SESSION_END).unwrap();
        assert_eq!(h.event, ClaudeEvent::SessionEnd { reason: "other".into() });
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(parse_hook("not json"), Err(ParseError::Json(_))));
        assert_eq!(parse_hook("{}"), Err(ParseError::Missing("session_id")));
        assert_eq!(parse_hook(r#"{"session_id":"x"}"#), Err(ParseError::Missing("hook_event_name")));
        // 未知事件不报错。
        let h = parse_hook(r#"{"session_id":"x","hook_event_name":"FutureEvent"}"#).unwrap();
        assert_eq!(h.event, ClaudeEvent::Other("FutureEvent".into()));
    }

    #[test]
    fn lifecycle_produces_expected_status_and_cues() {
        let start = parse_hook(SESSION_START).unwrap();
        let mut s = new_session(&start, 1);
        assert_eq!(apply_hook(&mut s, &start, 1), None);
        assert_eq!(s.status, SessionStatus::Idle);

        let prompt = parse_hook(USER_PROMPT).unwrap();
        assert_eq!(apply_hook(&mut s, &prompt, 2), None);
        assert_eq!(s.status, SessionStatus::Running);
        assert!(s.last_prompt.as_ref().unwrap().contains("factorial"));

        let perm = parse_hook(PERMISSION).unwrap();
        assert_eq!(apply_hook(&mut s, &perm, 3), Some(Cue::Waiting));
        assert_eq!(s.detail.as_deref(), Some("等待批准：Bash"));
        // 随后的 permission_prompt 通知不会重复举手，也保留工具名。
        let notif = parse_hook(NOTIFICATION).unwrap();
        assert_eq!(apply_hook(&mut s, &notif, 4), None);
        assert_eq!(s.detail.as_deref(), Some("等待批准：Bash"));

        // 批准后工具执行完成：回到运行中。
        let post = parse_hook(POST_TOOL_USE).unwrap();
        assert_eq!(apply_hook(&mut s, &post, 5), None);
        assert_eq!(s.status, SessionStatus::Running);

        let stop = parse_hook(STOP).unwrap();
        assert_eq!(apply_hook(&mut s, &stop, 6), Some(Cue::Done));
        assert_eq!(s.detail.as_deref(), Some("还有 1 个后台任务"));
        assert!(s.last_reply.as_ref().unwrap().starts_with("I've completed"));

        let fail = parse_hook(STOP_FAILURE).unwrap();
        assert_eq!(apply_hook(&mut s, &fail, 7), Some(Cue::Error));
        assert_eq!(s.detail.as_deref(), Some("rate_limit：429 Too Many Requests"));

        let end = parse_hook(SESSION_END).unwrap();
        assert_eq!(apply_hook(&mut s, &end, 8), None);
        assert_eq!(s.status, SessionStatus::Ended);
        assert_eq!(s.updated_ms, 8);
    }

    #[test]
    fn todowrite_replaces_plan() {
        let h = parse_hook(TODO_WRITE).unwrap();
        let mut s = new_session(&h, 0);
        apply_hook(&mut s, &h, 1);
        assert_eq!(s.plan.len(), 3);
        assert_eq!(s.plan[0].status, PlanStatus::Completed);
        assert_eq!(s.plan[1].status, PlanStatus::InProgress);
        assert_eq!(s.plan[1].active_text.as_deref(), Some("正在编写单元测试"));
        assert_eq!(s.plan_progress(), (1, 3));
    }

    #[test]
    fn task_tools_build_plan_incrementally() {
        let created = parse_hook(TASK_CREATED).unwrap();
        let mut s = new_session(&created, 0);
        apply_hook(&mut s, &created, 1);
        assert_eq!(s.plan.len(), 1);
        assert_eq!(s.plan[0].id.as_deref(), Some("task-001"));

        // TaskCreate 的 PostToolUse 带回同一个 id：不会重复。
        let create = parse_hook(TASK_CREATE).unwrap();
        apply_hook(&mut s, &create, 2);
        assert_eq!(s.plan.len(), 1);
        assert_eq!(s.plan[0].active_text.as_deref(), Some("Implementing user authentication"));

        let update = parse_hook(TASK_UPDATE).unwrap();
        apply_hook(&mut s, &update, 3);
        assert_eq!(s.plan[0].status, PlanStatus::InProgress);

        let done = parse_hook(TASK_COMPLETED).unwrap();
        apply_hook(&mut s, &done, 4);
        assert_eq!(s.plan[0].status, PlanStatus::Completed);

        // 删除。
        let del = parse_hook(
            r#"{"session_id":"abc123","cwd":"/p","hook_event_name":"PostToolUse","tool_name":"TaskUpdate","tool_input":{"taskId":"task-001","status":"deleted"},"tool_response":{}}"#,
        )
        .unwrap();
        apply_hook(&mut s, &del, 5);
        assert!(s.plan.is_empty());
    }

    #[test]
    fn task_update_with_unknown_id_requests_transcript() {
        let update = parse_hook(TASK_UPDATE).unwrap();
        let s = new_session(&update, 0);
        assert!(wants_transcript(&update, &s));
        let resume = parse_hook(SESSION_START).unwrap();
        assert!(wants_transcript(&resume, &s));
    }

    #[test]
    fn exit_plan_mode_markdown() {
        let items = plan_from_markdown("# Plan\n\n1. 读代码\n2. 改 bug\n   - 子项不算\n- [x] 已完成的一步\n\n其他文字");
        let texts: Vec<_> = items.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(texts, vec!["读代码", "改 bug", "已完成的一步"]);
        assert_eq!(items[2].status, PlanStatus::Completed);
    }

    #[test]
    fn rebuilds_plan_from_transcript() {
        let plan = plan_from_transcript(std::io::Cursor::new(TRANSCRIPT)).unwrap();
        let summary: Vec<_> = plan.iter().map(|p| (p.id.clone().unwrap_or_default(), p.text.clone(), p.status)).collect();
        assert_eq!(
            summary,
            vec![
                ("1".to_string(), "读取配置".to_string(), PlanStatus::Completed),
                ("2".to_string(), "写解析器".to_string(), PlanStatus::InProgress),
            ]
        );
        // 没有计划工具时返回 None。
        assert!(plan_from_transcript(std::io::Cursor::new("{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}\n")).is_none());
    }
}
