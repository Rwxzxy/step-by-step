//! 加密会话里传输的消息。

use serde::{Deserialize, Serialize};

use crate::agents::AgentSession;
use crate::calendar::EventRecord;
use crate::handoff::PetMsg;

use super::group::GroupState;

/// 连接建立后双方先交换的资料。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerInfo {
    pub machine_name: String,
    pub owner_name: String,
    pub hue: u16,
    pub app_version: String,
    /// 发送方是否会把消息正文 / 提示词发给接收方（接收方据此在界面上提示）。
    pub shares_content: bool,
    /// 发送方当前是否愿意接待来访的宠物。
    pub accepts_visitors: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "snake_case")]
pub enum WireMsg {
    Hello(PeerInfo),
    /// 发送方机器上的全部 Agent 会话（替换接收方保存的那台机器的会话列表）。
    AgentSnapshot { sessions: Vec<AgentSession> },
    AgentUpsert { session: AgentSession },
    AgentRemove { key: String },
    /// 日程记录（连接时发送全部，之后发送变化）。
    Calendar { records: Vec<EventRecord> },
    /// 家庭组状态（成员、组密钥、排列）。
    Group { state: GroupState },
    /// 你已被移出家庭组。
    Kicked,
    Pet(PetMsg),
    Ping,
    Pong,
}

impl WireMsg {
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serialize wire msg")
    }

    pub fn decode(bytes: &[u8]) -> Option<WireMsg> {
        serde_json::from_slice(bytes).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{AgentKind, AgentSession};

    #[test]
    fn roundtrip() {
        let msgs = vec![
            WireMsg::Ping,
            WireMsg::AgentUpsert { session: AgentSession::new(AgentKind::Codex, "s", "/p", 1) },
            WireMsg::Hello(PeerInfo {
                machine_name: "m".into(),
                owner_name: "o".into(),
                hue: 3,
                app_version: "1.0.0".into(),
                shares_content: false,
                accepts_visitors: true,
            }),
        ];
        for m in msgs {
            assert_eq!(WireMsg::decode(&m.encode()), Some(m));
        }
        assert_eq!(WireMsg::decode(b"{\"t\":\"nope\"}"), None);
    }
}
