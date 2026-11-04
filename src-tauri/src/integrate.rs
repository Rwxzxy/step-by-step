//! 一键接入 / 撤销 Claude Code 和 Codex CLI。

use std::path::PathBuf;

use serde::Serialize;
use steppy_core::install::{self, HookCommand};

use crate::local_api::endpoint_path;
use crate::storage;

#[cfg(windows)]
const HOOK_FILE: &str = "steppy-hook.exe";
#[cfg(not(windows))]
const HOOK_FILE: &str = "steppy-hook";

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// `~/.claude/settings.json`（尊重 CLAUDE_CONFIG_DIR）。
pub fn claude_settings_path() -> PathBuf {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(d) if !d.is_empty() => PathBuf::from(d).join("settings.json"),
        _ => home().join(".claude").join("settings.json"),
    }
}

/// `~/.codex`（尊重 CODEX_HOME）。
pub fn codex_home() -> PathBuf {
    match std::env::var_os("CODEX_HOME") {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => home().join(".codex"),
    }
}

pub fn codex_config_path() -> PathBuf {
    codex_home().join("config.toml")
}

/// 安装包里随程序一起分发的 hook（和主程序在同一目录）。
fn bundled_hook() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidates = [dir.join(HOOK_FILE), dir.join("binaries").join(HOOK_FILE)];
    candidates.into_iter().find(|p| p.is_file())
}

/// hook 的固定安装位置：数据目录下的 bin/。
/// 不直接引用安装目录里的文件，是因为 AppImage 等运行时路径会变，升级时安装目录也可能变。
pub fn installed_hook() -> PathBuf {
    storage::data_dir().join("bin").join(HOOK_FILE)
}

/// 把 hook 复制到固定位置（内容有变化才复制）。
pub fn ensure_hook_binary() -> anyhow::Result<PathBuf> {
    let dst = installed_hook();
    let src = bundled_hook()
        .ok_or_else(|| anyhow::anyhow!("找不到随程序附带的 steppy-hook，请重新安装步步"))?;
    let need_copy = match (std::fs::read(&src), std::fs::read(&dst)) {
        (Ok(a), Ok(b)) => a != b,
        _ => true,
    };
    if need_copy {
        std::fs::create_dir_all(dst.parent().unwrap())?;
        let tmp = dst.with_extension("new");
        std::fs::copy(&src, &tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        }
        if let Err(e) = std::fs::rename(&tmp, &dst) {
            // Windows 上 hook 正在被执行时无法替换：沿用旧文件。
            let _ = std::fs::remove_file(&tmp);
            if !dst.exists() {
                return Err(e.into());
            }
            log::warn!("hook 正在使用，暂不更新：{e}");
        }
    }
    Ok(dst)
}

fn hook_command() -> anyhow::Result<HookCommand> {
    let program = ensure_hook_binary()?;
    Ok(HookCommand {
        program: program.to_string_lossy().to_string(),
        endpoint_file: endpoint_path().to_string_lossy().to_string(),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentIntegration {
    pub installed: bool,
    pub config_path: String,
    pub config_exists: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntegrationStatus {
    pub claude: AgentIntegration,
    pub codex: AgentIntegration,
    pub hook_path: String,
    pub hook_available: bool,
}

fn status_for(path: PathBuf, check: fn(&str) -> bool) -> AgentIntegration {
    let (text, error) = match std::fs::read_to_string(&path) {
        Ok(t) => (t, None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), None),
        Err(e) => (String::new(), Some(e.to_string())),
    };
    AgentIntegration {
        installed: check(&text),
        config_exists: path.exists(),
        config_path: path.to_string_lossy().to_string(),
        error,
    }
}

pub fn status() -> IntegrationStatus {
    IntegrationStatus {
        claude: status_for(claude_settings_path(), install::claude::is_installed),
        codex: status_for(codex_config_path(), install::codex::is_installed),
        hook_path: installed_hook().to_string_lossy().to_string(),
        hook_available: bundled_hook().is_some() || installed_hook().exists(),
    }
}

/// 接入或撤销。返回给用户看的结果说明。
pub fn set(agent: &str, enable: bool) -> anyhow::Result<String> {
    let (path, label) = match agent {
        "claude" => (claude_settings_path(), "Claude Code"),
        "codex" => (codex_config_path(), "Codex CLI"),
        _ => anyhow::bail!("未知的 Agent：{agent}"),
    };
    let result = if enable {
        let cmd = hook_command()?;
        match agent {
            "claude" => install::apply_to_file(&path, |t| install::claude::install(t, &cmd)),
            _ => install::apply_to_file(&path, |t| install::codex::install(t, &cmd)),
        }
    } else {
        match agent {
            "claude" => install::apply_to_file(&path, install::claude::uninstall),
            _ => install::apply_to_file(&path, install::codex::uninstall),
        }
    };
    let (changed, backup) = result.map_err(|e| anyhow::anyhow!("{e}"))?;
    let action = if enable { "已接入" } else { "已撤销" };
    let mut msg = if changed {
        format!("{label} {action}，修改了 {}。", path.display())
    } else {
        format!("{label} 无需修改（{action}）。")
    };
    if let Some(b) = backup {
        msg.push_str(&format!("修改前的配置已备份到 {}。", b.display()));
    }
    if enable && agent == "claude" {
        msg.push_str("已经打开的 Claude Code 会话需要重启（或在其中运行 /hooks 查看）后生效。");
    }
    if enable && agent == "codex" {
        msg.push_str("新启动的 Codex 会话生效；进度和计划会从 ~/.codex/sessions 自动读取。");
    }
    Ok(msg)
}

/// 启动时：已接入的话更新 hook 文件。
pub fn refresh_on_startup() {
    let st = status();
    if st.claude.installed || st.codex.installed {
        if let Err(e) = ensure_hook_binary() {
            log::warn!("更新 hook 失败：{e}");
        }
    }
}
