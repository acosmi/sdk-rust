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
    classify_transport, iter_sse_lines, parse_http_error_with_retry_after, parse_stream_error,
    read_limited_text, CHAT_REQUEST_TIMEOUT_MS, DEFAULT_JSON_TIMEOUT_MS, MAX_ERROR_BODY_SIZE,
    MODEL_CACHE_TTL_MS,
};
use crate::core::retry::{effective_policy, EffectiveRetryPolicy, RetryPolicy, RetryRequestInfo};
use crate::core::store::{FileTokenStore, InMemoryTokenStore, TokenStore};
use crate::macros::open_string_union;
use crate::models::adapters::openai::parse_openai_response_to_anthropic;
use crate::models::adapters::{get_adapter_for_model, Adapter, ProviderFormat};
use crate::models::is_sse_comment_line;
use crate::models::stream_meta::{extract_anthropic_block_meta, BlockMeta};
use crate::models::types::{
    parse_settlement, parse_sources_event, zero_model_capabilities, ChatRequest, ChatResponse,
    EmbeddingRequest, EmbeddingResponse, ImageGenerationRequest, ImageGenerationResponse,
    InputModality, ManagedModel, ModelCapabilities, QuotaSummary, RerankRequest, RerankResponse,
    SourcesEvent, StreamEvent, StreamSettlement, VideoGenerationRequest, VideoTaskResponse,
};
use crate::models::wire_anthropic::AnthropicResponse;
use crate::shared::api_response::ApiResponse;
use crate::shared::errors::{Error, Result};
use futures::Stream;
use futures::StreamExt;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
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
pub(crate) struct ClientInner {
    // ── 不可变配置 ──
    server_url: String,
    compliance_base_url: Option<String>,
    api_base_url: Option<String>,
    oauth_metadata_profile: OAuthMetadataProfile,
    browser_refresh_mode: BrowserRefreshMode,
    refresh_proxy_url: Option<String>,
    /// 共享 HTTP client。
    http: reqwest::Client,
    store: Arc<dyn TokenStore>,
    /// 生效重试策略。
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
    /// V29 模型系数缓存（TTL 8s；对应 TS `coefCacheData`/`coefCacheTimeMs`）。
    /// `@deprecated` 系数已退役（网关恒返回 `[]`），仅向后兼容。
    coef_cache: RwLock<Option<(Vec<crate::billing::ModelCoefficient>, std::time::Instant)>>,
    /// 当前 WebSocket 长连接状态句柄（对应 TS `this.ws`）。`None` = 未连接。
    /// 重复 connect 前先 disconnect 旧连接（防 ws-reconnect-leak）。
    ws: tokio::sync::Mutex<Option<crate::notifications::WsHandle>>,
    /// P8 sanitize-bridge：请求前底线防御配置（体积 / deny-list / 深度）。`None` = 未配置
    /// （对齐 TS `defensiveCfg == null`）。并发安全 sync 读写（不跨 await）。
    #[cfg(feature = "sanitize")]
    pub(crate) defensive_cfg: RwLock<Option<crate::sanitize::MinimalSanitizeConfig>>,
    /// P8 sanitize-bridge：是否每次请求前从 `raw_messages` 剥 `acosmi_ephemeral` 标记块
    /// （对齐 TS `autoStripEphemeral`）。
    #[cfg(feature = "sanitize")]
    pub(crate) auto_strip_ephemeral: AtomicBool,
}

/// 主 API 客户端。`Clone` 廉价（内部 `Arc`）。
#[derive(Clone)]
pub struct Client {
    inner: Arc<ClientInner>,
}

