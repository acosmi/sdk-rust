//! P2 auth 行为测试：refresh 轮换（换新撤旧）+ 单航班 + syncFromDisk 采纳磁盘新版。
//!
//! 用一个最小内嵌 HTTP mock（std TcpListener，单线程逐请求应答）模拟 OAuth
//! discover + token 端点，端到端验证 Client::force_refresh / ensure_token 的刷新路径。

use acosmi::auth::TokenSet;
use acosmi::core::{BrowserRefreshMode, Client, Config};
use acosmi::TokenStore;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// 启动一个最小 OAuth mock server，返回 base URL（`http://127.0.0.1:port`）。
///
/// - `GET /.well-known/oauth-authorization-server/desktop` → ServerMetadata（token 端点指回自身）
/// - `POST /oauth/token`（refresh_token grant）→ 每次返回**递增的新 refresh_token**（轮换语义）
fn spawn_mock_oauth() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let refresh_count = Arc::new(AtomicUsize::new(0));
    let rc = refresh_count.clone();
    let base_for_meta = base.clone();

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let first_line = req.lines().next().unwrap_or("").to_string();

            let body = if first_line.contains("/.well-known/oauth-authorization-server/desktop") {
                format!(
                    r#"{{"issuer":"{b}","authorization_endpoint":"{b}/oauth/authorize","token_endpoint":"{b}/oauth/token","revocation_endpoint":"","registration_endpoint":"{b}/oauth/register","scopes_supported":["ai"]}}"#,
                    b = base_for_meta
                )
            } else if first_line.contains("POST /oauth/token") {
                let n = rc.fetch_add(1, Ordering::SeqCst) + 1;
                // 轮换：每次签发新的 access + refresh token。
                format!(
                    r#"{{"access_token":"AT{n}","token_type":"Bearer","expires_in":3600,"refresh_token":"RT{n}","scope":"ai"}}"#
                )
            } else {
                "{}".to_string()
            };

            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });

    (base, refresh_count)
}

fn expired_token(server_url: &str) -> TokenSet {
    TokenSet {
        access_token: "AT0".into(),
        refresh_token: "RT0".into(),
        // 过去时刻 → token_set_is_expired = true。
        expires_at: "2000-01-01T00:00:00Z".into(),
        scope: "ai".into(),
        client_id: "cid".into(),
        server_url: server_url.to_string(),
    }
}

#[tokio::test]
async fn force_refresh_rotates_refresh_token_and_persists() {
    let (base, refresh_count) = spawn_mock_oauth();
    let store = Arc::new(acosmi::InMemoryTokenStore::new());
    store.save(&expired_token(&base)).await.unwrap();

    let client = Client::create(Config {
        server_url: Some(base.clone()),
        store: Some(store.clone()),
        browser_refresh_mode: Some(BrowserRefreshMode::Direct),
        ..Default::default()
    })
    .await
    .unwrap();

    client.force_refresh(None).await.unwrap();

    // 轮换：内存 + 磁盘的 token 都应是新签发的 RT1（撤旧换新）。
    let in_mem = client.token_set().unwrap();
    assert_eq!(in_mem.access_token, "AT1");
    assert_eq!(in_mem.refresh_token, "RT1");
    let on_disk = store.load().await.unwrap().unwrap();
    assert_eq!(on_disk.refresh_token, "RT1");
    assert_eq!(refresh_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ensure_token_refreshes_when_expired() {
    let (base, refresh_count) = spawn_mock_oauth();
    let store = Arc::new(acosmi::InMemoryTokenStore::new());
    store.save(&expired_token(&base)).await.unwrap();

    let client = Client::create(Config {
        server_url: Some(base.clone()),
        store: Some(store.clone()),
        browser_refresh_mode: Some(BrowserRefreshMode::Direct),
        ..Default::default()
    })
    .await
    .unwrap();

    // 过期 token → ensure_token 触发刷新，返回新 access_token。
    let at = client.ensure_token(None).await.unwrap();
    assert_eq!(at, "AT1");
    assert_eq!(refresh_count.load(Ordering::SeqCst), 1);

    // 第二次调用：新 token 未过期 → 无锁直接返回，不再打 token 端点。
    let at2 = client.ensure_token(None).await.unwrap();
    assert_eq!(at2, "AT1");
    assert_eq!(refresh_count.load(Ordering::SeqCst), 1, "未过期不应再刷新");
}

#[tokio::test]
async fn ensure_token_adopts_disk_rotation_skipping_redundant_refresh() {
    // syncFromDisk：进入临界区前别的进程已 rotation（磁盘是未过期新 RT），
    // 本进程应采纳磁盘版并跳过多余刷新。
    let (base, refresh_count) = spawn_mock_oauth();
    let store = Arc::new(acosmi::InMemoryTokenStore::new());
    // 内存（client）持过期旧 token；磁盘（store）持未过期新 token（模拟别进程已 rotation）。
    store.save(&expired_token(&base)).await.unwrap();

    let client = Client::create(Config {
        server_url: Some(base.clone()),
        store: Some(store.clone()),
        browser_refresh_mode: Some(BrowserRefreshMode::Direct),
        ..Default::default()
    })
    .await
    .unwrap();

    // 现在让磁盘领先：写入一个未过期的新 token（不同 RT），client 内存仍是过期旧的。
    let fresh = TokenSet {
        access_token: "AT-DISK".into(),
        refresh_token: "RT-DISK".into(),
        expires_at: "2099-01-01T00:00:00Z".into(),
        scope: "ai".into(),
        client_id: "cid".into(),
        server_url: base.clone(),
    };
    store.save(&fresh).await.unwrap();

    let at = client.ensure_token(None).await.unwrap();
    // 采纳磁盘新版（未过期）→ 直接返回磁盘 token，未打 token 端点。
    assert_eq!(at, "AT-DISK");
    assert_eq!(
        refresh_count.load(Ordering::SeqCst),
        0,
        "采纳磁盘新版应跳过 refresh（防多进程 400）"
    );
}

#[tokio::test]
async fn ensure_token_unauthorized_without_login() {
    let store = Arc::new(acosmi::InMemoryTokenStore::new());
    let client = Client::create(Config {
        store: Some(store),
        ..Default::default()
    })
    .await
    .unwrap();
    let err = client.ensure_token(None).await.unwrap_err();
    assert!(err.to_string().contains("not authorized"));
}
