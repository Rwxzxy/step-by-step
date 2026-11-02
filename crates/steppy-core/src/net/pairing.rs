//! 配对：用 6 位配对码跑 SPAKE2，安全地把新机器加进家庭组。
//!
//! ```text
//! 新机器 J（输入配对码）                         成员 M（显示配对码）
//!   P1 { spake_J }                             →
//!                                              ← P2 { spake_M, confirm_M }
//!   P3 { confirm_J, seal(JoinRequest) }        →
//!                                              ← P4 { seal(组状态，含组密钥) }
//! ```
//!
//! * SPAKE2 让双方在配对码相同时得到同一个密钥 K；旁听者拿不到 K，也无法离线暴力破解配对码。
//! * 冒充一方的攻击者每次连接只能猜一个配对码；成员端在 [`MAX_ATTEMPTS`] 次失败后作废这个配对码。
//! * confirm 是对完整握手记录的 HMAC，确认双方算出了同一个 K（即配对码一致）。
//! * 之后的数据用从 K 派生的 ChaCha20-Poly1305 密钥加密，两个方向各用一把密钥。

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use serde::{Deserialize, Serialize};
use spake2::{Ed25519Group, Identity as SpakeId, Password, Spake2};

use super::group::{GroupState, Member};
use super::{hkdf32, hmac, hmac_verify};

/// 每个配对码允许的失败次数。
pub const MAX_ATTEMPTS: u32 = 5;
/// 配对码有效期。
pub const CODE_TTL_MS: i64 = 5 * 60_000;

