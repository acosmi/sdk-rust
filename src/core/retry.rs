//! SDK 内置 retry policy。端口自 `core/retry.ts`（其端口自 `acosmi-sdk-go/retry.go`）。
//!
//! 设计红线：
//!   1. GET 类查询默认 2x retry；
//!   2. POST 类业务默认 0 retry —— 计费安全红线（双扣保护）；
//!   3. Stream 路径强制 `max_attempts=1`；
//!   4. 401 refresh 与 retry 互斥（refresh 是 inner loop，不算 attempt）；
//!   5. `StreamError` 显式排除（SSE 中段错误不可重试）。

use crate::shared::errors::Error;
use std::sync::Arc;

/// 一次 retry 评估的请求快照 —— 仅供 `safe_to_retry` 闸门使用。
#[derive(Debug, Clone)]
pub struct RetryRequestInfo {
    pub method: String,
    pub url: String,
}

/// 错误层闸门：是否值得重试。
pub type RetryableFn = Arc<dyn Fn(&Error) -> bool + Send + Sync>;
/// 请求层闸门（计费安全核心）：当前请求是否值得重试。
pub type SafeToRetryFn = Arc<dyn Fn(&RetryRequestInfo) -> bool + Send + Sync>;

/// 配置 SDK 重试行为。`None` 字段使用默认值；整个 `RetryPolicy` 缺省（`None`）则禁用重试。
#[derive(Clone, Default)]
pub struct RetryPolicy {
    /// 总尝试次数（含首次）。1 = 不重试；默认 2。
    pub max_attempts: Option<u32>,
    /// 首次重试退避时长（毫秒）。默认 200。
    pub backoff_ms: Option<u64>,
    /// 退避最大值（指数增长封顶，毫秒）。默认 2000。
    pub backoff_max_ms: Option<u64>,
    /// 指数倍数。默认 2.0。
    pub backoff_mul: Option<f64>,
    /// 错误层闸门覆盖。默认 [`default_retryable`]。
    pub on_retryable: Option<RetryableFn>,
    /// 请求层闸门覆盖。默认 [`default_safe_to_retry`]。
    pub safe_to_retry: Option<SafeToRetryFn>,
}

/// 实际生效策略（字段已解析为非可选）。对应 TS `Required<RetryPolicy>`。
#[derive(Clone)]
pub struct EffectiveRetryPolicy {
    pub max_attempts: u32,
    pub backoff_ms: u64,
    pub backoff_max_ms: u64,
    pub backoff_mul: f64,
    pub on_retryable: RetryableFn,
    pub safe_to_retry: SafeToRetryFn,
}

/// 安全默认值。
pub const DEFAULT_MAX_ATTEMPTS: u32 = 2;
pub const DEFAULT_BACKOFF_MS: u64 = 200;
pub const DEFAULT_BACKOFF_MAX_MS: u64 = 2000;
pub const DEFAULT_BACKOFF_MUL: f64 = 2.0;

/// Retry-After 头的硬上限 —— 防止恶意服务器返回 `Retry-After: 999999` 卡死。
const RETRY_AFTER_UPPER_BOUND_MS: u64 = 60_000;

/// 计费安全闸门：仅 GET / HEAD / OPTIONS 视为幂等可重试；其余默认 false（双扣保护）。
/// 对应 TS `defaultSafeToRetry`。
pub fn default_safe_to_retry(req: &RetryRequestInfo) -> bool {
    matches!(
        req.method.to_uppercase().as_str(),
        "GET" | "HEAD" | "OPTIONS"
    )
}

/// 错误层闸门。显式排除 `StreamError`（流已部分写出，重试 = 双 token）；
/// HTTPError 5xx/429 与 NetworkError timeout/EOF 视为可重试；其它不重试。
/// 对应 TS `defaultRetryable`。
pub fn default_retryable(err: &Error) -> bool {
    match err {
        Error::Stream(_) => false,
        Error::Http(h) => h.status_code >= 500 || h.status_code == 429,
        Error::Network(n) => n.is_timeout() || n.is_eof(),
        _ => false,
    }
}

/// 实际生效策略（`None` → 禁用重试返回 `None`；字段缺失填默认值）。对应 TS `effectivePolicy`。
pub fn effective_policy(p: Option<&RetryPolicy>) -> Option<EffectiveRetryPolicy> {
    let p = p?;
    Some(EffectiveRetryPolicy {
        max_attempts: p
            .max_attempts
            .filter(|&v| v > 0)
            .unwrap_or(DEFAULT_MAX_ATTEMPTS),
        backoff_ms: p
            .backoff_ms
            .filter(|&v| v > 0)
            .unwrap_or(DEFAULT_BACKOFF_MS),
        backoff_max_ms: p
            .backoff_max_ms
            .filter(|&v| v > 0)
            .unwrap_or(DEFAULT_BACKOFF_MAX_MS),
        backoff_mul: p
            .backoff_mul
            .filter(|&v| v > 0.0)
            .unwrap_or(DEFAULT_BACKOFF_MUL),
        on_retryable: p
            .on_retryable
            .clone()
            .unwrap_or_else(|| Arc::new(default_retryable)),
        safe_to_retry: p
            .safe_to_retry
            .clone()
            .unwrap_or_else(|| Arc::new(default_safe_to_retry)),
    })
}

/// 计算第 `attempt` 次重试的退避时长（毫秒，`attempt` 从 0 起）。
/// 优先级：`HttpError.retry_after`（上限 60s）> 指数退避（封顶 `backoff_max_ms`）。
/// 对应 TS `computeBackoff`。
pub fn compute_backoff(p: &EffectiveRetryPolicy, attempt: u32, err: &Error) -> u64 {
    if let Error::Http(h) = err {
        if h.retry_after > 0 {
            let ms = (h.retry_after as u64).saturating_mul(1000);
            return ms.min(RETRY_AFTER_UPPER_BOUND_MS);
        }
    }
    let mut d = p.backoff_ms as f64;
    for _ in 0..attempt {
        d *= p.backoff_mul;
        if d > p.backoff_max_ms as f64 {
            return p.backoff_max_ms;
        }
    }
    d as u64
}
