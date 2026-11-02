//! 合并 / 撤销 `~/.codex/config.toml` 里的 `notify`。
//!
//! Codex 的 `notify` 只能有一个命令（argv 数组，Codex 会在末尾追加一个 JSON 参数）。
//! 如果用户已经配置了自己的 notify，我们不覆盖它，而是把它串在后面：
//!
//! ```toml
//! notify = ["<steppy-hook>", "codex-notify", "--endpoint", "<endpoint.json>", "--then", "notify-send", "Codex"]
//! ```
//!
//! steppy-hook 先把事件发给宠物，再原样调用 `--then` 之后的命令（同样追加 JSON）。
//! 撤销时把 `--then` 之后的部分还原成原来的 notify；没有原 notify 就删除这个键。

use toml_edit::{value, Array, DocumentMut, Item, Value};

use super::{is_our_program, HookCommand, InstallError};

pub const THEN_FLAG: &str = "--then";

fn parse(text: &str) -> Result<DocumentMut, InstallError> {
    text.parse::<DocumentMut>()
        .map_err(|e| InstallError::BadToml(e.to_string()))
}

/// 读出当前 notify（字符串数组）。
fn current_notify(doc: &DocumentMut) -> Result<Option<Vec<String>>, InstallError> {
    let Some(item) = doc.get("notify") else {
        return Ok(None);
    };
    let arr = item
        .as_array()
        .ok_or_else(|| InstallError::Shape("notify 不是字符串数组".into()))?;
    let mut out = Vec::new();
    for v in arr.iter() {
        out.push(
            v.as_str()
                .ok_or_else(|| InstallError::Shape("notify 里有非字符串元素".into()))?
                .to_string(),
        );
    }
    Ok(Some(out))
}

/// 如果 notify 是我们的，返回被串联的原命令（可能为空）。不是我们的返回 None。
pub fn chained_original(argv: &[String]) -> Option<Vec<String>> {
    let first = argv.first()?;
    if !is_our_program(first) {
        return None;
    }
    match argv.iter().position(|a| a == THEN_FLAG) {
        Some(i) => Some(argv[i + 1..].to_vec()),
        None => Some(Vec::new()),
    }
}

fn to_array(argv: &[String]) -> Array {
    let mut arr = Array::new();
    for a in argv {
        arr.push(a.as_str());
    }
    arr
}

/// 接入。
pub fn install(text: &str, cmd: &HookCommand) -> Result<String, InstallError> {
    let mut doc = parse(text)?;
    let original = match current_notify(&doc)? {
        None => Vec::new(),
        Some(argv) => chained_original(&argv).unwrap_or(argv),
    };
    let mut argv = vec![
        cmd.program.clone(),
        "codex-notify".to_string(),
        "--endpoint".to_string(),
        cmd.endpoint_file.clone(),
    ];
    if !original.is_empty() {
        argv.push(THEN_FLAG.to_string());
        argv.extend(original);
    }
    let new_value = Value::Array(to_array(&argv));
    match doc.get_mut("notify") {
        Some(item) => {
            // 保留原位置和注释。
            let decor = item.as_value().map(|v| v.decor().clone());
            let mut nv = new_value;
            if let Some(d) = decor {
                *nv.decor_mut() = d;
            }
            *item = Item::Value(nv);
        }
        None => {
            insert_top_level(&mut doc, new_value);
        }
    }
    Ok(doc.to_string())
}

/// 在根表最前面的键值区插入 notify（必须在任何 `[table]` 之前，否则会被当成子表的键）。
fn insert_top_level(doc: &mut DocumentMut, v: Value) {
    doc.insert("notify", value(v));
}

/// 撤销。
pub fn uninstall(text: &str) -> Result<String, InstallError> {
    let mut doc = parse(text)?;
    let Some(argv) = current_notify(&doc)? else {
        return Ok(text.to_string());
    };
    let Some(original) = chained_original(&argv) else {
        return Ok(text.to_string());
    };
    if original.is_empty() {
        doc.remove("notify");
    } else {
        let item = doc.get_mut("notify").unwrap();
        let decor = item.as_value().map(|v| v.decor().clone());
        let mut nv = Value::Array(to_array(&original));
        if let Some(d) = decor {
            *nv.decor_mut() = d;
        }
        *item = Item::Value(nv);
    }
    Ok(doc.to_string())
}

