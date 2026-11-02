//! Agent（Claude Code / Codex CLI）会话的通用模型与状态归约。

pub mod claude;
pub mod codex;

use serde::{Deserialize, Serialize};

/// Agent 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Codex,
}

impl AgentKind {
    pub fn label(self) -> &'static str {
        match self {
            AgentKind::Claude => "Claude",
            AgentKind::Codex => "Codex",
        }
    }
}

/// 会话状态。面板上显示为：空闲 / 运行中 / 等我回复 / 完成 / 出错 / 已结束。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// 会话刚开始，还没有收到任务。
    Idle,
    /// 正在执行。
    Running,
    /// 在等我回复或者批准。
    Waiting,
    /// 这一轮完成了。
    Done,
    /// 出错了。
    Error,
    /// 会话已经结束（进程退出 / /clear 等）。
    Ended,
}

impl SessionStatus {
    pub fn label_zh(self) -> &'static str {
        match self {
            SessionStatus::Idle => "空闲",
            SessionStatus::Running => "运行中",
            SessionStatus::Waiting => "等我回复",
            SessionStatus::Done => "完成",
            SessionStatus::Error => "出错",
            SessionStatus::Ended => "已结束",
        }
    }
}

/// 计划条目的完成情况。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Pending,
    InProgress,
    Completed,
}

impl PlanStatus {
    /// 解析 Claude（TodoWrite / TaskUpdate）和 Codex（update_plan）里用到的状态字符串。
    pub fn parse(s: &str) -> Option<PlanStatus> {
        match s {
            "pending" => Some(PlanStatus::Pending),
            "in_progress" | "in-progress" | "inProgress" => Some(PlanStatus::InProgress),
            "completed" | "done" => Some(PlanStatus::Completed),
            _ => None,
        }
    }
}

/// 计划清单里的一项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanItem {
    /// Claude 任务工具的 task id；TodoWrite / Codex 计划没有 id。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub text: String,
    pub status: PlanStatus,
    /// 进行中时的描述（Claude 的 activeForm）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_text: Option<String>,
}

impl PlanItem {
    pub fn new(text: impl Into<String>, status: PlanStatus) -> Self {
        PlanItem {
            id: None,
            text: text.into(),
            status,
            active_text: None,
        }
    }
}

/// 一个 Agent 会话的当前状态。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSession {
    pub agent: AgentKind,
    pub session_id: String,
    /// 项目目录（会话的工作目录）。
    pub cwd: String,
    pub status: SessionStatus,
    /// 状态补充说明，例如“等待批准：Bash”“rate_limit”。不包含消息正文。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub plan: Vec<PlanItem>,
    pub started_ms: i64,
    pub updated_ms: i64,
    /// 会话标题（Claude 的 session_title）。视为内容，默认不同步。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// 最近一条用户消息 / 提示词。视为内容，默认不同步。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_prompt: Option<String>,
    /// 最近一条 Agent 回复。视为内容，默认不同步。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reply: Option<String>,
}

/// 最多保留多少字的消息摘录。
pub const EXCERPT_CHARS: usize = 400;

/// 截断文本到 `max` 个字符。
pub fn excerpt(s: &str, max: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(max).collect();
    out.push('…');
    out
}

impl AgentSession {
    pub fn new(
        agent: AgentKind,
        session_id: impl Into<String>,
        cwd: impl Into<String>,
        now: i64,
    ) -> Self {
        AgentSession {
            agent,
            session_id: session_id.into(),
            cwd: cwd.into(),
            status: SessionStatus::Idle,
            detail: None,
            plan: Vec::new(),
            started_ms: now,
            updated_ms: now,
            title: None,
            last_prompt: None,
            last_reply: None,
        }
    }

    /// 会话唯一键：`agent:session_id`。
    pub fn key(&self) -> String {
        format!("{}:{}", self.agent.label().to_lowercase(), self.session_id)
    }

    /// 项目名：工作目录最后一段。
    pub fn project_name(&self) -> String {
        project_name(&self.cwd)
    }

    /// 发给其他机器时去掉消息正文、提示词等内容。
    pub fn redacted(&self) -> AgentSession {
        let mut s = self.clone();
        s.title = None;
        s.last_prompt = None;
        s.last_reply = None;
        s
    }

    /// 修改状态，返回是否发生了变化。
    pub fn set_status(&mut self, status: SessionStatus, detail: Option<String>) -> bool {
        let changed = self.status != status;
        self.status = status;
        self.detail = detail;
        changed
    }

    /// 计划完成进度 (已完成, 总数)。
    pub fn plan_progress(&self) -> (usize, usize) {
        let done = self
            .plan
            .iter()
            .filter(|p| p.status == PlanStatus::Completed)
            .count();
        (done, self.plan.len())
    }
}

/// 从路径里取最后一段作为项目名，兼容 Windows 和 Unix 分隔符。
pub fn project_name(cwd: &str) -> String {
    let trimmed = cwd.trim_end_matches(['/', '\\']);
    let name = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    if name.is_empty() {
        cwd.to_string()
    } else {
        name.to_string()
    }
}

/// 状态变化时宠物要做的反应。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cue {
    /// 一轮完成：开心。
    Done,
    /// 等我回复 / 批准：举手。
    Waiting,
    /// 出错：着急。
    Error,
}

/// 根据状态转换决定宠物反应：只在“进入”某状态时触发一次。
pub fn cue_for_transition(before: SessionStatus, after: SessionStatus) -> Option<Cue> {
    if before == after {
        return None;
    }
    match after {
        SessionStatus::Done => Some(Cue::Done),
        SessionStatus::Waiting => Some(Cue::Waiting),
        SessionStatus::Error => Some(Cue::Error),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_name_handles_both_separators() {
        assert_eq!(project_name("/home/me/proj"), "proj");
        assert_eq!(project_name("/home/me/proj/"), "proj");
        assert_eq!(project_name(r"C:\Users\me\code\app"), "app");
        assert_eq!(project_name("/"), "/");
    }

    #[test]
    fn redaction_drops_content() {
        let mut s = AgentSession::new(AgentKind::Claude, "s1", "/p", 1);
        s.last_prompt = Some("secret prompt".into());
        s.last_reply = Some("secret reply".into());
        s.title = Some("title".into());
        s.plan.push(PlanItem::new("step", PlanStatus::Pending));
        let r = s.redacted();
        assert!(r.last_prompt.is_none() && r.last_reply.is_none() && r.title.is_none());
        assert_eq!(r.plan.len(), 1);
    }

    #[test]
    fn cues_only_on_transition() {
        assert_eq!(
            cue_for_transition(SessionStatus::Running, SessionStatus::Done),
            Some(Cue::Done)
        );
        assert_eq!(
            cue_for_transition(SessionStatus::Done, SessionStatus::Done),
            None
        );
        assert_eq!(
            cue_for_transition(SessionStatus::Running, SessionStatus::Waiting),
            Some(Cue::Waiting)
        );
        assert_eq!(
            cue_for_transition(SessionStatus::Running, SessionStatus::Error),
            Some(Cue::Error)
        );
        assert_eq!(
            cue_for_transition(SessionStatus::Idle, SessionStatus::Running),
            None
        );
    }

    #[test]
    fn excerpt_truncates_by_chars() {
        assert_eq!(excerpt("  你好世界  ", 10), "你好世界");
        assert_eq!(excerpt("你好世界", 2), "你好…");
    }
}
