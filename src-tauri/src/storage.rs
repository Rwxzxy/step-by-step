//! 数据目录和 JSON 文件读写。
//!
//! 所有数据放在 `<本地数据目录>/steppy/`：
//! Windows `%LOCALAPPDATA%\steppy`，macOS `~/Library/Application Support/steppy`，
//! Linux `$XDG_DATA_HOME/steppy`（默认 `~/.local/share/steppy`）。
//! steppy-hook 在没有 `--endpoint` 参数时也按同样的规则找 endpoint.json。

use std::path::{Path, PathBuf};

use serde::{de::DeserializeOwned, Serialize};

pub fn data_dir() -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
    let dir = base.join("steppy");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn path(name: &str) -> PathBuf {
    data_dir().join(name)
}

pub fn read_json<T: DeserializeOwned>(p: &Path) -> Option<T> {
    let text = std::fs::read_to_string(p).ok()?;
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            log::warn!("无法解析 {}：{e}", p.display());
            None
        }
    }
}

/// 原子写入；`secret` 为 true 时在 Unix 上设置 0600 权限。
pub fn write_json<T: Serialize>(p: &Path, v: &T, secret: bool) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(v).map_err(std::io::Error::other)?;
    write_text(p, &text, secret)
}

pub fn write_text(p: &Path, text: &str, secret: bool) -> std::io::Result<()> {
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, text.as_bytes())?;
    #[cfg(unix)]
    if secret {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = secret;
    std::fs::rename(&tmp, p)
}