impl Client {
    /// 同步构造（对应 TS constructor）。校验并归一化 URL；不预载 token（见 [`Client::create`]）。
    ///
    /// # Examples
    ///
    /// ```
    /// use acosmi::{Client, Config};
    ///
    /// let client = Client::new(Config {
    ///     server_url: Some("https://acosmi.com".into()),
    ///     ..Default::default()
    /// })
    /// .unwrap();
    /// assert_eq!(client.base_url(), "https://acosmi.com");
    /// ```
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
                // 🔴 绝不设 client 级 `.timeout(...)`：reqwest 的 timeout 覆盖整个响应体（含 SSE 流）
                // 的总死线，会 abort >60s 的流式 SSE，也会抢先砍掉 11min 非流式 chat 的 per-request
                // 死线（TS 用 fetch 无 client 级总超时）。死线一律靠 per-request `derive_timeout_token`
                // （非流式）/ 调用方 signal（流式保长连接）派生。这里仅兜底连接握手。
                .connect_timeout(std::time::Duration::from_secs(30))
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
                coef_cache: RwLock::new(None),
                ws: tokio::sync::Mutex::new(None),
                #[cfg(feature = "sanitize")]
                defensive_cfg: RwLock::new(None),
                #[cfg(feature = "sanitize")]
                auto_strip_ephemeral: AtomicBool::new(false),
            }),
        })
    }

    /// crate 内访问共享内部状态（sanitize-bridge 等同 crate 子模块用）。
    #[cfg(feature = "sanitize")]
    pub(crate) fn inner(&self) -> &ClientInner {
        &self.inner
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

    /// 共享 HTTP client（内部 / 业务方法使用）。
    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.inner.http
    }

    /// V29 系数缓存槽（billing/entitlements 业务方法用）。
    pub(crate) fn coef_cache(
        &self,
    ) -> &RwLock<Option<(Vec<crate::billing::ModelCoefficient>, std::time::Instant)>> {
        &self.inner.coef_cache
    }

    /// 当前 WebSocket 状态槽（notifications/ws 业务方法用）。
    pub(crate) fn ws_slot(&self) -> &tokio::sync::Mutex<Option<crate::notifications::WsHandle>> {
        &self.inner.ws
    }

    /// 生效重试策略（`None` = 禁用）。
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
    pub(crate) async fn do_json_get_full<T: DeserializeOwned>(
        &self,
        path: &str,
        signal: Option<CancellationToken>,
    ) -> Result<(T, reqwest::header::HeaderMap)> {
        self.do_json_get_internal(path, signal, false).await
    }

    /// 公共端点 GET（**无 token**，对应 TS `doPublicJSON`）。skill-store 浏览/详情/resolve 用。
    ///
    /// 不附 Authorization（公共端点），不做 401 force_refresh 重试（无凭证可刷）。
    /// 非 2xx → [`Error::Http`]；空体 → 强 Err（公共 ApiResponse 端点不应空体）。
    pub(crate) async fn do_public_json_full<T: DeserializeOwned>(
        &self,
        path: &str,
        signal: Option<CancellationToken>,
    ) -> Result<T> {
        let url = self.api_url(path);
        // per-request 死线（对齐 TS doPublicJSON 30s）：去掉 client 级总超时后，公共 GET 必须
        // 自派生 30s 子 token，否则变无超时。超时或 parent signal 取消任一触发即 abort。
        let signal = self.derive_timeout_token(DEFAULT_JSON_TIMEOUT_MS, signal);
        let send = self.http().get(&url).send();
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
        if !status.is_success() {
            let retry_after = parse_retry_after_secs(resp.headers());
            let body = read_limited_text(resp.bytes_stream(), MAX_ERROR_BODY_SIZE).await?;
            return Err(Error::Http(parse_http_error_with_retry_after(
                status.as_u16(),
                &body,
                retry_after,
            )));
        }

        let text = resp
            .text()
            .await
            .map_err(|e| Error::other(format!("GET {path}: read body: {e}")))?;
        if text.is_empty() {
            return Err(Error::other(format!("GET {path}: empty response body")));
        }
        serde_json::from_str(&text).map_err(|e| Error::other(format!("GET {path}: decode: {e}")))
    }

    async fn do_json_get_internal<T: DeserializeOwned>(
        &self,
        path: &str,
        signal: Option<CancellationToken>,
        retried: bool,
    ) -> Result<(T, reqwest::header::HeaderMap)> {
        // per-request 死线（对齐 TS doJSONGet 30s）：去掉 client 级总超时后，GET 必须自派生 30s
        // 子 token，否则变无超时。401 重试沿用原 `signal` 各自重新派生（见下方递归调用）。
        let req_signal = self.derive_timeout_token(DEFAULT_JSON_TIMEOUT_MS, signal.clone());
        let token = self.ensure_token(req_signal.clone()).await?;
        let url = self.api_url(path);

        let send = self
            .http()
            .get(&url)
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .send();
        let resp = match &req_signal {
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

        // 401：单次 force_refresh 重试（防递归）。重试沿用原 `signal`（非派生子 token），
        // 递归入口会重新派生新的 30s 死线。
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

    /// 从缓存查找完整 [`ManagedModel`]（未命中返 `None`）。对应 TS `getCachedModel`。
    fn cached_model(&self, model_id: &str) -> Option<ManagedModel> {
        let cache = self.inner.model_cache.read().unwrap();
        cache
            .iter()
            .find(|m| m.id == model_id || m.model_id == model_id)
            .cloned()
    }

    /// 确保指定 `model_id` 的 [`ManagedModel`] 已在缓存中。对应 TS `ensureModelCached`。
    ///
    /// 1. 缓存命中 → 直接返回；
    /// 2. 未命中 → 调 `list_models` 刷新一次；
    /// 3. 刷新后仍未命中 → `Error::ModelNotFound`。
    ///
    /// 根因修复（对齐 TS）：消除未预热场景下 `provider="anthropic"` 硬编码回退，
    /// 该回退会让 non-anthropic 模型被按 Anthropic 格式编码并打到错误端点。
    pub async fn ensure_model_cached(
        &self,
        model_id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<ManagedModel> {
        if let Some(m) = self.cached_model(model_id) {
            return Ok(m);
        }
        self.list_models(signal, false).await?;
        if let Some(m) = self.cached_model(model_id) {
            return Ok(m);
        }
        Err(Error::ModelNotFound {
            model_id: model_id.to_string(),
        })
    }

    // ===========================================================================
    // 请求层（do_request / do_request_with_retry / do_json_full_raw）
    //
    // 红线区分：
    //   - do_request           = 单次 HTTP，**绝不重试**（流式路径独占）。
    //   - do_request_with_retry = 经 retry_policy + safe_to_retry 闸门的非流式包装
    //                             （POST 默认 safe_to_retry=false，不重试，计费安全）。
    //   - 401 单次 force_refresh 重试由 do_json_full_raw / 流式 gen 各自的 `retried` guard 管。
    // ===========================================================================

    /// 单次 HTTP 请求（**无重试**）。对应 TS `doRequest`。
    ///
    /// 传输层错误经 [`classify_transport`] 转 [`Error::Network`]（便于 retry policy 判定）。
    /// 取消信号经 `select!` 接入。**流式路径必须直接走此函数**，绝不经 [`Self::do_request_with_retry`]。
    pub(crate) async fn do_request(
        &self,
        method: reqwest::Method,
        url: &str,
        headers: &[(reqwest::header::HeaderName, String)],
        body: Option<&str>,
        signal: Option<&CancellationToken>,
    ) -> Result<reqwest::Response> {
        let mut rb = self.http().request(method.clone(), url);
        for (k, v) in headers {
            rb = rb.header(k.clone(), v.clone());
        }
        if let Some(b) = body {
            rb = rb.body(b.to_string());
        }
        let send = rb.send();
        let resp = match signal {
            Some(cancel) => tokio::select! {
                r = send => r,
                _ = cancel.cancelled() => {
                    return Err(Error::Network(crate::shared::errors::NetworkError::new(
                        format!("{method} {url}"),
                        url,
                        "request aborted",
                    )));
                }
            },
            None => send.await,
        };
        resp.map_err(|e| {
            let path = Url::parse(url)
                .map(|u| u.path().to_string())
                .unwrap_or_else(|_| url.to_string());
            Error::Network(classify_transport(&format!("{method} {path}"), url, &e))
        })
    }

    /// 带 RetryPolicy 的 [`Self::do_request`] 包装 —— **仅用于非流式路径**。对应 TS `doRequestWithRetry`。
    ///
    /// 闸门：`retry_policy` 缺省（`None`）或 `safe_to_retry` 判定不可重试（POST 默认 false）→
    /// 直接走单次 [`Self::do_request`]。仅 5xx/429 进入 retry 评估（构造 [`Error::Http`] 喂
    /// `on_retryable`）；transport 层错误（[`Error::Network`]）也喂 `on_retryable`。
    ///
    /// **流式路径（[`Self::chat_stream`] / [`Self::chat_messages_stream`]）绝不调用此函数**，
    /// 必须直接用 [`Self::do_request`]（重试 = 双 token + 重复消息 = 双扣，计费安全红线）。
    async fn do_request_with_retry(
        &self,
        method: reqwest::Method,
        url: &str,
        headers: &[(reqwest::header::HeaderName, String)],
        body: Option<&str>,
        signal: Option<&CancellationToken>,
    ) -> Result<reqwest::Response> {
        let policy = match self.retry_policy() {
            Some(p) => p.clone(),
            None => return self.do_request(method, url, headers, body, signal).await,
        };
        let info = RetryRequestInfo {
            method: method.as_str().to_string(),
            url: url.to_string(),
        };
        if !(policy.safe_to_retry)(&info) {
            return self.do_request(method, url, headers, body, signal).await;
        }

        let mut last_err: Option<Error> = None;
        let mut attempt: u32 = 0;
        while attempt < policy.max_attempts {
            match self
                .do_request(method.clone(), url, headers, body, signal)
                .await
            {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    // 仅 5xx/429 进入 retry 评估，其余直接返回。
                    if status < 500 && status != 429 {
                        return Ok(resp);
                    }
                    let retry_after = parse_retry_after_secs(resp.headers());
                    let peek = read_limited_text(resp.bytes_stream(), MAX_ERROR_BODY_SIZE).await?;
                    last_err = Some(Error::Http(parse_http_error_with_retry_after(
                        status,
                        &peek,
                        retry_after,
                    )));
                }
                Err(e) => last_err = Some(e),
            }

            if attempt + 1 == policy.max_attempts {
                break;
            }
            let err_ref = last_err.as_ref().expect("last_err set");
            if !(policy.on_retryable)(err_ref) {
                break;
            }
            let backoff = crate::core::retry::compute_backoff(&policy, attempt, err_ref);
            sleep_with_cancel(backoff, signal).await;
            attempt += 1;
        }
        Err(last_err.unwrap_or_else(|| Error::other("do_request_with_retry: no attempts ran")))
    }

    /// POST/GET JSON 请求，返回 **raw bytes** + 响应头（chat / 媒体用，不立即反序列化）。
    /// 对应 TS `doJSONFullRaw`。
    ///
    /// 行为：ensure_token → Bearer → [`Self::do_request_with_retry`]（POST 默认不重试）→
    /// 401 单次 force_refresh 重试（`retried` guard 防递归）→ 非 2xx 抛 [`Error::Http`]（含 Retry-After）→
    /// 读 body bytes。
    pub(crate) async fn do_json_full_raw(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&str>,
        signal: Option<CancellationToken>,
        timeout_ms: u64,
    ) -> Result<(Vec<u8>, reqwest::header::HeaderMap)> {
        // per-request 超时（对应 TS withRequestTimeout）：派生子 token，超时或 parent 取消任一触发即 abort。
        let ctl = self.derive_timeout_token(timeout_ms, signal);
        self.do_json_full_raw_internal(method, path, body, ctl.as_ref(), false)
            .await
    }

    async fn do_json_full_raw_internal(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&str>,
        signal: Option<&CancellationToken>,
        retried: bool,
    ) -> Result<(Vec<u8>, reqwest::header::HeaderMap)> {
        let token = self.ensure_token(signal.cloned()).await?;
        let url = self.api_url(path);

        let mut headers: Vec<(reqwest::header::HeaderName, String)> =
            vec![(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))];
        if body.is_some() {
            headers.push((
                reqwest::header::CONTENT_TYPE,
                "application/json".to_string(),
            ));
        }

        let resp = self
            .do_request_with_retry(method.clone(), &url, &headers, body, signal)
            .await?;

        // 401：单次 force_refresh 重试（防递归）。
        if resp.status().as_u16() == 401 && !retried {
            drop(resp); // 释放连接（对应 TS resp.body?.cancel()）。
            self.force_refresh(signal.cloned())
                .await
                .map_err(|e| Error::other(format!("unauthorized and refresh failed: {e}")))?;
            return Box::pin(self.do_json_full_raw_internal(method, path, body, signal, true))
                .await;
        }

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let retry_after = parse_retry_after_secs(resp.headers());
            let text = read_limited_text(resp.bytes_stream(), MAX_ERROR_BODY_SIZE).await?;
            return Err(Error::Http(parse_http_error_with_retry_after(
                status,
                &text,
                retry_after,
            )));
        }

        let headers = resp.headers().clone();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| Error::other(format!("{method} {path}: read body: {e}")))?;
        Ok((bytes.to_vec(), headers))
    }

    /// POST/GET JSON + ApiResponse 解包（业务码检查）+ **空体契约**（方案 §4.4）。对应 TS `doJSONFull<T>`。
    ///
    /// 空 body：返回 `Ok((None, headers))`（调用方据返回类型决定是否容忍）；
    /// 非空 body：反序列化 `T`，若 `T` 含 `code` 字段且 `code != 0` 抛 BusinessError。
    /// 这里返回 `Option<T>`，让调用方对空体显式处理（与 GET-only `do_json_get_full` 的强 Err 互补）。
    pub(crate) async fn do_json_full<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&str>,
        signal: Option<CancellationToken>,
    ) -> Result<(Option<T>, reqwest::header::HeaderMap)> {
        let (bytes, headers) = self
            .do_json_full_raw(method, path, body, signal, DEFAULT_JSON_TIMEOUT_MS)
            .await?;
        if bytes.is_empty() {
            // 空 body 成功响应：跳业务码检查，返回 None（对应 TS `undefined as T`）。
            return Ok((None, headers));
        }
        let result: T = serde_json::from_slice(&bytes)
            .map_err(|e| Error::other(format!("{path}: decode: {e}")))?;
        Ok((Some(result), headers))
    }

    // ===========================================================================
    // Chat
    // ===========================================================================

    /// 构建完整聊天请求体（v0.5.0 adapter 模式）。对应 TS `buildChatRequest`。
    ///
    /// `ensure_model_cached` → `get_adapter_for_model` → adapter `build_request_body`。
    /// 返回 `(JSON body 字符串, adapter)`。
    pub async fn build_chat_request(
        &self,
        model_id: &str,
        req: &ChatRequest,
        signal: Option<CancellationToken>,
    ) -> Result<(String, Adapter)> {
        // P8: apply_request_sanitizers —— 请求前防御（体积 / deny-list / 深度 / ephemeral 剥离）。
        // 对齐 TS buildChatRequest 开头调用：未配置零开销 early-return；失败返 Err 放弃本次请求。
        // sanitize 在浅拷贝上原地改写 raw_messages（不污染调用方 req）；用 Cow 避免无配置时拷贝。
        #[cfg(feature = "sanitize")]
        let sanitized: std::borrow::Cow<'_, ChatRequest> = {
            let mut s = req.clone();
            self.apply_request_sanitizers(&mut s)?;
            std::borrow::Cow::Owned(s)
        };
        #[cfg(feature = "sanitize")]
        let req: &ChatRequest = &sanitized;

        let m = self.ensure_model_cached(model_id, signal).await?;
        let adapter = get_adapter_for_model(&m);
        let caps = self
            .cached_capabilities(model_id)
            .unwrap_or_else(zero_model_capabilities);
        let body_map = adapter.build_request_body(&caps, req);
        let body = serde_json::to_string(&body_map)
            .map_err(|e| Error::other(format!("serialize chat request: {e}")))?;
        Ok((body, adapter))
    }

    /// 同步聊天（适合短回复）。对应 TS `chat`。
    ///
    /// 响应的 `token_remaining` / `call_remaining` / `model_token_remaining*` 来自服务端响应头
    /// （反映结算后余额）；缺失保持 [`ChatResponse`] 默认哨兵。v0.5.0 按 provider 路由
    /// `/anthropic` 或 `/chat`。
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use acosmi::Client;
    /// # use acosmi::models::{ChatRequest, ChatMessage};
    /// # async fn demo(client: &Client) -> Result<(), Box<dyn std::error::Error>> {
    /// let req = ChatRequest {
    ///     messages: Some(vec![ChatMessage { role: "user".into(), content: "Hi".into() }]),
    ///     max_tokens: Some(256),
    ///     ..Default::default()
    /// };
    /// let resp = client.chat("claude-opus-4-7", &req, None).await?;
    /// println!("{:?}", resp.content);
    /// # Ok(()) }
    /// ```
    pub async fn chat(
        &self,
        model_id: &str,
        req: &ChatRequest,
        signal: Option<CancellationToken>,
    ) -> Result<ChatResponse> {
        // 浅拷贝 + 强制 stream=false（避免原地 mutate 调用方 req）。
        let mut r = req.clone();
        r.stream = Some(false);

        let (body, adapter) = self
            .build_chat_request(model_id, &r, signal.clone())
            .await?;
        let endpoint = format!(
            "/managed-models/{}{}",
            urlencode(model_id),
            adapter.endpoint_suffix()
        );
        let (bytes, headers) = self
            .do_json_full_raw(
                reqwest::Method::POST,
                &endpoint,
                Some(&body),
                signal,
                CHAT_REQUEST_TIMEOUT_MS,
            )
            .await?;

        let mut resp = adapter.parse_response(&bytes)?;

        // 从响应头提取 token / call 余额（解析失败保持默认哨兵）。
        if let Some(v) = header_i64(&headers, "X-Token-Remaining") {
            resp.token_remaining = v;
        }
        if let Some(v) = header_i64(&headers, "X-Call-Remaining") {
            resp.call_remaining = v;
        }
        if let Some(v) = header_i64(&headers, "X-Token-Remaining-Model") {
            resp.model_token_remaining = v;
        }
        if let Some(v) = header_i64(&headers, "X-Token-Remaining-Model-ETU") {
            resp.model_token_remaining_etu = v;
        }
        Ok(resp)
    }

    // ===========================================================================
    // 向量 / 重排序（v2.9.0）—— 与 chat 同网关、同会员计费（Hold→Settle→Release）
    // ===========================================================================

    /// 向量（同步）。`POST /managed-models/:id/embeddings`。对应 TS `embeddings`。
    ///
    /// `model_id` 须为向量托管模型（`capabilities.supports_embedding=true`，上游接 DashScope）。
    /// 响应为 OpenAI `/v1/embeddings` 标准格式（网关直通，无 `{code,data}` 包装）。
    pub async fn embeddings(
        &self,
        model_id: &str,
        req: &EmbeddingRequest,
        signal: Option<CancellationToken>,
    ) -> Result<EmbeddingResponse> {
        let endpoint = format!("/managed-models/{}/embeddings", urlencode(model_id));
        let body = serde_json::to_string(req)
            .map_err(|e| Error::other(format!("serialize embedding request: {e}")))?;
        let (bytes, _) = self
            .do_json_full_raw(
                reqwest::Method::POST,
                &endpoint,
                Some(&body),
                signal,
                CHAT_REQUEST_TIMEOUT_MS,
            )
            .await?;
        serde_json::from_slice(&bytes).map_err(|e| Error::other(format!("{endpoint}: decode: {e}")))
    }

    /// 重排序（同步）。`POST /managed-models/:id/rerank`。对应 TS `rerank`。
    ///
    /// `model_id` 须为重排序托管模型（`capabilities.supports_rerank=true`，上游接 DashScope）。
    /// 统一扁平契约；响应 `{ results: [{ index, relevance_score, document? }], usage, model }`
    /// （网关已把上游原生嵌套 / OpenAI 兼容扁平两线路归一化，无 `{code,data}` 包装）。
    pub async fn rerank(
        &self,
        model_id: &str,
        req: &RerankRequest,
        signal: Option<CancellationToken>,
    ) -> Result<RerankResponse> {
        let endpoint = format!("/managed-models/{}/rerank", urlencode(model_id));
        let body = serde_json::to_string(req)
            .map_err(|e| Error::other(format!("serialize rerank request: {e}")))?;
        let (bytes, _) = self
            .do_json_full_raw(
                reqwest::Method::POST,
                &endpoint,
                Some(&body),
                signal,
                CHAT_REQUEST_TIMEOUT_MS,
            )
            .await?;
        serde_json::from_slice(&bytes).map_err(|e| Error::other(format!("{endpoint}: decode: {e}")))
    }

    // ===========================================================================
    // 媒体生成（v1.3+）—— 图片 / 视频生成托管模型（与 chat 同网关）
    // ===========================================================================

    /// 解析 nexus-v4 `{code,message,data}` 信封，`code!=0` 抛 BusinessError，返回 `data`。
    /// 对应 TS `unwrapAPIResponse`。
    fn unwrap_api_response<T: DeserializeOwned>(path: &str, bytes: &[u8]) -> Result<T> {
        let env: ApiResponse<T> = serde_json::from_slice(bytes)
            .map_err(|e| Error::other(format!("{path}: decode: {e}")))?;
        if let Some(err) = env.business_error() {
            return Err(err);
        }
        Ok(env.data)
    }

    /// 图片生成（同步）。`POST /managed-models/:id/images/generations`。对应 TS `generateImage`。
    ///
    /// `model_id` 须为图片生成托管模型（`capabilities.supports_image_generation=true`）。
    /// 图片生成耗时常超 30s，用 chat 同级超时（11min）容纳上游。
    pub async fn generate_image(
        &self,
        model_id: &str,
        req: &ImageGenerationRequest,
        signal: Option<CancellationToken>,
    ) -> Result<ImageGenerationResponse> {
        let endpoint = format!("/managed-models/{}/images/generations", urlencode(model_id));
        let body = serde_json::to_string(req)
            .map_err(|e| Error::other(format!("serialize image request: {e}")))?;
        let (bytes, _) = self
            .do_json_full_raw(
                reqwest::Method::POST,
                &endpoint,
                Some(&body),
                signal,
                CHAT_REQUEST_TIMEOUT_MS,
            )
            .await?;
        Self::unwrap_api_response(&endpoint, &bytes)
    }

    /// 创建视频生成任务（异步）。`POST /managed-models/:id/videos/generations`。对应 TS `generateVideo`。
    ///
    /// 返回 `task_id`；用 [`Self::poll_video_task`] 轮询直到 `status=completed`。
    /// 上报真物理量（视频秒数）需在 `poll_video_task` 时回传 `duration`。
    pub async fn generate_video(
        &self,
        model_id: &str,
        req: &VideoGenerationRequest,
        signal: Option<CancellationToken>,
    ) -> Result<VideoTaskResponse> {
        let endpoint = format!("/managed-models/{}/videos/generations", urlencode(model_id));
        let body = serde_json::to_string(req)
            .map_err(|e| Error::other(format!("serialize video request: {e}")))?;
        let (bytes, _) = self
            .do_json_full_raw(
                reqwest::Method::POST,
                &endpoint,
                Some(&body),
                signal,
                DEFAULT_JSON_TIMEOUT_MS,
            )
            .await?;
        Self::unwrap_api_response(&endpoint, &bytes)
    }

    /// 轮询视频任务状态。`GET /managed-models/:id/videos/tasks/:taskId`。对应 TS `pollVideoTask`。
    ///
    /// `duration_seconds`（创建时的时长，秒）透传给网关在 completed 时上报用量。
    pub async fn poll_video_task(
        &self,
        model_id: &str,
        task_id: &str,
        duration_seconds: Option<i64>,
        signal: Option<CancellationToken>,
    ) -> Result<VideoTaskResponse> {
        let mut endpoint = format!(
            "/managed-models/{}/videos/tasks/{}",
            urlencode(model_id),
            urlencode(task_id)
        );
        if let Some(d) = duration_seconds.filter(|&d| d > 0) {
            endpoint.push_str(&format!("?duration={d}"));
        }
        let (bytes, _) = self
            .do_json_full_raw(
                reqwest::Method::GET,
                &endpoint,
                None,
                signal,
                DEFAULT_JSON_TIMEOUT_MS,
            )
            .await?;
        Self::unwrap_api_response(&endpoint, &bytes)
    }

    // ===========================================================================
    // Anthropic 原生格式同步聊天（chat_messages*）
    // ===========================================================================

    /// Anthropic 原生格式同步聊天。对应 TS `chatMessages`。
    ///
    /// v0.5.0 按 provider 路由：Anthropic → `chat_messages_anthropic`（POST /anthropic）；
    /// 其他厂商 → `chat_messages_openai`（POST /chat，响应转换为 [`AnthropicResponse`]）。
    pub async fn chat_messages(
        &self,
        model_id: &str,
        req: &ChatRequest,
        signal: Option<CancellationToken>,
    ) -> Result<AnthropicResponse> {
        let m = self.ensure_model_cached(model_id, signal.clone()).await?;
        let adapter = get_adapter_for_model(&m);
        if adapter.format() == ProviderFormat::Anthropic {
            self.chat_messages_anthropic(model_id, req, adapter, signal)
                .await
        } else {
            self.chat_messages_openai(model_id, req, adapter, signal)
                .await
        }
    }

    async fn chat_messages_anthropic(
        &self,
        model_id: &str,
        req: &ChatRequest,
        adapter: Adapter,
        signal: Option<CancellationToken>,
    ) -> Result<AnthropicResponse> {
        let mut r = req.clone();
        r.stream = Some(false);
        let caps = self
            .cached_capabilities(model_id)
            .unwrap_or_else(zero_model_capabilities);
        let body_map = adapter.build_request_body(&caps, &r);
        let data = serde_json::to_string(&body_map)
            .map_err(|e| Error::other(format!("serialize messages request: {e}")))?;

        let endpoint = format!("/managed-models/{}/anthropic", urlencode(model_id));
        let (bytes, _) = self
            .do_json_full_raw(
                reqwest::Method::POST,
                &endpoint,
                Some(&data),
                signal,
                CHAT_REQUEST_TIMEOUT_MS,
            )
            .await?;

        // 尝试 APIResponse 包装 {"code":0,"message":"...","data":{...}}；data 缺失则 fall through
        // 直接当 AnthropicResponse 解（对齐 TS chatMessagesAnthropic 的 lenient 逻辑）。
        if let Ok(wrapper) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            if let Some(data_val) = wrapper.get("data").filter(|v| !v.is_null()) {
                let code = wrapper.get("code").and_then(|v| v.as_i64()).unwrap_or(0);
                if code != 0 {
                    let msg = wrapper
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    return Err(Error::business(code, msg));
                }
                return serde_json::from_value(data_val.clone()).map_err(|e| {
                    Error::other(format!("decode anthropic response (wrapped): {e}"))
                });
            }
        }
        serde_json::from_slice(&bytes)
            .map_err(|e| Error::other(format!("decode anthropic response: {e}")))
    }

    async fn chat_messages_openai(
        &self,
        model_id: &str,
        req: &ChatRequest,
        adapter: Adapter,
        signal: Option<CancellationToken>,
    ) -> Result<AnthropicResponse> {
        let mut r = req.clone();
        r.stream = Some(false);
        let caps = self
            .cached_capabilities(model_id)
            .unwrap_or_else(zero_model_capabilities);
        let body_map = adapter.build_request_body(&caps, &r);
        let data = serde_json::to_string(&body_map)
            .map_err(|e| Error::other(format!("serialize messages request: {e}")))?;

        let endpoint = format!(
            "/managed-models/{}{}",
            urlencode(model_id),
            adapter.endpoint_suffix()
        );
        let (bytes, _) = self
            .do_json_full_raw(
                reqwest::Method::POST,
                &endpoint,
                Some(&data),
                signal,
                CHAT_REQUEST_TIMEOUT_MS,
            )
            .await?;
        // OpenAI 格式响应 → AnthropicResponse 转换（对齐 TS）。
        parse_openai_response_to_anthropic(&bytes)
    }

    // ===========================================================================
    // 流式聊天（SSE）—— 计费安全红线：只 do_request，绝不重试
    // ===========================================================================

    /// 流式聊天（SSE），返回 `impl Stream<Item = Result<StreamEvent>>`。对应 TS `chatStream`。
    ///
    /// **🔴 计费安全红线**：流式路径**只走单次 `do_request`（内部），绝不经 retry 包装**——
    /// SSE 一旦部分写出，重试 = 双 token + 重复消息 = 双扣。401 仅做一次 force_refresh 重试
    /// （`retried` guard 防递归）。`signal` 经 `select!` 接入 SSE 读循环。
    pub fn chat_stream(
        &self,
        model_id: &str,
        req: &ChatRequest,
        signal: Option<CancellationToken>,
    ) -> impl Stream<Item = Result<StreamEvent>> {
        let this = self.clone();
        let model_id = model_id.to_string();
        let req = req.clone();
        async_stream::try_stream! {
            let inner = this.chat_stream_gen(&model_id, &req, signal, false);
            futures::pin_mut!(inner);
            while let Some(ev) = inner.next().await {
                yield ev?;
            }
        }
    }

    /// 流式聊天 + 自动解析结算 / 搜索来源事件。对应 TS `chatStreamWithUsage`。
    ///
    /// tagged 流：`Settle`（结算）/ `Sources`（搜索来源）/ `Content`（内容增量）。
    /// `started` 控制事件被过滤；`failed`/`error` 事件经 [`parse_stream_error`] 转 [`Error::Stream`]。
    pub fn chat_stream_with_usage(
        &self,
        model_id: &str,
        req: &ChatRequest,
        signal: Option<CancellationToken>,
    ) -> impl Stream<Item = Result<ChatUsageEvent>> {
        let this = self.clone();
        let model_id = model_id.to_string();
        let req = req.clone();
        async_stream::try_stream! {
            let inner = this.chat_stream(&model_id, &req, signal);
            futures::pin_mut!(inner);
            while let Some(ev) = inner.next().await {
                let ev = ev?;
                // 结算事件（settled / pending_settle）。
                if let Some(s) = parse_settlement(&ev) {
                    yield ChatUsageEvent::Settle(s);
                    continue;
                }
                // 搜索来源事件。
                if let Some(src) = parse_sources_event(&ev) {
                    yield ChatUsageEvent::Sources(src);
                    continue;
                }
                // 控制事件：过滤。
                if ev.event == "started" {
                    continue;
                }
                // 失败 / 错误事件：解析后抛出。
                if ev.event == "failed" || ev.event == "error" {
                    Err(Error::Stream(parse_stream_error(&ev.data)))?;
                }
                yield ChatUsageEvent::Content(ev);
            }
        }
    }

    /// Anthropic 原生格式流式聊天（SSE）。对应 TS `chatMessagesStream`。
    ///
    /// 调用 POST `/managed-models/:id/anthropic`（Anthropic 路由）或 `/chat`（OpenAI 路由，
    /// 经 `OpenAIStreamConverter` 转 Anthropic 兼容事件）。同样**只走 do_request，绝不重试**。
    pub fn chat_messages_stream(
        &self,
        model_id: &str,
        req: &ChatRequest,
        signal: Option<CancellationToken>,
    ) -> impl Stream<Item = Result<StreamEvent>> {
        let this = self.clone();
        let model_id = model_id.to_string();
        let req = req.clone();
        async_stream::try_stream! {
            let inner = this.chat_messages_stream_gen(&model_id, &req, signal, false);
            futures::pin_mut!(inner);
            while let Some(ev) = inner.next().await {
                yield ev?;
            }
        }
    }

    /// chat_stream 的真实实现（含 401 单次重试递归）。返回 `impl Stream`。
    ///
    /// 🔴 **只调用 [`Self::do_request`]（单次，无重试）**。401 且未重试过 → force_refresh →
    /// 递归一次（`retried=true`，防再次 401 递归）。
    fn chat_stream_gen<'a>(
        &'a self,
        model_id: &'a str,
        req: &'a ChatRequest,
        signal: Option<CancellationToken>,
        retried: bool,
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send + 'a>> {
        Box::pin(async_stream::try_stream! {
            let mut r = req.clone();
            r.stream = Some(true);
            let (body, adapter) = self.build_chat_request(model_id, &r, signal.clone()).await?;
            let token = self.ensure_token(signal.clone()).await?;

            let endpoint = format!(
                "/managed-models/{}{}",
                urlencode(model_id),
                adapter.endpoint_suffix()
            );
            let url = self.api_url(&endpoint);
            let headers = stream_headers(&token);

            // 🔴 红线：流式只走 do_request（单次），绝不 do_request_with_retry。
            let resp = self
                .do_request(reqwest::Method::POST, &url, &headers, Some(&body), signal.as_ref())
                .await?;

            // 401 单次重试：force_refresh 后递归一次（retried guard 防递归）。
            if resp.status().as_u16() == 401 && !retried {
                drop(resp);
                self.force_refresh(signal.clone()).await.map_err(|e| {
                    Error::other(format!("stream: unauthorized and refresh failed: {e}"))
                })?;
                let inner = self.chat_stream_gen(model_id, req, signal, true);
                futures::pin_mut!(inner);
                while let Some(ev) = inner.next().await {
                    yield ev?;
                }
                return;
            }

            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let retry_after = parse_retry_after_secs(resp.headers());
                let text = read_limited_text(resp.bytes_stream(), MAX_ERROR_BODY_SIZE).await?;
                Err(Error::Http(parse_http_error_with_retry_after(status, &text, retry_after)))?;
                return;
            }

            // Anthropic 路由：维护 block 元数据 map；OpenAI 路由：parse_stream_line 自处理。
            let mut block_type_map: Option<HashMap<i64, BlockMeta>> =
                if adapter.format() == ProviderFormat::Anthropic {
                    Some(HashMap::new())
                } else {
                    None
                };

            let lines = iter_sse_lines(resp.bytes_stream());
            futures::pin_mut!(lines);
            let mut current_event = String::new();
            loop {
                // 取消信号接入 SSE 读循环。cancel 触发 → 注入 abort 错误项。
                let next: Option<Result<String>> = match &signal {
                    Some(cancel) => tokio::select! {
                        l = lines.next() => l,
                        _ = cancel.cancelled() => Some(Err(Error::other("stream: aborted"))),
                    },
                    None => lines.next().await,
                };
                let line = match next {
                    Some(l) => l?,
                    None => break,
                };
                // 跳过 SSE 注释行（": keep-alive"）与空行。
                if is_sse_comment_line(&line) || line.is_empty() {
                    continue;
                }
                if let Some(rest) = line.strip_prefix("event:") {
                    current_event = rest.trim().to_string();
                } else if let Some(rest) = line.strip_prefix("data:") {
                    let data = rest.trim();
                    let (mut ev, done) = adapter.parse_stream_line(&current_event, data)?;
                    if done {
                        return;
                    }
                    if let Some(map) = block_type_map.as_mut() {
                        let (idx, bt, eph) =
                            extract_anthropic_block_meta(&current_event, data, map);
                        if !bt.is_empty() {
                            ev.block_index = Some(idx);
                            ev.block_type = Some(bt);
                            ev.ephemeral = Some(eph);
                        }
                    }
                    yield ev;
                }
            }
        })
    }

    /// chat_messages_stream 的真实实现（含 401 单次重试递归）。
    ///
    /// 🔴 同 [`Self::chat_stream_gen`]：只走 do_request，绝不重试。OpenAI 路由经
    /// `OpenAIStreamConverter` 转 Anthropic 兼容事件；Anthropic 路由原生事件直透 + block 元数据回填。
    fn chat_messages_stream_gen<'a>(
        &'a self,
        model_id: &'a str,
        req: &'a ChatRequest,
        signal: Option<CancellationToken>,
        retried: bool,
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send + 'a>> {
        Box::pin(async_stream::try_stream! {
            let mut r = req.clone();
            r.stream = Some(true);
            let (body, adapter) = self.build_chat_request(model_id, &r, signal.clone()).await?;
            let token = self.ensure_token(signal.clone()).await?;

            let endpoint = format!(
                "/managed-models/{}{}",
                urlencode(model_id),
                adapter.endpoint_suffix()
            );
            let url = self.api_url(&endpoint);
            let headers = stream_headers(&token);

            // 🔴 红线：流式只走 do_request（单次），绝不重试。
            let resp = self
                .do_request(reqwest::Method::POST, &url, &headers, Some(&body), signal.as_ref())
                .await?;

            if resp.status().as_u16() == 401 && !retried {
                drop(resp);
                self.force_refresh(signal.clone()).await.map_err(|e| {
                    Error::other(format!("messages stream: unauthorized and refresh failed: {e}"))
                })?;
                let inner = self.chat_messages_stream_gen(model_id, req, signal, true);
                futures::pin_mut!(inner);
                while let Some(ev) = inner.next().await {
                    yield ev?;
                }
                return;
            }

            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let retry_after = parse_retry_after_secs(resp.headers());
                let text = read_limited_text(resp.bytes_stream(), MAX_ERROR_BODY_SIZE).await?;
                Err(Error::Http(parse_http_error_with_retry_after(status, &text, retry_after)))?;
                return;
            }

            let is_openai = adapter.format() == ProviderFormat::OpenAI;
            let mut converter = if is_openai {
                Some(crate::models::new_openai_stream_converter())
            } else {
                None
            };
            let mut block_type_map: HashMap<i64, BlockMeta> = HashMap::new();

            let lines = iter_sse_lines(resp.bytes_stream());
            futures::pin_mut!(lines);
            let mut current_event = String::new();
            loop {
                let next: Option<Result<String>> = match &signal {
                    Some(cancel) => tokio::select! {
                        l = lines.next() => l,
                        _ = cancel.cancelled() => Some(Err(Error::other("messages stream: aborted"))),
                    },
                    None => lines.next().await,
                };
                let line = match next {
                    Some(l) => l?,
                    None => break,
                };
                if is_sse_comment_line(&line) || line.is_empty() {
                    continue;
                }
                if let Some(rest) = line.strip_prefix("event:") {
                    current_event = rest.trim().to_string();
                } else if let Some(rest) = line.strip_prefix("data:") {
                    let data = rest.trim();
                    if let Some(conv) = converter.as_mut() {
                        // OpenAI SSE → Anthropic 兼容事件。
                        let (events, done) = conv.convert(data)?;
                        for ev in events {
                            yield ev;
                        }
                        if done {
                            return;
                        }
                    } else {
                        // Anthropic SSE：原生事件直透 + content block 元数据回填。
                        let mut ev = StreamEvent {
                            event: current_event.clone(),
                            data: data.to_string(),
                            ..Default::default()
                        };
                        let (idx, bt, eph) =
                            extract_anthropic_block_meta(&current_event, data, &mut block_type_map);
                        if !bt.is_empty() {
                            ev.block_index = Some(idx);
                            ev.block_type = Some(bt);
                            ev.ephemeral = Some(eph);
                        }
                        yield ev;
                    }
                }
            }
        })
    }

    /// 派生 per-request 超时子 token：超时或 parent 取消任一触发即 abort。对应 TS `withRequestTimeout`。
    /// 流式 / 下载路径不应套此短超时（会切断长连接），故仅在非流式 JSON 路径使用。
    pub(crate) fn derive_timeout_token(
        &self,
        timeout_ms: u64,
        parent: Option<CancellationToken>,
    ) -> Option<CancellationToken> {
        if timeout_ms == 0 {
            return parent;
        }
        let child = CancellationToken::new();
        let trigger = child.clone();
        let parent_clone = parent.clone();
        tokio::spawn(async move {
            let timeout = tokio::time::sleep(std::time::Duration::from_millis(timeout_ms));
            match parent_clone {
                Some(p) => {
                    tokio::select! {
                        _ = timeout => trigger.cancel(),
                        _ = p.cancelled() => trigger.cancel(),
                    }
                }
                None => {
                    timeout.await;
                    trigger.cancel();
                }
            }
        });
        Some(child)
    }

    // ── 测试辅助（仅 cfg(test)）──

    /// 直接注入未过期 token（测试用，绕过 OAuth 登录流）。对应 TS `primeTokensForTest`。
    #[cfg(test)]
    fn prime_tokens_for_test(&self, tokens: TokenSet) {
        *self.inner.tokens.write().unwrap() = Some(tokens);
    }

    /// 直接注入 OAuth server 元数据（测试用，绕过 discover）。
    #[cfg(test)]
    fn prime_meta_for_test(&self, meta: ServerMetadata) {
        *self.inner.meta.write().unwrap() = Some(meta);
    }

    /// 把占位 ManagedModel 塞入缓存（测试用）。对应 TS `primeModelCacheForTest`。
    #[cfg(test)]
    fn prime_model_cache_for_test(&self, models: Vec<ManagedModel>) {
        *self.inner.model_cache.write().unwrap() = models;
        *self.inner.model_cache_time.write().unwrap() = Some(std::time::Instant::now());
    }
}

