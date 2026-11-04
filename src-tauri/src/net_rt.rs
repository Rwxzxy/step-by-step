//! 局域网：mDNS 发现、TCP 连接、握手、加密会话、配对、消息分发。

use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::{Deserialize, Serialize};
use steppy_core::net::group::Member;
use steppy_core::net::identity::node_id_from_pubkey_hex;
use steppy_core::net::pairing::{self, Host, Joiner, P1, P2, P3, P4};
use steppy_core::net::session::{CipherState, Hello1, Hello2, Hello3, Initiator, Responder};
use steppy_core::net::wire::WireMsg;
use steppy_core::net::{GroupState, NodeId, MAX_FRAME, MDNS_SERVICE};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Notify};

use crate::state::{Discovered, PairingWindow, PeerEntry, Shared};
use crate::{agents_rt, calendar_rt, notify, pet_rt, storage};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(8);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const IDLE_TIMEOUT: Duration = Duration::from_secs(45);
const PING_EVERY: Duration = Duration::from_secs(15);

struct Conn {
    id: u64,
    tx: mpsc::UnboundedSender<WireMsg>,
    kill: Arc<Notify>,
    initiated_by_me: bool,
    epoch: u32,
}

#[derive(Default)]
struct NetInner {
    conns: Mutex<HashMap<NodeId, Conn>>,
    next_id: AtomicU64,
    port: AtomicU16,
    mdns: Mutex<Option<ServiceDaemon>>,
    known_addrs: Mutex<BTreeMap<NodeId, Vec<SocketAddr>>>,
}

/// 网络层句柄，可在任意线程使用。
#[derive(Clone, Default)]
pub struct NetHandle {
    inner: Arc<NetInner>,
}

impl NetHandle {
    pub fn new() -> Self {
        let known: BTreeMap<NodeId, Vec<SocketAddr>> =
            storage::read_json(&storage::path("peers.json")).unwrap_or_default();
        let h = NetHandle::default();
        *h.inner.known_addrs.lock().unwrap() = known;
        h
    }

    pub fn send(&self, peer: &str, msg: WireMsg) {
        if let Some(c) = self.inner.conns.lock().unwrap().get(peer) {
            let _ = c.tx.send(msg);
        }
    }

    pub fn connected_peers(&self) -> Vec<NodeId> {
        self.inner.conns.lock().unwrap().keys().cloned().collect()
    }

    pub fn is_connected(&self, peer: &str) -> bool {
        self.inner.conns.lock().unwrap().contains_key(peer)
    }

    pub fn disconnect(&self, peer: &str) {
        if let Some(c) = self.inner.conns.lock().unwrap().remove(peer) {
            c.kill.notify_waiters();
            c.kill.notify_one();
        }
    }

    pub fn disconnect_all(&self) {
        let conns: Vec<Conn> = self
            .inner
            .conns
            .lock()
            .unwrap()
            .drain()
            .map(|(_, c)| c)
            .collect();
        for c in conns {
            c.kill.notify_waiters();
            c.kill.notify_one();
        }
    }

    /// 断开 epoch 比当前旧的会话，让对方用新组密钥重连。
    pub fn drop_stale_epochs(&self, current: u32) {
        let stale: Vec<NodeId> = self
            .inner
            .conns
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, c)| c.epoch < current)
            .map(|(k, _)| k.clone())
            .collect();
        for p in stale {
            self.disconnect(&p);
        }
    }

    pub fn port(&self) -> u16 {
        self.inner.port.load(Ordering::Relaxed)
    }

    fn remember_addr(&self, peer: &str, addr: SocketAddr) {
        let mut k = self.inner.known_addrs.lock().unwrap();
        let list = k.entry(peer.to_string()).or_default();
        list.retain(|a| a != &addr);
        list.insert(0, addr);
        list.truncate(4);
        let _ = storage::write_json(&storage::path("peers.json"), &*k, false);
    }
}

// ─────────────────────────── 帧 IO ───────────────────────────

async fn write_frame(s: &mut (impl AsyncWriteExt + Unpin), body: &[u8]) -> std::io::Result<()> {
    s.write_all(&(body.len() as u32).to_be_bytes()).await?;
    s.write_all(body).await?;
    s.flush().await
}

