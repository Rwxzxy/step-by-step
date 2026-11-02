//! steppy-hook：步步的极小 hook 命令。
//!
//! 用法：
//!
//! ```text
//! steppy-hook claude --endpoint <endpoint.json>          # Claude Code hook：stdin 是事件 JSON
//! steppy-hook codex-notify --endpoint <endpoint.json> [--then <原 notify 命令...>] <JSON>
//! ```
//!
//! 设计要求：
//! * 永远以退出码 0 结束、不向 stdout 输出任何东西（Claude Code 会解析 hook 的 stdout）。
//! * 宠物没运行时立刻退出：连接 127.0.0.1 被拒绝是瞬间的；连接超时 300 毫秒，整体最多约 2 秒。
//! * 用 endpoint 文件里的随机 token 鉴权，token 不出现在命令行参数里。

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
const IO_TIMEOUT: Duration = Duration::from_millis(1500);
const MAX_STDIN: u64 = 16 * 1024 * 1024;

fn default_endpoint() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library").join("Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
    };
    base.map(|b| b.join("steppy").join("endpoint.json"))
}

struct Endpoint {
    port: u16,
    token: String,
}

fn read_endpoint(path: &PathBuf) -> Option<Endpoint> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(Endpoint {
        port: v.get("port")?.as_u64()? as u16,
        token: v.get("token")?.as_str()?.to_string(),
    })
}

/// 把 body POST 给宠物。任何失败都静默返回。
fn post(ep: &Endpoint, path: &str, body: &[u8]) {
    let addr = SocketAddr::from(([127, 0, 0, 1], ep.port));
    let Ok(mut s) = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) else { return };
    let _ = s.set_write_timeout(Some(IO_TIMEOUT));
    let _ = s.set_read_timeout(Some(IO_TIMEOUT));
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        ep.token,
        body.len()
    );
    if s.write_all(head.as_bytes()).is_err() || s.write_all(body).is_err() {
        return;
    }
    let _ = s.flush();
    // 等到服务器确认收到（读状态行）即可。
    let mut buf = [0u8; 64];
    let _ = s.read(&mut buf);
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

fn run(args: Vec<String>) {
    let mode = args.first().map(|s| s.as_str()).unwrap_or("claude");
    let endpoint_path = arg_value(&args, "--endpoint").map(PathBuf::from).or_else(default_endpoint);
    let endpoint = endpoint_path.as_ref().and_then(read_endpoint);

    match mode {
        "codex-notify" => {
            // Codex 把 JSON 作为最后一个参数追加。
            let Some(json) = args.last().filter(|a| a.trim_start().starts_with('{')).cloned() else { return };
            if let Some(ep) = &endpoint {
                post(ep, "/v1/codex/notify", json.as_bytes());
            }
            // 串联用户原来的 notify 命令。
            if let Some(i) = args.iter().position(|a| a == "--then") {
                let chain: Vec<&String> = args[i + 1..args.len() - 1].iter().collect();
                if let Some((prog, rest)) = chain.split_first() {
                    let _ = Command::new(prog)
                        .args(rest)
                        .arg(&json)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn();
                }
            }
        }
        _ => {
            // Claude Code：先尽快读完 stdin，避免对方写管道时阻塞。
            let mut body = Vec::new();
            let _ = std::io::stdin().take(MAX_STDIN).read_to_end(&mut body);
            if body.is_empty() {
                return;
            }
            if let Some(ep) = &endpoint {
                post(ep, "/v1/claude", &body);
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    run(args);
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::Instant;

    fn write_endpoint(port: u16) -> (tempfile_dir::Dir, PathBuf) {
        let dir = tempfile_dir::Dir::new();
        let p = dir.path.join("endpoint.json");
        std::fs::write(&p, format!("{{\"port\":{port},\"token\":\"tok123\"}}")).unwrap();
        (dir, p)
    }

    /// 不依赖外部 crate 的临时目录。
    mod tempfile_dir {
        pub struct Dir {
            pub path: std::path::PathBuf,
        }
        impl Dir {
            pub fn new() -> Self {
                let path = std::env::temp_dir().join(format!(
                    "steppy-hook-test-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
                ));
                std::fs::create_dir_all(&path).unwrap();
                Dir { path }
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }
    }

    #[test]
    fn exits_fast_when_pet_not_running() {
        // 找一个空闲端口然后关掉，保证没有人在监听。
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let (_d, p) = write_endpoint(port);
        let ep = read_endpoint(&p).unwrap();
        let t = Instant::now();
        post(&ep, "/v1/claude", b"{}");
        assert!(t.elapsed() < Duration::from_millis(500), "{:?}", t.elapsed());
    }

    #[test]
    fn missing_endpoint_file_is_harmless() {
        assert!(read_endpoint(&PathBuf::from("/definitely/not/here.json")).is_none());
        run(vec!["codex-notify".into(), "--endpoint".into(), "/nope.json".into(), "not json".into()]);
    }

    #[test]
    fn sends_authorized_request() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (_d, p) = write_endpoint(port);
        let handle = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut req = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = s.read(&mut buf).unwrap();
                req.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&req);
                if let Some(i) = text.find("\r\n\r\n") {
                    if text.len() >= i + 4 + 9 {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            s.write_all(b"HTTP/1.1 204 No Content\r\n\r\n").unwrap();
            String::from_utf8(req).unwrap()
        });
        let ep = read_endpoint(&p).unwrap();
        post(&ep, "/v1/codex/notify", br#"{"a":1234}"#);
        let req = handle.join().unwrap();
        assert!(req.starts_with("POST /v1/codex/notify HTTP/1.1\r\n"));
        assert!(req.contains("Authorization: Bearer tok123\r\n"));
        assert!(req.ends_with(r#"{"a":1234}"#));
    }

    #[test]
    fn arg_parsing() {
        let args: Vec<String> = ["codex-notify", "--endpoint", "/x.json", "--then", "notify-send", "Codex", "{\"type\":1}"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(arg_value(&args, "--endpoint").as_deref(), Some("/x.json"));
        assert!(default_endpoint().is_some() || std::env::var_os("HOME").is_none());
    }
}
