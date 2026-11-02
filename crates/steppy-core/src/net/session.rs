//! 成员之间的会话：三次握手 + AEAD 帧。
//!
//! ```text
//! 发起方 I                                        响应方 R
//!   Hello1 { group_id, epoch, I, eph_I, nonce_I } →
//!                                                 ← Hello2 { R, eph_R, nonce_R, sig_R(T), mac_R(T) }
//!   Hello3 { sig_I(T), mac_I(T) }                 →
//! T = SHA256("steppy-hs-v1" ‖ group_id ‖ epoch ‖ I ‖ eph_I ‖ nonce_I ‖ R ‖ eph_R ‖ nonce_R)
//! 会话密钥 = HKDF(salt = 组密钥[epoch], ikm = X25519(eph_I, eph_R), info = T) → (I→R, R→I)
//! ```
//!
//! * 签名证明“我是成员名单里的这台机器”（被踢出的机器无法冒充其他成员）。
//! * MAC 证明“我持有这个 epoch 的组密钥”（不知道组密钥的机器什么都做不了）。
//! * 每次连接的临时 DH 密钥和随机数都是新的，旧会话的帧在新会话里无法解密（防跨会话重放）。
//! * 帧格式：`[8 字节计数器][密文+16 字节标签]`，nonce = 0x00000000 ‖ 计数器，AAD = 计数器。
//!   接收方要求计数器严格等于期望值，重放、乱序、丢帧都会被拒绝。

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x25519_dalek::{EphemeralSecret, PublicKey};

