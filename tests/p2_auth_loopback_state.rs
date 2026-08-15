#![cfg(feature = "desktop-loopback")]
//! p2_auth_loopback_state — 桌面 loopback OAuth state + 常驻多连接稳健性矩阵
//! (2026-08-15, 与 TS test/auth/desktop-loopback-state.test.ts / Go
//! auth_loopback_state_test.go 同契约; 另含 Rust 特有的多连接与端口关闭断言)。
//!
//! 契约: /callback 的每一种形态 (成功 / OAuth error / 畸形) 一律先验 state —— 恰好一个且与
//! 本次登录严格匹配; 缺失、重复、错值均以 state_mismatch 拒绝且不得结算成 auth_denied;
//! 结算只取首发, 后续回调 (含洪泛) 不得二次结算。非 /callback 探针、畸形请求、空连接
//! (浏览器预测性预连接) 不得终止等待中的登录; 成功 / 失败 / 取消一切终止路径都以监听端口
//! 关闭收尾。

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use acosmi::auth::{
    authorize, AuthorizeResult, LoginEvent, LoginOptions, ServerMetadata, ERR_AUTH_DENIED,
    ERR_STATE_MISMATCH, ERR_TIMEOUT, EVENT_AUTH_URL,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

fn meta() -> ServerMetadata {
    ServerMetadata {
        issuer: "https://acosmi.com".into(),
        authorization_endpoint: "https://acosmi.com/oauth/desktop/authorize".into(),
        token_endpoint: "https://acosmi.com/oauth/desktop/token".into(),
        revocation_endpoint: String::new(),
        registration_endpoint: String::new(),
        scopes_supported: vec![],
    }
}

/// query 参数转义（state 是 base64url 本可直拼；error_description 等需要）。
fn q(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn addr_of(redirect_uri: &str) -> String {
    let u = url::Url::parse(redirect_uri).expect("parse redirect_uri");
    format!("{}:{}", u.host_str().unwrap(), u.port().unwrap())
}

/// 驱动一次 authorize：driver 收到 (redirect_uri, state) 后自行开火。
/// 返回 (authorize 结果, 事件序列)。整体 15s 死线防 CI 悬挂。
async fn drive<F, Fut>(
    opts: LoginOptions,
    cancel: Option<CancellationToken>,
    driver: F,
) -> (
    Result<(AuthorizeResult, String), acosmi::Error>,
    Vec<LoginEvent>,
)
where
    F: FnOnce(String, String) -> Fut,
    Fut: Future<Output = ()>,
{
    let m = meta();
    let events: Arc<Mutex<Vec<LoginEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let (url_tx, url_rx) = tokio::sync::oneshot::channel::<String>();
    let url_tx = Mutex::new(Some(url_tx));
    let ev = events.clone();
    let handler = move |e: LoginEvent| {
        if e.r#type == EVENT_AUTH_URL {
            if let (Some(tx), Some(u)) = (url_tx.lock().unwrap().take(), e.url.clone()) {
                let _ = tx.send(u);
            }
        }
        ev.lock().unwrap().push(e);
    };
    let scopes = vec!["ai".to_string()];
    let auth_fut = authorize(&m, "client-1", &scopes, &opts, Some(&handler), cancel);
    let drv_fut = async move {
        let raw = url_rx.await.expect("auth_url event");
        let u = url::Url::parse(&raw).expect("parse auth url");
        let mut redirect_uri = String::new();
        let mut state = String::new();
        for (k, v) in u.query_pairs() {
            match k.as_ref() {
                "redirect_uri" => redirect_uri = v.into_owned(),
                "state" => state = v.into_owned(),
                _ => {}
            }
        }
        assert!(
            !redirect_uri.is_empty() && !state.is_empty(),
            "auth url missing redirect_uri/state: {raw}"
        );
        driver(redirect_uri, state).await;
    };
    let (res, ()) = tokio::time::timeout(Duration::from_secs(15), async {
        tokio::join!(auth_fut, drv_fut)
    })
    .await
    .expect("test deadline (15s)");
    let evs = events.lock().unwrap().clone();
    (res, evs)
}

/// 发一发 GET，返回完整响应文本（连不上/读不到按空串，5s 预算）。
async fn mini_get(full_url: &str) -> String {
    let u = url::Url::parse(full_url).expect("parse url");
    let addr = format!("{}:{}", u.host_str().unwrap(), u.port().unwrap());
    let path_q = match u.query() {
        Some(qs) => format!("{}?{}", u.path(), qs),
        None => u.path().to_string(),
    };
    let Ok(mut s) = TcpStream::connect(&addr).await else {
        return String::new();
    };
    let req = format!("GET {path_q} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    if s.write_all(req.as_bytes()).await.is_err() {
        return String::new();
    }
    let mut out = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut out)).await;
    String::from_utf8_lossy(&out).into_owned()
}

