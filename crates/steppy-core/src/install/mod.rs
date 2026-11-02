//! 一键接入 / 撤销：把宠物的 hook 合并进 Claude Code 和 Codex CLI 的用户配置。
//!
//! 这里只放纯函数（输入旧配置文本，输出新配置文本），文件读写和备份在 [`apply_to_file`]。
//! 所有合并都保留用户已有的 hooks / notify，撤销时只删除我们自己加的条目。

pub mod claude;
pub mod codex;

use std::path::{Path, PathBuf};

/// hook 命令。`program` 是 steppy-hook 可执行文件的绝对路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookCommand {
    pub program: String,
    /// hook 读取端口和 token 的 endpoint 文件路径。
    pub endpoint_file: String,
}

/// 用来识别“这是步步加的条目”的标记：可执行文件名。
pub const HOOK_BIN_STEM: &str = "steppy-hook";

/// 判断一个命令路径是不是步步的 hook。
pub fn is_our_program(cmd: &str) -> bool {
    let name = cmd.rsplit(['/', '\\']).next().unwrap_or(cmd);
    let name = name.trim_matches('"').to_ascii_lowercase();
    name == HOOK_BIN_STEM || name == format!("{HOOK_BIN_STEM}.exe")
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("配置文件不是合法的 JSON，为避免破坏它，没有修改：{0}")]
    BadJson(String),
    #[error("配置文件不是合法的 TOML，为避免破坏它，没有修改：{0}")]
    BadToml(String),
    #[error("配置结构不符合预期：{0}")]
    Shape(String),
    #[error("读写文件失败：{0}")]
    Io(#[from] std::io::Error),
}

/// 修改前备份：`<file>.steppy-backup-<时间>`。返回备份路径（文件不存在时为 None）。
pub fn backup_file(path: &Path) -> std::io::Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let backup = path.with_file_name(format!("{file_name}.steppy-backup-{stamp}"));
    std::fs::copy(path, &backup)?;
    Ok(Some(backup))
}

/// 读文件 → 变换 → 若有变化则先备份再写回。返回 (是否修改, 备份路径)。
pub fn apply_to_file(
    path: &Path,
    transform: impl FnOnce(&str) -> Result<String, InstallError>,
) -> Result<(bool, Option<PathBuf>), InstallError> {
    let old = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let new = transform(&old)?;
    if new == old {
        return Ok((false, None));
    }
    let backup = backup_file(path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 先写临时文件再改名，避免写一半。
    let tmp = path.with_extension("steppy-tmp");
    std::fs::write(&tmp, new.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok((true, backup))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_hook_program() {
        assert!(is_our_program(
            "/home/me/.local/share/steppy/bin/steppy-hook"
        ));
        assert!(is_our_program(
            r"C:\Users\Me Too\AppData\Local\Steppy\bin\steppy-hook.exe"
        ));
        assert!(is_our_program("\"C:\\x\\Steppy-Hook.EXE\""));
        assert!(!is_our_program("/usr/bin/notify-send"));
        assert!(!is_our_program("steppy-hooker"));
    }

    #[test]
    fn file_roundtrip_with_backup() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, "{}").unwrap();
        let (changed, backup) = apply_to_file(&p, |s| Ok(format!("{s} "))).unwrap();
        assert!(changed);
        let backup = backup.unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{}");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{} ");
        // 没变化就不写也不备份。
        let (changed, backup) = apply_to_file(&p, |s| Ok(s.to_string())).unwrap();
        assert!(!changed && backup.is_none());
        // 文件不存在时创建，不备份。
        let q = dir.path().join("sub/new.toml");
        let (changed, backup) = apply_to_file(&q, |_| Ok("a = 1\n".into())).unwrap();
        assert!(changed && backup.is_none());
        assert!(q.exists());
    }
}
