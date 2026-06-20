//! P5a 行为等价性测试：billing 金额三阵营端到端、skills 下载 50MB 上限、
//! notifications WebSocket 一次性 stream-ticket 取号。
//!
//! 用最小原始 HTTP/1.1 mock server（按到达顺序回放响应 + 记录 request 行），
//! 经预载 token 的 [`InMemoryTokenStore`] 走真实公开 API 路径。

use acosmi::core::{Client, Config, InMemoryTokenStore, TokenStore};
use acosmi::TokenSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct MockResponse {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}
impl MockResponse {
    fn ok_json(body: &str) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type", "application/json".into())],
            body: body.as_bytes().to_vec(),
        }
    }
    fn ok_bytes(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            headers: vec![],
            body,
        }
    }
    fn status(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: vec![],
            body: body.as_bytes().to_vec(),
        }
    }
    fn with_header(mut self, k: &'static str, v: &str) -> Self {
        self.headers.push((k, v.to_string()));
        self
    }
}

async fn spawn_mock(responses: Vec<MockResponse>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let log2 = log.clone();
    let count = Arc::new(AtomicUsize::new(0));
    tokio::spawn(async move {
        let mut idx = 0usize;
        while idx < responses.len() {
            let (mut sock, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => break,
            };
            let mut buf = vec![0u8; 16384];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let first = req.lines().next().unwrap_or("").to_string();
            let parts: Vec<&str> = first.split_whitespace().collect();
            if parts.len() >= 2 {
                log2.lock()
                    .unwrap()
                    .push(format!("{} {}", parts[0], parts[1]));
            }
            count.fetch_add(1, Ordering::SeqCst);

            let r = &responses[idx];
            idx += 1;
            let reason = match r.status {
                200 => "OK",
                429 => "Too Many Requests",
                401 => "Unauthorized",
                _ => "Status",
            };
            let mut head = format!(
                "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                r.status,
                reason,
                r.body.len()
            );
            for (k, v) in &r.headers {
                head.push_str(&format!("{k}: {v}\r\n"));
            }
            head.push_str("\r\n");
            let _ = sock.write_all(head.as_bytes()).await;
            let _ = sock.write_all(&r.body).await;
            let _ = sock.flush().await;
            let _ = sock.shutdown().await;
        }
    });
    (base, log)
}

async fn primed_client(base: &str) -> Client {
    let store = Arc::new(InMemoryTokenStore::new());
    store
        .save(&TokenSet {
            access_token: "AT0".into(),
            refresh_token: "RT0".into(),
            expires_at: "2999-01-01T00:00:00Z".into(),
            scope: String::new(),
            client_id: "cid".into(),
            server_url: base.to_string(),
        })
        .await
        .unwrap();
    Client::create(Config {
        server_url: Some(base.to_string()),
        store: Some(store),
        ..Default::default()
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn wallet_stats_f64_end_to_end() {
    // 钱包域金额 = float64（含小数）；网关返回 JSON 数字。
    let env = r#"{"code":0,"data":{"balance":88.25,"monthlyConsumption":12.5,"monthlyRecharge":100.0,"transactionCount":3}}"#;
    let (base, log) = spawn_mock(vec![MockResponse::ok_json(env)]).await;
    let client = primed_client(&base).await;

    let stats = client.get_wallet_stats(None).await.unwrap();
    assert_eq!(stats.balance, 88.25);
    assert_eq!(stats.monthly_consumption, 12.5);
    assert_eq!(stats.transaction_count, 3);
    assert_eq!(
        log.lock().unwrap().clone(),
        vec!["GET /api/v4/wallet/stats".to_string()]
    );
}

#[tokio::test]
async fn buy_response_amount_fen_i64_end_to_end() {
    // 商城金额 = 整数分 i64。
    let env = r#"{"code":0,"data":{"orderId":2002,"orderNo":"NO2","amountFen":3990,"orderStatus":"PENDING","paymentMethod":"WECHAT_NATIVE","paymentStatus":"PENDING"}}"#;
    let (base, _log) = spawn_mock(vec![MockResponse::ok_json(env)]).await;
    let client = primed_client(&base).await;

    let r = client.buy_token_package("pkg-1", None, None).await.unwrap();
    assert_eq!(r.amount_fen, 3990i64);
    assert_eq!(r.order_id, 2002i64);
}

#[tokio::test]
async fn skill_download_under_limit_succeeds_with_filename() {
    // 公开端点（无 token 也可，这里用预载 token），返回小 ZIP + Content-Disposition。
    let payload = b"PK\x03\x04zipbytes".to_vec();
    let resp = MockResponse::ok_bytes(payload.clone()).with_header(
        "Content-Disposition",
        "attachment; filename=\"mySkill.zip\"",
    );
    let (base, log) = spawn_mock(vec![resp]).await;
    let client = primed_client(&base).await;

    let dl = client.download_skill("skill-7", None).await.unwrap();
    assert_eq!(dl.data, payload);
    assert_eq!(dl.filename, "mySkill.zip");
    assert_eq!(
        log.lock().unwrap().clone(),
        vec!["GET /api/v4/skill-store/skill-7/download".to_string()]
    );
}

#[tokio::test]
async fn skill_download_over_50mb_limit_errors() {
    // 构造 > 50MB 响应体，期望 download_skill 报超限错误（不 OOM 截断）。
    let big = vec![0u8; (50 * 1024 * 1024) + 1024];
    let (base, _log) = spawn_mock(vec![MockResponse::ok_bytes(big)]).await;
    let client = primed_client(&base).await;

    let err = client.download_skill("big-skill", None).await.unwrap_err();
    assert!(
        err.to_string().contains("exceeds") && err.to_string().contains("50MB"),
        "expected 50MB limit error, got: {err}"
    );
}

#[tokio::test]
async fn skill_download_429_raises_rate_limit() {
    let resp = MockResponse::status(429, "rate limited").with_header("Retry-After", "30");
    let (base, _log) = spawn_mock(vec![resp]).await;
    let client = primed_client(&base).await;

    let err = client.download_skill("s", None).await.unwrap_err();
    match err {
        acosmi::Error::RateLimit { retry_after, .. } => assert_eq!(retry_after, "30"),
        other => panic!("expected RateLimit, got: {other:?}"),
    }
}

#[tokio::test]
async fn ws_connect_fetches_stream_ticket_first() {
    // WS 鉴权 = 一次性 stream-ticket 取号：connect 必须先 POST /ws/stream-ticket。
    // mock 返回 ticket 后，真实 WS 拨号会失败（非 WS server），但取号 request 已被记录。
    let ticket_env = r#"{"code":0,"data":{"ticket":"TKT-once-123","expiresIn":60}}"#;
    let (base, log) = spawn_mock(vec![MockResponse::ok_json(ticket_env)]).await;
    let client = primed_client(&base).await;

    // 不自动重连，避免后台 loop 反复拨号。
    let cfg = acosmi::WSConfig {
        auto_reconnect: Some(false),
        ..Default::default()
    };
    // 拨号必失败（mock 非 WS），connect 返 Err；关键是取号 request 已发出。
    let _ = client.connect(cfg, None).await;

    let recorded = log.lock().unwrap().clone();
    assert!(
        recorded.contains(&"POST /api/v4/ws/stream-ticket".to_string()),
        "stream-ticket 取号未发生，recorded={recorded:?}"
    );
    assert!(!client.is_connected().await);
}
