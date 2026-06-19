//! 主 API 客户端。端口自 `core/client.ts`（其端口自 `acosmi-sdk-go/client.go`）。
//!
//! 相位说明：P1 仅建**最小骨架**（Config / Client / 构造 / create / 同步 helper /
//! token 字段 / ensure_token 骨架）。chat / listModels / 各业务方法及 refresh 实现待 P2-P5。
//!
//! ## 可变状态模型（方案 §4.2）
//! TS 直接 mutate `this.tokens` / `this.meta` 并用 `withMu`(异步互斥) 包裹 refresh 临界区。
//! Rust 拆为：`RwLock<Option<TokenSet>>`（快速 sync 读写，不跨 await）+ `tokio::Mutex<()>`
//! 单航班门（refresh-rotation 临界区，跨 await）+ `AtomicBool`（login 进行中）。
//! Client 持 `Arc<ClientInner>`，`Clone` 廉价共享。

use crate::auth::auth as oauth;
use crate::auth::types::{token_set_is_expired, ServerMetadata, TokenSet};
use crate::core::http::{
    parse_http_error_with_retry_after, read_limited_text, MAX_ERROR_BODY_SIZE, MODEL_CACHE_TTL_MS,
};
use crate::core::retry::{effective_policy, EffectiveRetryPolicy, RetryPolicy};
use crate::core::store::{FileTokenStore, InMemoryTokenStore, TokenStore};
use crate::macros::open_string_union;
use crate::models::types::{
    zero_model_capabilities, InputModality, ManagedModel, ModelCapabilities, QuotaSummary,
};
use crate::shared::api_response::ApiResponse;
use crate::shared::errors::{Error, Result};
use serde::de::DeserializeOwned;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use tokio_util::sync::CancellationToken;
use url::Url;

/// 默认网关 base URL。
pub const DEFAULT_GATEWAY_BASE_URL: &str = "https://acosmi.com";
/// 非流式 JSON 子 client 默认超时（毫秒）。
pub const DEFAULT_API_TIMEOUT_MS: u64 = 60_000;

/// OAuth CORS 被拦截错误码标识。
pub const ERR_OAUTH_CORS_BLOCKED: &str = "oauth_cors_blocked";
/// refresh 代理失败错误码标识。
pub const ERR_REFRESH_PROXY_FAILED: &str = "refresh_proxy_failed";
/// token 过期错误码标识。
pub const ERR_TOKEN_EXPIRED: &str = "token_expired";

open_string_union! {
    /// 网关 entitlement 过滤状态（来自 `X-Entitlement-Filter-Status` 响应头）。开放联合。
    FilterStatus {
        OK => "ok",
        ADMIN_BYPASS => "admin-bypass",
        INTERNAL_BYPASS => "internal-bypass",
        DISABLED_BY_FLAG => "disabled-by-flag",
        FALLBACK_TKDIST_ERROR => "fallback-tkdist-error",
        FALLBACK_TKDIST_SKEW => "fallback-tkdist-deployment-skew",
        FALLBACK_NO_BUCKETS => "fallback-no-buckets",
        FALLBACK_MISSING_USER => "fallback-missing-userid",
        /// 空串 = Unknown / 老 nexus。
        UNKNOWN => "",
    }
}

/// 浏览器 token 刷新模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrowserRefreshMode {
    /// issuer 直刷（默认）。
    #[default]
    Direct,
    /// 经 refresh 代理（规避 issuer CORS）。
    ServerProxy,
    /// 不刷新。
    None,
}

/// OAuth 元数据 profile。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OAuthMetadataProfile {
    /// 桌面（默认）。
    #[default]
    Desktop,
    /// 浏览器 / Web。
    Web,
}

/// 客户端配置。所有字段可选。
///
/// 偏移说明：TS 有 `serverURL`/`baseURL`/`baseUrl` 三别名（JS 命名容忍）；Rust 收敛为
/// 单一 `server_url`。
#[derive(Default)]
pub struct Config {
    /// 网关 base URL；缺省 [`DEFAULT_GATEWAY_BASE_URL`]。
    pub server_url: Option<String>,
    /// 自定义 TokenStore；缺省按平台选（原生 = File）。
    pub store: Option<Arc<dyn TokenStore>>,
    /// 自定义 HTTP client（对应 TS `fetchImpl`）。
    pub http: Option<reqwest::Client>,
    /// 重试策略；`None` = 禁用重试（与 TS 默认一致）。
    pub retry_policy: Option<RetryPolicy>,
    /// compliance 端点 base override。
    pub compliance_base_url: Option<String>,
    /// 业务 API 端点 base override。
    pub api_base_url: Option<String>,
    /// OAuth 元数据 profile。
    pub oauth_metadata_profile: Option<OAuthMetadataProfile>,
    /// 浏览器刷新模式。
    pub browser_refresh_mode: Option<BrowserRefreshMode>,
    /// refresh 代理 URL（server-proxy 模式）。
    pub refresh_proxy_url: Option<String>,
}