/// 裸连接：可选发送任意字节，随即（或稍候）断开。用于模拟预连接/垃圾探针。
async fn raw_send(addr: &str, bytes: &[u8], linger_ms: u64) {
    if let Ok(mut s) = TcpStream::connect(addr).await {
        if !bytes.is_empty() {
            let _ = s.write_all(bytes).await;
        }
        tokio::time::sleep(Duration::from_millis(linger_ms)).await;
        drop(s);
    }
}

/// 断言监听端口在结算后**立即**拒连（单次尝试，刻意不重试轮询）。
///
/// 不轮询的理由是决定性的：`authorize` 在一切终止路径上都先 `drop(listener)` 再返回，
/// 所以 `drive` 一返回，首次 connect 就必须被拒 —— 无竞态。反过来，轮询式断言会**掩盖**
/// 取消泄漏：卡在 `accept()` 上的连接一旦被探针满足，泄漏的线程就跑完并释放端口，
/// 第二次探针于是"合法地"拒连（2026-08-15 红反证实测：轮询版在旧实现上是绿的）。
async fn assert_listener_refuses(redirect_uri: &str) {
    let addr = addr_of(redirect_uri);
    assert!(
        TcpStream::connect(&addr).await.is_err(),
        "loopback listener still accepting connections after settle: {addr}"
    );
}

// =============================================================================
// 1–2 成功路径
// =============================================================================

#[tokio::test]
async fn success_with_exact_state_returns_code_and_closes_port() {
    let seen = Arc::new(Mutex::new(String::new()));
    let seen2 = seen.clone();
    let (res, _evs) = drive(
        LoginOptions::default(),
        None,
        |redirect_uri, state| async move {
            *seen2.lock().unwrap() = redirect_uri.clone();
            let body = mini_get(&format!("{redirect_uri}?code=good-code&state={state}")).await;
            assert!(
                body.contains("授权成功"),
                "success page expected, got: {body}"
            );
        },
    )
    .await;
    let (result, _verifier) = res.expect("success path must settle Ok");
    assert_eq!(result.code, "good-code");
    let uri = seen.lock().unwrap().clone();
    assert_listener_refuses(&uri).await;
}

#[tokio::test]
async fn success_redirect_302_when_brand_url_configured() {
    let opts = LoginOptions {
        success_redirect_url: Some("https://acosmi.com/ok".into()),
        ..Default::default()
    };
    let (res, _evs) = drive(opts, None, |redirect_uri, state| async move {
        let body = mini_get(&format!("{redirect_uri}?code=good-code&state={state}")).await;
        assert!(
            body.starts_with("HTTP/1.1 302"),
            "302 expected, got: {body}"
        );
        assert!(
            body.contains("Location: https://acosmi.com/ok"),
            "brand Location expected, got: {body}"
        );
    })
    .await;
    assert_eq!(
        res.expect("success path must settle Ok").0.code,
        "good-code"
    );
}

// =============================================================================
// 3–6 state 闸：缺失 / 错值 / 重复
// =============================================================================

#[tokio::test]
async fn missing_state_rejected_as_state_mismatch() {
    let (res, evs) = drive(
        LoginOptions::default(),
        None,
        |redirect_uri, _state| async move {
            let body = mini_get(&format!("{redirect_uri}?code=attacker-code")).await;
            assert!(
                body.contains("回调校验未通过"),
                "failure page expected, got: {body}"
            );
        },
    )
    .await;
    let err = res.expect_err("missing state must settle Err").to_string();
    assert!(err.contains(ERR_STATE_MISMATCH), "got: {err}");
    assert!(err.contains("missing state"), "got: {err}");
    assert!(
        evs.iter()
            .any(|e| e.err_code.as_deref() == Some(ERR_STATE_MISMATCH)),
        "must emit state_mismatch event, got: {evs:?}"
    );
    assert!(
        !evs.iter()
            .any(|e| e.err_code.as_deref() == Some(ERR_AUTH_DENIED)),
        "must not emit auth_denied, got: {evs:?}"
    );
}