async fn read_frame(s: &mut (impl AsyncReadExt + Unpin)) -> std::io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len).await?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut buf = vec![0u8; len];
    s.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn send_json<T: Serialize>(s: &mut TcpStream, v: &T) -> anyhow::Result<()> {
    write_frame(s, &serde_json::to_vec(v)?).await?;
    Ok(())
}

async fn recv_json<T: for<'de> Deserialize<'de>>(
    s: &mut TcpStream,
    timeout: Duration,
) -> anyhow::Result<T> {
    let bytes = tokio::time::timeout(timeout, read_frame(s)).await??;
    Ok(serde_json::from_slice(&bytes)?)
}

/// 连接的第一帧。
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Opening {
    Session { hello: Hello1 },
    Pair { p1: P1 },
}

/// 配对的应答（成员 → 新机器）。
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PairReply {
    P2 { p2: P2 },
    Closed { reason: String },
}

// ─────────────────────────── 启动 ───────────────────────────

pub fn start(state: Shared) {
    tauri::async_runtime::spawn(async move {
        let listener = bind(&state).await;
        let Some(listener) = listener else {
            log::error!("无法监听局域网端口，联机功能不可用");
            return;
        };
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        state.net.inner.port.store(port, Ordering::Relaxed);
        log::info!("局域网监听端口 {port}");
        start_mdns(&state, port);
        let s2 = state.clone();
        tauri::async_runtime::spawn(async move { connector(s2).await });
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let s = state.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = handle_incoming(s, stream, addr).await {
                            log::debug!("来自 {addr} 的连接结束：{e}");
                        }
                    });
                }
                Err(e) => {
                    log::warn!("accept 失败：{e}");
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    });
}

async fn bind(state: &Shared) -> Option<TcpListener> {
    let base = state.settings().net_port;
    for p in base..base.saturating_add(10) {
        if let Ok(l) = TcpListener::bind(("0.0.0.0", p)).await {
            return Some(l);
        }
    }
    TcpListener::bind(("0.0.0.0", 0)).await.ok()
}

/// mDNS 广播内容：实例名是随机的 node id，TXT 里只有协议版本和两个标志位，
/// 不包含机器名、用户名或任何任务信息。
fn service_info(state: &Shared, port: u16) -> Option<ServiceInfo> {
    let me = state.me().clone();
    let member = if state.group.lock().unwrap().is_some() {
        "1"
    } else {
        "0"
    };
    let pairing = if state
        .pairing
        .lock()
        .unwrap()
        .as_ref()
        .map(|p| p.expires > Instant::now())
        .unwrap_or(false)
    {
        "1"
    } else {
        "0"
    };
    let props = [("v", "1"), ("m", member), ("p", pairing)];
    ServiceInfo::new(
        MDNS_SERVICE,
        &me,
        &format!("{me}.local."),
        "",
        port,
        &props[..],
    )
    .ok()
    .map(|i| i.enable_addr_auto())
}

pub fn refresh_mdns(state: &Shared) {
    let port = state.net.port();
    if port == 0 {
        return;
    }
    let info = service_info(state, port);
    if let (Some(d), Some(info)) = (state.net.inner.mdns.lock().unwrap().as_ref(), info) {
        if let Err(e) = d.register(info) {
            log::warn!("mDNS 注册失败：{e}");
        }
    }
}

