//! OAuth 2.1 PKCE 流程。端口自 `auth/auth.ts`（其端口自 `acosmi-sdk-go/auth.go`）。
//!
//! 含：
//!   - Discovery（`discover_with_profile` / `discover` / `discover_web_oauth_metadata`）
//!   - Dynamic Client Registration（`register` / `register_web_oauth_client`，RFC 7591）
//!   - PKCE（`generate_code_verifier` + S256 `code_challenge` / `generate_state`）
//!   - Token Exchange（`exchange_code` / `exchange_code_with_expiry`）
//!   - Refresh（`refresh_token`，轮换：换新撤旧由 Client 协调）
//!   - Revoke（`revoke_token`，服务端不支持时静默跳过）
//!   - 浏览器 Web OAuth 原语（`create_web_authorization_request` /
//!     `complete_web_authorization_request`，平台无关纯逻辑：PKCE / state 校验 / 兑换）
//!   - loopback `authorize()`（桌面 OAuth，`#[cfg(feature = "desktop-loopback")]`）
//!
//! 所有 HTTP 经传入的 `reqwest::Client`（对应 TS `fetchImpl`），auth 专用 30s 超时。

use crate::auth::types::{ClientRegistration, ServerMetadata, TokenResponse, TokenSet};
use crate::shared::errors::{Error, Result};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::time::Duration;

/// auth 专用超时（与 Go `authHTTPClient` 30s 一致）。
const AUTH_TIMEOUT: Duration = Duration::from_secs(30);

// =============================================================================
// OAuthTokenEndpointError —— token 端点结构化错误（对应 TS 同名子类）
// =============================================================================

/// token 端点（exchange / refresh）的结构化错误。携带 OAuth `error` 码用于
/// `is_invalid_grant_error` 判定（invalid_grant → 本地 token 失效，需重登）。
#[derive(Debug, Clone)]
pub struct OAuthTokenEndpointError {
    pub status: u16,
    pub oauth_error: String,
    pub error_description: String,
}

impl std::fmt::Display for OAuthTokenEndpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let detail = if !self.error_description.is_empty() {
            &self.error_description
        } else {
            &self.oauth_error
        };
        write!(f, "token: HTTP {}: {}", self.status, detail)
    }
}

impl std::error::Error for OAuthTokenEndpointError {}

/// 判定错误是否为 OAuth `invalid_grant`（refresh token 失效）。对应 TS `isInvalidGrantError`。
pub fn is_invalid_grant_error(err: &Error) -> bool {
    matches!(err, Error::OAuthTokenEndpoint(e) if e.oauth_error == "invalid_grant")
}

/// 检测 SSL/TLS 相关错误（企业代理 Zscaler 等）。对应 TS `isSSLError`。
pub fn is_ssl_error(msg: &str) -> bool {
    msg.contains("tls:") || msg.contains("x509:") || msg.contains("certificate")
}

// =============================================================================
// Discovery
// =============================================================================

/// OAuth metadata profile —— 决定 well-known 端点的路径后缀。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthMetadataProfile {
    /// 桌面 loopback OAuth。
    Desktop,
    /// 浏览器 Web OAuth。
    Web,
}

impl OAuthMetadataProfile {
    /// well-known 端点路径后缀。
    pub fn as_str(self) -> &'static str {
        match self {
            OAuthMetadataProfile::Desktop => "desktop",
            OAuthMetadataProfile::Web => "web",
        }
    }
}

