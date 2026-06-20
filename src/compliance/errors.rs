//! Java numeric ErrorCode → SDK symbolic key。端口自 `compliance/errors.ts`。
//!
//! 设计原则：
//!   - Java 后端通过 `cn.iocoder.yudao.module.compliance.enums.ErrorCodeConstants` 暴露数值
//!     错误码（1-031-xxx-xxx）。本文件按段位 + 具体码值映射为 SDK 内部 symbolic key，仅作
//!     SDK 分支判断 / 文档说明，不作为 wire response 反序列化字段。
//!   - 不要尝试从后端 message 文案做正则识别 —— message 是中文可变的，code 才是合同。
//!   - retryable / terminal / step_up_required 严格按"是否安全自动重发"语义：
//!       * step_up_required: 客户端引导用户重新做 OAuth introspection / step-up，不要静默 retry。
//!       * retryable: 仅对幂等的网络/资源短暂错误为 true（当前默认全为 false，因为 compliance
//!         写接口严禁自动重发）。
//!       * terminal: 不需要 SDK 再次轮询；用户必须用新 idempotency-key 重新发起。
//!
//! Rust 化：TS `ComplianceErrorKey` 是 50+ 项的字符串字面量 union。本文件端口为 **enum +
//! serde rename**（每个 variant 的 wire 字符串 = TS 字面量），便于 `retry_advice` 在编译期
//! 穷举映射不漏（match 缺分支 = 编译错误）。

use crate::shared::errors::Error;
use serde::{Deserialize, Serialize};