fn start_mdns(state: &Shared, port: u16) {
    let daemon = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => {
            log::warn!("mDNS 不可用（可在设置里手动添加对方地址）：{e}");
            return;
        }
    };
    if let Some(info) = service_info(state, port) {
        if let Err(e) = daemon.register(info) {
            log::warn!("mDNS 注册失败：{e}");
        }
    }
    let receiver = match daemon.browse(MDNS_SERVICE) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("mDNS 浏览失败：{e}");
            return;
        }
    };
    *state.net.inner.mdns.lock().unwrap() = Some(daemon);
    let state = state.clone();
    std::thread::Builder::new()
        .name("mdns".into())
        .spawn(move || {
            while let Ok(ev) = receiver.recv() {
                match ev {
                    ServiceEvent::ServiceResolved(info) => {
                        let node = info
                            .get_fullname()
                            .split('.')
                            .next()
                            .unwrap_or("")
                            .to_string();
                        if node.is_empty() || &node == state.me() {
                            continue;
                        }
                        let port = info.get_port();
                        let mut addrs: Vec<SocketAddr> = info
                            .get_addresses()
                            .iter()
                            .filter(|ip| !ip.is_loopback())
                            .filter(|ip| match ip {
                                IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) != 0xfe80,
                                _ => true,
                            })
                            .map(|ip| SocketAddr::new(*ip, port))
                            .collect();
                        addrs.sort_by_key(|a| a.is_ipv6());
                        let pairing_open = info.get_property_val_str("p") == Some("1");
                        state.discovered.lock().unwrap().insert(
                            node,
                            Discovered {
                                addrs,
                                pairing_open,
                                seen: Instant::now(),
                            },
                        );
                        state.mark_dirty();
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => {
                        let node = fullname.split('.').next().unwrap_or("").to_string();
                        state.discovered.lock().unwrap().remove(&node);
                        state.mark_dirty();
                    }
                    _ => {}
                }
            }
        })
        .ok();
}

// ─────────────────────────── 主动连接 ───────────────────────────

