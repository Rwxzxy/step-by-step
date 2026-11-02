//! Codex CLI：`notify` 回调 payload 与 `~/.codex/sessions/**/rollout-*.jsonl` 的增量解析。
//!
//! 格式依据 openai/codex 仓库源码（2026-11）：
//!
//! * `codex-rs/hooks/src/legacy_notify.rs`：`notify` 把一个 JSON 作为最后一个 argv 传给命令，
//!   形如 `{"type":"agent-turn-complete","thread-id","turn-id","cwd","client","input-messages","last-assistant-message"}`。
//! * `codex-rs/history/src/rollout_payload.rs`：rollout 每行是 `{"timestamp", "type", "payload"}`，
//!   `type` 为 `session_meta` / `response_item` / `event_msg` / `turn_context` / `compacted` 等。
//! * `codex-rs/rollout/src/policy.rs`：持久化的 `event_msg` 包括 `task_started`（别名 `turn_started`）、
//!   `task_complete`（别名 `turn_complete`，可带 `error`）、`turn_aborted`、`user_message`、`agent_message`。
//!   `plan_update` 和审批请求 **不会** 写入 rollout，所以计划要从 `response_item` 里
//!   `name == "update_plan"` 的 `function_call`（`arguments` 是 JSON 字符串）读取。
//! * `codex-rs/protocol/src/plan_tool.rs`：`UpdatePlanArgs { explanation?, plan: [{ step, status }] }`。
//! * 冷数据会被压缩成 `.jsonl.zst`，活跃会话始终是明文 `.jsonl`，我们只跟踪明文文件。

use serde_json::Value;

use super::{
    cue_for_transition, excerpt, AgentKind, AgentSession, Cue, PlanItem, PlanStatus, SessionStatus,
    EXCERPT_CHARS,
};

/// `notify` 回调的内容。
#[derive(Debug, Clone, PartialEq)]
pub struct CodexNotify {
    pub thread_id: String,
    pub turn_id: String,
    pub cwd: String,
    pub client: Option<String>,
    pub input_messages: Vec<String>,
    pub last_assistant_message: Option<String>,
}

/// 解析 notify 的 JSON 参数。只认 `agent-turn-complete`。
pub fn parse_notify(raw: &str) -> Option<CodexNotify> {
    let v: Value = serde_json::from_str(raw).ok()?;
    if v.get("type")?.as_str()? != "agent-turn-complete" {
        return None;
    }
    let get = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|s| s.to_string());
    Some(CodexNotify {
        thread_id: get("thread-id")?,
        turn_id: get("turn-id").unwrap_or_default(),
        cwd: get("cwd").unwrap_or_default(),
        client: get("client"),
        input_messages: v
            .get("input-messages")
            .and_then(|a| a.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|m| m.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        last_assistant_message: get("last-assistant-message"),
    })
}

/// 应用 notify：这一轮完成。
pub fn apply_notify(session: &mut AgentSession, n: &CodexNotify, now: i64) -> Option<Cue> {
    let before = session.status;
    if !n.cwd.is_empty() {
        session.cwd = n.cwd.clone();
    }
    if let Some(m) = n.input_messages.last() {
        session.last_prompt = Some(excerpt(m, EXCERPT_CHARS));
    }
    if let Some(m) = &n.last_assistant_message {
        session.last_reply = Some(excerpt(m, EXCERPT_CHARS));
    }
    // 如果 rollout 已经报告了出错，就不要覆盖成完成。
    if session.status != SessionStatus::Error {
        session.set_status(SessionStatus::Done, None);
    }
    session.updated_ms = now;
    cue_for_transition(before, session.status)
}

/// 从 rollout 文件名里取 thread id：`rollout-YYYY-MM-DDThh-mm-ss-<uuid>[_<rollout id>].jsonl`。
pub fn thread_id_from_filename(name: &str) -> Option<String> {
    let core = name.strip_prefix("rollout-")?.strip_suffix(".jsonl")?;
    if core.len() < 21 || core.as_bytes().get(19) != Some(&b'-') {
        return None;
    }
    let ids = &core[20..];
    let thread = ids.split('_').next()?;
    (thread.len() >= 8).then(|| thread.to_string())
}

/// 增量读取 JSONL：只交出完整的行，半行留到下次。
#[derive(Debug, Default, Clone)]
pub struct LineBuffer {
    pending: Vec<u8>,
}

