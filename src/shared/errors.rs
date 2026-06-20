//! 跨域类型化错误。
//!
//! 端口自 `acosmi-sdk-ts/src/shared/errors.ts`（其本身端口自 `acosmi-sdk-go/types.go`）。
//!
//! TS 侧是 7 个 `Error` 子类（`instanceof` 区分）：`RateLimitError` / `BusinessError` /
//! `OrderTerminalError` / `ModelNotFoundError` / `HTTPError` / `NetworkError` / `StreamError`。
//! Rust 惯例改为顶层 [`Error`] enum（`match` 替代 `instanceof`）：富字段的
//! `HttpError`/`NetworkError`/`StreamError` 拆为独立结构体并由 enum 包装，其余 4 类内联为变体。
//! 变体/结构体名 1:1 对齐 TS 类名（跨语言锚点；`HTTPError`→`HttpError` 仅大小写惯例偏移）。

use std::fmt;

/// crate 统一 `Result`。
pub type Result<T> = std::result::Result<T, Error>;

// =============================================================================
// 富字段错误结构体（对应 TS 的 HTTPError / NetworkError / StreamError 子类）
// =============================================================================

/// 结构化 HTTP 非 2xx 错误。对应 TS `HTTPError`。
#[derive(Debug, Clone)]
pub struct HttpError {
    /// HTTP 状态码。
    pub status_code: u16,
    /// `anthropic.error.type` / `openai.error.type`，缺失为空串。
    pub r#type: String,
    /// `Retry-After` 头解析的秒数，0 表示未提供或解析失败。
    pub retry_after: i64,
    /// 原始响应体（截断到 `max_error_body_size`）。
    pub body: String,
    /// 解析出的 error.message（TS 仅用于 Display，不作公开字段；Rust 内部持有以复刻 Display）。
    pub message: String,
}

impl HttpError {
    /// 对齐 TS `new HTTPError(statusCode, opts)`。
    pub fn new(
        status_code: u16,
        r#type: impl Into<String>,
        message: impl Into<String>,
        retry_after: i64,
        body: impl Into<String>,
    ) -> Self {
        Self {
            status_code,
            r#type: r#type.into(),
            retry_after,
            body: body.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 严格对齐 TS constructor 四分支（type → message → body → 仅 status）。
        if !self.r#type.is_empty() {
            write!(
                f,
                "HTTP {}: [{}] {}",
                self.status_code, self.r#type, self.message
            )
        } else if !self.message.is_empty() {
            write!(f, "HTTP {}: {}", self.status_code, self.message)
        } else if !self.body.is_empty() {
            write!(f, "HTTP {}: {}", self.status_code, self.body)
        } else {
            write!(f, "HTTP {}", self.status_code)
        }
    }
}

impl std::error::Error for HttpError {}

/// 结构化网络层错误（传输失败，区别于上游业务错误）。对应 TS `NetworkError`。
#[derive(Debug, Clone)]
pub struct NetworkError {
    /// 操作描述，例如 `"POST /v1/messages"`。
    pub op: String,
    /// 请求 URL（脱敏后）。
    pub url: String,
    /// 底层 cause 的消息（已解析为字符串）。
    pub cause: String,
    pub timeout: bool,
    pub eof: bool,
}

impl NetworkError {
    pub fn new(op: impl Into<String>, url: impl Into<String>, cause: impl Into<String>) -> Self {
        let cause = cause.into();
        Self {
            op: op.into(),
            url: url.into(),
            cause: if cause.is_empty() {
                "network error".to_string()
            } else {
                cause
            },
            timeout: false,
            eof: false,
        }
    }
    /// retry policy: `is_timeout() || is_eof()` 任一为 true → 默认可重试。
    pub fn is_timeout(&self) -> bool {
        self.timeout
    }
    pub fn is_eof(&self) -> bool {
        self.eof
    }
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.op, self.url, self.cause)
    }
}

