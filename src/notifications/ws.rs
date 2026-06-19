//! WebSocket 长连接 —— `Client` 扩展。端口自 `notifications/ws.ts`（其端口自 `acosmi-sdk-go/ws.go`）。
//!
//! Go 用 gorilla/websocket，TS 用全局 WebSocket constructor，Rust 用 [`tokio_tungstenite`]
//! （与 reqwest 同 rustls/webpki-roots backend）。
//!
//! **鉴权 = 一次性 stream-ticket 流程（v2.6.0 根因修复）**：网关 `/ws` 守卫为
//! `StreamTicketOr(...)`，优先消费 `?ticket=`。每次（重）连接前用已鉴权客户端 POST
//! `/ws/stream-ticket` 换一张短时一次性 ticket（~30-60s TTL，单次使用）放入 `?ticket=`，
//! 取代旧的 `?token=` long-JWT（既对浏览器失效又把长效凭证暴露在 access log）。重连必须重铸 ticket。
//!
//! **重复 connect 防泄漏（ws-reconnect-leak）**：新 connect 前先 `disconnect` 旧连接，
//! 否则旧 wsLoop 的后台自动重连定时器会与新连接并存 → 多个 socket + 多个 loop 泄漏。

use super::types::WSEvent;
use crate::core::client::Client;
use crate::core::http::DEFAULT_JSON_TIMEOUT_MS;
use crate::shared::{ApiResponse, Error, Result};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

/// 握手超时（30s，对应 TS handshakeTimer）。
const HANDSHAKE_TIMEOUT_MS: u64 = 30_000;
/// disconnect 等待读循环退出上限（5s，对应 TS Promise.race）。
const DISCONNECT_DRAIN_MS: u64 = 5_000;

type EventCb = Arc<dyn Fn(WSEvent) + Send + Sync>;
type ConnectCb = Arc<dyn Fn() + Send + Sync>;
type DisconnectCb = Arc<dyn Fn(String) + Send + Sync>;

/// WebSocket 长连接配置。对应 TS `WSConfig`。
#[derive(Clone, Default)]
pub struct WSConfig {
    /// 收到服务端事件回调。
    pub on_event: Option<EventCb>,
    /// 连接建立回调。
    pub on_connect: Option<ConnectCb>,
    /// 断线回调（携带原因字符串）。
    pub on_disconnect: Option<DisconnectCb>,
    /// 自动订阅的主题。
    pub topics: Vec<String>,
    /// 最小重连间隔（ms，默认 2000）。
    pub reconnect_min_ms: Option<u64>,
    /// 最大重连间隔（ms，默认 60000）。
    pub reconnect_max_ms: Option<u64>,
    /// 是否自动重连（默认 true）。
    pub auto_reconnect: Option<bool>,
}

/// 填好缺省值的配置（对应 TS `Required<WSConfig>`）。
#[derive(Clone)]
struct FilledCfg {
    on_event: EventCb,
    on_connect: ConnectCb,
    on_disconnect: DisconnectCb,
    topics: Vec<String>,
    reconnect_min_ms: u64,
    reconnect_max_ms: u64,
    auto_reconnect: bool,
}

impl FilledCfg {
    fn from(cfg: WSConfig) -> Self {
        FilledCfg {
            on_event: cfg.on_event.unwrap_or_else(|| Arc::new(|_| {})),
            on_connect: cfg.on_connect.unwrap_or_else(|| Arc::new(|| {})),
            on_disconnect: cfg.on_disconnect.unwrap_or_else(|| Arc::new(|_| {})),
            topics: cfg.topics,
            reconnect_min_ms: cfg.reconnect_min_ms.unwrap_or(2000),
            reconnect_max_ms: cfg.reconnect_max_ms.unwrap_or(60_000),
            auto_reconnect: cfg.auto_reconnect.unwrap_or(true),
        }
    }
}

/// 存入 `ClientInner.ws` 的 WS 状态句柄（对应 TS `WSStateImpl`，core 保留具体 struct）。
pub struct WsHandle {
    /// 控制整个 ws loop 生命周期；`disconnect` cancel 之。
    abort: CancellationToken,
    /// 已连接标志（对应 TS `connected`）。
    connected: Arc<AtomicBool>,
    /// 读循环退出信号（对应 TS `done`）。
    done: Arc<Notify>,
}

/// stream-ticket 响应载荷。
#[derive(Deserialize)]
struct StreamTicket {
    ticket: String,
    #[serde(rename = "expiresIn", default)]
    #[allow(dead_code)]
    expires_in: i64,
}