#[tokio::test]
async fn wrong_state_rejected() {
    let (res, _evs) = drive(
        LoginOptions::default(),
        None,
        |redirect_uri, _state| async move {
            mini_get(&format!("{redirect_uri}?code=x&state=wrong-state")).await;
        },
    )
    .await;
    let err = res.expect_err("wrong state must settle Err").to_string();
    assert!(err.contains("does not match pending state"), "got: {err}");
}

#[tokio::test]
async fn duplicate_state_rejected_even_when_first_correct() {
    let (res, _evs) = drive(
        LoginOptions::default(),
        None,
        |redirect_uri, state| async move {
            mini_get(&format!("{redirect_uri}?code=x&state={state}&state=wrong")).await;
        },
    )
    .await;
    let err = res
        .expect_err("duplicate state must settle Err")
        .to_string();
    assert!(err.contains("multiple state"), "got: {err}");
}

#[tokio::test]
async fn duplicate_identical_correct_state_rejected() {
    let (res, _evs) = drive(
        LoginOptions::default(),
        None,
        |redirect_uri, state| async move {
            mini_get(&format!(
                "{redirect_uri}?code=x&state={state}&state={state}"
            ))
            .await;
        },
    )
    .await;
    let err = res
        .expect_err("duplicate state must settle Err")
        .to_string();
    assert!(err.contains("multiple state"), "got: {err}");
}

// =============================================================================
// 7–9 OAuth error 回调：未认证 vs 用户真拒绝
// =============================================================================

#[tokio::test]
async fn error_callback_without_state_is_state_mismatch_not_denied() {
    let (res, evs) = drive(
        LoginOptions::default(),
        None,
        |redirect_uri, _state| async move {
            mini_get(&format!(
                "{redirect_uri}?error=access_denied&error_description=nope"
            ))
            .await;
        },
    )
    .await;
    let err = res
        .expect_err("unauthenticated error callback must settle Err")
        .to_string();
    assert!(err.contains(ERR_STATE_MISMATCH), "got: {err}");
    assert!(!err.contains("denied:"), "must not read as denial: {err}");
    assert!(
        !err.contains("access_denied") && !err.contains("nope"),
        "must not echo callback values: {err}"
    );
    assert!(
        !evs.iter()
            .any(|e| e.err_code.as_deref() == Some(ERR_AUTH_DENIED)),
        "must not emit auth_denied for unauthenticated error callback, got: {evs:?}"
    );
}

#[tokio::test]
async fn user_denial_with_valid_state_is_auth_denied() {
    let (res, evs) = drive(
        LoginOptions::default(),
        None,
        |redirect_uri, state| async move {
            let desc = q("user said no");
            mini_get(&format!(
                "{redirect_uri}?error=access_denied&error_description={desc}&state={state}"
            ))
            .await;
        },
    )
    .await;
    let err = res.expect_err("denial must settle Err").to_string();
    assert!(
        err.contains("authorization denied: user said no"),
        "got: {err}"
    );
    assert!(
        evs.iter()
            .any(|e| e.err_code.as_deref() == Some(ERR_AUTH_DENIED)),
        "must emit auth_denied, got: {evs:?}"
    );
}

#[tokio::test]
async fn denied_page_html_escapes_error_description() {
    let (res, _evs) = drive(
        LoginOptions::default(),
        None,
        |redirect_uri, state| async move {
            let desc = q("<script>alert(1)</script>");
            let body = mini_get(&format!(
                "{redirect_uri}?error=x&error_description={desc}&state={state}"
            ))
            .await;
            assert!(body.contains("&lt;script&gt;"), "must escape, got: {body}");
            assert!(
                !body.contains("<script>alert"),
                "raw script must not reach page, got: {body}"
            );
        },
    )
    .await;
    let err = res.expect_err("denial must settle Err").to_string();
    assert!(err.contains("authorization denied"), "got: {err}");
}

// =============================================================================
// 10–12 常驻多连接稳健性：探针 / 空连接 / 洪泛
// =============================================================================

