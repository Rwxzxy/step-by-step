//! 127.0.0.1 上的 hook 接口。
//!
//! * `POST /v1/claude`：Claude Code hook 的 stdin JSON。
//! * `POST /v1/codex/notify`：Codex notify 的 JSON。
//! * `GET /v1/ping`：诊断。
//!
//! 只监听回环地址，所有请求必须带 `Authorization: Bearer <token>`。token 在第一次运行时随机生成并保存
//! （Unix 上权限 0600）；程序运行期间把端口和 token 写进数据目录的 endpoint.json，hook 从那里读取，
//! token 不会出现在命令行参数里。程序退出时删除 endpoint.json。

use std::io::Read;
use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

use crate::agents_rt::AgentInput;
use crate::storage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointFile {
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

pub const DEFAULT_PORT: u16 = 47890;
const MAX_BODY: usize = 16 * 1024 * 1024;

pub fn endpoint_path() -> std::path::PathBuf {
    storage::path("endpoint.json")
}

/// token 每次安装生成一次，单独保存（0600）；endpoint.json 只在程序运行时存在。
fn load_or_create_token() -> String {
    let p = storage::path("hook-token");
    if let Ok(t) = std::fs::read_to_string(&p) {
        let t = t.trim().to_string();
        if t.len() >= 32 {
            return t;
        }
    }
    use rand::RngCore;
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    let t = hex::encode(b);
    if let Err(e) = storage::write_text(&p, &t, true) {
        log::error!("保存 hook token 失败：{e}");
    }
    t
}

/// 程序退出时删除 endpoint.json，hook 发现文件不存在就立即退出（不必尝试连接）。
pub fn remove_endpoint() {
    let _ = std::fs::remove_file(endpoint_path());
}

/// 启动接口线程。返回实际端口。
pub fn start(tx: Sender<AgentInput>, state: crate::state::Shared) -> anyhow::Result<u16> {
    let token = load_or_create_token();
    let server = tiny_http::Server::http(("127.0.0.1", DEFAULT_PORT))
        .or_else(|_| tiny_http::Server::http(("127.0.0.1", 0)))
        .map_err(|e| anyhow::anyhow!("无法监听本机端口：{e}"))?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .unwrap_or(DEFAULT_PORT);
    storage::write_json(
        &endpoint_path(),
        &EndpointFile {
            port,
            token: token.clone(),
            pid: std::process::id(),
        },
        true,
    )?;
    log::info!("hook 接口监听 127.0.0.1:{port}");
    std::thread::Builder::new()
        .name("local-api".into())
        .spawn(move || {
            for mut req in server.incoming_requests() {
                let authorized = req
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Authorization"))
                    .map(|h| {
                        let v = h.value.as_str();
                        let given = v.strip_prefix("Bearer ").unwrap_or("");
                        given.as_bytes().ct_eq(token.as_bytes()).into()
                    })
                    .unwrap_or(false);
                if !authorized {
                    let _ = req.respond(tiny_http::Response::empty(401));
                    continue;
                }
                let url = req.url().to_string();
                let method = req.method().clone();
                if url.starts_with("/v1/debug/") {
                    let mut body = String::new();
                    let _ = req
                        .as_reader()
                        .take(MAX_BODY as u64)
                        .read_to_string(&mut body);
                    let resp = crate::e2e::handle(&state, method.as_str(), &url, &body)
                        .unwrap_or((404, String::new()));
                    let _ = req
                        .respond(tiny_http::Response::from_string(resp.1).with_status_code(resp.0));
                    continue;
                }
                if method == tiny_http::Method::Get && url == "/v1/ping" {
                    let _ = req.respond(tiny_http::Response::from_string("{\"ok\":true}"));
                    continue;
                }
                if method != tiny_http::Method::Post {
                    let _ = req.respond(tiny_http::Response::empty(405));
                    continue;
                }
                let mut body = Vec::new();
                if req
                    .as_reader()
                    .take(MAX_BODY as u64 + 1)
                    .read_to_end(&mut body)
                    .is_err()
                    || body.len() > MAX_BODY
                {
                    let _ = req.respond(tiny_http::Response::empty(413));
                    continue;
                }
                let text = String::from_utf8_lossy(&body).to_string();
                let input = match url.as_str() {
                    "/v1/claude" => Some(AgentInput::Claude(text)),
                    "/v1/codex/notify" => Some(AgentInput::CodexNotify(text)),
                    _ => None,
                };
                match input {
                    Some(i) => {
                        // 先回复，hook 就可以立即退出。
                        let _ = req.respond(tiny_http::Response::empty(204));
                        let _ = tx.send(i);
                    }
                    None => {
                        let _ = req.respond(tiny_http::Response::empty(404));
                    }
                }
            }
        })?;
    Ok(port)
}