/// 从 well-known 端点获取指定 profile 的 OAuth 服务元数据（RFC 8414）。
///
/// `server_url` 可能含路径（如 `https://acosmi.ai/api/v4`）；well-known 端点按 RFC 8414
/// 必须在 origin 根路径：`https://acosmi.ai/.well-known/oauth-authorization-server/<profile>`。
pub async fn discover_with_profile(
    http: &reqwest::Client,
    server_url: &str,
    profile: OAuthMetadataProfile,
) -> Result<ServerMetadata> {
    // 去尾随 '/' 后解析；取 origin（scheme://host[:port]）。
    let trimmed = server_url.trim_end_matches('/');
    let parsed = url::Url::parse(trimmed)
        .map_err(|e| Error::other(format!("discover: invalid server URL: {e}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| Error::other("discover: invalid server URL: missing host"))?;
    let mut origin = format!("{}://{}", parsed.scheme(), host);
    if let Some(port) = parsed.port() {
        origin.push_str(&format!(":{port}"));
    }
    let endpoint = format!(
        "{origin}/.well-known/oauth-authorization-server/{}",
        profile.as_str()
    );

    let resp = http
        .get(&endpoint)
        .timeout(AUTH_TIMEOUT)
        .send()
        .await
        .map_err(|e| Error::other(format!("discover: {e}")))?;

    if !resp.status().is_success() {
        return Err(Error::other(format!("discover: HTTP {}", resp.status())));
    }

    let meta: ServerMetadata = resp
        .json()
        .await
        .map_err(|e| Error::other(format!("discover: decode: {e}")))?;

    if meta.token_endpoint.is_empty() || meta.authorization_endpoint.is_empty() {
        return Err(Error::other(format!(
            "discover: metadata missing required endpoints (token={}, auth={})",
            meta.token_endpoint, meta.authorization_endpoint
        )));
    }

    Ok(meta)
}

/// 从 well-known 端点获取 Desktop OAuth 服务元数据。对应 TS `discover`。
pub async fn discover(http: &reqwest::Client, server_url: &str) -> Result<ServerMetadata> {
    discover_with_profile(http, server_url, OAuthMetadataProfile::Desktop).await
}

/// 从 well-known 端点获取 Web OAuth 服务元数据。对应 TS `discoverWebOAuthMetadata`。
///
/// 与 `discover` 行为一致，但请求 `web` profile（csign 等第一方 Web 应用必须用此 profile；
/// `desktop` 返回的是桌面 loopback 端点，不可混用）。
pub async fn discover_web_oauth_metadata(
    http: &reqwest::Client,
    server_url: &str,
) -> Result<ServerMetadata> {
    discover_with_profile(http, server_url, OAuthMetadataProfile::Web).await
}

// =============================================================================
// Dynamic Client Registration (RFC 7591)
// =============================================================================

/// 动态注册桌面客户端，获取 client_id。对应 TS `register`。
pub async fn register(
    http: &reqwest::Client,
    meta: &ServerMetadata,
    app_name: &str,
) -> Result<ClientRegistration> {
    let body = serde_json::json!({
        "client_name": app_name,
        "token_endpoint_auth_method": "none",
        "grant_types": ["authorization_code", "refresh_token"],
        "redirect_uris": ["http://127.0.0.1/callback"],
        "response_types": ["code"],
    });
    post_register(http, &meta.registration_endpoint, body).await
}

/// Web OAuth 动态注册参数。对应 TS `RegisterWebOAuthClientOptions`。
#[derive(Debug, Clone)]
pub struct RegisterWebOAuthClientOptions {
    /// 客户端展示名（RFC 7591 client_name）。
    pub client_name: String,
    /// 浏览器 Web OAuth 的 redirect_uri 列表。
    pub redirect_uris: Vec<String>,
    /// 可选 scope 列表 —— 提供时以空格连接写入 RFC 7591 scope 字段。
    pub scopes: Option<Vec<String>>,
}

/// 动态注册 Web OAuth（浏览器）客户端，获取 client_id。对应 TS `registerWebOAuthClient`。
///
/// 与 `register`（桌面 loopback，硬编码 redirect_uri=127.0.0.1）的区别：本函数允许传入
/// 任意 Web redirect_uri，供 csign 等第一方 Web 应用使用。auth method 仍为 `none`（PKCE public）。
pub async fn register_web_oauth_client(
    http: &reqwest::Client,
    meta: &ServerMetadata,
    opts: &RegisterWebOAuthClientOptions,
) -> Result<ClientRegistration> {
    let mut body = serde_json::json!({
        "client_name": opts.client_name,
        "token_endpoint_auth_method": "none",
        "grant_types": ["authorization_code", "refresh_token"],
        "redirect_uris": opts.redirect_uris,
        "response_types": ["code"],
    });
    if let Some(scopes) = &opts.scopes {
        body["scope"] = serde_json::Value::String(scopes.join(" "));
    }
    post_register(http, &meta.registration_endpoint, body).await
}

async fn post_register(
    http: &reqwest::Client,
    endpoint: &str,
    body: serde_json::Value,
) -> Result<ClientRegistration> {
    let resp = http
        .post(endpoint)
        .timeout(AUTH_TIMEOUT)
        .json(&body)
        .send()
        .await
        .map_err(|e| Error::other(format!("register: {e}")))?;

    let status = resp.status().as_u16();
    if status != 200 && status != 201 {
        return Err(Error::other(format!("register: HTTP {status}")));
    }

    resp.json()
        .await
        .map_err(|e| Error::other(format!("register: decode: {e}")))
}

// =============================================================================
// PKCE
// =============================================================================

/// 生成 32 字节加密随机数，base64url 无填充编码 —— PKCE verifier / state 共用随机源。
fn random_base64url_32() -> String {
    use rand::RngCore;
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    base64url_no_pad(&b)
}

/// 生成 PKCE code_verifier（32 字节随机 → base64url 无填充）。
pub fn generate_code_verifier() -> String {
    random_base64url_32()
}

/// 生成 OAuth `state` 参数（32 字节随机 → base64url 无填充）。对应 TS `generateState`。
///
/// 与 `generate_code_verifier` 共用随机源。Web OAuth 流程用它做 CSRF 防护：授权请求带上
/// state，callback 回来必须逐字符比对。
pub fn generate_state() -> String {
    random_base64url_32()
}

/// S256 code_challenge：SHA-256(verifier) → base64url 无填充。
///
/// # Examples
///
/// ```
/// use acosmi::{generate_code_verifier, code_challenge};
///
/// let verifier = generate_code_verifier();
/// let challenge = code_challenge(&verifier);
/// assert!(!challenge.is_empty());
/// assert_eq!(code_challenge(&verifier), challenge); // 同 verifier 确定性
/// ```
pub fn code_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64url_no_pad(&digest)
}

fn base64url_no_pad(b: &[u8]) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    URL_SAFE_NO_PAD.encode(b)
}

// =============================================================================
// LoginWithHandler 事件模型
// =============================================================================

/// 登录流程事件类型。对应 TS `LoginEventType`。
pub const EVENT_AUTH_URL: &str = "auth_url";
pub const EVENT_COMPLETE: &str = "complete";
pub const EVENT_ERROR: &str = "error";

/// 登录错误分类码。对应 TS `LoginErrCode` 常量。
pub const ERR_DISCOVERY: &str = "discovery_failed";
pub const ERR_REGISTRATION: &str = "registration_failed";
pub const ERR_BROWSER_OPEN: &str = "browser_open_failed";
pub const ERR_AUTH_DENIED: &str = "auth_denied";
pub const ERR_TIMEOUT: &str = "auth_timeout";
pub const ERR_TOKEN_EXCHANGE: &str = "token_exchange_failed";
pub const ERR_SSL_PROXY: &str = "ssl_proxy_detected";
/// Web OAuth callback 的 state 与发起时不一致（CSRF 防护）。
pub const ERR_STATE_MISMATCH: &str = "state_mismatch";

/// 登录流程事件。对应 TS `LoginEvent`。
#[derive(Debug, Clone, Default)]
pub struct LoginEvent {
    /// 事件类型（[`EVENT_AUTH_URL`] / [`EVENT_COMPLETE`] / [`EVENT_ERROR`]）。
    pub r#type: String,
    /// 授权 URL（`auth_url` 事件）。
    pub url: Option<String>,
    /// 错误消息（`error` 事件）。
    pub error: Option<String>,
    /// 错误分类码（`error` 事件）。
    pub err_code: Option<String>,
}

impl LoginEvent {
    pub fn auth_url(url: impl Into<String>) -> Self {
        Self {
            r#type: EVENT_AUTH_URL.to_string(),
            url: Some(url.into()),
            ..Default::default()
        }
    }
    pub fn complete() -> Self {
        Self {
            r#type: EVENT_COMPLETE.to_string(),
            ..Default::default()
        }
    }
    pub fn error(code: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            r#type: EVENT_ERROR.to_string(),
            err_code: Some(code.into()),
            error: Some(error.into()),
            ..Default::default()
        }
    }
}