/// `chat_stream_with_usage` 的 tagged 事件。对应 TS `{kind:'content'|'sources'|'settle'}`。
#[derive(Debug, Clone)]
pub enum ChatUsageEvent {
    /// 内容增量事件。
    Content(StreamEvent),
    /// 搜索来源事件。
    Sources(SourcesEvent),
    /// 结算事件（token 消耗 + 剩余余额）。
    Settle(StreamSettlement),
}

/// 流式请求公共头（Bearer + JSON + SSE Accept）。
fn stream_headers(token: &str) -> Vec<(reqwest::header::HeaderName, String)> {
    vec![
        (reqwest::header::AUTHORIZATION, format!("Bearer {token}")),
        (
            reqwest::header::CONTENT_TYPE,
            "application/json".to_string(),
        ),
        (reqwest::header::ACCEPT, "text/event-stream".to_string()),
    ]
}

/// 从响应头解析 `Retry-After` 秒数（0 = 无 / 解析失败）。
fn parse_retry_after_secs(headers: &reqwest::header::HeaderMap) -> i64 {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|&s| s > 0)
        .unwrap_or(0)
}

/// 从响应头解析 i64（解析失败 → None）。对应 TS `parseInt(headers.get(...))`。
fn header_i64(headers: &reqwest::header::HeaderMap, name: &str) -> Option<i64> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<i64>().ok())
}