impl LineBuffer {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(pos) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=pos).collect();
            let text = String::from_utf8_lossy(&line[..line.len() - 1])
                .trim_end_matches('\r')
                .to_string();
            if !text.trim().is_empty() {
                out.push(text);
            }
        }
        out
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

/// 一个 rollout 文件对应的会话追踪器。
#[derive(Debug, Clone)]
pub struct CodexTracker {
    pub session: AgentSession,
    /// 已经读到的字节偏移。
    pub offset: u64,
    buffer: LineBuffer,
    /// 子 Agent / 内部会话不显示在面板上。
    pub hidden: bool,
    /// 是否已经看到 session_meta。
    pub has_meta: bool,
}

impl CodexTracker {
    pub fn new(thread_id: impl Into<String>, now: i64) -> Self {
        CodexTracker {
            session: AgentSession::new(AgentKind::Codex, thread_id, "", now),
            offset: 0,
            buffer: LineBuffer::default(),
            hidden: false,
            has_meta: false,
        }
    }

    /// 喂入新读到的字节，返回期间产生的宠物反应（最后一个）。
    /// `replay` 为 true 时（第一次扫描已有文件）不产生反应。
    pub fn feed(&mut self, bytes: &[u8], now: i64, replay: bool) -> Option<Cue> {
        self.offset += bytes.len() as u64;
        let mut cue = None;
        for line in self.buffer.feed(bytes) {
            if let Some(c) = self.apply_line(&line, now) {
                cue = Some(c);
            }
        }
        if replay {
            None
        } else {
            cue
        }
    }

    /// 应用一行 rollout。
    pub fn apply_line(&mut self, line: &str, now: i64) -> Option<Cue> {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return None;
        };
        let before = self.session.status;
        let ts = v
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.timestamp_millis())
            .unwrap_or(now);
        let payload = v.get("payload").cloned().unwrap_or(Value::Null);
        let s = &mut self.session;
        match v.get("type").and_then(|t| t.as_str()) {
            Some("session_meta") => {
                self.has_meta = true;
                if let Some(id) = payload.get("id").and_then(|i| i.as_str()) {
                    s.session_id = id.to_string();
                }
                if let Some(cwd) = payload.get("cwd").and_then(|c| c.as_str()) {
                    s.cwd = cwd.to_string();
                }
                s.started_ms = ts;
                // 子 Agent（source = {"subagent": ...}）和内部会话不展示。
                if let Some(src) = payload.get("source") {
                    if src.get("subagent").is_some() || src.get("internal").is_some() {
                        self.hidden = true;
                    }
                }
                if payload
                    .get("parent_thread_id")
                    .and_then(|p| p.as_str())
                    .is_some()
                {
                    self.hidden = true;
                }
            }
            Some("turn_context") => {
                if let Some(cwd) = payload.get("cwd").and_then(|c| c.as_str()) {
                    s.cwd = cwd.to_string();
                }
            }
            Some("event_msg") => match payload.get("type").and_then(|t| t.as_str()) {
                Some("task_started") | Some("turn_started") => {
                    s.set_status(SessionStatus::Running, None);
                }
                Some("user_message") => {
                    if let Some(m) = payload.get("message").and_then(|m| m.as_str()) {
                        s.last_prompt = Some(excerpt(m, EXCERPT_CHARS));
                    }
                    s.set_status(SessionStatus::Running, None);
                }
                Some("agent_message") => {
                    if let Some(m) = payload.get("message").and_then(|m| m.as_str()) {
                        s.last_reply = Some(excerpt(m, EXCERPT_CHARS));
                    }
                }
                Some("task_complete") | Some("turn_complete") => {
                    if let Some(m) = payload.get("last_agent_message").and_then(|m| m.as_str()) {
                        s.last_reply = Some(excerpt(m, EXCERPT_CHARS));
                    }
                    match payload.get("error").filter(|e| !e.is_null()) {
                        Some(err) => {
                            let msg = err
                                .get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("出错");
                            s.set_status(SessionStatus::Error, Some(excerpt(msg, 80)));
                        }
                        None => {
                            s.set_status(SessionStatus::Done, None);
                        }
                    }
                }
                Some("turn_aborted") => {
                    let reason = payload.get("reason").and_then(|r| r.as_str()).unwrap_or("");
                    if let Some(err) = payload.get("error").filter(|e| !e.is_null()) {
                        let msg = err
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("出错");
                        s.set_status(SessionStatus::Error, Some(excerpt(msg, 80)));
                    } else {
                        match reason {
                            "interrupted" => {
                                s.set_status(SessionStatus::Done, Some("已中断".into()));
                            }
                            "budget_limited" => {
                                s.set_status(SessionStatus::Error, Some("超出预算".into()));
                            }
                            // replaced / review_ended：会紧接着开始新一轮，不改状态。
                            _ => {}
                        }
                    }
                }
                _ => {}
            },
            Some("response_item") => {
                let ty = payload.get("type").and_then(|t| t.as_str());
                let name = payload.get("name").and_then(|n| n.as_str());
                if matches!(ty, Some("function_call") | Some("custom_tool_call"))
                    && name == Some("update_plan")
                {
                    let args = payload
                        .get("arguments")
                        .or_else(|| payload.get("input"))
                        .cloned()
                        .unwrap_or(Value::Null);
                    let args = match args {
                        Value::String(s) => {
                            serde_json::from_str::<Value>(&s).unwrap_or(Value::Null)
                        }
                        other => other,
                    };
                    if let Some(plan) = parse_update_plan(&args) {
                        s.plan = plan;
                    }
                }
            }
            _ => {}
        }
        s.updated_ms = ts.max(s.updated_ms);
        cue_for_transition(before, s.status)
    }
}

