//! 局域网联机：身份、家庭组、配对、会话加密、线上消息。
//!
//! 安全设计概要（详见 README 的“安全设计”一节）：
//!
//! * 每台机器有一个 Ed25519 身份密钥，node id 是公钥哈希的前 16 个十六进制字符。
//! * 家庭组有一把 32 字节的组密钥（按 epoch 轮换）和成员名单（含每个成员的公钥）。
//! * 新机器加入：已入组的机器显示 6 位配对码，双方用 SPAKE2 基于配对码协商出临时密钥，
//!   互相确认后，用它加密传递成员信息和组密钥。配对码错误只会导致确认失败，不泄露任何信息。
//! * 两台成员机器建立连接时做一次握手：双方交换 X25519 临时公钥和随机数，
//!   用身份密钥对握手记录签名、用组密钥做 HMAC，会话密钥 = HKDF(组密钥, DH 结果, 握手记录)。
//!   之后每帧都用 ChaCha20-Poly1305 加密并认证，帧计数器必须严格递增（防重放、防篡改）。
//! * 踢出成员时生成新的组密钥（epoch + 1），被踢的机器既没有新密钥，也不在成员名单里。

pub mod group;
pub mod identity;
pub mod pairing;
pub mod session;
pub mod wire;

use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;

pub use group::{GroupKey, GroupState, Member};
pub use identity::Identity;

pub type NodeId = String;

/// 协议版本。
pub const PROTOCOL_VERSION: u8 = 1;

/// mDNS 服务类型。
pub const MDNS_SERVICE: &str = "_steppy._tcp.local.";

/// 单帧最大长度。
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

pub(crate) fn hkdf32(salt: Option<&[u8]>, ikm: &[u8], info: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(salt, ikm);
    let mut out = [0u8; 32];
    hk.expand(info, &mut out).expect("hkdf length");
    out
}

pub(crate) fn hmac(key: &[u8], data: &[&[u8]]) -> [u8; 32] {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("hmac key");
    for d in data {
        m.update(d);
    }
    m.finalize().into_bytes().into()
}

pub(crate) fn hmac_verify(key: &[u8], data: &[&[u8]], tag: &[u8]) -> bool {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("hmac key");
    for d in data {
        m.update(d);
    }
    m.verify_slice(tag).is_ok()
}

pub(crate) fn random_bytes<const N: usize>() -> [u8; N] {
    use rand::RngCore;
    let mut b = [0u8; N];
    rand::rngs::OsRng.fill_bytes(&mut b);
    b
}

/// 生成 6 位数字配对码。
pub fn new_pairing_code() -> String {
    use rand::Rng;
    format!("{:06}", rand::rngs::OsRng.gen_range(0..1_000_000u32))
}

/// 读写长度前缀帧（4 字节大端长度 + 内容）。同步版本，供测试和简单场景使用。
pub mod framing {
    use std::io::{Read, Write};

    pub fn write_frame<W: Write>(w: &mut W, body: &[u8]) -> std::io::Result<()> {
        if body.len() > super::MAX_FRAME {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "frame too large"));
        }
        w.write_all(&(body.len() as u32).to_be_bytes())?;
        w.write_all(body)?;
        w.flush()
    }

    pub fn read_frame<R: Read>(r: &mut R) -> std::io::Result<Vec<u8>> {
        let mut len = [0u8; 4];
        r.read_exact(&mut len)?;
        let len = u32::from_be_bytes(len) as usize;
        if len > super::MAX_FRAME {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too large"));
        }
        let mut buf = vec![0u8; len];
        r.read_exact(&mut buf)?;
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_codes_are_six_digits() {
        for _ in 0..100 {
            let c = new_pairing_code();
            assert_eq!(c.len(), 6);
            assert!(c.chars().all(|c| c.is_ascii_digit()));
        }
    }

    #[test]
    fn framing_roundtrip_and_limit() {
        let mut buf = Vec::new();
        framing::write_frame(&mut buf, b"hello").unwrap();
        framing::write_frame(&mut buf, b"").unwrap();
        let mut r = std::io::Cursor::new(buf);
        assert_eq!(framing::read_frame(&mut r).unwrap(), b"hello");
        assert_eq!(framing::read_frame(&mut r).unwrap(), b"");
        let mut huge = Vec::new();
        huge.extend_from_slice(&(u32::MAX).to_be_bytes());
        assert!(framing::read_frame(&mut std::io::Cursor::new(huge)).is_err());
    }
}