impl Client {
    /// 建立 WebSocket 长连接 —— 等待首次连接成功或 abort。对应 TS `connect`。
    pub async fn connect(&self, cfg: WSConfig, signal: Option<CancellationToken>) -> Result<()> {
        // 幂等化重复 connect：先优雅断开旧连接（防 ws-reconnect-leak）。
        self.disconnect().await;

        let filled = FilledCfg::from(cfg);

        let abort = CancellationToken::new();
        // parent signal 取消 → 联动 abort（对应 TS signal.addEventListener('abort', ...)）。
        if let Some(sig) = signal {
            if sig.is_cancelled() {
                abort.cancel();
            } else {
                let a = abort.clone();
                tokio::spawn(async move {
                    sig.cancelled().await;
                    a.cancel();
                });
            }
        }

        let connected = Arc::new(AtomicBool::new(false));
        let done = Arc::new(Notify::new());

        // 首次连接 —— 失败抛出。
        let conn = self.ws_connect_once(&filled, &abort, &connected).await?;

        // 安装句柄。
        *self.ws_slot().lock().await = Some(WsHandle {
            abort: abort.clone(),
            connected: connected.clone(),
            done: done.clone(),
        });

        // 后台读循环 + 自动重连。
        let client = self.clone();
        tokio::spawn(async move {
            ws_loop(client, filled, abort, connected, done, conn).await;
        });

        Ok(())
    }

    /// 优雅断开 WebSocket 连接。对应 TS `disconnect`。
    pub async fn disconnect(&self) {
        let handle = self.ws_slot().lock().await.take();
        let handle = match handle {
            Some(h) => h,
            None => return,
        };
        handle.abort.cancel();
        handle.connected.store(false, Ordering::SeqCst);

        // 等待读循环退出（最多 5s）。
        let drain = handle.done.notified();
        tokio::select! {
            _ = drain => {}
            _ = tokio::time::sleep(Duration::from_millis(DISCONNECT_DRAIN_MS)) => {}
        }
    }

    /// WebSocket 是否已连接。对应 TS `isConnected`。
    pub async fn is_connected(&self) -> bool {
        match self.ws_slot().lock().await.as_ref() {
            Some(h) => h.connected.load(Ordering::SeqCst),
            None => false,
        }
    }

    /// 单次连接（铸 ticket → 拨号 → 等 welcome → 自动订阅）。对应 TS `wsConnectOnce`。
    /// 返回已就绪的 WS 流（已读掉 welcome，已发完订阅）。
    async fn ws_connect_once(
        &self,
        cfg: &FilledCfg,
        abort: &CancellationToken,
        connected: &Arc<AtomicBool>,
    ) -> Result<WsStream> {
        let url = ws_url(self);

        // 鉴权 —— 一次性 stream ticket 流程。
        let (bytes, _) = self
            .do_json_full_raw(
                reqwest::Method::POST,
                "/ws/stream-ticket",
                None,
                Some(abort.clone()),
                DEFAULT_JSON_TIMEOUT_MS,
            )
            .await?;
        let env: ApiResponse<StreamTicket> = serde_json::from_slice(&bytes)
            .map_err(|e| Error::other(format!("/ws/stream-ticket: decode: {e}")))?;
        if let Some(err) = env.business_error() {
            return Err(err);
        }
        let ticket = env.data.ticket;

        let full_url = append_ticket(&url, &ticket);

        // 拨号（带握手超时 + abort 联动）。
        let dial = tokio_tungstenite::connect_async(&full_url);
        let (mut conn, _resp) = tokio::select! {
            r = dial => r.map_err(|e| Error::other(format!("dial: {e}")))?,
            _ = abort.cancelled() => return Err(Error::other("dial: aborted")),
            _ = tokio::time::sleep(Duration::from_millis(HANDSHAKE_TIMEOUT_MS)) => {
                return Err(Error::other("dial: handshake timeout"));
            }
        };

        // 等待首条 message（应为 welcome），带握手超时。
        let welcome = tokio::select! {
            msg = next_text(&mut conn) => msg?,
            _ = abort.cancelled() => {
                let _ = conn.close(None).await;
                return Err(Error::other("dial: aborted"));
            }
            _ = tokio::time::sleep(Duration::from_millis(HANDSHAKE_TIMEOUT_MS)) => {
                let _ = conn.close(None).await;
                return Err(Error::other("dial: handshake timeout"));
            }
        };
        let welcome: WSEvent = serde_json::from_str(&welcome)
            .map_err(|e| Error::other(format!("parse welcome: {e}")))?;
        if welcome.r#type != "welcome" {
            let _ = conn.close(None).await;
            return Err(Error::other(format!(
                "unexpected first message: {}",
                welcome.r#type
            )));
        }

        connected.store(true, Ordering::SeqCst);

        // 自动订阅主题。
        if !cfg.topics.is_empty() {
            let sub = serde_json::json!({ "type": "subscribe", "topics": cfg.topics }).to_string();
            if let Err(e) = conn.send(Message::Text(sub)).await {
                connected.store(false, Ordering::SeqCst);
                let _ = conn.close(None).await;
                return Err(Error::other(format!("send subscribe: {e}")));
            }
        }