async fn connector(state: Shared) {
    let mut last_manual = Instant::now() - Duration::from_secs(60);
    loop {
        let group = state.group.lock().unwrap().clone();
        if let Some(g) = group {
            let me = state.me().clone();
            let targets: Vec<NodeId> = g
                .active_members()
                .map(|m| m.node_id.clone())
                .filter(|n| n != &me && !state.net.is_connected(n))
                .collect();
            for node in targets {
                let mut addrs: Vec<SocketAddr> = state
                    .discovered
                    .lock()
                    .unwrap()
                    .get(&node)
                    .map(|d| d.addrs.clone())
                    .unwrap_or_default();
                for a in state
                    .net
                    .inner
                    .known_addrs
                    .lock()
                    .unwrap()
                    .get(&node)
                    .cloned()
                    .unwrap_or_default()
                {
                    if !addrs.contains(&a) {
                        addrs.push(a);
                    }
                }
                for addr in addrs {
                    if state.net.is_connected(&node) {
                        break;
                    }
                    let s = state.clone();
                    match tokio::time::timeout(
                        CONNECT_TIMEOUT + HANDSHAKE_TIMEOUT,
                        connect_session(s, addr),
                    )
                    .await
                    {
                        Ok(Ok(())) => break,
                        Ok(Err(e)) => log::debug!("连接 {node}@{addr} 失败：{e}"),
                        Err(_) => log::debug!("连接 {node}@{addr} 超时"),
                    }
                }
            }
            // 手动地址：不知道对方 node id，握手后才知道。
            if last_manual.elapsed() > Duration::from_secs(15) {
                last_manual = Instant::now();
                for a in state.settings().manual_peers {
                    let Some(addr) = parse_addr(&a, state.settings().net_port) else {
                        continue;
                    };
                    let known_connected = state
                        .net
                        .inner
                        .known_addrs
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|(n, list)| list.contains(&addr) && state.net.is_connected(n));
                    if known_connected {
                        continue;
                    }
                    let s = state.clone();
                    let _ = tokio::time::timeout(
                        CONNECT_TIMEOUT + HANDSHAKE_TIMEOUT,
                        connect_session(s, addr),
                    )
                    .await;
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

pub fn parse_addr(s: &str, default_port: u16) -> Option<SocketAddr> {
    let s = s.trim();
    if let Ok(a) = s.parse::<SocketAddr>() {
        return Some(a);
    }
    s.parse::<IpAddr>()
        .ok()
        .map(|ip| SocketAddr::new(ip, default_port))
}

async fn connect_session(state: Shared, addr: SocketAddr) -> anyhow::Result<()> {
    let group = state
        .group
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| anyhow::anyhow!("不在家庭组里"))?;
    // 先用当前 epoch，失败再依次用保存的旧 epoch（离线期间错过了换密钥）。
    let mut epochs: Vec<u32> = group.keys.iter().map(|k| k.epoch).collect();
    epochs.sort_unstable_by(|a, b| b.cmp(a));
    let mut last_err = anyhow::anyhow!("没有可用的组密钥");
    for epoch in epochs {
        let mut stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await??;
        stream.set_nodelay(true).ok();
        let (init, hello) = Initiator::start(&state.identity, &group, Some(epoch));
        send_json(&mut stream, &Opening::Session { hello }).await?;
        let h2: Hello2 = match recv_json(&mut stream, HANDSHAKE_TIMEOUT).await {
            Ok(h) => h,
            Err(e) => {
                last_err = e;
                continue;
            }
        };
        match init.finish(&state.identity, &group, &h2) {
            Ok((h3, session)) => {
                send_json(&mut stream, &h3).await?;
                state.net.remember_addr(&session.peer, addr);
                // 连接本身在独立的任务里运行：调用方给“连接 + 握手”设了超时，
                // 如果把会话循环也放在超时里，超时一到读取就会被取消（写入却还在继续）。
                tauri::async_runtime::spawn(run_connection(
                    state,
                    stream,
                    session.peer,
                    session.epoch,
                    true,
                    session.send,
                    session.recv,
                    Some(addr),
                ));
                return Ok(());
            }
            Err(e) => last_err = anyhow::anyhow!("握手失败：{e}"),
        }
    }
    Err(last_err)
}

// ─────────────────────────── 被动连接 ───────────────────────────

async fn handle_incoming(
    state: Shared,
    mut stream: TcpStream,
    addr: SocketAddr,
) -> anyhow::Result<()> {
    stream.set_nodelay(true).ok();
    let opening: Opening = recv_json(&mut stream, HANDSHAKE_TIMEOUT).await?;
    match opening {
        Opening::Session { hello } => {
            let group = state
                .group
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| anyhow::anyhow!("不在家庭组里"))?;
            // 握手失败时直接断开，不回复任何信息（未配对的机器什么都看不到）。
            let (resp, h2) = Responder::respond(&state.identity, &group, &hello)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            send_json(&mut stream, &h2).await?;
            let h3: Hello3 = recv_json(&mut stream, HANDSHAKE_TIMEOUT).await?;
            let session = resp
                .finish(&group, &h3)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            run_connection(
                state,
                stream,
                session.peer,
                session.epoch,
                false,
                session.send,
                session.recv,
                Some(addr),
            )
            .await;
            Ok(())
        }
        Opening::Pair { p1 } => host_pairing(state, stream, p1).await,
    }
}

// ─────────────────────────── 会话 ───────────────────────────

#[allow(clippy::too_many_arguments)]
async fn run_connection(
    state: Shared,
    stream: TcpStream,
    peer: NodeId,
    epoch: u32,
    initiated_by_me: bool,
    mut send: CipherState,
    mut recv: CipherState,
    _addr: Option<SocketAddr>,
) {
    let me = state.me().clone();
    let (tx, mut rx) = mpsc::unbounded_channel::<WireMsg>();
    let kill = Arc::new(Notify::new());
    let id = state.net.inner.next_id.fetch_add(1, Ordering::Relaxed);
    // 两边同时连接时，只保留“node id 较小的一方发起”的那条。
    let preferred = initiated_by_me == (me < peer);
    {
        let mut conns = state.net.inner.conns.lock().unwrap();
        if let Some(existing) = conns.get(&peer) {
            let existing_preferred = existing.initiated_by_me == (me < peer);
            if existing_preferred && !preferred {
                return;
            }
            existing.kill.notify_one();
        }
        conns.insert(
            peer.clone(),
            Conn {
                id,
                tx: tx.clone(),
                kill: kill.clone(),
                initiated_by_me,
                epoch,
            },
        );
    }
    on_connected(&state, &peer, epoch);

    let (mut rd, mut wr) = stream.into_split();
    let kill_w = kill.clone();
    let writer = tokio::spawn(async move {
        let mut ping = tokio::time::interval(PING_EVERY);
        loop {
            tokio::select! {
                m = rx.recv() => {
                    let Some(m) = m else { break };
                    let frame = send.seal(&m.encode());
                    log::trace!("写入第 {id} 号连接 {} 字节", frame.len());
                    if write_frame(&mut wr, &frame).await.is_err() { break; }
                    log::trace!("写完第 {id} 号连接");
                }
                _ = ping.tick() => {
                    let frame = send.seal(&WireMsg::Ping.encode());
                    if write_frame(&mut wr, &frame).await.is_err() { break; }
                }
                _ = kill_w.notified() => break,
            }
        }
        let _ = wr.shutdown().await;
    });

    loop {
        tokio::select! {
            r = tokio::time::timeout(IDLE_TIMEOUT, read_frame(&mut rd)) => {
                let frame = match r {
                    Ok(Ok(f)) => f,
                    other => {
                        log::debug!("第 {id} 号连接读取结束：{:?}", other.map(|x| x.map(|f| f.len())));
                        break;
                    }
                };
                log::trace!("第 {id} 号连接读到 {} 字节", frame.len());
                let plain = match recv.open(&frame) {
                    Ok(p) => p,
                    Err(e) => {
                        log::warn!("来自 {peer} 的帧被拒绝：{e}");
                        break;
                    }
                };
                match WireMsg::decode(&plain) {
                    Some(m) => on_wire(&state, &peer, m),
                    None => log::debug!("无法解析来自 {peer} 的消息"),
                }
            }
            _ = kill.notified() => break,
        }
    }
    kill.notify_one();
    let _ = writer.await;
    let removed = {
        let mut conns = state.net.inner.conns.lock().unwrap();
        if conns.get(&peer).map(|c| c.id) == Some(id) {
            conns.remove(&peer);
            true
        } else {
            false
        }
    };
    if removed {
        on_disconnected(&state, &peer);
    }
}

fn on_connected(state: &Shared, peer: &str, epoch: u32) {
    log::info!("已连接 {peer}（epoch {epoch}）");
    // 同一台机器可能先后建立两条连接（双方同时发起），不要用新连接清掉已收到的资料。
    state
        .peers
        .lock()
        .unwrap()
        .entry(peer.to_string())
        .or_insert(PeerEntry { info: None });
    let mut info = state.peer_info();
    info.shares_content = state.settings().shares_content_with(peer);
    state.net.send(peer, WireMsg::Hello(info));
    if let Some(g) = state.group.lock().unwrap().clone() {
        state.net.send(peer, WireMsg::Group { state: g.clone() });
        if epoch < g.epoch() {
            // 对方还在用旧密钥：发完新的组状态后断开，让它用新密钥重连。
            let s = state.clone();
            let p = peer.to_string();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                s.net.disconnect(&p);
            });
        }
    }
    state.net.send(peer, agents_rt::snapshot_for(state, peer));
    state.net.send(
        peer,
        WireMsg::Calendar {
            records: calendar_rt::all_records(state),
        },
    );
    pet_rt::on_peer_connected(state, peer);
    state.mark_dirty();
}