/// 登录选项（对应 TS `LoginOptions`，函数选项模式）。
#[derive(Debug, Clone, Default)]
pub struct LoginOptions {
    /// 跳过自动打开浏览器（由调用方控制浏览器）。
    pub skip_browser: bool,
    /// SSO 场景下的 email 预填。
    pub login_hint: Option<String>,
    /// 登录方法（如 `"sso"`）。
    pub login_method: Option<String>,
    /// 强制组织登录。
    pub org_uuid: Option<String>,
    /// 自定义 token 有效期（秒）。
    pub expires_in: Option<i64>,
    /// 桌面 loopback 授权成功后 302 重定向的品牌成功页 URL（替代本地 127.0.0.1 HTML）。
    ///
    /// 必须随调用方部署 origin 变化，不可硬编码单一域名；缺失 / 非法时回退本地 HTML（零回归）。
    pub success_redirect_url: Option<String>,
}

// =============================================================================
// Token Exchange
// =============================================================================

/// authorization_code 换 token。对应 TS `exchangeCode`。
pub async fn exchange_code(
    http: &reqwest::Client,
    meta: &ServerMetadata,
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenResponse> {
    let params = [
        ("grant_type", "authorization_code"),
        ("client_id", client_id),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("code_verifier", code_verifier),
    ];
    post_token(http, &meta.token_endpoint, &params).await
}

/// 与 [`exchange_code`] 相同，但附带 `expires_in` 参数（setup-token 模式）。
/// 对应 TS `exchangeCodeWithExpiry`（TS barrel 漏 re-export，此处补齐导出）。
pub async fn exchange_code_with_expiry(
    http: &reqwest::Client,
    meta: &ServerMetadata,
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
    expires_in: i64,
) -> Result<TokenResponse> {
    let expires_in_str = expires_in.to_string();
    let params = [
        ("grant_type", "authorization_code"),
        ("client_id", client_id),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("code_verifier", code_verifier),
        ("expires_in", expires_in_str.as_str()),
    ];
    post_token(http, &meta.token_endpoint, &params).await
}

/// 刷新 access_token。对应 TS `refreshToken`。
///
/// 轮换语义（换新撤旧）由 Client 协调：网关签发新 refresh_token，旧 RT 服务端作废。
pub async fn refresh_token(
    http: &reqwest::Client,
    meta: &ServerMetadata,
    client_id: &str,
    refresh_token_value: &str,
) -> Result<TokenResponse> {
    let params = [
        ("grant_type", "refresh_token"),
        ("client_id", client_id),
        ("refresh_token", refresh_token_value),
    ];
    post_token(http, &meta.token_endpoint, &params).await
}

/// 吊销 token。服务端不支持吊销（`revocation_endpoint` 为空）时静默跳过。对应 TS `revokeToken`。
pub async fn revoke_token(
    http: &reqwest::Client,
    meta: &ServerMetadata,
    token: &str,
) -> Result<()> {
    if meta.revocation_endpoint.is_empty() {
        return Ok(());
    }
    let params = [("token", token)];
    http.post(&meta.revocation_endpoint)
        .timeout(AUTH_TIMEOUT)
        .form(&params)
        .send()
        .await
        .map_err(|e| Error::other(format!("revoke: {e}")))?;
    Ok(())
}

async fn post_token(
    http: &reqwest::Client,
    endpoint: &str,
    params: &[(&str, &str)],
) -> Result<TokenResponse> {
    let resp = http
        .post(endpoint)
        .timeout(AUTH_TIMEOUT)
        .form(params)
        .send()
        .await
        .map_err(|e| Error::other(format!("token request: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        // 尽力解析 OAuth error body；失败则空串（bug-for-bug 与 TS 一致）。
        let (oauth_error, error_description) = match resp.json::<serde_json::Value>().await {
            Ok(body) => (
                body.get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                body.get("error_description")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
            ),
            Err(_) => (String::new(), String::new()),
        };
        return Err(Error::OAuthTokenEndpoint(OAuthTokenEndpointError {
            status,
            oauth_error,
            error_description,
        }));
    }

    resp.json()
        .await
        .map_err(|e| Error::other(format!("token: decode: {e}")))
}

/// 从 `TokenResponse` 构造可持久化的 `TokenSet`。对应 TS `newTokenSet`。
///
/// 根因修复 #14：`expires_in=0` 会导致 token 立即过期 → 无限刷新循环。最少保证 60 秒有效期。
pub fn new_token_set(resp: &TokenResponse, client_id: &str, server_url: &str) -> TokenSet {
    let mut expires_in = resp.expires_in;
    if expires_in < 60 {
        expires_in = 60;
    }
    let expires_at = (Utc::now() + chrono::Duration::seconds(expires_in)).to_rfc3339_opts(
        chrono::SecondsFormat::Millis,
        true, // use_z：UTC 后缀 'Z'，对齐 TS toISOString()。
    );
    TokenSet {
        access_token: resp.access_token.clone(),
        refresh_token: resp.refresh_token.clone().unwrap_or_default(),
        expires_at,
        scope: resp.scope.clone().unwrap_or_default(),
        client_id: client_id.to_string(),
        server_url: server_url.to_string(),
    }
}

// =============================================================================
// Web OAuth（浏览器整页重定向流程，平台无关纯逻辑）
// =============================================================================

/// Web OAuth 授权请求 —— `create_web_authorization_request` 的返回结构。对应 TS `WebAuthorizationRequest`。
///
/// 发起方应把整个对象持久化为 pending 状态（HttpOnly cookie 等），callback 回来时取出供
/// `complete_web_authorization_request` 校验。
#[derive(Debug, Clone)]
pub struct WebAuthorizationRequest {
    /// 完整授权 URL —— 浏览器整页跳转到此地址。
    pub auth_url: String,
    /// CSRF state —— callback 必须回传同值。
    pub state: String,
    /// PKCE code_verifier —— 换 token 时使用，切勿泄露给浏览器 URL。
    pub verifier: String,
    /// 动态注册得到的 client_id。
    pub client_id: String,
    /// 本次授权使用的 redirect_uri。
    pub redirect_uri: String,
    /// OAuth issuer / SDK 业务域名（用于 callback 阶段重新发现 metadata）。
    pub server_url: String,
    /// 创建时间戳（Unix 毫秒），发起方可据此实现 TTL 过期判定。
    pub created_at: i64,
}

/// `create_web_authorization_request` 参数。对应 TS `CreateWebAuthorizationRequestOptions`。
#[derive(Debug, Clone)]
pub struct CreateWebAuthorizationRequestOptions {
    /// 动态注册得到的 client_id。
    pub client_id: String,
    /// redirect_uri（须与动态注册登记的一致）。
    pub redirect_uri: String,
    /// 请求的 scope 列表 —— 以空格连接写入 auth_url。
    pub scopes: Vec<String>,
    /// OAuth issuer / SDK 业务域名。
    pub server_url: String,
    /// 可选 —— SSO 场景下预填邮箱。
    pub login_hint: Option<String>,
}

/// 构造 Web OAuth 授权请求。对应 TS `createWebAuthorizationRequest`。
///
/// 生成 PKCE verifier + S256 challenge + CSRF state，按 OAuth 2.1 拼装授权 URL
/// （含 `state` 参数 —— 桌面 `authorize` 不带 state，Web 流程必须带）。
///
/// 返回的 [`WebAuthorizationRequest`] 应整体持久化为 pending 状态，callback 阶段交给
/// `complete_web_authorization_request`。
pub fn create_web_authorization_request(
    meta: &ServerMetadata,
    opts: &CreateWebAuthorizationRequestOptions,
) -> Result<WebAuthorizationRequest> {
    let verifier = generate_code_verifier();
    let challenge = code_challenge(&verifier);
    let state = generate_state();

    let mut auth_url = url::Url::parse(&meta.authorization_endpoint).map_err(|e| {
        Error::other(format!(
            "createWebAuthorizationRequest: invalid auth endpoint: {e}"
        ))
    })?;
    {
        let mut q = auth_url.query_pairs_mut();
        q.append_pair("response_type", "code");
        q.append_pair("client_id", &opts.client_id);
        q.append_pair("redirect_uri", &opts.redirect_uri);
        q.append_pair("code_challenge", &challenge);
        q.append_pair("code_challenge_method", "S256");
        q.append_pair("state", &state);
        q.append_pair("scope", &opts.scopes.join(" "));
        if let Some(hint) = &opts.login_hint {
            q.append_pair("login_hint", hint);
        }
    }

    Ok(WebAuthorizationRequest {
        auth_url: auth_url.to_string(),
        state,
        verifier,
        client_id: opts.client_id.clone(),
        redirect_uri: opts.redirect_uri.clone(),
        server_url: opts.server_url.clone(),
        created_at: Utc::now().timestamp_millis(),
    })
}

/// `complete_web_authorization_request` 的 pending 状态（[`WebAuthorizationRequest`] 的子集）。
/// 对应 TS `WebAuthorizationPending`。
#[derive(Debug, Clone)]
pub struct WebAuthorizationPending {
    pub state: String,
    pub verifier: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub server_url: String,
}

/// `complete_web_authorization_request` 的 callback 参数。对应 TS `WebAuthorizationCallbackParams`。
#[derive(Debug, Clone)]
pub struct WebAuthorizationCallbackParams {
    /// 授权服务器回传的 authorization code。
    pub code: String,
    /// 授权服务器回传的 state —— 必须与 `pending.state` 一致。
    pub state: String,
}

/// 完成 Web OAuth 授权 —— 校验 state → 换 token → 返回可持久化的 `TokenSet`。
/// 对应 TS `completeWebAuthorizationRequest`。
///
/// state 不匹配时返 `Err`（CSRF 防护）。内部依次：
/// `discover_web_oauth_metadata(pending.server_url)` → `exchange_code(...)` → `new_token_set(...)`。
pub async fn complete_web_authorization_request(
    http: &reqwest::Client,
    pending: &WebAuthorizationPending,
    params: &WebAuthorizationCallbackParams,
) -> Result<TokenSet> {
    if params.code.is_empty() {
        return Err(Error::other(
            "completeWebAuthorizationRequest: missing authorization code",
        ));
    }
    if params.state != pending.state {
        return Err(Error::other(format!(
            "completeWebAuthorizationRequest: {ERR_STATE_MISMATCH}: callback state does not match pending state (possible CSRF)"
        )));
    }

    let meta = discover_web_oauth_metadata(http, &pending.server_url).await?;
    let resp = exchange_code(
        http,
        &meta,
        &pending.client_id,
        &params.code,
        &pending.redirect_uri,
        &pending.verifier,
    )
    .await?;
    Ok(new_token_set(
        &resp,
        &pending.client_id,
        &pending.server_url,
    ))
}

// =============================================================================
// authorize (loopback HTTP server) —— 桌面 OAuth
// =============================================================================

/// `authorize` 的返回（授权码 + 实际 redirect_uri）。对应 TS `AuthorizeResult`。
#[derive(Debug, Clone)]
pub struct AuthorizeResult {
    pub code: String,
    pub redirect_uri: String,
}

/// 按 OAuth 2.1 PKCE 拼装授权 URL（loopback 桌面流程；不带 state，对齐 TS `authorize`）。
///
/// 与浏览器 Web 流程的区别：桌面 loopback 不带 `state` 参数（本地回环天然防 CSRF），
/// 走 `org_uuid` / `login_method` 等桌面专属参数。`#[cfg(feature = "desktop-loopback")]`
/// 下的 `authorize` 用此 helper 构造 URL。
pub fn build_desktop_authorization_url(
    meta: &ServerMetadata,
    client_id: &str,
    redirect_uri: &str,
    challenge: &str,
    scopes: &[String],
    opts: &LoginOptions,
) -> Result<String> {
    let mut auth_url = url::Url::parse(&meta.authorization_endpoint)
        .map_err(|e| Error::other(format!("authorize: invalid auth endpoint: {e}")))?;
    {
        let mut q = auth_url.query_pairs_mut();
        q.append_pair("client_id", client_id);
        q.append_pair("redirect_uri", redirect_uri);
        q.append_pair("response_type", "code");
        q.append_pair("code_challenge", challenge);
        q.append_pair("code_challenge_method", "S256");
        if !scopes.is_empty() {
            q.append_pair("scope", &scopes.join(" "));
        }
        if let Some(hint) = &opts.login_hint {
            q.append_pair("login_hint", hint);
        }
        if let Some(method) = &opts.login_method {
            q.append_pair("login_method", method);
        }
        if let Some(org) = &opts.org_uuid {
            q.append_pair("orgUUID", org);
        }
    }
    Ok(auth_url.to_string())
}

/// 校验调用方提供的桌面授权成功页重定向 URL。对应 TS `resolveSuccessRedirect`。
///
/// 仅接受 `http(s)` 绝对 URL —— 拒绝 `javascript:` / `data:` 等其他协议（纵深防御，防开放重定向）。
/// 缺失或非法时返回 `None`，调用方据此回退本地成功 HTML。
pub fn resolve_success_redirect(raw: Option<&str>) -> Option<String> {
    let raw = raw?;
    let u = url::Url::parse(raw).ok()?;
    match u.scheme() {
        "http" | "https" => Some(u.to_string()),
        _ => None,
    }
}

#[cfg(feature = "desktop-loopback")]
pub use loopback::authorize;

#[cfg(feature = "desktop-loopback")]
mod loopback {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// HTML 转义（对应 TS `htmlEscape`）。
    fn html_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;")
    }

    /// 执行 OAuth 2.1 PKCE 授权流程（loopback HTTP server，桌面 only）。对应 TS `authorize`。
    ///
    /// 1) 启动本地 127.0.0.1 HTTP server（随机端口）
    /// 2) 由调用方打开浏览器（`handler` 收到 `auth_url` 事件）—— 不引入 `open` dep，
    ///    SDK 只构造 URL 并发 `EVENT_AUTH_URL`；是否自动打开浏览器由调用方决定
    /// 3) 接收 `/callback` 拿到 authorization code
    /// 4) 返回 code + verifier 供后续 token 交换
    ///
    /// 取消：传入的 `CancellationToken` 触发时返回超时错误。
    ///
    /// 注：TS 版在 `skip_browser=false` 时自动 spawn `open`/`xdg-open`/`rundll32`；Rust 原生
    /// 版**不内置浏览器拉起**（避免引新 dep），统一由调用方监听 `EVENT_AUTH_URL` 事件后自行打开。
    /// 这是相位内的合理取舍（与方案 §4.3 "loopback 用 tiny_http + open" 的差异已记录）。
    #[allow(clippy::too_many_arguments)]
    pub async fn authorize(
        meta: &ServerMetadata,
        client_id: &str,
        scopes: &[String],
        opts: &LoginOptions,
        handler: Option<&(dyn Fn(LoginEvent) + Send + Sync)>,
        signal: Option<tokio_util::sync::CancellationToken>,
    ) -> Result<(AuthorizeResult, String)> {
        let emit = |e: LoginEvent| {
            if let Some(h) = handler {
                h(e);
            }
        };

        let verifier = generate_code_verifier();
        let challenge = code_challenge(&verifier);

        // 1) 启动本地 callback server（阻塞 listener 放 blocking 线程）。
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| Error::other(format!("authorize: bind loopback: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| Error::other(format!("authorize: local_addr: {e}")))?
            .port();
        let redirect_uri = format!("http://127.0.0.1:{port}/callback");

        // 2) 构造授权 URL + 发事件。
        let auth_url = build_desktop_authorization_url(
            meta,
            client_id,
            &redirect_uri,
            &challenge,
            scopes,
            opts,
        )?;
        emit(LoginEvent::auth_url(auth_url.clone()));

        let success_redirect = resolve_success_redirect(opts.success_redirect_url.as_deref());

        // 3) 在 blocking 线程上 accept 单个 callback 请求。
        let accept = tokio::task::spawn_blocking(move || -> Result<String> {
            // listener accept 单连接（OAuth callback 是单次跳转）。
            let (mut stream, _) = listener
                .accept()
                .map_err(|e| Error::other(format!("authorize: accept: {e}")))?;
            // 读请求行（"GET /callback?code=... HTTP/1.1"）。
            let mut buf = [0u8; 8192];
            let n = stream
                .read(&mut buf)
                .map_err(|e| Error::other(format!("authorize: read: {e}")))?;
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();

            // 解析 query。基址仅用于相对路径解析。
            let full = format!("http://127.0.0.1{path}");
            let parsed = url::Url::parse(&full)
                .map_err(|e| Error::other(format!("authorize: parse callback url: {e}")))?;
            if parsed.path() != "/callback" {
                write_http(&mut stream, 404, "text/plain", "");
                return Err(Error::other("authorize: unexpected callback path"));
            }
            let mut code: Option<String> = None;
            let mut err_desc = String::new();
            let mut err_code = String::new();
            for (k, v) in parsed.query_pairs() {
                match k.as_ref() {
                    "code" => code = Some(v.to_string()),
                    "error_description" => err_desc = v.to_string(),
                    "error" => err_code = v.to_string(),
                    _ => {}
                }
            }

            match code {
                Some(c) => {
                    // 成功页：品牌 302 或本地 HTML。
                    if let Some(redir) = &success_redirect {
                        let resp = format!(
                            "HTTP/1.1 302 Found\r\nLocation: {redir}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        );
                        let _ = stream.write_all(resp.as_bytes());
                    } else {
                        let html = "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>授权成功</title></head>\
                            <body style=\"font-family:system-ui,sans-serif;text-align:center;padding:60px 20px\">\
                            <h2>授权成功</h2><p>已完成身份认证, 请返回应用继续使用。</p>\
                            <p style=\"color:#888;font-size:14px\">此窗口将在 3 秒后自动关闭…</p>\
                            <script>setTimeout(function(){window.close()},3000)</script></body></html>";
                        write_http(&mut stream, 200, "text/html; charset=utf-8", html);
                    }
                    Ok(c)
                }
                None => {
                    let msg = if !err_desc.is_empty() {
                        err_desc
                    } else {
                        err_code
                    };
                    let html = format!(
                        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>授权失败</title></head>\
                        <body style=\"font-family:system-ui,sans-serif;text-align:center;padding:60px 20px\">\
                        <h2>授权失败</h2><p>{}</p>\
                        <p style=\"color:#888;font-size:14px\">可以关闭此窗口。</p></body></html>",
                        html_escape(&msg)
                    );
                    write_http(&mut stream, 200, "text/html; charset=utf-8", &html);
                    Err(Error::other(format!("authorization denied: {msg}")))
                }
            }
        });

        // 4) race：callback vs 取消。
        let code = if let Some(cancel) = signal {
            tokio::select! {
                res = accept => res
                    .map_err(|e| Error::other(format!("authorize: join: {e}")))?,
                _ = cancel.cancelled() => Err(Error::other("authorization timed out")),
            }
        } else {
            accept
                .await
                .map_err(|e| Error::other(format!("authorize: join: {e}")))?
        };

        match code {
            Ok(c) => Ok((
                AuthorizeResult {
                    code: c,
                    redirect_uri,
                },
                verifier,
            )),
            Err(e) => {
                let msg = e.to_string();
                let code = if msg.contains("denied") {
                    ERR_AUTH_DENIED
                } else if msg.contains("timed out") {
                    ERR_TIMEOUT
                } else {
                    ERR_TOKEN_EXCHANGE
                };
                emit(LoginEvent::error(code, msg));
                Err(e)
            }
        }
    }

    fn write_http(stream: &mut std::net::TcpStream, status: u16, content_type: &str, body: &str) {
        let reason = match status {
            200 => "OK",
            404 => "Not Found",
            _ => "OK",
        };
        let resp = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_rfc7636_vector() {
        // RFC 7636 Appendix B 测试向量。
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = code_challenge(verifier);
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn verifier_and_state_are_base64url_43_chars() {
        // 32 字节 base64url 无填充 = 43 字符。
        let v = generate_code_verifier();
        let s = generate_state();
        assert_eq!(v.len(), 43);
        assert_eq!(s.len(), 43);
        assert!(!v.contains('+') && !v.contains('/') && !v.contains('='));
        assert_ne!(v, s); // 两次调用应不同（极高概率）。
    }

    #[test]
    fn new_token_set_floors_short_expiry_to_60s() {
        let resp = TokenResponse {
            access_token: "at".into(),
            token_type: "Bearer".into(),
            expires_in: 0, // 应被 floor 到 60s（防无限刷新）。
            refresh_token: Some("rt".into()),
            scope: Some("ai".into()),
        };
        let ts = new_token_set(&resp, "cid", "https://acosmi.com");
        assert_eq!(ts.client_id, "cid");
        assert_eq!(ts.refresh_token, "rt");
        // expires_at 应是有效 RFC3339 且在未来。
        let parsed = chrono::DateTime::parse_from_rfc3339(&ts.expires_at).unwrap();
        assert!(parsed > Utc::now());
        // 'Z' 后缀（对齐 toISOString）。
        assert!(ts.expires_at.ends_with('Z'));
    }

    #[test]
    fn new_token_set_missing_refresh_and_scope_default_empty() {
        let resp = TokenResponse {
            access_token: "at".into(),
            token_type: "Bearer".into(),
            expires_in: 3600,
            refresh_token: None,
            scope: None,
        };
        let ts = new_token_set(&resp, "cid", "https://acosmi.com");
        assert_eq!(ts.refresh_token, "");
        assert_eq!(ts.scope, "");
    }

    #[test]
    fn resolve_success_redirect_rejects_non_http() {
        assert!(resolve_success_redirect(None).is_none());
        assert!(resolve_success_redirect(Some("javascript:alert(1)")).is_none());
        assert!(resolve_success_redirect(Some("data:text/html,x")).is_none());
        assert!(resolve_success_redirect(Some("not a url")).is_none());
        assert!(resolve_success_redirect(Some("https://acosmi.com/ok")).is_some());
        assert!(resolve_success_redirect(Some("http://127.0.0.1/cb")).is_some());
    }

    #[test]
    fn is_invalid_grant_detects_oauth_error() {
        let e = Error::OAuthTokenEndpoint(OAuthTokenEndpointError {
            status: 400,
            oauth_error: "invalid_grant".into(),
            error_description: "refresh token not found".into(),
        });
        assert!(is_invalid_grant_error(&e));
        let other = Error::other("nope");
        assert!(!is_invalid_grant_error(&other));
    }

    #[test]
    fn build_desktop_authorization_url_has_pkce_params() {
        let meta = ServerMetadata {
            issuer: "https://acosmi.com".into(),
            authorization_endpoint: "https://acosmi.com/oauth/authorize".into(),
            token_endpoint: "https://acosmi.com/oauth/token".into(),
            revocation_endpoint: String::new(),
            registration_endpoint: "https://acosmi.com/oauth/register".into(),
            scopes_supported: vec![],
        };
        let opts = LoginOptions {
            login_hint: Some("a@b.com".into()),
            org_uuid: Some("org1".into()),
            ..Default::default()
        };
        let url = build_desktop_authorization_url(
            &meta,
            "cid",
            "http://127.0.0.1:1234/callback",
            "CHAL",
            &["ai".to_string(), "skills".to_string()],
            &opts,
        )
        .unwrap();
        assert!(url.contains("code_challenge=CHAL"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("client_id=cid"));
        assert!(url.contains("scope=ai+skills"));
        assert!(url.contains("login_hint=a%40b.com"));
        assert!(url.contains("orgUUID=org1"));
        // 桌面流程不带 state。
        assert!(!url.contains("state="));
    }
}