/// SDK 内部 symbolic key；用于代码分支判断与文档。**不是 wire contract**。
///
/// 端口自 TS `ComplianceErrorKey`。serde rename 使每个 variant 序列化为 TS 同名字面量字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ComplianceErrorKey {
    // 通用 / token / scope
    #[serde(rename = "COMPLIANCE_UNAUTHORIZED")]
    ComplianceUnauthorized,
    #[serde(rename = "COMPLIANCE_INSUFFICIENT_SCOPE")]
    ComplianceInsufficientScope,
    #[serde(rename = "COMPLIANCE_STEP_UP_REQUIRED")]
    ComplianceStepUpRequired,
    #[serde(rename = "COMPLIANCE_TOKEN_INVALID")]
    ComplianceTokenInvalid,
    // 主体快照
    #[serde(rename = "SUBJECT_SNAPSHOT_REQUIRED")]
    SubjectSnapshotRequired,
    #[serde(rename = "SUBJECT_SNAPSHOT_NOT_FOUND")]
    SubjectSnapshotNotFound,
    #[serde(rename = "SUBJECT_SNAPSHOT_TENANT_MISMATCH")]
    SubjectSnapshotTenantMismatch,
    // Evidence / Timestamp / Package / Report
    #[serde(rename = "EVIDENCE_ASSET_NOT_FOUND")]
    EvidenceAssetNotFound,
    #[serde(rename = "EVIDENCE_ASSET_HASH_MISMATCH")]
    EvidenceAssetHashMismatch,
    #[serde(rename = "EVIDENCE_ASSET_PAYLOAD_REQUIRED")]
    EvidenceAssetPayloadRequired,
    #[serde(rename = "TIMESTAMP_TOKEN_NOT_FOUND")]
    TimestampTokenNotFound,
    #[serde(rename = "TIMESTAMP_PROVIDER_FAILED")]
    TimestampProviderFailed,
    #[serde(rename = "TIMESTAMP_PROVIDER_UNKNOWN")]
    TimestampProviderUnknown,
    #[serde(rename = "TIMESTAMP_LOCAL_VERIFY_FAILED")]
    TimestampLocalVerifyFailed,
    #[serde(rename = "TIMESTAMP_PROVIDER_NOT_AVAILABLE")]
    TimestampProviderNotAvailable,
    #[serde(rename = "EVIDENCE_PACKAGE_NOT_FOUND")]
    EvidencePackageNotFound,
    #[serde(rename = "EVIDENCE_PACKAGE_TIMESTAMP_REQUIRED")]
    EvidencePackageTimestampRequired,
    #[serde(rename = "EVIDENCE_PACKAGE_MANIFEST_HASH_MISMATCH")]
    EvidencePackageManifestHashMismatch,
    #[serde(rename = "REPORT_NOT_FOUND")]
    ReportNotFound,
    #[serde(rename = "REPORT_ALREADY_PUBLISHED")]
    ReportAlreadyPublished,
    #[serde(rename = "REPORT_DRAFT_REQUIRED")]
    ReportDraftRequired,
    #[serde(rename = "EVIDENCE_VERIFY_TARGET_REQUIRED")]
    EvidenceVerifyTargetRequired,
    #[serde(rename = "EVIDENCE_VERIFY_TARGET_NOT_FOUND")]
    EvidenceVerifyTargetNotFound,
    // Provider request
    #[serde(rename = "PROVIDER_REQUEST_UNKNOWN_NO_RETRY")]
    ProviderRequestUnknownNoRetry,
    #[serde(rename = "PROVIDER_CALLBACK_SOURCE_INVALID")]
    ProviderCallbackSourceInvalid,
    #[serde(rename = "PROVIDER_NOT_CONFIGURED")]
    ProviderNotConfigured,
    #[serde(rename = "PROVIDER_REQUEST_NOT_FOUND")]
    ProviderRequestNotFound,
    #[serde(rename = "PROVIDER_REQUEST_IDEMPOTENCY_REQUIRED")]
    ProviderRequestIdempotencyRequired,
    #[serde(rename = "PROVIDER_REQUEST_STATUS_NOT_TERMINAL")]
    ProviderRequestStatusNotTerminal,
    // Envelope
    #[serde(rename = "ENVELOPE_NOT_FOUND")]
    EnvelopeNotFound,
    #[serde(rename = "ENVELOPE_TENANT_MISMATCH")]
    EnvelopeTenantMismatch,
    #[serde(rename = "ENVELOPE_STATE_NOT_ALLOWED")]
    EnvelopeStateNotAllowed,
    #[serde(rename = "ENVELOPE_GATE_CLOSED")]
    EnvelopeGateClosed,
    #[serde(rename = "CONTRACT_NOT_FOUND")]
    ContractNotFound,
    #[serde(rename = "CONTRACT_HASH_MISMATCH")]
    ContractHashMismatch,
    #[serde(rename = "PROVIDER_AUTHORIZATION_NOT_CONFIRMED")]
    ProviderAuthorizationNotConfirmed,
    #[serde(rename = "ENVELOPE_EVIDENCE_NOT_READY")]
    EnvelopeEvidenceNotReady,
    // Seal approval
    #[serde(rename = "SEAL_ASSET_NOT_FOUND")]
    SealAssetNotFound,
    #[serde(rename = "SEAL_APPROVAL_NOT_FOUND")]
    SealApprovalNotFound,
    #[serde(rename = "SEAL_APPROVAL_STATE_NOT_APPROVED")]
    SealApprovalStateNotApproved,
    #[serde(rename = "SEAL_APPROVAL_EXPIRED")]
    SealApprovalExpired,
    #[serde(rename = "SEAL_APPROVAL_ALREADY_USED")]
    SealApprovalAlreadyUsed,
    #[serde(rename = "SEAL_APPROVAL_NONCE_USED")]
    SealApprovalNonceUsed,
    #[serde(rename = "SEAL_APPROVAL_SEAL_MISMATCH")]
    SealApprovalSealMismatch,
    #[serde(rename = "SEAL_APPROVAL_LOCATION_MISMATCH")]
    SealApprovalLocationMismatch,
    #[serde(rename = "SEAL_APPROVAL_TRANSACTOR_MISMATCH")]
    SealApprovalTransactorMismatch,
    #[serde(rename = "SEAL_APPROVAL_CONTRACT_HASH_MISMATCH")]
    SealApprovalContractHashMismatch,
    #[serde(rename = "SEAL_APPROVAL_INVALID_TRANSITION")]
    SealApprovalInvalidTransition,
    #[serde(rename = "SEAL_USE_ALREADY_CONSUMED")]
    SealUseAlreadyConsumed,
    // Audit chain
    #[serde(rename = "AUDIT_CHAIN_TAMPER_DETECTED")]
    AuditChainTamperDetected,
    // Billing
    #[serde(rename = "BILLING_COMMIT_REQUIRES_LOCAL_VERIFY")]
    BillingCommitRequiresLocalVerify,
    #[serde(rename = "BILLING_COMMIT_REQUIRES_PROVIDER_SUCCESS")]
    BillingCommitRequiresProviderSuccess,
    #[serde(rename = "BILLING_CALLBACK_CANNOT_COMMIT")]
    BillingCallbackCannotCommit,
    #[serde(rename = "BILLING_PROVIDER_UNKNOWN_NOT_COMMITTABLE")]
    BillingProviderUnknownNotCommittable,
    #[serde(rename = "BILLING_S2S_FORBIDDEN")]
    BillingS2sForbidden,
    // SDK fallback
    #[serde(rename = "UNKNOWN_COMPLIANCE_ERROR")]
    UnknownComplianceError,
}

/// 单个错误的 SDK 视图。对应 TS `ComplianceErrorInfo`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComplianceErrorInfo {
    /// Java numeric error code (wire contract)。
    pub code: i64,
    /// 服务端原始 message（中文，可变）；仅用于日志/展示。
    pub message: String,
    /// SDK 分支判断用 symbolic key。
    pub key: ComplianceErrorKey,
    /// 是否安全自动重试 —— compliance 写接口几乎全为 false。
    pub retryable: bool,
    /// 是否已经是终态 —— 用户必须用新 idempotency-key 重新发起。
    pub terminal: bool,
    /// 高风险动作需要 step-up / introspection。
    #[serde(rename = "stepUpRequired")]
    pub step_up_required: bool,
}