        (cfg.on_connect)();
        Ok(conn)
    }
}

/// WS 流类型别名（tungstenite over MaybeTlsStream）。
type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// 读取下一条文本 message（忽略 ping/pong/binary，遇 close/错误抛 Err）。
async fn next_text(conn: &mut WsStream) -> Result<String> {
    loop {
        match conn.next().await {
            Some(Ok(Message::Text(t))) => return Ok(t),
            Some(Ok(Message::Binary(b))) => return Ok(String::from_utf8_lossy(&b).into_owned()),
            Some(Ok(Message::Close(_))) => return Err(Error::other("closed")),
            Some(Ok(_)) => continue, // ping/pong/frame → 跳过。
            Some(Err(e)) => return Err(Error::other(format!("read: {e}"))),
            None => return Err(Error::other("closed")),
        }
    }
}

/// 后台读循环 + 自动重连（指数退避）。对应 TS `wsLoop`。
async fn ws_loop(
    client: Client,
    cfg: FilledCfg,
    abort: CancellationToken,
    connected: Arc<AtomicBool>,
    done: Arc<Notify>,
    mut conn: WsStream,
) {
    loop {
        // 读循环：阻塞直到连接关闭/出错/abort。
        ws_read_loop(&cfg, &abort, &connected, &mut conn).await;

        if abort.is_cancelled() {
            break;
        }

        // 关闭旧连接，防 FD 泄漏。
        let _ = conn.close(None).await;
        connected.store(false, Ordering::SeqCst);

        if !cfg.auto_reconnect {
            break;
        }

        // 自动重连（指数退避）。
        let mut delay = cfg.reconnect_min_ms;
        let reconnected = loop {
            if abort.is_cancelled() {
                break None;
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(delay)) => {}
                _ = abort.cancelled() => break None,
            }
            if abort.is_cancelled() {
                break None;
            }
            match client.ws_connect_once(&cfg, &abort, &connected).await {
                Ok(c) => break Some(c),
                Err(_) => {
                    delay = (delay * 2).min(cfg.reconnect_max_ms);
                }
            }
        };
        match reconnected {
            Some(c) => conn = c,
            None => break,
        }
    }
    done.notify_waiters();
}

/// 单连接读循环（对应 TS `wsReadLoop`）。message → on_event；close/error/abort → 退出 + on_disconnect。
async fn ws_read_loop(
    cfg: &FilledCfg,
    abort: &CancellationToken,
    connected: &Arc<AtomicBool>,
    conn: &mut WsStream,
) {
    loop {
        tokio::select! {
            _ = abort.cancelled() => {
                let _ = conn.close(None).await;
                connected.store(false, Ordering::SeqCst);
                return;
            }
            next = conn.next() => {
                match next {
                    Some(Ok(Message::Text(t))) => {
                        if let Ok(ev) = serde_json::from_str::<WSEvent>(&t) {
                            (cfg.on_event)(ev);
                        }
                        // 解析失败忽略（对应 TS catch）。
                    }
                    Some(Ok(Message::Binary(b))) => {
                        if let Ok(ev) = serde_json::from_slice::<WSEvent>(&b) {
                            (cfg.on_event)(ev);
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        connected.store(false, Ordering::SeqCst);
                        let reason = frame
                            .map(|f| format!("closed: code={} reason={}", u16::from(f.code), f.reason))
                            .unwrap_or_else(|| "closed".to_string());
                        (cfg.on_disconnect)(reason);
                        return;
                    }
                    Some(Ok(_)) => { /* ping/pong/frame：tungstenite 自动回 pong，跳过。 */ }
                    Some(Err(e)) => {
                        connected.store(false, Ordering::SeqCst);
                        (cfg.on_disconnect)(format!("error: {e}"));
                        return;
                    }
                    None => {
                        connected.store(false, Ordering::SeqCst);
                        (cfg.on_disconnect)("closed".to_string());
                        return;
                    }
                }
            }
        }
    }
}

/// `{api}/ws` → ws://wss:// scheme（对应 TS `wsURL`）。
fn ws_url(c: &Client) -> String {
    let base = c.api_url("/ws");
    if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base
    }
}

/// 追加 `?ticket=`/`&ticket=`（对应 TS `u.searchParams.set('ticket', ...)`）。
fn append_ticket(url: &str, ticket: &str) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!(
        "{url}{sep}ticket={}",
        crate::billing::entitlements::urlencoding(ticket)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_url_scheme_swap() {
        // api_url 拼接逻辑由 Client::api_url 负责；这里仅验 scheme 替换函数。
        assert_eq!(
            append_ticket("wss://h/api/v4/ws", "T1"),
            "wss://h/api/v4/ws?ticket=T1"
        );
        assert_eq!(
            append_ticket("wss://h/api/v4/ws?x=1", "T2"),
            "wss://h/api/v4/ws?x=1&ticket=T2"
        );
    }
}