/// 退避 sleep，可被取消信号打断（取消即立即返回，不再退避）。对应 TS `sleep(ms, signal)`。
async fn sleep_with_cancel(ms: u64, signal: Option<&CancellationToken>) {
    let dur = std::time::Duration::from_millis(ms);
    match signal {
        Some(cancel) => {
            tokio::select! {
                _ = tokio::time::sleep(dur) => {}
                _ = cancel.cancelled() => {}
            }
        }
        None => tokio::time::sleep(dur).await,
    }
}

/// URL path 段编码（对齐 TS `encodeURIComponent`）。仅对 path-segment 不安全字符百分号编码。
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
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
///
/// # Examples
///
/// ```
/// use acosmi::core::normalize_gateway_base_url;
///
/// assert_eq!(
///     normalize_gateway_base_url("https://gw.example/api/v4/").unwrap(),
///     "https://gw.example/api/v4",
/// );
/// assert!(normalize_gateway_base_url("wss://session.example").is_err()); // 仅 http/https
/// ```
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

#[cfg(test)]
mod p4_tests {
    use super::*;
    use crate::auth::types::{ServerMetadata, TokenSet};
    use crate::models::types::ManagedModel;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::Arc as StdArc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    // ── 纯函数红线（无需网络）──

    #[test]
    fn urlencode_matches_encodeuricomponent() {
        // 普通段不变。
        assert_eq!(urlencode("claude-opus"), "claude-opus");
        // 斜杠 / 空格 / 中文须百分号编码（防 path 注入）。
        assert_eq!(urlencode("a b/c"), "a%20b%2Fc");
        assert_eq!(urlencode("模型"), "%E6%A8%A1%E5%9E%8B");
        // encodeURIComponent 保留字符不编码。
        assert_eq!(urlencode("a-_.!~*'()"), "a-_.!~*'()");
    }