/// Client 可变 token 状态。
struct ClientInner {
    // ── 不可变配置 ──
    server_url: String,
    compliance_base_url: Option<String>,
    api_base_url: Option<String>,
    oauth_metadata_profile: OAuthMetadataProfile,
    browser_refresh_mode: BrowserRefreshMode,
    refresh_proxy_url: Option<String>,
    /// 共享 HTTP client。P4（chat/请求层）起使用。
    #[allow(dead_code)]
    http: reqwest::Client,
    store: Arc<dyn TokenStore>,
    /// 生效重试策略。P4（请求层）起使用。
    #[allow(dead_code)]
    retry_policy: Option<EffectiveRetryPolicy>,

    // ── 可变状态 ──
    tokens: RwLock<Option<TokenSet>>,
    /// 模型列表缓存（缺省模式 list_models 写入，全集模式不写）。对应 TS `modelCache`。
    model_cache: RwLock<Vec<crate::models::ManagedModel>>,
    /// 模型缓存写入时刻（TTL 判定基准）。对应 TS `modelCacheTimeMs`。
    model_cache_time: RwLock<Option<std::time::Instant>>,
    /// lazy 加载的 OAuth server 元数据（discover 结果缓存）。
    meta: RwLock<Option<ServerMetadata>>,
    /// login 进行中标志（单航班）。`ensure_token` 在 tokens==null 时据此决定是否等待。
    login_in_flight: AtomicBool,
    /// refresh-rotation 临界区门（对应 TS withMu）：进程内单航班，串行化 refresh。
    mu: tokio::sync::Mutex<()>,
    /// login 就绪信号（等待方解阻塞）。login 成功 / token 写入后 `notify_waiters()`。
    token_ready: tokio::sync::Notify,
}

/// 主 API 客户端。`Clone` 廉价（内部 `Arc`）。
#[derive(Clone)]
pub struct Client {
    inner: Arc<ClientInner>,
}

impl Client {
    /// 同步构造（对应 TS constructor）。校验并归一化 URL；不预载 token（见 [`Client::create`]）。
    pub fn new(cfg: Config) -> Result<Self> {
        let server_url = match &cfg.server_url {
            Some(s) => normalize_gateway_base_url(s)?,
            None => DEFAULT_GATEWAY_BASE_URL.to_string(),
        };
        let compliance_base_url = match &cfg.compliance_base_url {
            Some(s) => Some(normalize_override_base_url(s, "complianceBaseURL")?),
            None => None,
        };
        let api_base_url = match &cfg.api_base_url {
            Some(s) => Some(normalize_override_base_url(s, "apiBaseURL")?),
            None => None,
        };

        let http = match cfg.http {
            Some(c) => c,
            None => reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(DEFAULT_API_TIMEOUT_MS))
                .build()
                .map_err(Error::from)?,
        };

        let store: Arc<dyn TokenStore> = match cfg.store {
            Some(s) => s,
            None => default_store(),
        };

        let retry_policy = effective_policy(cfg.retry_policy.as_ref());

