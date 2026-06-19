//! 跨域统一失败补救建议（retryAdvice）。端口自 `shared/retry-advice.ts`。
//!
//! 相位说明：TS 文件含「compliance 错误 key → reason」映射表与
//! `complianceErrorToRetryAdvice` 投影，依赖 `compliance/errors.ts` 的
//! `ComplianceErrorKey` / `ComplianceErrorInfo`（P6）。本阶段（P1）只落
//! **compliance 无关**部分；compliance 耦合部分待 P6 compliance 落地后补齐。
//!
//! `RetryAdvice` 是【叠加层】，不替换 `core/retry.rs` 的 `RetryPolicy`。

use serde::{Deserialize, Serialize};

/// 失败补救原因。开放枚举的【封闭】部分（11 项覆盖全集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryAdviceReason {
    Unknown,
    Retrying,
    Failed,
    GateClosed,
    StepUpRequired,
    TenantMismatch,
    InsufficientScope,
    QuotaExceeded,
    ProviderTimeout,
    LocalVerifyFailed,
    BillingPreflightFailed,
}

/// `RetryAdviceReason` 全集（11 项）—— 供迭代 / 校验使用。
pub const RETRY_ADVICE_REASONS: [RetryAdviceReason; 11] = [
    RetryAdviceReason::Unknown,
    RetryAdviceReason::Retrying,
    RetryAdviceReason::Failed,
    RetryAdviceReason::GateClosed,
    RetryAdviceReason::StepUpRequired,
    RetryAdviceReason::TenantMismatch,
    RetryAdviceReason::InsufficientScope,
    RetryAdviceReason::QuotaExceeded,
    RetryAdviceReason::ProviderTimeout,
    RetryAdviceReason::LocalVerifyFailed,
    RetryAdviceReason::BillingPreflightFailed,
];

/// 失败补救建议统一模型（§6.6）。wire 字段为 camelCase。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryAdvice {
    /// 是否值得【自动】重试。compliance 写接口几乎恒为 false（双扣红线）。
    pub retryable: bool,
    /// 建议的重试等待时长（秒）；与 `HttpError.retry_after` 单位一致。
    #[serde(
        rename = "retryAfter",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub retry_after: Option<i64>,
    /// 重试时是否必须沿用【同一】幂等键。
    #[serde(rename = "sameIdempotencyKeyRequired")]
    pub same_idempotency_key_required: bool,
    /// 是否需要人工介入，不能纯自动恢复。
    #[serde(rename = "manualActionRequired")]
    pub manual_action_required: bool,
    /// 归一化失败原因。
    pub reason: RetryAdviceReason,
    /// 面向终端用户的提示文案。
    #[serde(
        rename = "userMessage",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub user_message: Option<String>,
    /// 面向开发者的诊断信息。
    #[serde(
        rename = "developerMessage",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub developer_message: Option<String>,
    /// 支持工单关联码（如 `compliance:1031004004`）。
    #[serde(
        rename = "supportCode",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub support_code: Option<String>,
}

/// Go OAuth 标准错误字符串 → `RetryAdviceReason`。未登记的兜底 `Unknown`。
/// 对应 TS `retryReasonForOAuthError`（仅登记与 reason 有同名概念的标准 OAuth 错误）。
pub fn retry_reason_for_oauth_error(oauth_error: &str) -> RetryAdviceReason {
    match oauth_error {
        "insufficient_scope" => RetryAdviceReason::InsufficientScope,
        "invalid_token"
        | "invalid_grant"
        | "invalid_request"
        | "access_denied"
        | "unsupported_grant_type" => RetryAdviceReason::Failed,
        _ => RetryAdviceReason::Unknown,
    }
}

// P6（compliance 落地后补齐）：
//   - COMPLIANCE_KEY_TO_RETRY_REASON 映射表（穷举 ComplianceErrorKey）
//   - retry_reason_for_compliance_key(key) -> RetryAdviceReason
//   - compliance_error_to_retry_advice(info: &ComplianceErrorInfo) -> RetryAdvice