fn on_disconnected(state: &Shared, peer: &str) {
    log::info!("与 {peer} 断开");
    state.peers.lock().unwrap().remove(peer);
    pet_rt::on_peer_disconnected(state, peer);
    state.mark_dirty();
}

fn on_wire(state: &Shared, peer: &str, msg: WireMsg) {
    if log::log_enabled!(log::Level::Trace) {
        let kind = format!("{msg:?}");
        log::trace!("收到 {peer}：{}", kind.chars().take(60).collect::<String>());
    }
    match msg {
        WireMsg::Hello(info) => {
            if let Some(p) = state.peers.lock().unwrap().get_mut(peer) {
                p.info = Some(info);
            }
            state.mark_dirty();
        }
        WireMsg::AgentSnapshot { sessions } => agents_rt::on_remote_snapshot(state, peer, sessions),
        WireMsg::AgentUpsert { session } => agents_rt::on_remote_upsert(state, peer, session),
        WireMsg::AgentRemove { key } => agents_rt::on_remote_remove(state, peer, &key),
        WireMsg::Calendar { records } => calendar_rt::on_remote(state, peer, records),
        WireMsg::Group { state: incoming } => on_group(state, peer, incoming),
        WireMsg::Kicked => on_kicked(state, peer),
        WireMsg::Pet(m) => pet_rt::on_pet_msg(state, peer, m),
        WireMsg::Ping => state.net.send(peer, WireMsg::Pong),
        WireMsg::Pong => {}
    }
}

