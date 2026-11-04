`notify.json` 摘自 openai/codex 源码 `codex-rs/hooks/src/legacy_notify.rs` 的测试期望值。
rollout 样例按 `codex-rs/history/src/rollout_payload.rs`（`{timestamp,type,payload}` 外层）、
`codex-rs/protocol/src/protocol.rs`（`task_started` / `task_complete` / `user_message` / `agent_message`）
和 `codex-rs/protocol/src/plan_tool.rs`（`update_plan` 参数）构造。