    #[test]
    fn header_i64_parses_or_none() {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("X-Token-Remaining", "12345".parse().unwrap());
        h.insert("X-Bad", "nan".parse().unwrap());
        assert_eq!(header_i64(&h, "X-Token-Remaining"), Some(12345));
        assert_eq!(header_i64(&h, "X-Bad"), None);
        assert_eq!(header_i64(&h, "X-Missing"), None);
    }

    #[test]
    fn parse_retry_after_secs_filters_nonpositive() {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert(reqwest::header::RETRY_AFTER, "30".parse().unwrap());
        assert_eq!(parse_retry_after_secs(&h), 30);
        h.insert(reqwest::header::RETRY_AFTER, "0".parse().unwrap());
        assert_eq!(parse_retry_after_secs(&h), 0);
    }

    /// 流式行解析：[DONE] → done；普通事件 event/data 透传（Anthropic 自然结束靠连接关闭）。
    #[test]
    fn anthropic_adapter_parse_stream_line_basic() {
        let adapter = Adapter::Anthropic;
        // [DONE] → done。
        let (_, done) = adapter.parse_stream_line("", "[DONE]").unwrap();
        assert!(done);
        // 普通 delta → 非 done，event/data 透传。
        let (ev, done2) = adapter
            .parse_stream_line(
                "content_block_delta",
                r#"{"index":0,"delta":{"type":"text_delta","text":"hi"}}"#,
            )
            .unwrap();
        assert!(!done2);
        assert_eq!(ev.event, "content_block_delta");
    }

