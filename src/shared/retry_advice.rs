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

// =============================================================================
// compliance 错误 key → reason 映射（P1 延后，P6 compliance 落地后补齐）。
//
// 依赖 `compliance::errors::{ComplianceErrorKey, ComplianceErrorInfo}`。Rust 中 shared 与
// compliance 同 crate 跨模块互引不构成循环（compliance::errors 引 shared::errors::Error，
// 本处引 compliance::errors::ComplianceErrorKey，无类型定义环）。
// =============================================================================

use crate::compliance::errors::{ComplianceErrorInfo, ComplianceErrorKey};

/// `ComplianceErrorKey` → `RetryAdviceReason`。**穷举映射**（match 缺分支 = 编译错误，
/// 保证后端新增 key 时 Rust 侧强制更新）。对应 TS `COMPLIANCE_KEY_TO_RETRY_REASON`。
pub fn retry_reason_for_compliance_key(key: ComplianceErrorKey) -> RetryAdviceReason {
    use ComplianceErrorKey as K;
    use RetryAdviceReason as R;
    match key {
        // 通用 / token / scope
        K::ComplianceUnauthorized => R::Failed,
        K::ComplianceInsufficientScope => R::InsufficientScope,
        K::ComplianceStepUpRequired => R::StepUpRequired,
        K::ComplianceTokenInvalid => R::Failed,
        // 主体快照
        K::SubjectSnapshotRequired => R::Failed,
        K::SubjectSnapshotNotFound => R::Failed,
        K::SubjectSnapshotTenantMismatch => R::TenantMismatch,
        // Evidence / Timestamp / Package / Report
        K::EvidenceAssetNotFound => R::Failed,
        K::EvidenceAssetHashMismatch => R::LocalVerifyFailed,
        K::EvidenceAssetPayloadRequired => R::Failed,
        K::TimestampTokenNotFound => R::Failed,
        K::TimestampProviderFailed => R::Failed,
        K::TimestampProviderUnknown => R::Unknown,
        K::TimestampLocalVerifyFailed => R::LocalVerifyFailed,
        K::TimestampProviderNotAvailable => R::ProviderTimeout,
        K::EvidencePackageNotFound => R::Failed,
        K::EvidencePackageTimestampRequired => R::Failed,
        K::EvidencePackageManifestHashMismatch => R::LocalVerifyFailed,
        K::ReportNotFound => R::Failed,
        K::ReportAlreadyPublished => R::Failed,
        K::ReportDraftRequired => R::Failed,
        K::EvidenceVerifyTargetRequired => R::Failed,
        K::EvidenceVerifyTargetNotFound => R::Failed,
        // Provider request
        K::ProviderRequestUnknownNoRetry => R::Unknown,
        K::ProviderCallbackSourceInvalid => R::Failed,
        K::ProviderNotConfigured => R::GateClosed,
        K::ProviderRequestNotFound => R::Failed,
        K::ProviderRequestIdempotencyRequired => R::Failed,
        K::ProviderRequestStatusNotTerminal => R::Retrying,
        // Envelope
        K::EnvelopeNotFound => R::Failed,
        K::EnvelopeTenantMismatch => R::TenantMismatch,
        K::EnvelopeStateNotAllowed => R::Failed,
        K::EnvelopeGateClosed => R::GateClosed,
        K::ContractNotFound => R::Failed,
        K::ContractHashMismatch => R::LocalVerifyFailed,
        K::ProviderAuthorizationNotConfirmed => R::Failed,
        K::EnvelopeEvidenceNotReady => R::Retrying,
        // Seal approval / use
        K::SealAssetNotFound => R::Failed,
        K::SealApprovalNotFound => R::Failed,
        K::SealApprovalStateNotApproved => R::Failed,
        K::SealApprovalExpired => R::Failed,
        K::SealApprovalAlreadyUsed => R::Failed,
        K::SealApprovalNonceUsed => R::Failed,
        K::SealApprovalSealMismatch => R::Failed,
        K::SealApprovalLocationMismatch => R::Failed,
        K::SealApprovalTransactorMismatch => R::Failed,
        K::SealApprovalContractHashMismatch => R::LocalVerifyFailed,
        K::SealApprovalInvalidTransition => R::Failed,
        K::SealUseAlreadyConsumed => R::Failed,
        // Audit chain
        K::AuditChainTamperDetected => R::LocalVerifyFailed,
        // Billing
        K::BillingCommitRequiresLocalVerify => R::LocalVerifyFailed,
        K::BillingCommitRequiresProviderSuccess => R::BillingPreflightFailed,
        K::BillingCallbackCannotCommit => R::BillingPreflightFailed,
        K::BillingProviderUnknownNotCommittable => R::BillingPreflightFailed,
        K::BillingS2sForbidden => R::Failed,
        // SDK fallback
        K::UnknownComplianceError => R::Unknown,
    }
}

/// `ComplianceErrorInfo` → `RetryAdvice`。对应 TS `complianceErrorToRetryAdvice`。
pub fn compliance_error_to_retry_advice(info: &ComplianceErrorInfo) -> RetryAdvice {
    RetryAdvice {
        retryable: info.retryable,
        retry_after: None,
        same_idempotency_key_required: !info.terminal,
        manual_action_required: info.terminal || info.step_up_required,
        reason: retry_reason_for_compliance_key(info.key),
        user_message: None,
        developer_message: Some(info.message.clone()),
        support_code: Some(format!("compliance:{}", info.code)),
    }
}

#[cfg(test)]
mod compliance_tests {
    use super::*;

    #[test]
    fn step_up_key_maps() {
        assert_eq!(
            retry_reason_for_compliance_key(ComplianceErrorKey::ComplianceStepUpRequired),
            RetryAdviceReason::StepUpRequired
        );
        assert_eq!(
            retry_reason_for_compliance_key(ComplianceErrorKey::EnvelopeGateClosed),
            RetryAdviceReason::GateClosed
        );
        assert_eq!(
            retry_reason_for_compliance_key(ComplianceErrorKey::SubjectSnapshotTenantMismatch),
            RetryAdviceReason::TenantMismatch
        );
        assert_eq!(
            retry_reason_for_compliance_key(ComplianceErrorKey::UnknownComplianceError),
            RetryAdviceReason::Unknown
        );
    }

    #[test]
    fn advice_terminal_sets_manual_and_drops_same_key() {
        let info = ComplianceErrorInfo {
            code: 1031004004,
            message: "gate closed".to_string(),
            key: ComplianceErrorKey::EnvelopeGateClosed,
            retryable: false,
            terminal: true,
            step_up_required: false,
        };
        let advice = compliance_error_to_retry_advice(&info);
        assert!(!advice.retryable);
        assert!(advice.manual_action_required);
        assert!(!advice.same_idempotency_key_required);
        assert_eq!(advice.reason, RetryAdviceReason::GateClosed);
        assert_eq!(
            advice.support_code.as_deref(),
            Some("compliance:1031004004")
        );
    }

    #[test]
    fn advice_step_up_requires_manual() {
        let info = ComplianceErrorInfo {
            code: 1031000013,
            message: "step up".to_string(),
            key: ComplianceErrorKey::ComplianceStepUpRequired,
            retryable: false,
            terminal: false,
            step_up_required: true,
        };
        let advice = compliance_error_to_retry_advice(&info);
        assert!(advice.manual_action_required);
        // 非终态 → 仍要求同一幂等键。
        assert!(advice.same_idempotency_key_required);
        assert_eq!(advice.reason, RetryAdviceReason::StepUpRequired);
    }
}