/// numeric → symbolic 映射（与 Java ErrorCodeConstants 同步）。对应 TS `CODE_TO_KEY`。
fn code_to_key(code: i64) -> Option<ComplianceErrorKey> {
    use ComplianceErrorKey as K;
    Some(match code {
        // 通用 / token / scope (1-031-000-xxx)
        1031000001 => K::ComplianceUnauthorized,
        1031000002..=1031000011 => K::ComplianceTokenInvalid,
        1031000012 => K::ComplianceInsufficientScope,
        1031000013 => K::ComplianceStepUpRequired,
        // Subject snapshot (1-031-001-xxx)
        1031001001 => K::SubjectSnapshotNotFound,
        1031001002 => K::SubjectSnapshotTenantMismatch,
        1031001003 => K::SubjectSnapshotRequired,
        // Evidence / Timestamp / Package / Report (1-031-002-xxx)
        1031002001 => K::EvidenceAssetNotFound,
        1031002002 => K::SubjectSnapshotTenantMismatch,
        1031002003 => K::EvidenceAssetHashMismatch,
        1031002004 | 1031002005 => K::EvidenceAssetPayloadRequired,
        1031002006 => K::TimestampTokenNotFound,
        1031002007 => K::TimestampProviderFailed,
        1031002008 => K::TimestampProviderUnknown,
        1031002009 => K::TimestampLocalVerifyFailed,
        1031002010 => K::TimestampProviderNotAvailable,
        1031002011 => K::EvidencePackageNotFound,
        1031002012 => K::EvidencePackageTimestampRequired,
        1031002013 => K::EvidencePackageManifestHashMismatch,
        1031002014 => K::ReportNotFound,
        1031002015 => K::ReportAlreadyPublished,
        1031002016 => K::ReportDraftRequired,
        1031002017 => K::EvidenceVerifyTargetRequired,
        1031002018 => K::EvidenceVerifyTargetNotFound,
        // Provider request (1-031-003-xxx)
        1031003001 => K::ProviderRequestUnknownNoRetry,
        1031003002 => K::ProviderCallbackSourceInvalid,
        1031003003 => K::ProviderNotConfigured,
        1031003010 => K::ProviderRequestNotFound,
        1031003011 => K::ProviderRequestIdempotencyRequired,
        1031003012 => K::ProviderRequestStatusNotTerminal,
        // Envelope (1-031-004-xxx)
        1031004001 => K::EnvelopeNotFound,
        1031004002 => K::EnvelopeTenantMismatch,
        1031004003 => K::EnvelopeStateNotAllowed,
        1031004004 => K::EnvelopeGateClosed,
        1031004005 => K::ContractNotFound,
        1031004006 => K::ContractHashMismatch,
        1031004007 => K::ContractNotFound,
        1031004008 | 1031004009 => K::ProviderAuthorizationNotConfirmed,
        1031004010 => K::EnvelopeEvidenceNotReady,
        // Seal approval / use (1-031-005-xxx)
        1031005001 => K::SealAssetNotFound,
        1031005010 | 1031005011 => K::SealApprovalNotFound,
        1031005012 => K::SealApprovalStateNotApproved,
        1031005013 => K::SealApprovalExpired,
        1031005014 => K::SealApprovalAlreadyUsed,
        1031005015 => K::SealApprovalNonceUsed,
        1031005016 => K::SealApprovalSealMismatch,
        1031005017 => K::SealApprovalLocationMismatch,
        1031005018 => K::SealApprovalTransactorMismatch,
        1031005019 => K::SealApprovalContractHashMismatch,
        1031005020 => K::SealApprovalInvalidTransition,
        1031005030 => K::SealUseAlreadyConsumed,
        // Billing (1-031-006-xxx)
        1031006004 => K::BillingCommitRequiresLocalVerify,
        1031006005 => K::BillingCommitRequiresProviderSuccess,
        1031006007 => K::BillingCallbackCannotCommit,
        1031006008 => K::BillingProviderUnknownNotCommittable,
        1031006009 => K::BillingS2sForbidden,
        // Audit (1-031-007-xxx)
        1031007011 => K::AuditChainTamperDetected,
        _ => return None,
    })
}

/// 该 key 是否需要 step-up。对应 TS `STEP_UP_KEYS`。
fn is_step_up_key(key: ComplianceErrorKey) -> bool {
    matches!(key, ComplianceErrorKey::ComplianceStepUpRequired)
}

