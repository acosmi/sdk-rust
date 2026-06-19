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

use crate::auth::types::{ServerMetadata, TokenSet};
use crate::core::retry::{effective_policy, EffectiveRetryPolicy, RetryPolicy};
use crate::core::store::{FileTokenStore, InMemoryTokenStore, TokenStore};
use crate::macros::open_string_union;
use crate::shared::errors::{Error, Result};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
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
    /// lazy 加载的 OAuth server 元数据。P2 起填充。
    #[allow(dead_code)]
    meta: RwLock<Option<ServerMetadata>>,
    /// login 进行中标志（单航班）。P2 起使用。
    #[allow(dead_code)]
    login_in_flight: AtomicBool,
    /// refresh-rotation 临界区门（对应 TS withMu）。P2 起使用。
    #[allow(dead_code)]
    mu: tokio::sync::Mutex<()>,
    /// login 就绪信号（等待方解阻塞）。P2 起使用。
    #[allow(dead_code)]
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

    /// 返回有效 access_token，过期自动刷新（P1 骨架：仅返回当前 token；
    /// 过期检查 + refresh + 单航班登录等待待 P2 实现）。
    pub async fn ensure_token(&self) -> Result<String> {
        if let Some(t) = self.token_set() {
            // P2: 检查 expires_at 偏移 30s，必要时走 force_refresh。
            return Ok(t.access_token);
        }
        Err(Error::other("not authorized: call login() first"))
    }

    /// 强制刷新 token（401 重试用）。P1 骨架：未实现，待 P2。
    pub async fn force_refresh(&self) -> Result<()> {
        Err(Error::other("force_refresh not implemented until P2"))
    }
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