    #[test]
    fn endpoint_suffix_routing_per_adapter() {
        // Anthropic → /anthropic, OpenAI → /chat（chat 端点拼接红线）。
        assert_eq!(Adapter::Anthropic.endpoint_suffix(), "/anthropic");
        assert_eq!(Adapter::OpenAI.endpoint_suffix(), "/chat");
        let ep = format!(
            "/managed-models/{}{}",
            urlencode("m id"),
            Adapter::OpenAI.endpoint_suffix()
        );
        assert_eq!(ep, "/managed-models/m%20id/chat");
    }

    // ── 端到端：原始 HTTP/1.1 mock server ──

    struct MockResponse {
        status: u16,
        headers: Vec<(&'static str, String)>,
        body: String,
    }
    impl MockResponse {
        fn ok(body: &str) -> Self {
            Self {
                status: 200,
                headers: vec![],
                body: body.to_string(),
            }
        }
        fn with_header(mut self, k: &'static str, v: &str) -> Self {
            self.headers.push((k, v.to_string()));
            self
        }
        fn status(status: u16, body: &str) -> Self {
            Self {
                status,
                headers: vec![],
                body: body.to_string(),
            }
        }
    }

    /// 启一个按到达顺序逐个回放 `responses` 的 mock server，记录收到的 request 行（METHOD PATH）。
    /// 返回 (base_url, 记录器句柄, JoinHandle)。
    async fn spawn_mock(
        responses: Vec<MockResponse>,
    ) -> (
        String,
        StdArc<std::sync::Mutex<Vec<String>>>,
        StdArc<AtomicUsize>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");
        let log = StdArc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let count = StdArc::new(AtomicUsize::new(0));
        let log2 = log.clone();
        let count2 = count.clone();
        tokio::spawn(async move {
            let mut idx = 0usize;
            while idx < responses.len() {
                let (mut sock, _) = match listener.accept().await {
                    Ok(s) => s,
                    Err(_) => break,
                };
                // 读到 header 结束（\r\n\r\n），再按 Content-Length 读 body（够测试）。
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let first = req.lines().next().unwrap_or("").to_string();
                let parts: Vec<&str> = first.split_whitespace().collect();
                if parts.len() >= 2 {
                    log2.lock()
                        .unwrap()
                        .push(format!("{} {}", parts[0], parts[1]));
                }
                count2.fetch_add(1, AtomicOrdering::SeqCst);

                let r = &responses[idx];
                idx += 1;
                let reason = match r.status {
                    200 => "OK",
                    401 => "Unauthorized",
                    500 => "Internal Server Error",
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
                let _ = sock.write_all(r.body.as_bytes()).await;
                let _ = sock.flush().await;
                // 优雅关闭写端，确保 Content-Length 字节全部送达后再 EOF（避免 reqwest IncompleteBody）。
                let _ = sock.shutdown().await;
            }
        });
        (base, log, count)
    }

    fn unexpired_token(base: &str) -> TokenSet {
        TokenSet {
            access_token: "AT0".to_string(),
            refresh_token: "RT0".to_string(),
            // 远未来。
            expires_at: "2999-01-01T00:00:00Z".to_string(),
            scope: String::new(),
            client_id: "cid".to_string(),
            server_url: base.to_string(),
        }
    }

    fn primed_client(base: &str) -> Client {
        let client = Client::new(Config {
            server_url: Some(base.to_string()),
            ..Default::default()
        })
        .unwrap();
        client.prime_tokens_for_test(unexpired_token(base));
        // 缓存一个 Anthropic 模型，避免 build_chat_request 触发 listModels。
        client.prime_model_cache_for_test(vec![ManagedModel {
            id: "test-model".to_string(),
            provider: "anthropic".to_string(),
            model_id: "test-model".to_string(),
            is_enabled: true,
            ..Default::default()
        }]);
        client
    }

    #[tokio::test]
    async fn chat_routes_to_anthropic_endpoint_and_reads_headers() {
        // Anthropic 响应体（最小）。
        let body = r#"{"id":"msg_1","type":"message","model":"test-model","role":"assistant","content":[],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}"#;
        let resp = MockResponse::ok(body)
            .with_header("X-Token-Remaining", "999")
            .with_header("X-Call-Remaining", "42");
        let (base, log, _) = spawn_mock(vec![resp]).await;
        let client = primed_client(&base);

        let req = ChatRequest::default();
        let out = client.chat("test-model", &req, None).await.unwrap();
        // 端点路由红线：POST 到 /api/v4/managed-models/test-model/anthropic。
        let recorded = log.lock().unwrap().clone();
        assert_eq!(recorded.len(), 1);
        assert_eq!(
            recorded[0],
            "POST /api/v4/managed-models/test-model/anthropic"
        );
        // 响应头余额回填。
        assert_eq!(out.token_remaining, 999);
        assert_eq!(out.call_remaining, 42);
        assert_eq!(out.id, "msg_1");
    }

    #[tokio::test]
    async fn do_json_full_empty_body_returns_none() {
        // 空体成功响应 → Ok(None)（方案 §4.4 空体契约）。
        let (base, _, _) = spawn_mock(vec![MockResponse::ok("")]).await;
        let client = Client::new(Config {
            server_url: Some(base.clone()),
            ..Default::default()
        })
        .unwrap();
        client.prime_tokens_for_test(unexpired_token(&base));
        let (out, _) = client
            .do_json_full::<serde_json::Value>(reqwest::Method::GET, "/whatever", None, None)
            .await
            .unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn embedding_rerank_request_serialization() {
        // EmbeddingInput untagged: Single → 裸字符串, Batch → 数组; 可选字段缺省不出现。
        let single = EmbeddingRequest {
            input: crate::models::types::EmbeddingInput::Single("hello".into()),
            dimensions: Some(512),
            encoding_format: None,
        };
        let v = serde_json::to_value(&single).unwrap();
        assert_eq!(v["input"], serde_json::json!("hello"));
        assert_eq!(v["dimensions"], serde_json::json!(512));
        assert!(v.get("encoding_format").is_none());

        let batch = EmbeddingRequest {
            input: crate::models::types::EmbeddingInput::Batch(vec!["a".into(), "b".into()]),
            dimensions: None,
            encoding_format: None,
        };
        let vb = serde_json::to_value(&batch).unwrap();
        assert_eq!(vb["input"], serde_json::json!(["a", "b"]));
        assert!(vb.get("dimensions").is_none());

        let rr = RerankRequest {
            query: "q".into(),
            documents: vec!["d0".into(), "d1".into()],
            top_n: Some(2),
            return_documents: Some(true),
            instruct: None,
        };
        let vr = serde_json::to_value(&rr).unwrap();
        assert_eq!(vr["query"], serde_json::json!("q"));
        assert_eq!(vr["documents"], serde_json::json!(["d0", "d1"]));
        assert_eq!(vr["top_n"], serde_json::json!(2));
        assert_eq!(vr["return_documents"], serde_json::json!(true));
        assert!(vr.get("instruct").is_none());
    }

    #[tokio::test]
    async fn embeddings_posts_and_parses_openai_response() {
        let body = r#"{"object":"list","model":"text-embedding-v4","data":[{"object":"embedding","index":0,"embedding":[0.1,0.2,0.3]}],"usage":{"prompt_tokens":5,"total_tokens":5}}"#;
        let (base, log, _) = spawn_mock(vec![MockResponse::ok(body)]).await;
        let client = primed_client(&base);

        let req = EmbeddingRequest {
            input: crate::models::types::EmbeddingInput::Single("hello".into()),
            dimensions: Some(512),
            encoding_format: None,
        };
        let resp = client.embeddings("text-embedding-v4", &req, None).await.unwrap();

        let recorded = log.lock().unwrap().clone();
        assert_eq!(recorded.len(), 1);
        assert!(
            recorded[0].contains("POST") && recorded[0].contains("/managed-models/text-embedding-v4/embeddings"),
            "unexpected request line: {}",
            recorded[0]
        );
        assert_eq!(resp.model, "text-embedding-v4");
        assert_eq!(resp.data.len(), 1);
        assert_eq!(resp.data[0].embedding, vec![0.1, 0.2, 0.3]);
        assert_eq!(resp.usage.total_tokens, 5);
    }

    #[tokio::test]
    async fn rerank_posts_and_parses_normalized_response() {
        let body = r#"{"results":[{"index":1,"relevance_score":0.93,"document":"doc1"},{"index":0,"relevance_score":0.42,"document":"doc0"}],"usage":{"total_tokens":79},"model":"gte-rerank-v2"}"#;
        let (base, log, _) = spawn_mock(vec![MockResponse::ok(body)]).await;
        let client = primed_client(&base);

        let req = RerankRequest {
            query: "q".into(),
            documents: vec!["doc0".into(), "doc1".into()],
            top_n: Some(2),
            return_documents: Some(true),
            instruct: None,
        };
        let resp = client.rerank("gte-rerank-v2", &req, None).await.unwrap();

        let recorded = log.lock().unwrap().clone();
        assert!(
            recorded[0].contains("POST") && recorded[0].contains("/managed-models/gte-rerank-v2/rerank"),
            "unexpected request line: {}",
            recorded[0]
        );
        assert_eq!(resp.results.len(), 2);
        assert_eq!(resp.results[0].index, 1);
        assert!((resp.results[0].relevance_score - 0.93).abs() < 1e-9);
        assert_eq!(resp.results[0].document.as_deref(), Some("doc1"));
        assert_eq!(resp.usage.total_tokens, 79);
        assert_eq!(resp.model.as_deref(), Some("gte-rerank-v2"));
    }

    #[tokio::test]
    async fn chat_401_single_refresh_retry_then_succeeds() {
        // 序列：①chat POST → 401 ②token_endpoint refresh → 200 新 token ③chat POST 重试 → 200。
        let chat_body = r#"{"id":"msg_2","type":"message","model":"test-model","role":"assistant","content":[],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}"#;
        let token_body = r#"{"access_token":"AT1","token_type":"Bearer","expires_in":3600,"refresh_token":"RT1"}"#;
        let (base, log, count) = spawn_mock(vec![
            MockResponse::status(401, "{}"),
            MockResponse::ok(token_body),
            MockResponse::ok(chat_body),
        ])
        .await;

        let client = primed_client(&base);
        // 注入 meta，使 force_refresh 的 refresh_direct 不触发 discover，直接打 token_endpoint。
        client.prime_meta_for_test(ServerMetadata {
            issuer: base.clone(),
            authorization_endpoint: format!("{base}/authorize"),
            token_endpoint: format!("{base}/token"),
            revocation_endpoint: format!("{base}/revoke"),
            registration_endpoint: format!("{base}/register"),
            scopes_supported: vec![],
        });

        let req = ChatRequest::default();
        let out = client.chat("test-model", &req, None).await.unwrap();
        assert_eq!(out.id, "msg_2");

        let recorded = log.lock().unwrap().clone();
        // 红线：恰 3 次请求（chat 401 → refresh → chat 重试），不无限递归。
        assert_eq!(count.load(AtomicOrdering::SeqCst), 3, "exactly one retry");
        assert_eq!(
            recorded[0],
            "POST /api/v4/managed-models/test-model/anthropic"
        );
        assert!(
            recorded[1].starts_with("POST /token"),
            "refresh hits token_endpoint: {}",
            recorded[1]
        );
        assert_eq!(
            recorded[2],
            "POST /api/v4/managed-models/test-model/anthropic"
        );
        // token 已轮换。
        assert_eq!(client.token_set().unwrap().access_token, "AT1");
    }

    #[tokio::test]
    async fn chat_401_twice_does_not_recurse() {
        // 序列：①chat → 401 ②refresh → 200 ③chat 重试 → 又 401 → 不再重试，抛 HttpError(401)。
        let token_body = r#"{"access_token":"AT1","token_type":"Bearer","expires_in":3600,"refresh_token":"RT1"}"#;
        let (base, _log, count) = spawn_mock(vec![
            MockResponse::status(401, "{}"),
            MockResponse::ok(token_body),
            MockResponse::status(401, "{}"),
        ])
        .await;
        let client = primed_client(&base);
        client.prime_meta_for_test(ServerMetadata {
            issuer: base.clone(),
            authorization_endpoint: format!("{base}/authorize"),
            token_endpoint: format!("{base}/token"),
            revocation_endpoint: format!("{base}/revoke"),
            registration_endpoint: format!("{base}/register"),
            scopes_supported: vec![],
        });
        let req = ChatRequest::default();
        let err = client.chat("test-model", &req, None).await.unwrap_err();
        match err {
            Error::Http(h) => assert_eq!(h.status_code, 401),
            other => panic!("expected HttpError(401), got {other:?}"),
        }
        // 红线：第二次 401 不再触发 refresh/重试 —— 恰 3 次请求。
        assert_eq!(
            count.load(AtomicOrdering::SeqCst),
            3,
            "guard prevents recursion"
        );
    }

    // ── 超时回归（真实时钟 + 控制组对照）──
    //
    // 设计：reqwest 内部计时器（含 connect_timeout）与 tokio paused 虚拟时钟混用会
    // 不确定地抢先误触，故这些回归测试用真实时钟 + 小间隔，并用「控制组 client（显式
    // 短全局 .timeout()）会被砍」对照「默认 client（无全局 .timeout()）不被砍」，
    // 来确定性坐实：`Client::new` 默认构造的 http 没有覆盖响应体（含 SSE）的全局总超时。

    /// 启一个流式 SSE mock server：建连后立即发响应头 + 第一个 chunk，随后每隔
    /// `gap` 真实毫秒下发一个 chunk，共 `n_events` 个。用于「全局总超时是否砍响应体」对照。
    async fn spawn_sse_mock_realtime(gap: std::time::Duration, n_events: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");
        tokio::spawn(async move {
            let (mut sock, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => return,
            };
            let mut buf = vec![0u8; 8192];
            let _ = sock.read(&mut buf).await;

            let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                        Transfer-Encoding: chunked\r\n\r\n";
            if sock.write_all(head.as_bytes()).await.is_err() {
                return;
            }
            let _ = sock.flush().await;

            for i in 0..n_events {
                if i > 0 {
                    tokio::time::sleep(gap).await;
                }
                let payload = format!(
                    "event: content_block_delta\n\
                     data: {{\"type\":\"content_block_delta\",\"index\":0,\
                     \"delta\":{{\"type\":\"text_delta\",\"text\":\"chunk{i}\"}}}}\n\n"
                );
                let chunk = format!("{:x}\r\n{}\r\n", payload.len(), payload);
                if sock.write_all(chunk.as_bytes()).await.is_err() {
                    return;
                }
                if sock.flush().await.is_err() {
                    return;
                }
            }
            let _ = sock.write_all(b"0\r\n\r\n").await;
            let _ = sock.flush().await;
            let _ = sock.shutdown().await;
        });
        base
    }

    fn primed_client_with_http(base: &str, http: reqwest::Client) -> Client {
        let client = Client::new(Config {
            server_url: Some(base.to_string()),
            http: Some(http),
            ..Default::default()
        })
        .unwrap();
        client.prime_tokens_for_test(unexpired_token(base));
        client.prime_model_cache_for_test(vec![ManagedModel {
            id: "test-model".to_string(),
            provider: "anthropic".to_string(),
            model_id: "test-model".to_string(),
            is_enabled: true,
            ..Default::default()
        }]);
        client
    }

    /// 控制组：显式带 250ms 全局 `.timeout()` 的 client（模拟「旧 60s 全局总超时」的语义，
    /// 只是阈值缩小到 250ms 以便快速触发）。流式总时长跨过 250ms → 必被 reqwest abort。
    #[tokio::test]
    async fn stream_aborted_by_explicit_global_timeout_control() {
        // chunk 间隔 200ms × 3 → 流式总时长 ≈ 400ms > 250ms 全局 timeout。
        let base = spawn_sse_mock_realtime(std::time::Duration::from_millis(200), 3).await;
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(250))
            .build()
            .unwrap();
        let client = primed_client_with_http(&base, http);

        let req = ChatRequest::default();
        let stream = client.chat_stream("test-model", &req, None);
        futures::pin_mut!(stream);

        let mut err_seen = false;
        let mut count = 0usize;
        while let Some(ev) = stream.next().await {
            match ev {
                Ok(_) => count += 1,
                Err(_) => {
                    err_seen = true;
                    break;
                }
            }
        }
        // 控制组红线：全局 .timeout() 会在响应体中途 abort SSE 流（拿不全 3 个 chunk）。
        assert!(
            err_seen && count < 3,
            "带全局 .timeout() 的 client 必在流式响应体中途被 abort（got {count} chunks, err={err_seen}）"
        );
    }

    /// 主回归：`Client::new` 默认构造的 http **无全局总超时** → 同样跨过 250ms 的流式
    /// 响应体不被 abort，收齐全部 chunk。证明已去除击穿流式 SSE 的全局 60s 总超时。
    #[tokio::test]
    async fn stream_survives_long_body_no_global_timeout() {
        // 同样的间隔/总时长（≈400ms），但用默认 client（无全局 timeout）。
        let base = spawn_sse_mock_realtime(std::time::Duration::from_millis(200), 3).await;
        let client = primed_client(&base); // 默认 http，无 .timeout()

        let req = ChatRequest::default();
        let stream = client.chat_stream("test-model", &req, None);
        futures::pin_mut!(stream);

        let mut events: Vec<StreamEvent> = Vec::new();
        while let Some(ev) = stream.next().await {
            let ev = ev.expect("默认 client 无全局总超时，流式响应体不应被 abort");
            events.push(ev);
        }
        assert_eq!(
            events.len(),
            3,
            "默认 client 必收齐 3 个 chunk（无全局总超时砍响应体）"
        );
        for (i, ev) in events.iter().enumerate() {
            assert_eq!(ev.event, "content_block_delta");
            assert!(ev.data.contains(&format!("chunk{i}")));
        }
    }

    /// 非流式回归：默认 client 对「响应延迟 > 控制组 timeout」的非流式 chat 不被全局总超时砍。
    /// 控制组（250ms 全局 timeout）会 abort；默认 client 等到响应（≈400ms）成功。
    #[tokio::test]
    async fn nonstream_chat_survives_slow_response_no_global_timeout() {
        // 单次非流式响应：accept 后延迟 400ms 才回响应体。
        async fn spawn_slow_json(delay: std::time::Duration) -> String {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let base = format!("http://{addr}");
            tokio::spawn(async move {
                if let Ok((mut sock, _)) = listener.accept().await {
                    let mut buf = vec![0u8; 8192];
                    let _ = sock.read(&mut buf).await;
                    tokio::time::sleep(delay).await;
                    let body = r#"{"id":"msg_slow","type":"message","model":"test-model","role":"assistant","content":[],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}"#;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.flush().await;
                    let _ = sock.shutdown().await;
                }
            });
            base
        }

        // 控制组：250ms 全局 timeout 对 400ms 慢响应 → 被砍。
        let ctrl_base = spawn_slow_json(std::time::Duration::from_millis(400)).await;
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(250))
            .build()
            .unwrap();
        let ctrl = primed_client_with_http(&ctrl_base, http);
        let err = ctrl
            .chat("test-model", &ChatRequest::default(), None)
            .await
            .expect_err("控制组 250ms 全局 timeout 必砍 400ms 慢响应");
        match err {
            Error::Network(n) => assert!(n.timeout, "控制组应为 reqwest 超时错误，got: {n:?}"),
            other => panic!("expected Network timeout, got {other:?}"),
        }

        // 默认 client：无全局总超时 → 同样的 400ms 慢响应应成功（per-request 死线 11min 远未到）。
        let base = spawn_slow_json(std::time::Duration::from_millis(400)).await;
        let client = primed_client(&base);
        let out = client
            .chat("test-model", &ChatRequest::default(), None)
            .await
            .expect("默认 client 无全局总超时，400ms 慢响应不应被砍");
        assert_eq!(out.id, "msg_slow");
    }
}