/// 合并组状态，必要时轮换密钥并转发。
fn on_group(state: &Shared, from: &str, incoming: GroupState) {
    let me = state.me().clone();
    let (changed, removed_me, epoch, snapshot) = {
        let mut guard = state.group.lock().unwrap();
        let Some(g) = guard.as_mut() else { return };
        let before_epoch = g.epoch();
        let mut changed = g.merge(&incoming);
        if g.needs_rekey() && g.is_active_member(&me) {
            g.rekey();
            changed = true;
        }
        let removed_me = !g.is_active_member(&me);
        let _ = before_epoch;
        (changed, removed_me, g.epoch(), g.clone())
    };
    if removed_me {
        leave_locally(state, "你已被移出家庭组");
        return;
    }
    if changed {
        state.save_group();
        for p in state.net.connected_peers() {
            if p != from || snapshot != incoming {
                state.net.send(
                    &p,
                    WireMsg::Group {
                        state: snapshot.clone(),
                    },
                );
            }
        }
        // 断开已被移除的成员和旧 epoch 的连接。
        for p in state.net.connected_peers() {
            if !snapshot.is_active_member(&p) {
                state.net.disconnect(&p);
            }
        }
        let s = state.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            s.net.drop_stale_epochs(epoch);
        });
        state.mark_dirty();
    }
}

fn on_kicked(state: &Shared, from: &str) {
    let legit = state
        .group
        .lock()
        .unwrap()
        .as_ref()
        .map(|g| g.is_active_member(from))
        .unwrap_or(false);
    if legit {
        leave_locally(state, "你已被移出家庭组");
    }
}

/// 清除本机的组状态。
pub fn leave_locally(state: &Shared, reason: &str) {
    *state.group.lock().unwrap() = None;
    state.save_group();
    state.net.disconnect_all();
    state.agents.lock().unwrap().remote.clear();
    pet_rt::on_left_group(state);
    refresh_mdns(state);
    notify::info(state, reason);
    state.emit("steppy://toast", reason.to_string());
    state.mark_dirty();
}

/// 广播组状态给所有在线成员。
pub fn broadcast_group(state: &Shared) {
    if let Some(g) = state.group.lock().unwrap().clone() {
        for p in state.net.connected_peers() {
            state.net.send(&p, WireMsg::Group { state: g.clone() });
        }
    }
}

// ─────────────────────────── 家庭组操作 ───────────────────────────

pub fn create_group(state: &Shared) -> anyhow::Result<()> {
    let mut g = state.group.lock().unwrap();
    if g.is_some() {
        anyhow::bail!("已经在家庭组里了");
    }
    let s = state.settings();
    *g = Some(GroupState::create(
        &state.identity,
        &s.machine_name,
        &s.owner_name,
        s.pet_hue,
        steppy_core::now_ms(),
    ));
    drop(g);
    state.save_group();
    refresh_mdns(state);
    state.mark_dirty();
    Ok(())
}

pub fn start_pairing(state: &Shared) -> anyhow::Result<String> {
    if state.group.lock().unwrap().is_none() {
        create_group(state)?;
    }
    let code = steppy_core::net::new_pairing_code();
    *state.pairing.lock().unwrap() = Some(PairingWindow {
        code: code.clone(),
        expires: Instant::now() + Duration::from_millis(pairing::CODE_TTL_MS as u64),
        failures: 0,
    });
    refresh_mdns(state);
    state.mark_dirty();
    let s = state.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(pairing::CODE_TTL_MS as u64 + 500)).await;
        let expired = s
            .pairing
            .lock()
            .unwrap()
            .as_ref()
            .map(|p| p.expires <= Instant::now())
            .unwrap_or(false);
        if expired {
            cancel_pairing(&s);
        }
    });
    Ok(code)
}

pub fn cancel_pairing(state: &Shared) {
    *state.pairing.lock().unwrap() = None;
    refresh_mdns(state);
    state.mark_dirty();
}