const ID_MEMBER: &[u8] = b"steppy-member";
const ID_JOINER: &[u8] = b"steppy-joiner";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct P1 {
    pub spake: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct P2 {
    pub spake: String,
    pub confirm: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct P3 {
    pub confirm: String,
    pub sealed: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct P4 {
    pub sealed: String,
}

/// 新机器在 P3 里告诉成员的自我介绍。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinRequest {
    pub member: Member,
}

/// 成员在 P4 里发给新机器的内容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinAccepted {
    pub group: GroupState,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PairError {
    #[error("配对码不正确")]
    WrongCode,
    #[error("消息格式错误")]
    Malformed,
    #[error("解密失败")]
    Decrypt,
}

struct Keys {
    confirm: [u8; 32],
    seal_j2m: [u8; 32],
    seal_m2j: [u8; 32],
}

fn derive(k: &[u8]) -> Keys {
    Keys {
        confirm: hkdf32(None, k, b"steppy-pair-confirm"),
        seal_j2m: hkdf32(None, k, b"steppy-pair-j2m"),
        seal_m2j: hkdf32(None, k, b"steppy-pair-m2j"),
    }
}

fn seal(key: &[u8; 32], plaintext: &[u8]) -> String {
    let c = ChaCha20Poly1305::new(Key::from_slice(key));
    // 每把密钥只用一次，固定 nonce 是安全的。
    let ct = c.encrypt(Nonce::from_slice(&[0u8; 12]), plaintext).expect("encrypt");
    hex::encode(ct)
}

fn open(key: &[u8; 32], sealed: &str) -> Result<Vec<u8>, PairError> {
    let ct = hex::decode(sealed).map_err(|_| PairError::Malformed)?;
    let c = ChaCha20Poly1305::new(Key::from_slice(key));
    c.decrypt(Nonce::from_slice(&[0u8; 12]), ct.as_slice()).map_err(|_| PairError::Decrypt)
}

fn spake(id_a: &[u8], id_b: &[u8], code: &str, as_a: bool) -> (Spake2<Ed25519Group>, Vec<u8>) {
    let pw = Password::new(code.trim().as_bytes());
    if as_a {
        Spake2::<Ed25519Group>::start_a(&pw, &SpakeId::new(id_a), &SpakeId::new(id_b))
    } else {
        Spake2::<Ed25519Group>::start_b(&pw, &SpakeId::new(id_a), &SpakeId::new(id_b))
    }
}

// ───────────── 新机器一侧 ─────────────

pub struct Joiner {
    state: Spake2<Ed25519Group>,
    msg: Vec<u8>,
}

pub struct JoinerAwaitingP4 {
    keys: Keys,
}

impl Joiner {
    pub fn start(code: &str) -> (Joiner, P1) {
        let (state, msg) = spake(ID_MEMBER, ID_JOINER, code, false);
        (Joiner { state, msg: msg.clone() }, P1 { spake: hex::encode(msg) })
    }

    pub fn on_p2(self, p2: &P2, me: &Member) -> Result<(JoinerAwaitingP4, P3), PairError> {
        let theirs = hex::decode(&p2.spake).map_err(|_| PairError::Malformed)?;
        let k = self.state.finish(&theirs).map_err(|_| PairError::WrongCode)?;
        let keys = derive(&k);
        let confirm = hex::decode(&p2.confirm).map_err(|_| PairError::Malformed)?;
        if !hmac_verify(&keys.confirm, &[b"M", &theirs, &self.msg], &confirm) {
            return Err(PairError::WrongCode);
        }
        let req = serde_json::to_vec(&JoinRequest { member: me.clone() }).expect("json");
        let p3 = P3 {
            confirm: hex::encode(hmac(&keys.confirm, &[b"J", &theirs, &self.msg])),
            sealed: seal(&keys.seal_j2m, &req),
        };
        Ok((JoinerAwaitingP4 { keys }, p3))
    }
}

impl JoinerAwaitingP4 {
    pub fn on_p4(self, p4: &P4) -> Result<JoinAccepted, PairError> {
        let pt = open(&self.keys.seal_m2j, &p4.sealed)?;
        serde_json::from_slice(&pt).map_err(|_| PairError::Malformed)
    }
}

// ───────────── 成员一侧 ─────────────

pub struct Host {
    keys: Keys,
    mine: Vec<u8>,
    theirs: Vec<u8>,
}

impl Host {
    /// 收到 P1 后计算自己的 SPAKE2 消息和 confirm。
    pub fn on_p1(code: &str, p1: &P1) -> Result<(Host, P2), PairError> {
        let theirs = hex::decode(&p1.spake).map_err(|_| PairError::Malformed)?;
        let (state, mine) = spake(ID_MEMBER, ID_JOINER, code, true);
        let k = state.finish(&theirs).map_err(|_| PairError::Malformed)?;
        let keys = derive(&k);
        let confirm = hex::encode(hmac(&keys.confirm, &[b"M", &mine, &theirs]));
        let p2 = P2 { spake: hex::encode(&mine), confirm };
        Ok((Host { keys, mine, theirs }, p2))
    }

    /// 校验新机器的 confirm 并解出它的自我介绍。配对码错误时返回 WrongCode（应计入失败次数）。
    pub fn on_p3(&self, p3: &P3) -> Result<JoinRequest, PairError> {
        let confirm = hex::decode(&p3.confirm).map_err(|_| PairError::Malformed)?;
        if !hmac_verify(&self.keys.confirm, &[b"J", &self.mine, &self.theirs], &confirm) {
            return Err(PairError::WrongCode);
        }
        let pt = open(&self.keys.seal_j2m, &p3.sealed)?;
        serde_json::from_slice(&pt).map_err(|_| PairError::Malformed)
    }

    pub fn p4(&self, accepted: &JoinAccepted) -> P4 {
        P4 { sealed: seal(&self.keys.seal_m2j, &serde_json::to_vec(accepted).expect("json")) }
    }
}

#[cfg(test)]
mod tests {
    use super::super::identity::Identity;
    use super::*;

    fn me_member(id: &Identity) -> Member {
        Member {
            node_id: id.node_id.clone(),
            pubkey: id.public_hex(),
            machine_name: "笔记本".into(),
            owner_name: "小明".into(),
            hue: 120,
            info_version: 1,
            joined_ms: 0,
            removed: false,
        }
    }

    #[test]
    fn successful_pairing_transfers_group_key() {
        let host_id = Identity::generate();
        let mut group = GroupState::create(&host_id, "台式机", "小明", 30, 0);
        let joiner_id = Identity::generate();

        let (joiner, p1) = Joiner::start("123456");
        let (host, p2) = Host::on_p1("123456", &p1).unwrap();
        let (awaiting, p3) = joiner.on_p2(&p2, &me_member(&joiner_id)).unwrap();
        let req = host.on_p3(&p3).unwrap();
        assert_eq!(req.member.node_id, joiner_id.node_id);
        group.add_member(req.member);
        let p4 = host.p4(&JoinAccepted { group: group.clone() });
        let acc = awaiting.on_p4(&p4).unwrap();
        assert_eq!(acc.group.current_key().key, group.current_key().key);
        assert!(acc.group.is_active_member(&joiner_id.node_id));
        // 传输中看不到明文组密钥。
        assert!(!p4.sealed.contains(&group.current_key().key));
    }

    #[test]
    fn wrong_code_is_detected_by_both_sides() {
        let joiner_id = Identity::generate();
        let (joiner, p1) = Joiner::start("111111");
        let (host, p2) = Host::on_p1("222222", &p1).unwrap();
        // 新机器发现成员的 confirm 不对。
        assert_eq!(joiner.on_p2(&p2, &me_member(&joiner_id)).err(), Some(PairError::WrongCode));
        // 一个不检查 confirm、直接乱发 P3 的攻击者也会被成员拒绝。
        let forged = P3 { confirm: hex::encode([0u8; 32]), sealed: "00".repeat(40) };
        assert_eq!(host.on_p3(&forged).err(), Some(PairError::WrongCode));
    }

    #[test]
    fn tampered_payload_rejected() {
        let joiner_id = Identity::generate();
        let (joiner, p1) = Joiner::start("654321");
        let (host, p2) = Host::on_p1("654321", &p1).unwrap();
        let (awaiting, mut p3) = joiner.on_p2(&p2, &me_member(&joiner_id)).unwrap();
        let mut bytes = hex::decode(&p3.sealed).unwrap();
        bytes[0] ^= 1;
        p3.sealed = hex::encode(bytes);
        assert_eq!(host.on_p3(&p3).err(), Some(PairError::Decrypt));
        let p4 = P4 { sealed: "00".repeat(32) };
        assert_eq!(awaiting.on_p4(&p4).err(), Some(PairError::Decrypt));
    }

    #[test]
    fn eavesdropper_cannot_decrypt_with_guessed_code() {
        // 旁听者拿到 P1/P2/P4，用另一个配对码自己跑 SPAKE2 也得不到同一个密钥。
        let host_id = Identity::generate();
        let group = GroupState::create(&host_id, "台式机", "小明", 30, 0);
        let joiner_id = Identity::generate();
        let (joiner, p1) = Joiner::start("424242");
        let (host, p2) = Host::on_p1("424242", &p1).unwrap();
        let (_awaiting, p3) = joiner.on_p2(&p2, &me_member(&joiner_id)).unwrap();
        host.on_p3(&p3).unwrap();
        let p4 = host.p4(&JoinAccepted { group });
        let (eve, _) = Joiner::start("000000");
        assert!(eve.on_p2(&p2, &me_member(&joiner_id)).is_err());
        let fake = JoinerAwaitingP4 { keys: derive(b"guess") };
        assert!(fake.on_p4(&p4).is_err());
    }
}