pub fn is_installed(text: &str) -> bool {
    let Ok(doc) = parse(text) else { return false };
    matches!(current_notify(&doc), Ok(Some(argv)) if chained_original(&argv).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd() -> HookCommand {
        HookCommand {
            program: "/home/me/.local/share/steppy/bin/steppy-hook".into(),
            endpoint_file: "/home/me/.local/share/steppy/endpoint.json".into(),
        }
    }

    const USER_CONFIG: &str = r#"# 我的 Codex 配置
model = "gpt-5.5-codex"
# 完成时弹通知
notify = ["notify-send", "Codex"]
approval_policy = "on-request"

[mcp_servers.docs]
command = "docs-mcp"
args = ["--port", "9000"]

[profiles.fast]
model = "gpt-5.5-mini"
"#;

    #[test]
    fn install_into_empty_config() {
        let out = install("", &cmd()).unwrap();
        let doc: DocumentMut = out.parse().unwrap();
        let argv: Vec<_> = doc["notify"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            argv,
            vec![
                cmd().program,
                "codex-notify".into(),
                "--endpoint".into(),
                cmd().endpoint_file
            ]
        );
        assert!(is_installed(&out));
        assert_eq!(uninstall(&out).unwrap().trim(), "");
    }

    #[test]
    fn install_chains_existing_notify_and_keeps_rest() {
        let out = install(USER_CONFIG, &cmd()).unwrap();
        assert!(out.contains("# 我的 Codex 配置"));
        assert!(out.contains("# 完成时弹通知"));
        assert!(out.contains("[mcp_servers.docs]"));
        let doc: DocumentMut = out.parse().unwrap();
        let argv: Vec<_> = doc["notify"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            &argv[4..],
            &["--then".to_string(), "notify-send".into(), "Codex".into()]
        );
        assert_eq!(doc["model"].as_str(), Some("gpt-5.5-codex"));
        assert_eq!(
            doc["mcp_servers"]["docs"]["command"].as_str(),
            Some("docs-mcp")
        );
        // notify 仍在顶层，不会掉进 [profiles.fast]。
        assert!(doc["profiles"]["fast"].get("notify").is_none());
    }

    #[test]
    fn install_twice_does_not_double_chain() {
        let once = install(USER_CONFIG, &cmd()).unwrap();
        let twice = install(&once, &cmd()).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn uninstall_restores_original_exactly() {
        let installed = install(USER_CONFIG, &cmd()).unwrap();
        let restored = uninstall(&installed).unwrap();
        assert_eq!(restored, USER_CONFIG);
        assert!(!is_installed(&restored));
    }

    #[test]
    fn new_key_goes_before_tables() {
        let cfg = "model = \"x\"\n\n[profiles.fast]\nmodel = \"y\"\n";
        let out = install(cfg, &cmd()).unwrap();
        let notify_pos = out.find("notify").unwrap();
        let table_pos = out.find("[profiles.fast]").unwrap();
        assert!(notify_pos < table_pos, "{out}");
        assert_eq!(uninstall(&out).unwrap(), cfg);
    }

    #[test]
    fn uninstall_leaves_foreign_notify_alone() {
        assert_eq!(uninstall(USER_CONFIG).unwrap(), USER_CONFIG);
    }

    #[test]
    fn rejects_bad_config() {
        assert!(matches!(
            install("model = ", &cmd()),
            Err(InstallError::BadToml(_))
        ));
        assert!(matches!(
            install("notify = \"oops\"", &cmd()),
            Err(InstallError::Shape(_))
        ));
    }

    #[test]
    fn chained_original_parsing() {
        let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            chained_original(&v(&["/a/steppy-hook", "codex-notify"])),
            Some(vec![])
        );
        assert_eq!(
            chained_original(&v(&["/a/steppy-hook", "codex-notify", "--then", "x", "y"])),
            Some(v(&["x", "y"]))
        );
        assert_eq!(chained_original(&v(&["notify-send"])), None);
    }
}
