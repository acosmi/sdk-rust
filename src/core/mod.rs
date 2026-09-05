//! 运行时基座域：主 Client、TokenStore、Retry、HTTP 辅助。
//!
//! 对齐 `core/index.ts`。`sanitize_bridge`（Client × sanitize 胶水）依赖
//! `models::ChatRequest` + `sanitize`，feature `sanitize` 门控（P8 接通）。

pub mod client;
pub mod http;
pub mod retry;
#[cfg(feature = "sanitize")]
pub mod sanitize_bridge;
pub mod store;

pub use client::{
    normalize_gateway_base_url, normalize_override_base_url, BrowserRefreshMode, ChatUsageEvent,
    Client, Config, FilterStatus, OAuthMetadataProfile, DEFAULT_API_TIMEOUT_MS,
    DEFAULT_GATEWAY_BASE_URL, ERR_OAUTH_CORS_BLOCKED, ERR_REFRESH_PROXY_FAILED, ERR_TOKEN_EXPIRED,
};
pub use http::{
    is_order_success, is_order_terminal, parse_http_error, parse_http_error_with_retry_after,
    parse_stream_error, CHAT_REQUEST_TIMEOUT_MS, COEF_CACHE_TTL_MS, DEFAULT_JSON_TIMEOUT_MS,
    MAX_DOWNLOAD_SIZE, MAX_ERROR_BODY_SIZE, MAX_SSE_LINE_SIZE, MODEL_CACHE_TTL_MS,
};
pub use retry::{
    compute_backoff, default_retryable, default_safe_to_retry, effective_policy,
    EffectiveRetryPolicy, RetryPolicy, RetryRequestInfo,
};
pub use store::{
    new_file_token_store, FileLockDefaults, FileTokenStore, InMemoryTokenStore, TokenStore,
    FILE_LOCK_DEFAULTS,
};

pub mod observer;
pub use observer::{
    read_gateway_request_id, ChatOptions, GatewayRequestIDCallback, UpstreamActivityCallback,
    GATEWAY_REQUEST_ID_HEADER,
};

pub mod transport;
pub use transport::{
    HttpBody, HttpClient, HttpContext, HttpPurpose, HttpRequest, HttpResponse, HttpResponseMode,
    HttpTransport, TransportError,
};

#[cfg(feature = "native-http")]
pub use http::{classify_transport, iter_sse_lines, read_limited, read_limited_text};
pub use http::{
    iter_sse_lines_result, iter_sse_lines_result_with_cap, read_limited_result,
    read_limited_text_result,
};

pub mod authority;
pub use authority::{AuthorityResult, AuthorityState, StrictTokenAuthority, TokenAuthorityError};