pub fn kick(state: &Shared, node: &str) -> anyhow::Result<()> {
    let me = state.me().clone();
    if node == me {
        anyhow::bail!("不能踢出自己，请使用“退出家庭组”");
    }
    // 先通知被踢的机器，再轮换密钥。
    state.net.send(node, WireMsg::Kicked);
    {
        let mut g = state.group.lock().unwrap();
        let g = g.as_mut().ok_or_else(|| anyhow::anyhow!("不在家庭组里"))?;
        g.remove_member(node, &me)
            .ok_or_else(|| anyhow::anyhow!("没有这个成员"))?;
    }
    state.save_group();
    let s = state.clone();
    let n = node.to_string();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        s.net.disconnect(&n);
    });
    broadcast_group(state);
    state.agents.lock().unwrap().remote.remove(node);
    pet_rt::on_member_removed(state, node);
    state.mark_dirty();
    Ok(())
}

pub fn leave_group(state: &Shared) -> anyhow::Result<()> {
    let me = state.me().clone();
    {
        let mut g = state.group.lock().unwrap();
        let Some(g) = g.as_mut() else {
            anyhow::bail!("不在家庭组里")
        };
        if let Some(m) = g.members.get_mut(&me) {
            m.removed = true;
        }
    }
    // 告诉其他成员我走了（他们会因此换密钥）。
    broadcast_group(state);
    let s = state.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(800)).await;
        leave_locally(&s, "已退出家庭组");
    });
    Ok(())
}

pub fn set_layout(state: &Shared, order: Vec<NodeId>) -> anyhow::Result<()> {
    let me = state.me().clone();
    {
        let mut g = state.group.lock().unwrap();
        let g = g.as_mut().ok_or_else(|| anyhow::anyhow!("不在家庭组里"))?;
        g.set_layout(order, &me);
    }
    state.save_group();
    broadcast_group(state);
    state.mark_dirty();
    Ok(())
}

/// 本机资料变化后更新组里的成员信息。
pub fn update_my_info(state: &Shared) {
    let s = state.settings();
    let me = state.me().clone();
    let changed = state
        .group
        .lock()
        .unwrap()
        .as_mut()
        .map(|g| g.update_member_info(&me, &s.machine_name, &s.owner_name, s.pet_hue))
        .unwrap_or(false);
    if changed {
        state.save_group();
        broadcast_group(state);
    }
    for p in state.net.connected_peers() {
        let mut info = state.peer_info();
        info.shares_content = s.shares_content_with(&p);
        state.net.send(&p, WireMsg::Hello(info));
        // 隐私设置可能变了：重发会话快照。
        state.net.send(&p, agents_rt::snapshot_for(state, &p));
    }
}

// ─────────────────────────── 配对 ───────────────────────────

fn my_member(state: &Shared) -> Member {
    let s = state.settings();
    Member {
        node_id: state.me().clone(),
        pubkey: state.identity.public_hex(),
        machine_name: s.machine_name,
        owner_name: s.owner_name,
        hue: s.pet_hue,
        info_version: 1,
        joined_ms: steppy_core::now_ms(),
        removed: false,
    }
}