#[tokio::test]
async fn non_callback_probe_gets_404_and_login_survives() {
    let (res, _evs) = drive(
        LoginOptions::default(),
        None,
        |redirect_uri, state| async move {
            let base = redirect_uri.trim_end_matches("/callback").to_string();
            let fav = mini_get(&format!("{base}/favicon.ico")).await;
            assert!(
                fav.starts_with("HTTP/1.1 404"),
                "favicon must 404, got: {fav}"
            );
            let root = mini_get(&format!("{base}/")).await;
            assert!(
                root.starts_with("HTTP/1.1 404"),
                "root must 404, got: {root}"
            );
            // 关键断言：探针之后登录仍然存活，合法回调照常成功。
            mini_get(&format!("{redirect_uri}?code=good-code&state={state}")).await;
        },
    )
    .await;
    assert_eq!(res.expect("login must survive probes").0.code, "good-code");
}

#[tokio::test]
async fn empty_and_garbage_connections_do_not_kill_login() {
    let (res, _evs) = drive(
        LoginOptions::default(),
        None,
        |redirect_uri, state| async move {
            let addr = addr_of(&redirect_uri);
            // 浏览器预测性预连接：连上不发字节再断开。
            raw_send(&addr, b"", 50).await;
            // 畸形请求行。
            raw_send(&addr, b"FOO BAR\r\n\r\n", 0).await;
            raw_send(&addr, b"", 120).await;
            mini_get(&format!("{redirect_uri}?code=good-code&state={state}")).await;
        },
    )
    .await;
    assert_eq!(
        res.expect("login must survive dumb connections").0.code,
        "good-code"
    );
}

#[tokio::test]
async fn flood_of_bad_callbacks_settles_once_and_tears_down() {
    let seen = Arc::new(Mutex::new(String::new()));
    let seen2 = seen.clone();
    let (res, _evs) = drive(
        LoginOptions::default(),
        None,
        |redirect_uri, _state| async move {
            *seen2.lock().unwrap() = redirect_uri.clone();
            let mut hs = Vec::new();
            for i in 0..6 {
                let uri = redirect_uri.clone();
                hs.push(tokio::spawn(async move {
                    mini_get(&format!("{uri}?code=attacker-{i}&state=wrong-{i}")).await;
                }));
            }
            for h in hs {
                let _ = h.await;
            }
        },
    )
    .await;
    let err = res.expect_err("flood must settle Err").to_string();
    assert!(err.contains(ERR_STATE_MISMATCH), "got: {err}");
    // 迟到的合法回调不能复活已结算的登录：端口必须已关闭。
    let uri = seen.lock().unwrap().clone();
    assert_listener_refuses(&uri).await;
}

// =============================================================================
// 13–14 取消
// =============================================================================

#[tokio::test]
async fn cancellation_settles_as_timeout_and_closes_listener() {
    let tok = CancellationToken::new();
    let tok2 = tok.clone();
    let seen = Arc::new(Mutex::new(String::new()));
    let seen2 = seen.clone();
    let (res, evs) = drive(
        LoginOptions::default(),
        Some(tok),
        |redirect_uri, _state| async move {
            *seen2.lock().unwrap() = redirect_uri;
            tok2.cancel();
        },
    )
    .await;
    let err = res.expect_err("cancel must settle Err").to_string();
    assert!(err.contains("timed out"), "got: {err}");
    assert!(
        evs.iter()
            .any(|e| e.err_code.as_deref() == Some(ERR_TIMEOUT)),
        "cancel must emit auth_timeout event, got: {evs:?}"
    );
    // 旧实现在此泄漏：blocking 线程永久卡在 accept() 上，监听端口跟着它一起活着
    //（直到有人连上来被它吃掉，或进程退出；期间 runtime 关不掉——见文件头注）。
    let uri = seen.lock().unwrap().clone();
    assert_listener_refuses(&uri).await;
}

#[tokio::test]
async fn pre_cancelled_token_times_out_immediately() {
    let tok = CancellationToken::new();
    tok.cancel();
    let (res, _evs) = drive(LoginOptions::default(), Some(tok), |_uri, _state| async {}).await;
    let err = res.expect_err("pre-cancelled must settle Err").to_string();
    assert!(err.contains("timed out"), "got: {err}");
}
