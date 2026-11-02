//! 本机身份：Ed25519 密钥对。

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::NodeId;

#[derive(Clone)]
pub struct Identity {
    signing: SigningKey,
    pub node_id: NodeId,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("node_id", &self.node_id)
            .finish_non_exhaustive()
    }
}

/// 持久化格式。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityFile {
    pub secret_hex: String,
}

/// 由公钥计算 node id。
pub fn node_id_for(pubkey: &VerifyingKey) -> NodeId {
    hex::encode(&Sha256::digest(pubkey.as_bytes())[..8])
}

/// 由十六进制公钥计算 node id；公钥无效时返回 None。
pub fn node_id_from_pubkey_hex(pub_hex: &str) -> Option<NodeId> {
    let bytes: [u8; 32] = hex::decode(pub_hex).ok()?.try_into().ok()?;
    let vk = VerifyingKey::from_bytes(&bytes).ok()?;
    Some(node_id_for(&vk))
}

impl Identity {
    pub fn generate() -> Self {
        let signing = SigningKey::generate(&mut rand::rngs::OsRng);
        Self::from_signing(signing)
    }

    fn from_signing(signing: SigningKey) -> Self {
        let node_id = node_id_for(&signing.verifying_key());
        Identity { signing, node_id }
    }

    pub fn from_file(f: &IdentityFile) -> Option<Self> {
        let bytes: [u8; 32] = hex::decode(&f.secret_hex).ok()?.try_into().ok()?;
        Some(Self::from_signing(SigningKey::from_bytes(&bytes)))
    }

    pub fn to_file(&self) -> IdentityFile {
        IdentityFile {
            secret_hex: hex::encode(self.signing.to_bytes()),
        }
    }

    pub fn public_hex(&self) -> String {
        hex::encode(self.signing.verifying_key().as_bytes())
    }

    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.signing.sign(msg).to_bytes()
    }
}

/// 用十六进制公钥验证签名，同时确认公钥与 node id 对应。
pub fn verify(pub_hex: &str, node_id: &str, msg: &[u8], sig: &[u8]) -> bool {
    let Ok(bytes) = hex::decode(pub_hex) else {
        return false;
    };
    let Ok(arr): Result<[u8; 32], _> = bytes.try_into() else {
        return false;
    };
    let Ok(vk) = VerifyingKey::from_bytes(&arr) else {
        return false;
    };
    if node_id_for(&vk) != node_id {
        return false;
    }
    let Ok(sig_arr): Result<[u8; 64], _> = sig.try_into() else {
        return false;
    };
    vk.verify(msg, &Signature::from_bytes(&sig_arr)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_verify_and_persist() {
        let id = Identity::generate();
        assert_eq!(id.node_id.len(), 16);
        let sig = id.sign(b"hi");
        assert!(verify(&id.public_hex(), &id.node_id, b"hi", &sig));
        assert!(!verify(&id.public_hex(), &id.node_id, b"ho", &sig));
        let other = Identity::generate();
        // 公钥与 node id 不对应：拒绝。
        assert!(!verify(&other.public_hex(), &id.node_id, b"hi", &sig));
        let restored = Identity::from_file(&id.to_file()).unwrap();
        assert_eq!(restored.node_id, id.node_id);
        assert!(Identity::from_file(&IdentityFile {
            secret_hex: "zz".into()
        })
        .is_none());
        assert_eq!(
            node_id_from_pubkey_hex(&id.public_hex()),
            Some(id.node_id.clone())
        );
        assert_eq!(node_id_from_pubkey_hex("00"), None);
    }
}