        Ok(Client {
            inner: Arc::new(ClientInner {
                server_url,
                compliance_base_url,
                api_base_url,
                oauth_metadata_profile: cfg.oauth_metadata_profile.unwrap_or_default(),
                browser_refresh_mode: cfg.browser_refresh_mode.unwrap_or_default(),
                refresh_proxy_url: cfg.refresh_proxy_url,
                http,
                store,
                retry_policy,
                tokens: RwLock::new(None),
                model_cache: RwLock::new(Vec::new()),
                model_cache_time: RwLock::new(None),
                meta: RwLock::new(None),
                login_in_flight: AtomicBool::new(false),
                mu: tokio::sync::Mutex::new(()),
                token_ready: tokio::sync::Notify::new(),
            }),
        })
    }

    /// 异步工厂（对应 TS `Client.create`）。从 store 预载 token；store 损坏静默忽略。
    pub async fn create(cfg: Config) -> Result<Self> {
        let client = Client::new(cfg)?;
        if let Ok(Some(t)) = client.inner.store.load().await {
            *client.inner.tokens.write().unwrap() = Some(t);
        }
        Ok(client)
    }

    // ── 同步 helper（对应 TS isAuthorized / getServerURL / getBaseURL / getTokenSet）──

    /// 是否已持有 token（同步）。
    pub fn is_authorized(&self) -> bool {
        self.inner.tokens.read().unwrap().is_some()
    }

    /// 网关 base URL。
    pub fn server_url(&self) -> &str {
        &self.inner.server_url
    }

    /// `server_url` 的别名（对应 TS getBaseURL）。
    pub fn base_url(&self) -> &str {
        &self.inner.server_url
    }

    /// 当前 token 快照（克隆）。
    pub fn token_set(&self) -> Option<TokenSet> {
        self.inner.tokens.read().unwrap().clone()
    }

    /// compliance 端点 base override（`None` = 走默认 `{server_url}/admin-api`）。
    pub fn compliance_base_url(&self) -> Option<&str> {
        self.inner.compliance_base_url.as_deref()
    }

    /// 业务 API 端点 base override（`None` = 走默认 `server_url`）。
    pub fn api_base_url(&self) -> Option<&str> {
        self.inner.api_base_url.as_deref()
    }

    /// OAuth 元数据 profile。
    pub fn oauth_metadata_profile(&self) -> OAuthMetadataProfile {
        self.inner.oauth_metadata_profile
    }

    /// 浏览器刷新模式。
    pub fn browser_refresh_mode(&self) -> BrowserRefreshMode {
        self.inner.browser_refresh_mode
    }

    /// refresh 代理 URL。
    pub fn refresh_proxy_url(&self) -> Option<&str> {
        self.inner.refresh_proxy_url.as_deref()
    }

    /// 共享 HTTP client（内部 / 业务方法使用）。P4 起使用。
    #[allow(dead_code)]
    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.inner.http
    }

    /// 生效重试策略（`None` = 禁用）。P4 起使用。
    #[allow(dead_code)]
    pub(crate) fn retry_policy(&self) -> Option<&EffectiveRetryPolicy> {
        self.inner.retry_policy.as_ref()
    }

    // ── 授权生命周期（对应 TS login / loginWithHandler / logout）──

    /// 完整授权流程：发现 → 注册 → 授权（PKCE loopback）→ 换 token → 持久化。
    /// 对应 TS `login`。`app_name` 为桌面智能体名称；`scopes` 见 [`crate::auth::scopes`] 预设。
    pub async fn login(
        &self,
        app_name: &str,
        scopes: &[String],
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        self.login_internal(app_name, scopes, None, &Default::default(), signal)
            .await
    }

    /// 带事件回调的登录流程（CrabCode 使用）。对应 TS `loginWithHandler`。
    ///
    /// `handler` 在以下时刻被调用：`EVENT_AUTH_URL`（授权 URL 就绪）/ `EVENT_COMPLETE`
    /// （登录成功，tokens 已持久化）/ `EVENT_ERROR`（某步骤失败，附错误码）。
    pub async fn login_with_handler(
        &self,
        app_name: &str,
        scopes: &[String],
        handler: Option<&(dyn Fn(oauth::LoginEvent) + Send + Sync)>,
        opts: &oauth::LoginOptions,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        self.login_internal(app_name, scopes, handler, opts, signal)
            .await
    }

    async fn login_internal(
        &self,
        app_name: &str,
        scopes: &[String],
        handler: Option<&(dyn Fn(oauth::LoginEvent) + Send + Sync)>,
        opts: &oauth::LoginOptions,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        let emit = |e: oauth::LoginEvent| {
            if let Some(h) = handler {
                h(e);
            }
        };
        let emit_error = |code: &str, err: &Error| {
            emit(oauth::LoginEvent::error(code, err.to_string()));
        };

        // 单航班门：标记 login 进行中，确保 finally 复位。
        self.inner.login_in_flight.store(true, Ordering::SeqCst);
        let result = self
            .login_steps(app_name, scopes, handler, opts, signal, &emit, &emit_error)
            .await;
        self.inner.login_in_flight.store(false, Ordering::SeqCst);
        result
    }

    #[allow(clippy::too_many_arguments)]
    async fn login_steps(
        &self,
        app_name: &str,
        scopes: &[String],
        handler: Option<&(dyn Fn(oauth::LoginEvent) + Send + Sync)>,
        opts: &oauth::LoginOptions,
        signal: Option<CancellationToken>,
        emit: &dyn Fn(oauth::LoginEvent),
        emit_error: &dyn Fn(&str, &Error),
    ) -> Result<()> {
        // 1. 发现
        let meta = match oauth::discover(self.http(), &self.inner.server_url).await {
            Ok(m) => m,
            Err(e) => {
                emit_error(oauth::ERR_DISCOVERY, &e);
                return Err(Error::other(format!("discovery failed: {e}")));
            }
        };
        *self.inner.meta.write().unwrap() = Some(meta.clone());

        // 2. 已有 client_id 则复用，无则注册
        let mut client_id = self.cached_client_id();
        if client_id.is_empty() {
            match oauth::register(self.http(), &meta, app_name).await {
                Ok(reg) => client_id = reg.client_id,
                Err(e) => {
                    emit_error(oauth::ERR_REGISTRATION, &e);
                    return Err(Error::other(format!("registration failed: {e}")));
                }
            }
        }

        // 3. 授权（PKCE + loopback callback）。失败 → 清 client_id 重注册再试一次。
        let (result, verifier) = match self
            .do_authorize(&meta, &client_id, scopes, opts, handler, signal.clone())
            .await
        {
            Ok(r) => r,
            Err(first_err) => {
                match oauth::register(self.http(), &meta, app_name).await {
                    Ok(reg) => client_id = reg.client_id,
                    Err(reg_err) => {
                        emit_error(oauth::ERR_REGISTRATION, &reg_err);
                        return Err(Error::other(format!(
                            "authorization failed (retry registration also failed): {first_err}"
                        )));
                    }
                }
                match self
                    .do_authorize(&meta, &client_id, scopes, opts, handler, signal.clone())
                    .await
                {
                    Ok(r) => r,
                    Err(e2) => return Err(Error::other(format!("authorization failed: {e2}"))),
                }
            }
        };

        // 4. 换 token（支持自定义 expires_in）
        let token_resp = {
            let exchange = if let Some(exp) = opts.expires_in.filter(|&e| e > 0) {
                oauth::exchange_code_with_expiry(
                    self.http(),
                    &meta,
                    &client_id,
                    &result.code,
                    &result.redirect_uri,
                    &verifier,
                    exp,
                )
                .await
            } else {
                oauth::exchange_code(
                    self.http(),
                    &meta,
                    &client_id,
                    &result.code,
                    &result.redirect_uri,
                    &verifier,
                )
                .await
            };
            match exchange {
                Ok(r) => r,
                Err(e) => {
                    let code = if oauth::is_ssl_error(&e.to_string()) {
                        oauth::ERR_SSL_PROXY
                    } else {
                        oauth::ERR_TOKEN_EXCHANGE
                    };
                    emit_error(code, &e);
                    return Err(Error::other(format!("token exchange failed: {e}")));
                }
            }
        };

        // 5. 持久化 + 通知等待方
        let tokens = oauth::new_token_set(&token_resp, &client_id, &self.inner.server_url);
        *self.inner.tokens.write().unwrap() = Some(tokens.clone());
        self.inner.token_ready.notify_waiters();
        if let Err(e) = self.inner.store.save(&tokens).await {
            return Err(Error::other(format!("save tokens: {e}")));
        }

        // 6. 完成
        emit(oauth::LoginEvent::complete());
        Ok(())
    }

    /// loopback 授权步骤。feature `desktop-loopback` 启用时走真实 loopback HTTP server；
    /// 否则返回占位错误（desktop-loopback: P2 占位）。
    #[allow(unused_variables)]
    async fn do_authorize(
        &self,
        meta: &ServerMetadata,
        client_id: &str,
        scopes: &[String],
        opts: &oauth::LoginOptions,
        handler: Option<&(dyn Fn(oauth::LoginEvent) + Send + Sync)>,
        signal: Option<CancellationToken>,
    ) -> Result<(oauth::AuthorizeResult, String)> {
        #[cfg(feature = "desktop-loopback")]
        {
            oauth::authorize(meta, client_id, scopes, opts, handler, signal).await
        }
        #[cfg(not(feature = "desktop-loopback"))]
        {
            // desktop-loopback: 未启用 feature 时不内置 loopback HTTP server。
            // 浏览器侧应改用 create_web_authorization_request / complete_web_authorization_request
            // （平台无关纯逻辑）；桌面 loopback 登录请开启 `desktop-loopback` feature。
            Err(Error::other(
                "login() requires the `desktop-loopback` feature (loopback HTTP callback server); \
                 for browser flows use create_web_authorization_request / complete_web_authorization_request",
            ))
        }
    }

    /// 吊销 token 并清除本地存储。对应 TS `logout`。
    pub async fn logout(&self, signal: Option<CancellationToken>) -> Result<()> {
        let _ = &signal; // revoke 经 auth helper（auth 专用超时）；取消信号当前未穿透到 revoke。
        let tokens = self.inner.tokens.write().unwrap().take();
        let mut meta = self.inner.meta.write().unwrap().take();
        // 重置等待信号：下次 login 重新触发等待→唤醒流程。
        self.inner.login_in_flight.store(false, Ordering::SeqCst);

        if let Some(tokens) = tokens {
            if meta.is_none() {
                // token-lifecycle discovery：revoke 必须打与签发 token 同 profile 的端点。
                match self.discover_for_lifecycle().await {
                    Ok(m) => meta = Some(m),
                    Err(e) => {
                        eprintln!("[acosmi-sdk] warning: discover for revocation failed: {e}");
                    }
                }
            }
            if let Some(meta) = meta {
                // 吊销失败静默忽略（best-effort）。
                let _ = oauth::revoke_token(self.http(), &meta, &tokens.access_token).await;
                let _ = oauth::revoke_token(self.http(), &meta, &tokens.refresh_token).await;
            }
        }

        self.inner.store.clear().await
    }

    // ── Token 管理（对应 TS ensureToken / forceRefresh）──

    /// 确保有有效 access_token，过期则自动刷新。对应 TS `ensureToken`。
    ///
    /// 并发语义（对齐 TS withMu + storeWithLock + syncFromDisk + tokenReady）：
    ///   - tokens==null 且 login 进行中 → 等 `token_ready`（配合 `signal` 取消）；非进行中 → 报未授权。
    ///   - 未过期 → 直接返回（无锁路径）。
    ///   - 过期 → 双层串行：`mu`（进程内单航班）+ `store.lock`（跨进程临界区）；进入临界区后
    ///     先 `store.load()` 重读磁盘（多进程 rotation 防 400），双检过期决定是否真刷新。
    pub async fn ensure_token(&self, signal: Option<CancellationToken>) -> Result<String> {
        let mut tokens = self.token_set();

        if tokens.is_none() {
            if !self.inner.login_in_flight.load(Ordering::SeqCst) {
                return Err(Error::other("not authorized, call login() first"));
            }
            // login 进行中：等待 token 就绪或 abort。
            // 先注册 notified()（避免 lost-wakeup），再复检 token；循环到 token 出现或 abort。
            loop {
                let notified = self.inner.token_ready.notified();
                if let Some(t) = self.token_set() {
                    tokens = Some(t);
                    break;
                }
                if !self.inner.login_in_flight.load(Ordering::SeqCst) {
                    // login 已结束但仍无 token（失败 / 被 logout 重置）。
                    return Err(Error::other("not authorized, call login() first"));
                }
                match &signal {
                    Some(cancel) => {
                        tokio::select! {
                            _ = notified => {}
                            _ = cancel.cancelled() => {
                                return Err(Error::other("waiting for token: aborted"));
                            }
                        }
                    }
                    None => notified.await,
                }
                if let Some(t) = self.token_set() {
                    tokens = Some(t);
                    break;
                }
            }
        }

        let tokens = tokens.expect("tokens present after wait");
        if !token_set_is_expired(&tokens) {
            return Ok(tokens.access_token);
        }

        // 需刷新 — 双层串行：mu 进程内 + store.lock 跨进程。
        let _g = self.inner.mu.lock().await;
        let _lock = self.inner.store.lock().await?;

        // 进入临界区后先同步磁盘（别的进程可能已 rotation）。
        self.sync_from_disk().await;
        // 双检。
        let cur = self
            .token_set()
            .ok_or_else(|| Error::other("not authorized, call login() first"))?;
        if !token_set_is_expired(&cur) {
            return Ok(cur.access_token);
        }

        self.refresh_current_token(signal).await?;
        self.token_set()
            .map(|t| t.access_token)
            .ok_or_else(|| Error::other("not authorized, call login() first"))
    }

    /// 强制刷新 token（401 重试用）。对应 TS `forceRefresh`。
    ///
    /// 同 [`Self::ensure_token`] 的刷新路径：mu + store.lock + syncFromDisk。别的进程刚 rotation
    /// 过的话磁盘上是新 RT，本进程用磁盘新 RT 即可成功；否则用旧 RT 必撞 "refresh token not found" 400。
    pub async fn force_refresh(&self, signal: Option<CancellationToken>) -> Result<()> {
        let _g = self.inner.mu.lock().await;
        let _lock = self.inner.store.lock().await?;
        self.sync_from_disk().await;
        if self.token_set().is_none() {
            return Err(Error::other("no tokens to refresh"));
        }
        self.refresh_current_token(signal).await
    }

    // ── 私有 helper ──

    fn cached_client_id(&self) -> String {
        self.inner
            .tokens
            .read()
            .unwrap()
            .as_ref()
            .map(|t| t.client_id.clone())
            .unwrap_or_default()
    }

    /// token-lifecycle discovery：revoke / refresh 必须打与签发 token 同 profile 的端点。
    async fn discover_for_lifecycle(&self) -> Result<ServerMetadata> {
        let profile = match self.inner.oauth_metadata_profile {
            OAuthMetadataProfile::Desktop => oauth::OAuthMetadataProfile::Desktop,
            OAuthMetadataProfile::Web => oauth::OAuthMetadataProfile::Web,
        };
        oauth::discover_with_profile(self.http(), &self.inner.server_url, profile).await
    }

    /// 刷新当前 token（轮换：换新撤旧）。`browser_refresh_mode` 决定 direct / server-proxy / none。
    async fn refresh_current_token(&self, signal: Option<CancellationToken>) -> Result<()> {
        if self.token_set().is_none() {
            return Err(Error::other("no tokens to refresh"));
        }
        match self.inner.browser_refresh_mode {
            BrowserRefreshMode::None => Err(Error::other(format!(
                "{ERR_TOKEN_EXPIRED}: token refresh disabled"
            ))),
            BrowserRefreshMode::ServerProxy => self.refresh_via_proxy(signal).await,
            BrowserRefreshMode::Direct => self.refresh_direct().await,
        }
    }

    async fn refresh_direct(&self) -> Result<()> {
        let cur = self
            .token_set()
            .ok_or_else(|| Error::other("no tokens to refresh"))?;

        // 确保 meta（与签发同 profile）。
        if self.inner.meta.read().unwrap().is_none() {
            match self.discover_for_lifecycle().await {
                Ok(m) => *self.inner.meta.write().unwrap() = Some(m),
                Err(e) => return Err(Error::other(format!("discover for refresh: {e}"))),
            }
        }
        let meta = self
            .inner
            .meta
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| Error::other("discover for refresh: no metadata"))?;

        let token_resp = match oauth::refresh_token(
            self.http(),
            &meta,
            &cur.client_id,
            &cur.refresh_token,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                if oauth::is_invalid_grant_error(&e) {
                    self.clear_invalid_refresh_token().await;
                    return Err(Error::other(format!(
                        "refresh token invalid; local tokens cleared: {e}"
                    )));
                }
                let msg = e.to_string();
                if is_likely_browser_oauth_cors_error(&msg) {
                    return Err(Error::other(format!(
                        "{ERR_OAUTH_CORS_BLOCKED}: refresh token: {msg}"
                    )));
                }
                return Err(Error::other(format!("refresh token: {msg}")));
            }
        };

        let new_set = oauth::new_token_set(&token_resp, &cur.client_id, &self.inner.server_url);
        *self.inner.tokens.write().unwrap() = Some(new_set.clone());
        self.save_refreshed_token(&new_set).await;
        Ok(())
    }

    async fn refresh_via_proxy(&self, signal: Option<CancellationToken>) -> Result<()> {
        let cur = self
            .token_set()
            .ok_or_else(|| Error::other("no tokens to refresh"))?;
        let proxy_url = self.inner.refresh_proxy_url.as_deref().ok_or_else(|| {
            Error::other(format!(
                "{ERR_REFRESH_PROXY_FAILED}: refreshProxyURL is required"
            ))
        })?;

        let server_url = if cur.server_url.is_empty() {
            self.inner.server_url.clone()
        } else {
            cur.server_url.clone()
        };
        let body = serde_json::json!({
            "client_id": cur.client_id,
            "refresh_token": cur.refresh_token,
            "server_url": server_url,
        });

        let req = self.http().post(proxy_url).json(&body);
        let send = req.send();
        let resp = match signal {
            Some(cancel) => tokio::select! {
                r = send => r,
                _ = cancel.cancelled() => {
                    return Err(Error::other(format!("{ERR_REFRESH_PROXY_FAILED}: aborted")));
                }
            },
            None => send.await,
        }
        .map_err(|e| Error::other(format!("{ERR_REFRESH_PROXY_FAILED}: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let (mut message, oauth_error) = match resp.json::<serde_json::Value>().await {
                Ok(b) => {
                    let err = b
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let desc = b
                        .get("error_description")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    (if desc.is_empty() { err.clone() } else { desc }, err)
                }
                Err(_) => (String::new(), String::new()),
            };
            if oauth_error == "invalid_grant" {
                self.clear_invalid_refresh_token().await;
                return Err(Error::other(format!(
                    "{ERR_REFRESH_PROXY_FAILED}: refresh token invalid; local tokens cleared"
                )));
            }
            if message.is_empty() {
                message = format!("HTTP {status}");
            }
            return Err(Error::other(format!(
                "{ERR_REFRESH_PROXY_FAILED}: HTTP {status}: {message}"
            )));
        }

        #[derive(serde::Deserialize)]
        struct ProxyResp {
            #[serde(default)]
            #[serde(rename = "tokenSet")]
            token_set: Option<TokenSet>,
        }
        let parsed: ProxyResp = resp
            .json()
            .await
            .map_err(|e| Error::other(format!("{ERR_REFRESH_PROXY_FAILED}: decode: {e}")))?;
        let new_set = parsed.token_set.ok_or_else(|| {
            Error::other(format!(
                "{ERR_REFRESH_PROXY_FAILED}: response missing tokenSet"
            ))
        })?;

        *self.inner.tokens.write().unwrap() = Some(new_set.clone());
        self.save_refreshed_token(&new_set).await;
        Ok(())
    }

    async fn save_refreshed_token(&self, tokens: &TokenSet) {
        if let Err(e) = self.inner.store.save(tokens).await {
            eprintln!("[acosmi-sdk] warning: save refreshed token failed: {e}");
        }
    }

    async fn clear_invalid_refresh_token(&self) {
        *self.inner.tokens.write().unwrap() = None;
        *self.inner.meta.write().unwrap() = None;
        self.inner.login_in_flight.store(false, Ordering::SeqCst);
        if let Err(e) = self.inner.store.clear().await {
            eprintln!("[acosmi-sdk] warning: clear invalid token failed: {e}");
        }
    }

    /// 从磁盘同步 token（refresh 前）。对应 TS `syncFromDisk`。
    ///
    /// 别的进程 rotation 后磁盘 refresh_token 已变，本进程内存仍是旧 R0；不同步直接 refresh
    /// 必撞网关 400。load 失败保留内存继续（容错）。
    async fn sync_from_disk(&self) {
        let on_disk = match self.inner.store.load().await {
            Ok(Some(t)) => t,
            Ok(None) => return,
            // 磁盘读失败（损坏 / 权限）— 不阻塞 refresh，让后续 refresh_token 暴露真实错误。
            Err(_) => return,
        };
        let adopt = {
            let cur = self.inner.tokens.read().unwrap();
            match cur.as_ref() {
                None => true,
                Some(c) => on_disk.refresh_token != c.refresh_token,
            }
        };
        if adopt {
            *self.inner.tokens.write().unwrap() = Some(on_disk);
        }
    }

    // ===========================================================================
    // URL 拼接（对应 TS apiURL）
    // ===========================================================================

    /// 业务 API URL 拼接。`api_base_url` 覆盖（未配置时 === server_url）；尾段非 `/api/v4` 时追加。
    /// 对应 TS `apiURL`。
    pub fn api_url(&self, path: &str) -> String {
        let mut base = self
            .inner
            .api_base_url
            .clone()
            .unwrap_or_else(|| self.inner.server_url.clone());
        if !base.ends_with("/api/v4") {
            base.push_str("/api/v4");
        }
        base.push_str(path);
        base
    }

    // ===========================================================================
    // 通用 JSON GET（最小 do_json_get：GET + Bearer + ApiResponse 解包 + 401 单次重试）
    // ===========================================================================

    /// GET 请求并反序列化为 `T`，同时返回响应头。对应 TS `doJSONFull<T>('GET',...)` 的 GET 子集。
    ///
    /// 行为对齐 `doJSONFullInternal`：ensure_token → Bearer → 非 2xx 抛 HttpError（含 Retry-After）→
    /// 401 单次 force_refresh 重试 → 空 body 跳业务码（这里 GET 端点都返回 ApiResponse，调用方自处理）→
    /// JSON 反序列化 + 业务码检查（`code != 0` 抛 BusinessError）。
    async fn do_json_get_full<T: DeserializeOwned>(
        &self,
        path: &str,
        signal: Option<CancellationToken>,
    ) -> Result<(T, reqwest::header::HeaderMap)> {
        self.do_json_get_internal(path, signal, false).await
    }

    async fn do_json_get_internal<T: DeserializeOwned>(
        &self,
        path: &str,
        signal: Option<CancellationToken>,
        retried: bool,
    ) -> Result<(T, reqwest::header::HeaderMap)> {
        let token = self.ensure_token(signal.clone()).await?;
        let url = self.api_url(path);

        let send = self
            .http()
            .get(&url)
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .send();
        let resp = match &signal {
            Some(cancel) => tokio::select! {
                r = send => r,
                _ = cancel.cancelled() => {
                    return Err(Error::other(format!("GET {path}: aborted")));
                }
            },
            None => send.await,
        }
        .map_err(|e| {
            Error::Network(crate::core::http::classify_transport(
                &format!("GET {path}"),
                &url,
                &e,
            ))
        })?;

        let status = resp.status();

        // 401：单次 force_refresh 重试（防递归）。
        if status.as_u16() == 401 && !retried {
            self.force_refresh(signal.clone())
                .await
                .map_err(|e| Error::other(format!("unauthorized and refresh failed: {e}")))?;
            return Box::pin(self.do_json_get_internal(path, signal, true)).await;
        }

        if !status.is_success() {
            let retry_after = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<i64>().ok())
                .filter(|&s| s > 0)
                .unwrap_or(0);
            let body = read_limited_text(resp.bytes_stream(), MAX_ERROR_BODY_SIZE).await?;
            return Err(Error::Http(parse_http_error_with_retry_after(
                status.as_u16(),
                &body,
                retry_after,
            )));
        }

        let headers = resp.headers().clone();
        let text = resp
            .text()
            .await
            .map_err(|e| Error::other(format!("GET {path}: read body: {e}")))?;
        if text.is_empty() {
            // 空 body 成功响应 —— ApiResponse<T> 端点不应空体；按强类型显式 Err（方案 §4.4）。
            return Err(Error::other(format!("GET {path}: empty response body")));
        }
        let result: T = serde_json::from_str(&text)
            .map_err(|e| Error::other(format!("GET {path}: decode: {e}")))?;
        Ok((result, headers))
    }

    // ===========================================================================
    // Managed Models
    // ===========================================================================

    /// 获取可用的托管模型列表。对应 TS `listModels`。
    ///
    /// 不返回 entitlement-filter-status header；想据 fallback 状态显示降级提示请改用
    /// [`Self::list_models_with_status`]。`include_locked=true` 请求全集模式（picker=1）。
    pub async fn list_models(
        &self,
        signal: Option<CancellationToken>,
        include_locked: bool,
    ) -> Result<Vec<ManagedModel>> {
        Ok(self
            .list_models_with_status(signal, include_locked)
            .await?
            .0)
    }

    /// 获取可用模型列表，同时返回 `X-Entitlement-Filter-Status` header。对应 TS `listModelsWithStatus`。
    ///
    /// `include_locked=true` → 全集模式（picker=1，含越档 locked 模型供 picker 展示），
    /// **不写** model_cache（避免 locked 模型混入可用集）；缺省模式照旧缓存。
    pub async fn list_models_with_status(
        &self,
        signal: Option<CancellationToken>,
        include_locked: bool,
    ) -> Result<(Vec<ManagedModel>, FilterStatus)> {
        let path = if include_locked {
            "/managed-models?picker=1"
        } else {
            "/managed-models"
        };
        let (result, headers): (ApiResponse<Vec<ManagedModel>>, _) =
            self.do_json_get_full(path, signal).await?;
        if let Some(err) = result.business_error() {
            return Err(err);
        }
        // v1.2：写缓存前归一化 input_modalities（snake）→ input_modalities（camel `inputModalities`）。
        let normalized = normalize_input_modalities(result.data);

        if !include_locked {
            *self.inner.model_cache.write().unwrap() = normalized.clone();
            *self.inner.model_cache_time.write().unwrap() = Some(std::time::Instant::now());
        }

        let status: FilterStatus = headers
            .get("X-Entitlement-Filter-Status")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())
            .map(FilterStatus::from)
            .unwrap_or_else(|| FilterStatus::UNKNOWN.into());
        Ok((normalized, status))
    }

    /// 查询当前用户账户级权益总览（v0.19+）。对应 TS `getQuotaSummary`。
    pub async fn get_quota_summary(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<QuotaSummary> {
        let (resp, _): (ApiResponse<QuotaSummary>, _) = self
            .do_json_get_full("/entitlements/quota-summary", signal)
            .await?;
        if let Some(err) = resp.business_error() {
            return Err(err);
        }
        Ok(resp.data)
    }

    /// 查询单个模型的能力矩阵。优先从 list_models 缓存读取，miss 时刷新一次。
    /// 对应 TS `getModelCapabilities`。仍 miss → 零值（与 Go/TS 一致）。
    pub async fn get_model_capabilities(
        &self,
        model_id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<ModelCapabilities> {
        if let Some(caps) = self.cached_capabilities(model_id) {
            return Ok(caps);
        }
        self.list_models(signal, false).await?;
        if let Some(caps) = self.cached_capabilities(model_id) {
            return Ok(caps);
        }
        // 模型不在列表中，返回零值。
        Ok(zero_model_capabilities())
    }

    /// 从缓存查 capabilities（缓存空或过 TTL → None）。对应 TS `getCachedCapabilities`。
    fn cached_capabilities(&self, model_id: &str) -> Option<ModelCapabilities> {
        let cache = self.inner.model_cache.read().unwrap();
        let time = *self.inner.model_cache_time.read().unwrap();
        let expired = match time {
            Some(t) => t.elapsed().as_millis() as u64 > MODEL_CACHE_TTL_MS,
            None => true,
        };
        if cache.is_empty() || expired {
            return None;
        }
        for m in cache.iter() {
            if m.id == model_id || m.model_id == model_id {
                return Some(m.capabilities.clone());
            }
        }
        None
    }
}

/// 归一化上游 ManagedModel 列表的 `input_modalities`（snake）→ `inputModalities`（camel）。
/// 对应 TS `normalizeInputModalities`：camelCase 已存在则保留；仅 snake 存在时拷贝并滤白名单；
/// 都没有则保 `None`（"未声明" ≠ text-only）。
fn normalize_input_modalities(mut models: Vec<ManagedModel>) -> Vec<ManagedModel> {
    for m in models.iter_mut() {
        if m.input_modalities.is_some() {
            // camelCase 已存在 —— 清掉 snake 镜像字段，避免重复持有。
            m.input_modalities_snake = None;
            continue;
        }
        if let Some(snake) = m.input_modalities_snake.take() {
            let filtered: Vec<InputModality> = snake
                .into_iter()
                .filter_map(|v| match v.as_str() {
                    Some("text") => Some(InputModality::Text),
                    Some("image") => Some(InputModality::Image),
                    _ => None,
                })
                .collect();
            m.input_modalities = Some(filtered);
        }
    }
    models
}

/// 浏览器 OAuth CORS 错误启发式判定（对应 TS `isLikelyBrowserOAuthCORSError`）。
fn is_likely_browser_oauth_cors_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("failed to fetch")
        || lower.contains("networkerror")
        || lower.contains("cors")
        || lower.contains("http 403")
}