impl std::error::Error for NetworkError {}

/// 流式失败事件的结构化表示。对应 TS `StreamError`。
#[derive(Debug, Clone, Default)]
pub struct StreamError {
    /// 例：`"empty_response"` / `"rate_limit"` / `"overloaded"` / `""`。
    pub code: String,
    /// 例：`"provider"` / `"settlement"`。
    pub stage: String,
    /// 用户友好提示（历史字段，与 `raw_error` 区分）。
    pub user_message: String,
    /// gateway 原始 error 字符串。
    pub raw_error: String,
    /// 客户端是否值得重试。
    pub retryable: bool,
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let body = if !self.raw_error.is_empty() {
            &self.raw_error
        } else {
            &self.user_message
        };
        if !self.stage.is_empty() {
            write!(f, "stream failed: {}: {}", self.stage, body)
        } else {
            write!(f, "stream failed: {body}")
        }
    }
}

impl std::error::Error for StreamError {}

// =============================================================================
// 顶层 Error enum
// =============================================================================

/// SDK 统一错误。变体名对齐 TS 的 7 个错误子类 + Rust 传播用工具变体。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// 下载限流错误（429）。对应 TS `RateLimitError`。
    #[error("{message}")]
    RateLimit {
        message: String,
        retry_after: String,
        raw: String,
    },

    /// API 业务层错误（HTTP 200 但 `code != 0`）。对应 TS `BusinessError`。
    #[error("API error (code={code}): {message}")]
    Business { code: i64, message: String },

    /// 订单到达非成功终态。对应 TS `OrderTerminalError`。
    #[error("order {order_id} terminated: {status}")]
    OrderTerminal { order_id: String, status: String },

    /// 模型缓存未命中且刷新后仍未找到。对应 TS `ModelNotFoundError`。
    #[error(
        "managed model \"{model_id}\" not found (list models to refresh cache, or verify model id)"
    )]
    ModelNotFound { model_id: String },

    /// 结构化 HTTP 非 2xx 错误。
    #[error(transparent)]
    Http(#[from] HttpError),

    /// 结构化网络层错误。
    #[error(transparent)]
    Network(#[from] NetworkError),

    /// 流式失败事件。
    #[error(transparent)]
    Stream(#[from] StreamError),

    /// agent run SSE `error` 事件（`throwOnError` 为 true 时抛出）。对应 TS `AgentRunStreamError`。
    /// 携带 stage/code/retryable 供调用方判定。
    #[error("agent run failed: {}{message}", stage.as_ref().map(|s| format!("{s}: ")).unwrap_or_default())]
    AgentRunStream {
        code: String,
        stage: Option<String>,
        message: String,
        retryable: bool,
    },

    /// OAuth token 端点（exchange / refresh）结构化错误。对应 TS `OAuthTokenEndpointError`。
    /// 携带 OAuth `error` 码用于 `is_invalid_grant_error` 判定。
    #[error(transparent)]
    OAuthTokenEndpoint(#[from] crate::auth::auth::OAuthTokenEndpointError),

    // ── Rust 传播用工具变体（TS 无对应；用于 `?` 链路）──
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Http2(#[from] reqwest::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// 其它（含 URL 校验、解析等无专属变体的错误）。
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// 便捷构造 `RateLimit`。
    pub fn rate_limit(
        message: impl Into<String>,
        retry_after: impl Into<String>,
        raw: impl Into<String>,
    ) -> Self {
        Error::RateLimit {
            message: message.into(),
            retry_after: retry_after.into(),
            raw: raw.into(),
        }
    }
    /// 便捷构造 `Business`。
    pub fn business(code: i64, message: impl Into<String>) -> Self {
        Error::Business {
            code,
            message: message.into(),
        }
    }
    /// 便捷构造 `Other`。
    pub fn other(message: impl Into<String>) -> Self {
        Error::Other(message.into())
    }
}
