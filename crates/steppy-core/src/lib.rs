//! 步步 Steppy 的纯逻辑核心。
//!
//! 这里不依赖任何 GUI，也不做网络 IO，所有模块都可以在无界面的 CI 机器上单元测试：
//!
//! * [`agents`]：Claude Code hook payload / transcript 解析，Codex rollout / notify 解析，会话状态归约。
//! * [`install`]：把宠物的 hook 合并进 `~/.claude/settings.json` 和 `~/.codex/config.toml`，以及撤销。
//! * [`calendar`]：日程模型、版本向量合并（纯函数）、重复规则展开、提醒计算、.ics 导入导出。
//! * [`net`]：家庭组、SPAKE2 配对、带身份认证的会话握手、AEAD 帧加密与防重放、线上消息格式。
//! * [`handoff`]：宠物串门的交接状态机，保证同一时刻只在一台机器上。
//! * [`pet`]：宠物在屏幕底边行走的运动逻辑。
//! * [`quiet`]：免打扰时段判断。

pub mod agents;
pub mod calendar;
pub mod handoff;
pub mod holiday;
pub mod install;
pub mod net;
pub mod pet;
pub mod quiet;

/// 当前毫秒时间戳（UTC）。
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