use super::group::GroupState;
use super::identity::{verify, Identity};
use super::{hkdf32, hmac, hmac_verify, random_bytes, NodeId, PROTOCOL_VERSION};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello1 {
    pub v: u8,
    pub group_id: String,
    pub epoch: u32,
    pub node_id: NodeId,
    pub eph: String,
    pub nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello2 {
    pub node_id: NodeId,
    pub eph: String,
    pub nonce: String,
    pub sig: String,
    pub mac: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello3 {
    pub sig: String,
    pub mac: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HandshakeError {
    #[error("协议版本不兼容")]
    Version,
    #[error("不是同一个家庭组")]
    WrongGroup,
    #[error("没有这个 epoch 的组密钥")]
    UnknownEpoch,
    #[error("对方不是组成员")]
    NotMember,
    #[error("签名无效")]
    BadSignature,
    #[error("组密钥校验失败")]
    BadMac,
    #[error("消息格式错误")]
    Malformed,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("帧太短")]
    Short,
    #[error("重放或乱序的帧（期望 {expected}，收到 {got}）")]
    Replay { expected: u64, got: u64 },
    #[error("认证失败：帧被篡改或密钥错误")]
    Auth,
}

fn transcript(h1: &Hello1, r_node: &str, r_eph: &str, r_nonce: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"steppy-hs-v1");
    for part in [
        h1.group_id.as_bytes(),
        &h1.epoch.to_be_bytes(),
        h1.node_id.as_bytes(),
        h1.eph.as_bytes(),
        h1.nonce.as_bytes(),
        r_node.as_bytes(),
        r_eph.as_bytes(),
        r_nonce.as_bytes(),
    ] {
        h.update((part.len() as u32).to_be_bytes());
        h.update(part);
    }
    h.finalize().into()
}

fn auth_key(group_key: &[u8; 32], epoch: u32) -> [u8; 32] {
    hkdf32(None, group_key, &[b"steppy-auth".as_slice(), &epoch.to_be_bytes()].concat())
}

fn decode32(s: &str) -> Result<[u8; 32], HandshakeError> {
    hex::decode(s).ok().and_then(|v| v.try_into().ok()).ok_or(HandshakeError::Malformed)
}

fn derive_keys(group_key: &[u8; 32], shared: &[u8; 32], t: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let i2r = hkdf32(Some(group_key), shared, &[b"steppy-i2r".as_slice(), t].concat());
    let r2i = hkdf32(Some(group_key), shared, &[b"steppy-r2i".as_slice(), t].concat());
    (i2r, r2i)
}

/// 单向加密状态。
pub struct CipherState {
    cipher: ChaCha20Poly1305,
    counter: u64,
}

impl CipherState {
    pub fn new(key: [u8; 32]) -> Self {
        CipherState { cipher: ChaCha20Poly1305::new(Key::from_slice(&key)), counter: 0 }
    }

    fn nonce(counter: u64) -> [u8; 12] {
        let mut n = [0u8; 12];
        n[4..].copy_from_slice(&counter.to_be_bytes());
        n
    }

    pub fn seal(&mut self, plaintext: &[u8]) -> Vec<u8> {
        let c = self.counter;
        self.counter += 1;
        let aad = c.to_be_bytes();
        let ct = self
            .cipher
            .encrypt(Nonce::from_slice(&Self::nonce(c)), Payload { msg: plaintext, aad: &aad })
            .expect("encrypt");
        let mut out = Vec::with_capacity(8 + ct.len());
        out.extend_from_slice(&aad);
        out.extend_from_slice(&ct);
        out
    }

    pub fn open(&mut self, frame: &[u8]) -> Result<Vec<u8>, FrameError> {
        if frame.len() < 8 + 16 {
            return Err(FrameError::Short);
        }
        let got = u64::from_be_bytes(frame[..8].try_into().unwrap());
        if got != self.counter {
            return Err(FrameError::Replay { expected: self.counter, got });
        }
        let pt = self
            .cipher
            .decrypt(Nonce::from_slice(&Self::nonce(got)), Payload { msg: &frame[8..], aad: &frame[..8] })
            .map_err(|_| FrameError::Auth)?;
        self.counter += 1;
        Ok(pt)
    }
}

/// 建立好的会话。
pub struct Session {
    pub peer: NodeId,
    pub epoch: u32,
    pub send: CipherState,
    pub recv: CipherState,
}

/// 发起方的中间状态。
pub struct Initiator {
    hello1: Hello1,
    eph: EphemeralSecret,
}

impl Initiator {
    /// 用指定 epoch（默认当前）发起握手。
    pub fn start(me: &Identity, group: &GroupState, epoch: Option<u32>) -> (Initiator, Hello1) {
        let eph = EphemeralSecret::random_from_rng(rand::rngs::OsRng);
        let eph_pub = PublicKey::from(&eph);
        let hello1 = Hello1 {
            v: PROTOCOL_VERSION,
            group_id: group.group_id.clone(),
            epoch: epoch.unwrap_or_else(|| group.epoch()),
            node_id: me.node_id.clone(),
            eph: hex::encode(eph_pub.as_bytes()),
            nonce: hex::encode(random_bytes::<32>()),
        };
        (Initiator { hello1: hello1.clone(), eph }, hello1)
    }

    pub fn finish(self, me: &Identity, group: &GroupState, h2: &Hello2) -> Result<(Hello3, Session), HandshakeError> {
        let gk = group.key_for(self.hello1.epoch).ok_or(HandshakeError::UnknownEpoch)?.bytes();
        let peer = group.members.get(&h2.node_id).filter(|m| !m.removed).ok_or(HandshakeError::NotMember)?;
        let t = transcript(&self.hello1, &h2.node_id, &h2.eph, &h2.nonce);
        let ak = auth_key(&gk, self.hello1.epoch);
        let mac = hex::decode(&h2.mac).map_err(|_| HandshakeError::Malformed)?;
        if !hmac_verify(&ak, &[b"R", &t], &mac) {
            return Err(HandshakeError::BadMac);
        }
        let sig = hex::decode(&h2.sig).map_err(|_| HandshakeError::Malformed)?;
        if !verify(&peer.pubkey, &peer.node_id, &[b"R".as_slice(), &t].concat(), &sig) {
            return Err(HandshakeError::BadSignature);
        }
        let r_eph = PublicKey::from(decode32(&h2.eph)?);
        let shared = self.eph.diffie_hellman(&r_eph);
        let (i2r, r2i) = derive_keys(&gk, shared.as_bytes(), &t);
        let h3 = Hello3 {
            sig: hex::encode(me.sign(&[b"I".as_slice(), &t].concat())),
            mac: hex::encode(hmac(&ak, &[b"I", &t])),
        };
        Ok((
            h3,
            Session { peer: h2.node_id.clone(), epoch: self.hello1.epoch, send: CipherState::new(i2r), recv: CipherState::new(r2i) },
        ))
    }
}

/// 响应方的中间状态。
pub struct Responder {
    t: [u8; 32],
    peer: NodeId,
    epoch: u32,
    keys: ([u8; 32], [u8; 32]),
    ak: [u8; 32],
}

impl Responder {
    pub fn respond(me: &Identity, group: &GroupState, h1: &Hello1) -> Result<(Responder, Hello2), HandshakeError> {
        if h1.v != PROTOCOL_VERSION {
            return Err(HandshakeError::Version);
        }
        if h1.group_id != group.group_id {
            return Err(HandshakeError::WrongGroup);
        }
        if !group.is_active_member(&h1.node_id) || h1.node_id == me.node_id {
            return Err(HandshakeError::NotMember);
        }
        let gk = group.key_for(h1.epoch).ok_or(HandshakeError::UnknownEpoch)?.bytes();
        let eph = EphemeralSecret::random_from_rng(rand::rngs::OsRng);
        let eph_pub = hex::encode(PublicKey::from(&eph).as_bytes());
        let nonce = hex::encode(random_bytes::<32>());
        let t = transcript(h1, &me.node_id, &eph_pub, &nonce);
        let ak = auth_key(&gk, h1.epoch);
        let i_eph = PublicKey::from(decode32(&h1.eph)?);
        let shared = eph.diffie_hellman(&i_eph);
        let keys = derive_keys(&gk, shared.as_bytes(), &t);
        let h2 = Hello2 {
            node_id: me.node_id.clone(),
            eph: eph_pub,
            nonce,
            sig: hex::encode(me.sign(&[b"R".as_slice(), &t].concat())),
            mac: hex::encode(hmac(&ak, &[b"R", &t])),
        };
        Ok((Responder { t, peer: h1.node_id.clone(), epoch: h1.epoch, keys, ak }, h2))
    }

    pub fn finish(self, group: &GroupState, h3: &Hello3) -> Result<Session, HandshakeError> {
        let peer = group.members.get(&self.peer).filter(|m| !m.removed).ok_or(HandshakeError::NotMember)?;
        let mac = hex::decode(&h3.mac).map_err(|_| HandshakeError::Malformed)?;
        if !hmac_verify(&self.ak, &[b"I", &self.t], &mac) {
            return Err(HandshakeError::BadMac);
        }
        let sig = hex::decode(&h3.sig).map_err(|_| HandshakeError::Malformed)?;
        if !verify(&peer.pubkey, &peer.node_id, &[b"I".as_slice(), &self.t].concat(), &sig) {
            return Err(HandshakeError::BadSignature);
        }
        Ok(Session {
            peer: self.peer,
            epoch: self.epoch,
            send: CipherState::new(self.keys.1),
            recv: CipherState::new(self.keys.0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::group::Member;
    use super::*;

    fn member(id: &Identity) -> Member {
        Member {
            node_id: id.node_id.clone(),
            pubkey: id.public_hex(),
            machine_name: "m".into(),
            owner_name: "o".into(),
            hue: 0,
            info_version: 1,
            joined_ms: 0,
            removed: false,
        }
    }

    fn setup() -> (Identity, Identity, GroupState) {
        let a = Identity::generate();
        let b = Identity::generate();
        let mut g = GroupState::create(&a, "A", "甲", 0, 0);
        g.add_member(member(&b));
        (a, b, g)
    }

    fn handshake(a: &Identity, ga: &GroupState, b: &Identity, gb: &GroupState) -> Result<(Session, Session), HandshakeError> {
        let (init, h1) = Initiator::start(a, ga, None);
        let (resp, h2) = Responder::respond(b, gb, &h1)?;
        let (h3, sa) = init.finish(a, ga, &h2)?;
        let sb = resp.finish(gb, &h3)?;
        Ok((sa, sb))
    }

    #[test]
    fn handshake_and_bidirectional_frames() {
        let (a, b, g) = setup();
        let (mut sa, mut sb) = handshake(&a, &g, &b, &g).unwrap();
        assert_eq!(sa.peer, b.node_id);
        assert_eq!(sb.peer, a.node_id);
        for i in 0..5 {
            let f = sa.send.seal(format!("a->b {i}").as_bytes());
            assert_eq!(sb.recv.open(&f).unwrap(), format!("a->b {i}").as_bytes());
            let f = sb.send.seal(b"b->a");
            assert_eq!(sa.recv.open(&f).unwrap(), b"b->a");
        }
    }

    #[test]
    fn tampered_frames_are_rejected() {
        let (a, b, g) = setup();
        let (mut sa, mut sb) = handshake(&a, &g, &b, &g).unwrap();
        let mut f = sa.send.seal(b"secret schedule");
        let last = f.len() - 1;
        f[last] ^= 1;
        assert_eq!(sb.recv.open(&f), Err(FrameError::Auth));
        // 改计数器（AAD）也会失败。
        let mut f2 = sa.send.seal(b"x");
        f2[7] ^= 1;
        assert!(sb.recv.open(&f2).is_err());
        // 篡改密文中间字节。
        let mut sa2 = handshake(&a, &g, &b, &g).unwrap();
        let mut f3 = sa2.0.send.seal(b"hello world");
        f3[10] ^= 0x80;
        assert_eq!(sa2.1.recv.open(&f3), Err(FrameError::Auth));
        assert_eq!(sb.recv.open(&[0u8; 5]), Err(FrameError::Short));
    }

    #[test]
    fn replayed_frames_are_rejected() {
        let (a, b, g) = setup();
        let (mut sa, mut sb) = handshake(&a, &g, &b, &g).unwrap();
        let f1 = sa.send.seal(b"one");
        let f2 = sa.send.seal(b"two");
        assert!(sb.recv.open(&f1).is_ok());
        assert_eq!(sb.recv.open(&f1), Err(FrameError::Replay { expected: 1, got: 0 }));
        assert!(sb.recv.open(&f2).is_ok());
        // 旧会话的帧不能在新会话里重放。
        let (_, mut sb_new) = handshake(&a, &g, &b, &g).unwrap();
        assert!(sb_new.recv.open(&f1).is_err());
        // 跳过帧（乱序）也被拒绝。
        let _f3 = sa.send.seal(b"three");
        let f4 = sa.send.seal(b"four");
        assert!(matches!(sb.recv.open(&f4), Err(FrameError::Replay { .. })));
    }

    #[test]
    fn wrong_group_key_is_rejected() {
        let (a, b, g) = setup();
        // b 手里的组密钥被替换了（比如被踢出后用旧状态，或伪造者）。
        let mut forged = g.clone();
        forged.keys.last_mut().unwrap().key = hex::encode([7u8; 32]);
        let (init, h1) = Initiator::start(&a, &g, None);
        let (_resp, h2) = Responder::respond(&b, &forged, &h1).unwrap();
        assert_eq!(init.finish(&a, &g, &h2).err(), Some(HandshakeError::BadMac));
        // 反过来，发起方密钥错误：响应方在第三步拒绝。
        let (init, h1) = Initiator::start(&a, &forged, None);
        let (resp, h2) = Responder::respond(&b, &g, &h1).unwrap();
        // 发起方先发现响应方的 MAC 不对。
        assert_eq!(init.finish(&a, &forged, &h2).err(), Some(HandshakeError::BadMac));
        // 构造一个用错误密钥算出的 Hello3。
        let bogus = Hello3 { sig: hex::encode(a.sign(b"whatever")), mac: hex::encode([0u8; 32]) };
        assert_eq!(resp.finish(&g, &bogus).err(), Some(HandshakeError::BadMac));
    }

    #[test]
    fn frames_with_wrong_session_key_fail() {
        let (a, b, g) = setup();
        let (mut sa, _) = handshake(&a, &g, &b, &g).unwrap();
        let mut stranger = CipherState::new([9u8; 32]);
        let f = sa.send.seal(b"hi");
        assert_eq!(stranger.open(&f), Err(FrameError::Auth));
    }

    #[test]
    fn non_members_and_kicked_members_are_rejected() {
        let (a, b, mut g) = setup();
        let outsider = Identity::generate();
        let (_, h1) = Initiator::start(&outsider, &g, None);
        assert_eq!(Responder::respond(&b, &g, &h1).err(), Some(HandshakeError::NotMember));

        // 踢出 b 并轮换密钥。b 还拿着旧状态。
        let b_old_state = g.clone();
        g.remove_member(&b.node_id, &a.node_id);
        // b 用旧 epoch 发起：成员检查失败。
        let (_, h1) = Initiator::start(&b, &b_old_state, None);
        assert_eq!(Responder::respond(&a, &g, &h1).err(), Some(HandshakeError::NotMember));
        // b 冒充 a 的 node id 用旧密钥连接第三台机器：签名对不上。
        let c = Identity::generate();
        let mut g3 = b_old_state.clone();
        g3.add_member(member(&c));
        let (mut init, mut h1) = Initiator::start(&b, &g3, None);
        h1.node_id = a.node_id.clone();
        init.hello1.node_id = a.node_id.clone();
        let (resp, h2) = Responder::respond(&c, &g3, &h1).unwrap();
        let (h3, _) = init.finish(&b, &g3, &h2).unwrap();
        assert_eq!(resp.finish(&g3, &h3).err(), Some(HandshakeError::BadSignature));
    }

    #[test]
    fn old_epoch_still_accepted_for_catch_up() {
        let (a, b, mut g) = setup();
        let c = Identity::generate();
        g.add_member(member(&c));
        let b_view = g.clone(); // b 离线期间错过了轮换
        g.remove_member(&c.node_id, &a.node_id);
        assert_eq!(g.epoch(), 1);
        // b 用 epoch 0 连接 a：a 仍保留旧密钥，允许建立会话以便把新状态发给 b。
        let (sa, sb) = handshake(&b, &b_view, &a, &g).unwrap();
        assert_eq!(sa.epoch, 0);
        assert_eq!(sb.epoch, 0);
        // 未知 epoch：拒绝。
        let (_, mut h1) = Initiator::start(&b, &b_view, None);
        h1.epoch = 99;
        assert_eq!(Responder::respond(&a, &g, &h1).err(), Some(HandshakeError::UnknownEpoch));
        // 不同组：拒绝。
        let other = GroupState::create(&b, "B", "乙", 0, 0);
        let (_, h1) = Initiator::start(&b, &other, None);
        assert_eq!(Responder::respond(&a, &g, &h1).err(), Some(HandshakeError::WrongGroup));
    }
}