/// 成员一侧：处理新机器的配对请求。
async fn host_pairing(state: Shared, mut stream: TcpStream, p1: P1) -> anyhow::Result<()> {
    let code: Option<String> = {
        let p = state.pairing.lock().unwrap();
        p.as_ref()
            .filter(|w| w.expires > Instant::now())
            .map(|w| w.code.clone())
    };
    let Some(code) = code else {
        send_json(
            &mut stream,
            &PairReply::Closed {
                reason: "对方没有在配对".into(),
            },
        )
        .await
        .ok();
        anyhow::bail!("未开放配对");
    };
    let (host, p2) = Host::on_p1(&code, &p1).map_err(|e| anyhow::anyhow!("{e}"))?;
    send_json(&mut stream, &PairReply::P2 { p2 }).await?;
    let p3: P3 = recv_json(&mut stream, Duration::from_secs(15)).await?;
    let req = match host.on_p3(&p3) {
        Ok(r) => r,
        Err(e) => {
            let mut close = false;
            if let Some(w) = state.pairing.lock().unwrap().as_mut() {
                w.failures += 1;
                close = w.failures >= pairing::MAX_ATTEMPTS;
            }
            if close {
                cancel_pairing(&state);
                notify::info(&state, "配对码输错次数太多，已停止配对");
            }
            anyhow::bail!("配对失败：{e}");
        }
    };
    // 检查公钥和 node id 对应。
    if node_id_from_pubkey_hex(&req.member.pubkey).as_deref() != Some(req.member.node_id.as_str()) {
        anyhow::bail!("新机器的身份信息无效");
    }
    let mut member = req.member.clone();
    member.removed = false;
    let group = {
        let mut g = state.group.lock().unwrap();
        let g = g.as_mut().ok_or_else(|| anyhow::anyhow!("不在家庭组里"))?;
        if g.members
            .get(&member.node_id)
            .map(|m| m.removed)
            .unwrap_or(false)
        {
            anyhow::bail!("这台机器曾被移出家庭组，需要重新安装后再加入");
        }
        g.add_member(member.clone());
        g.clone()
    };
    state.save_group();
    let p4 = host.p4(&pairing::JoinAccepted { group });
    send_json(&mut stream, &p4).await?;
    cancel_pairing(&state);
    broadcast_group(&state);
    notify::info(&state, &format!("「{}」加入了家庭组", member.machine_name));
    state.emit(
        "steppy://toast",
        format!("「{}」加入了家庭组", member.machine_name),
    );
    state.mark_dirty();
    Ok(())
}

/// 新机器一侧：输入配对码加入家庭组。
pub async fn join_group(state: Shared, code: String) -> anyhow::Result<String> {
    let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
    if code.len() != 6 {
        anyhow::bail!("配对码是 6 位数字");
    }
    if state.group.lock().unwrap().is_some() {
        anyhow::bail!("这台机器已经在一个家庭组里，请先退出");
    }
    let mut candidates: Vec<SocketAddr> = state
        .discovered
        .lock()
        .unwrap()
        .values()
        .filter(|d| d.pairing_open && d.seen.elapsed() < Duration::from_secs(600))
        .flat_map(|d| d.addrs.clone())
        .collect();
    let port = state.settings().net_port;
    for a in state.settings().manual_peers {
        if let Some(addr) = parse_addr(&a, port) {
            if !candidates.contains(&addr) {
                candidates.push(addr);
            }
        }
    }
    if candidates.is_empty() {
        anyhow::bail!("没有找到正在显示配对码的设备。请先在已入组的电脑上点“添加新设备”，并确认两台电脑在同一局域网（或在设置里手动填写对方地址）。");
    }
    let me = my_member(&state);
    let mut last_err = String::from("连接失败");
    for addr in candidates {
        match try_join(&state, addr, &code, &me).await {
            Ok(group) => {
                let host_name = group
                    .active_members()
                    .find(|m| m.node_id != me.node_id)
                    .map(|m| m.machine_name.clone())
                    .unwrap_or_default();
                *state.group.lock().unwrap() = Some(group);
                state.save_group();
                refresh_mdns(&state);
                state.mark_dirty();
                return Ok(format!("已加入家庭组（通过「{host_name}」）"));
            }
            Err(e) => last_err = e.to_string(),
        }
    }
    anyhow::bail!(last_err)
}

async fn try_join(
    _state: &Shared,
    addr: SocketAddr,
    code: &str,
    me: &Member,
) -> anyhow::Result<GroupState> {
    let mut stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await??;
    let (joiner, p1) = Joiner::start(code);
    send_json(&mut stream, &Opening::Pair { p1 }).await?;
    let reply: PairReply = recv_json(&mut stream, Duration::from_secs(10)).await?;
    let p2 = match reply {
        PairReply::P2 { p2 } => p2,
        PairReply::Closed { reason } => anyhow::bail!(reason),
    };
    let (awaiting, p3) = joiner.on_p2(&p2, me).map_err(|e| anyhow::anyhow!("{e}"))?;
    send_json(&mut stream, &p3).await?;
    let p4: P4 = recv_json(&mut stream, Duration::from_secs(10)).await?;
    let accepted = awaiting.on_p4(&p4).map_err(|e| anyhow::anyhow!("{e}"))?;
    if !accepted.group.is_active_member(&me.node_id) {
        anyhow::bail!("对方没有把这台机器加入家庭组");
    }
    Ok(accepted.group)
}