fn default_store() -> Arc<dyn TokenStore> {
    // 原生：优先 File；HOME 不可用时退 InMemory。
    if std::env::var_os("HOME").is_some() || std::env::var_os("USERPROFILE").is_some() {
        Arc::new(FileTokenStore::new(None))
    } else {
        Arc::new(InMemoryTokenStore::new())
    }
}

// =============================================================================
// URL 归一化（对应 TS normalizeGatewayBaseURL / normalizeOverrideBaseURL）
// =============================================================================

/// 归一化网关 base URL：仅许 http/https；拒空 / 拒 query / 拒 fragment / 拒非法 URL；
/// 去尾随 `/`，返回 `scheme://host[:port]/path`。
pub fn normalize_gateway_base_url(input: &str) -> Result<String> {
    normalize_base(input, "serverURL")
}

/// 同 [`normalize_gateway_base_url`]，用于 complianceBaseURL / apiBaseURL override。
pub fn normalize_override_base_url(raw: &str, label: &str) -> Result<String> {
    normalize_base(raw, label)
}

fn normalize_base(input: &str, label: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(Error::other(format!("{label}: must not be empty")));
    }
    let u = Url::parse(trimmed).map_err(|e| Error::other(format!("{label}: invalid URL: {e}")))?;
    match u.scheme() {
        "http" | "https" => {}
        s => {
            return Err(Error::other(format!(
                "{label}: unsupported scheme \"{s}\" (only http/https allowed)"
            )))
        }
    }
    if u.query().is_some() || u.fragment().is_some() {
        return Err(Error::other(format!(
            "{label}: must not contain query or fragment"
        )));
    }
    let host = u
        .host_str()
        .ok_or_else(|| Error::other(format!("{label}: missing host")))?;
    let mut out = format!("{}://{}", u.scheme(), host);
    if let Some(port) = u.port() {
        out.push_str(&format!(":{port}"));
    }
    let path = u.path().trim_end_matches('/');
    out.push_str(path);
    Ok(out)
}
