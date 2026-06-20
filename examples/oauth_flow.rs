//! oauth_flow — 手动 OAuth 2.1 PKCE 流程（CLI / 自定义流程）。
//!
//! 端口自 `acosmi-sdk-ts/examples/auth-oauth-flow.ts`。
//!
//! 演示底层 auth helper（绕过 `Client::login` 的封装），适用于需要自管 token 的 CLI：
//!   1. `discover`  —— 拉取 OAuth Authorization Server 元数据（RFC 8414）。
//!   2. `register`  —— RFC 7591 动态客户端注册，拿 client_id。
//!   3. PKCE —— `generate_code_verifier` + `code_challenge`（S256）+ `generate_state`，手动拼授权
//!      URL（不内置 loopback server，避免 `desktop-loopback` feature 依赖）。
//!   4. `exchange_code` —— 用 redirect 回调里的 `code` + `code_verifier` 换 token。
//!   5. `new_token_set` + `TokenStore` —— 持久化 token，后续复用。
//!   6. `refresh_token` —— 用 refresh_token 静默续期。
//!
//! 环境变量：
//!   - `ACOSMI_SERVER_URL`（必填）：网关 base URL。
//!   - `ACOSMI_TOKEN_FILE`（可选）：token 持久化路径，缺省 `./oauth-tokens.json`。
//!   - `ACOSMI_AUTH_CODE`（可选）：浏览器授权后回调里的 `code`（演示 exchange 步骤；缺省时跳过）。
//!
//! 说明：大多数场景直接用 `client.login(app_name, &scopes, None)` 即可（内部封装下面全部步骤）。
//!
//! 运行：`cargo run --example oauth_flow`（CI 仅 `cargo build --example oauth_flow`）。

use acosmi::{
    all_scopes, code_challenge, discover, exchange_code, generate_code_verifier, generate_state,
    new_token_set, refresh_token, token_set_is_expired, FileTokenStore, TokenStore,
};

// 本地 loopback redirect URI（与动态注册时声明的回调一致）。
const REDIRECT_URI: &str = "http://127.0.0.1/callback";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server_url = std::env::var("ACOSMI_SERVER_URL").expect("ACOSMI_SERVER_URL is required");
    let token_file =
        std::env::var("ACOSMI_TOKEN_FILE").unwrap_or_else(|_| "./oauth-tokens.json".into());

    let scopes = all_scopes();
    let store = FileTokenStore::new(Some(token_file.into()));
    let http = reqwest::Client::new();

    // 0) 已有持久化 token 且未过期 → 直接复用，跳过整个授权流程。
    let existing = store.load().await?;
    if let Some(t) = &existing {
        if !token_set_is_expired(t) {
            println!("[token] reusing valid token from store, scope={}", t.scope);
            return Ok(());
        }
    }

    // 1) discover —— OAuth Authorization Server 元数据。
    let meta = discover(&http, &server_url).await?;
    println!("[discover] issuer={}", meta.issuer);
    println!("[discover] token_endpoint={}", meta.token_endpoint);

    // 2) register —— 动态注册一个 client，拿到 client_id（已有 token 则复用其 client_id）。
    let client_id = match &existing {
        Some(t) if !t.client_id.is_empty() => t.client_id.clone(),
        _ => {
            acosmi::register(&http, &meta, "Auth Flow Example")
                .await?
                .client_id
        }
    };
    println!("[register] client_id={client_id}");

    // 3) refresh-first：store 里有过期 token 但带 refresh_token → 先尝试静默刷新。
    let mut token_set = None;
    if let Some(t) = &existing {
        if !t.refresh_token.is_empty() {
            match refresh_token(&http, &meta, &t.client_id, &t.refresh_token).await {
                Ok(resp) => {
                    token_set = Some(new_token_set(&resp, &t.client_id, &server_url));
                    println!("[refresh] token refreshed without re-authorizing");
                }
                Err(e) => {
                    eprintln!("[refresh] failed, falling back to full authorize: {e}");
                }
            }
        }
    }

    // 4) 没有可刷新的 token → 走完整 PKCE 授权。
    if token_set.is_none() {
        // PKCE 原语：verifier 落地保存（换 token 时回传），challenge 进授权 URL。
        let verifier = generate_code_verifier();
        let challenge = code_challenge(&verifier);
        let state = generate_state();

        let auth_url = format!(
            "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
            meta.authorization_endpoint,
            urlencoding(&client_id),
            urlencoding(REDIRECT_URI),
            urlencoding(&scopes.join(" ")),
            urlencoding(&state),
            urlencoding(&challenge),
        );
        println!("[authorize] open in browser:\n  {auth_url}");

        // 浏览器授权后回调 URL 携带 `code` —— 这里从环境变量读取（CLI 真实流程应起 loopback server 捕获，
        // 或开启 `desktop-loopback` feature 用 `client.login(...)`）。
        let code = match std::env::var("ACOSMI_AUTH_CODE") {
            Ok(c) if !c.is_empty() => c,
            _ => {
                println!(
                    "[authorize] 设置 ACOSMI_AUTH_CODE=<浏览器回调里的 code> 后重跑以完成 token 兑换。"
                );
                return Ok(());
            }
        };

        // 5) exchange_code —— code + code_verifier 换 token。
        let resp = exchange_code(&http, &meta, &client_id, &code, REDIRECT_URI, &verifier).await?;
        let ts = new_token_set(&resp, &client_id, &server_url);
        println!("[exchange] access token acquired, scope={}", ts.scope);
        token_set = Some(ts);
    }

    // 6) 持久化 token，供下次启动复用 / Client 直接读取。
    let token_set = token_set.expect("token_set present after refresh or exchange");
    store.save(&token_set).await?;
    println!(
        "[store] token persisted; expires_at={}",
        token_set.expires_at
    );

    Ok(())
}

/// 最小 URL 查询参数转义（仅示例用；生产可用 `url::form_urlencoded`）。
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