/// 该 key 是否终态。对应 TS `TERMINAL_KEYS`。
fn is_terminal_key(key: ComplianceErrorKey) -> bool {
    use ComplianceErrorKey as K;
    matches!(
        key,
        K::EnvelopeGateClosed
            | K::ProviderNotConfigured
            | K::ProviderRequestUnknownNoRetry
            | K::BillingCallbackCannotCommit
            | K::BillingCommitRequiresLocalVerify
            | K::BillingCommitRequiresProviderSuccess
            | K::BillingProviderUnknownNotCommittable
            | K::BillingS2sForbidden
            | K::SealApprovalNonceUsed
            | K::SealApprovalExpired
            | K::SealApprovalContractHashMismatch
            | K::SealApprovalSealMismatch
            | K::SealApprovalLocationMismatch
            | K::SealApprovalTransactorMismatch
            | K::SealUseAlreadyConsumed
            | K::ContractHashMismatch
            | K::TimestampLocalVerifyFailed
            | K::EvidencePackageManifestHashMismatch
            | K::AuditChainTamperDetected
            | K::ProviderRequestNotFound
            | K::EvidenceAssetHashMismatch
            | K::EvidenceVerifyTargetNotFound
            | K::ReportAlreadyPublished
    )
}

/// 该 key 是否 retryable。对应 TS `RETRYABLE_KEYS`（当前为空 —— 真正网络层短暂错误才允许）。
fn is_retryable_key(_key: ComplianceErrorKey) -> bool {
    false
}

/// 把一个 `BusinessError`（`Error::Business`）分类为 compliance 视图。对应 TS
/// `classifyComplianceError`。
///
/// 非 compliance 段位的错误码会回退到 `UnknownComplianceError`，调用方应该当作业务错误处理。
/// 传入的 `Error` 必须是 `Error::Business` 变体；其它变体不携带 numeric code，返回回退视图。
pub fn classify_compliance_error(err: &Error) -> ComplianceErrorInfo {
    let (code, message) = match err {
        Error::Business { code, message } => (*code, message.clone()),
        _ => (0, err.to_string()),
    };
    let key = code_to_key(code).unwrap_or(ComplianceErrorKey::UnknownComplianceError);
    ComplianceErrorInfo {
        code,
        message,
        key,
        retryable: is_retryable_key(key),
        terminal: is_terminal_key(key),
        step_up_required: is_step_up_key(key),
    }
}

/// 判断 `BusinessError` 是否属于 compliance 段位（1-031-xxx-xxx）。对应 TS
/// `isComplianceBusinessError`。
///
/// 不在段位的错误码可能来自其它 yudao 模块，按通用业务错误处理即可。非 `Error::Business`
/// 变体返回 false。
pub fn is_compliance_business_error(err: &Error) -> bool {
    matches!(err, Error::Business { code, .. } if (1031000000..=1031999999).contains(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_serializes_to_symbolic_string() {
        let s = serde_json::to_string(&ComplianceErrorKey::EnvelopeGateClosed).unwrap();
        assert_eq!(s, "\"ENVELOPE_GATE_CLOSED\"");
        let back: ComplianceErrorKey =
            serde_json::from_str("\"SEAL_USE_ALREADY_CONSUMED\"").unwrap();
        assert_eq!(back, ComplianceErrorKey::SealUseAlreadyConsumed);
    }

    #[test]
    fn classify_step_up() {
        let info = classify_compliance_error(&Error::business(1031000013, "需 step-up"));
        assert_eq!(info.key, ComplianceErrorKey::ComplianceStepUpRequired);
        assert!(info.step_up_required);
        assert!(!info.terminal);
        assert!(!info.retryable);
    }

    #[test]
    fn classify_terminal_gate_closed() {
        let info = classify_compliance_error(&Error::business(1031004004, "gate closed"));
        assert_eq!(info.key, ComplianceErrorKey::EnvelopeGateClosed);
        assert!(info.terminal);
        assert!(!info.step_up_required);
    }

    #[test]
    fn classify_token_invalid_band() {
        for code in [1031000002, 1031000007, 1031000011] {
            let info = classify_compliance_error(&Error::business(code, "x"));
            assert_eq!(info.key, ComplianceErrorKey::ComplianceTokenInvalid);
        }
    }

    #[test]
    fn classify_unknown_out_of_band() {
        let info = classify_compliance_error(&Error::business(9999, "other module"));
        assert_eq!(info.key, ComplianceErrorKey::UnknownComplianceError);
        assert!(!info.terminal);
    }

    #[test]
    fn is_compliance_band_detection() {
        assert!(is_compliance_business_error(&Error::business(
            1031004004, ""
        )));
        assert!(!is_compliance_business_error(&Error::business(
            1010000001, ""
        )));
        assert!(!is_compliance_business_error(&Error::other("not business")));
    }
}