/// 解析 `update_plan` 的参数。
pub fn parse_update_plan(args: &Value) -> Option<Vec<PlanItem>> {
    let plan = args.get("plan")?.as_array()?;
    Some(
        plan.iter()
            .filter_map(|p| {
                let step = p.get("step")?.as_str()?;
                let status = p
                    .get("status")
                    .and_then(|s| s.as_str())
                    .and_then(PlanStatus::parse)
                    .unwrap_or(PlanStatus::Pending);
                Some(PlanItem::new(step, status))
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROLLOUT: &str = include_str!("../../tests/fixtures/codex/rollout-2026-11-02T09-15-00-0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b.jsonl");
    const NOTIFY: &str = include_str!("../../tests/fixtures/codex/notify.json");

    #[test]
    fn parses_notify_payload() {
        let n = parse_notify(NOTIFY).unwrap();
        assert_eq!(n.thread_id, "b5f6c1c2-1111-2222-3333-444455556666");
        assert_eq!(n.turn_id, "12345");
        assert_eq!(n.cwd, "/Users/example/project");
        assert_eq!(n.client.as_deref(), Some("codex-tui"));
        assert_eq!(
            n.input_messages,
            vec!["Rename `foo` to `bar` and update the callsites."]
        );
        assert_eq!(
            n.last_assistant_message.as_deref(),
            Some("Rename complete and verified `cargo build` succeeds.")
        );
        assert!(parse_notify(r#"{"type":"something-else"}"#).is_none());
        assert!(parse_notify("garbage").is_none());
    }

    #[test]
    fn filename_thread_id() {
        assert_eq!(
            thread_id_from_filename(
                "rollout-2026-11-02T09-15-00-0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b.jsonl"
            )
            .as_deref(),
            Some("0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b")
        );
        assert_eq!(
            thread_id_from_filename("rollout-2026-11-02T09-15-00-0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b_0199ffff-0000-7000-8000-000000000000.jsonl").as_deref(),
            Some("0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b")
        );
        assert!(thread_id_from_filename("rollout-x.jsonl.zst").is_none());
        assert!(thread_id_from_filename("other.jsonl").is_none());
    }

    #[test]
    fn full_rollout_replay() {
        let mut t = CodexTracker::new("x", 0);
        let cue = t.feed(ROLLOUT.as_bytes(), 0, true);
        assert_eq!(cue, None, "回放已有文件时不应触发反应");
        let s = &t.session;
        assert_eq!(s.session_id, "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b");
        assert_eq!(s.cwd, "/home/me/web-app");
        assert_eq!(s.project_name(), "web-app");
        assert_eq!(s.status, SessionStatus::Done);
        assert_eq!(s.plan.len(), 3);
        assert_eq!(s.plan_progress(), (3, 3));
        assert_eq!(
            s.last_prompt.as_deref(),
            Some("把登录页的按钮改成蓝色，然后跑测试")
        );
        assert_eq!(
            s.last_reply.as_deref(),
            Some("按钮已改为蓝色，测试全部通过。")
        );
        assert!(!t.hidden);
        assert_eq!(t.offset as usize, ROLLOUT.len());
    }

    #[test]
    fn incremental_feed_handles_partial_lines() {
        let mut t = CodexTracker::new("x", 0);
        let bytes = ROLLOUT.as_bytes();
        // 一个字节一个字节喂，结果应与一次性喂入相同。
        let mut last_cue = None;
        for b in bytes {
            if let Some(c) = t.feed(std::slice::from_ref(b), 0, false) {
                last_cue = Some(c);
            }
        }
        assert_eq!(last_cue, Some(Cue::Done));
        assert_eq!(t.session.plan_progress(), (3, 3));
        assert_eq!(t.offset as usize, bytes.len());
    }

    #[test]
    fn plan_progress_mid_turn() {
        let mut t = CodexTracker::new("x", 0);
        // 只喂到第一次 update_plan。
        let upto: String = ROLLOUT.lines().take(6).map(|l| format!("{l}\n")).collect();
        t.feed(upto.as_bytes(), 0, false);
        assert_eq!(t.session.status, SessionStatus::Running);
        let st: Vec<_> = t.session.plan.iter().map(|p| p.status).collect();
        assert_eq!(
            st,
            vec![
                PlanStatus::InProgress,
                PlanStatus::Pending,
                PlanStatus::Pending
            ]
        );
        assert_eq!(t.session.plan[0].text, "修改按钮样式");
    }

    #[test]
    fn error_and_abort() {
        let mut t = CodexTracker::new("x", 0);
        t.apply_line(r#"{"timestamp":"2026-11-02T10:00:00.000Z","type":"event_msg","payload":{"type":"task_started","turn_id":"1"}}"#, 0);
        let cue = t.apply_line(
            r#"{"timestamp":"2026-11-02T10:00:01.000Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"1","last_agent_message":null,"error":{"message":"stream disconnected before completion","codex_error_info":null}}}"#,
            0,
        );
        assert_eq!(cue, Some(Cue::Error));
        assert_eq!(
            t.session.detail.as_deref(),
            Some("stream disconnected before completion")
        );

        t.apply_line(r#"{"timestamp":"2026-11-02T10:00:02.000Z","type":"event_msg","payload":{"type":"turn_started","turn_id":"2"}}"#, 0);
        let cue = t.apply_line(r#"{"timestamp":"2026-11-02T10:00:03.000Z","type":"event_msg","payload":{"type":"turn_aborted","turn_id":"2","reason":"interrupted"}}"#, 0);
        assert_eq!(cue, Some(Cue::Done));
        assert_eq!(t.session.detail.as_deref(), Some("已中断"));
    }

    #[test]
    fn subagent_sessions_hidden() {
        let mut t = CodexTracker::new("x", 0);
        t.apply_line(
            r#"{"timestamp":"2026-11-02T10:00:00.000Z","type":"session_meta","payload":{"id":"abc","session_id":"root","timestamp":"2026-11-02T10:00:00.000Z","cwd":"/p","originator":"codex_cli_rs","cli_version":"0.200.0","source":{"subagent":"review"}}}"#,
            0,
        );
        assert!(t.hidden);
    }

    #[test]
    fn notify_after_rollout_does_not_double_cue() {
        let mut t = CodexTracker::new("x", 0);
        t.feed(ROLLOUT.as_bytes(), 0, true);
        let n = parse_notify(NOTIFY).unwrap();
        assert_eq!(apply_notify(&mut t.session, &n, 1), None);
        // 而从运行中收到 notify 会触发“完成”。
        t.session.status = SessionStatus::Running;
        assert_eq!(apply_notify(&mut t.session, &n, 2), Some(Cue::Done));
    }

    #[test]
    fn ignores_garbage_lines() {
        let mut buf = LineBuffer::default();
        let lines = buf.feed(b"{\"a\":1}\r\n\n  \npartial");
        assert_eq!(lines, vec!["{\"a\":1}".to_string()]);
        assert_eq!(buf.pending_len(), 7);
        let mut t = CodexTracker::new("x", 0);
        assert_eq!(t.apply_line("not json", 0), None);
        assert_eq!(
            t.apply_line(r#"{"record_type":"response","index":0}"#, 0),
            None
        );
    }
}
