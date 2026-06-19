//! SDK-safe 签署 envelope 公共领域类型。端口自 `compliance/signing/types.ts`。
//!
//! 设计原则见 `compliance/evidence/mod.rs` 顶部说明。

use crate::shared::pagination::PageRequest;
use serde::{Deserialize, Serialize};

// =============================================================================
// Signing Envelope
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningEnvelope {
    pub id: i64,
    #[serde(rename = "envelopeNo")]
    pub envelope_no: String,
    #[serde(
        rename = "applicantUserId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub applicant_user_id: Option<String>,
    pub status: String,
    #[serde(
        rename = "primaryContractId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub primary_contract_id: Option<i64>,
    #[serde(
        rename = "contractHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contract_hash: Option<String>,
    #[serde(
        rename = "hashAlgorithm",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub hash_algorithm: Option<String>,
    #[serde(
        rename = "billingGroupId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub billing_group_id: Option<String>,
    #[serde(rename = "chainId", default, skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<String>,
    #[serde(rename = "requestId", default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(
        rename = "pendingReason",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pending_reason: Option<String>,
    #[serde(rename = "signedAt", default, skip_serializing_if = "Option::is_none")]
    pub signed_at: Option<String>,
    #[serde(
        rename = "evidenceReadyAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub evidence_ready_at: Option<String>,
    #[serde(
        rename = "committedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub committed_at: Option<String>,
}

/// 创建 envelope 请求；只保留调用方业务字段，内部主体快照和履约通道由后端推导。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreateSigningEnvelopeRequest {
    #[serde(
        rename = "envelopeNo",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub envelope_no: Option<String>,
    #[serde(rename = "requestId", default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// 调用方稳定 idempotency-key（不传则由 Idempotency-Key header 兜底）。
    #[serde(
        rename = "idempotencyKey",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub idempotency_key: Option<String>,
    #[serde(
        rename = "billingGroupId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub billing_group_id: Option<String>,
    #[serde(rename = "chainId", default, skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<String>,
}

/// sign 请求；service 默认闸门关闭，写示例必须处理 `ENVELOPE_GATE_CLOSED` 错误。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SignEnvelopeRequest {
    #[serde(
        rename = "approvalRequestId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub approval_request_id: Option<i64>,
    #[serde(
        rename = "contractId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contract_id: Option<i64>,
    #[serde(
        rename = "contractHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contract_hash: Option<String>,
    #[serde(rename = "sealId", default, skip_serializing_if = "Option::is_none")]
    pub seal_id: Option<i64>,
    #[serde(
        rename = "signLocationType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sign_location_type: Option<String>,
    #[serde(
        rename = "signLocationPayload",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sign_location_payload: Option<String>,
    #[serde(
        rename = "transactorId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub transactor_id: Option<i64>,
    #[serde(rename = "requestId", default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(
        rename = "idempotencyKey",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub idempotency_key: Option<String>,
}

/// H5 短链请求（与 [`SignEnvelopeRequest`] 同构）。对应 TS `CreateH5SigningUrlRequest`。
pub type CreateH5SigningUrlRequest = SignEnvelopeRequest;

// =============================================================================
// List / Page (compliance gateway S1 — gap-register U-1)
// =============================================================================

/// 签署 envelope 分页【列表项】视图。对应后端 G1 `SigningEnvelopePageItem`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningEnvelopePageItem {
    pub id: i64,
    #[serde(rename = "envelopeNo")]
    pub envelope_no: String,
    #[serde(
        rename = "applicantUserId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub applicant_user_id: Option<String>,
    pub status: String,
    #[serde(
        rename = "primaryContractId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub primary_contract_id: Option<i64>,
    #[serde(
        rename = "contractHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contract_hash: Option<String>,
    #[serde(
        rename = "hashAlgorithm",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub hash_algorithm: Option<String>,
    #[serde(
        rename = "billingGroupId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub billing_group_id: Option<String>,
    #[serde(rename = "chainId", default, skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<String>,
    #[serde(rename = "requestId", default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(
        rename = "pendingReason",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pending_reason: Option<String>,
    #[serde(rename = "signedAt", default, skip_serializing_if = "Option::is_none")]
    pub signed_at: Option<String>,
    #[serde(
        rename = "evidenceReadyAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub evidence_ready_at: Option<String>,
    #[serde(
        rename = "committedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub committed_at: Option<String>,
    /// 创建时间 ISO-8601。
    #[serde(rename = "createTime")]
    pub create_time: String,
}

/// `list_signing_envelopes` 请求参数。
#[derive(Debug, Clone, Default)]
pub struct ListSigningEnvelopesRequest {
    pub page: PageRequest,
    /// envelope 状态过滤。
    pub status: Option<String>,
    pub create_time_start: Option<String>,
    pub create_time_end: Option<String>,
}

// =============================================================================
// Envelope Completion (compliance gateway S4 — gap-register U-10 / U-12)
// =============================================================================

/// 签署 envelope 下挂的合同【列表项】视图。对应后端 G4 `EnvelopeContractItem`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeContractItem {
    /// 合同行 id（数值主键）。
    pub id: i64,
    /// 所属 envelope id。
    #[serde(rename = "envelopeId")]
    pub envelope_id: i64,
    /// 合同编号。
    #[serde(rename = "contractNo")]
    pub contract_no: String,
    /// 合同标题。
    pub title: String,
    /// 合同文件 MIME 类型。
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    /// 合同文件字节数。
    pub size: i64,
    /// 哈希算法（如 `sha256`）。
    #[serde(rename = "hashAlgorithm")]
    pub hash_algorithm: String,
    /// 合同原文内容哈希。
    #[serde(rename = "contentHash")]
    pub content_hash: String,
    /// 签署后内容哈希（未签署时缺省）。
    #[serde(
        rename = "signedContentHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub signed_content_hash: Option<String>,
    /// 合同状态。
    pub status: String,
    /// 创建时间 ISO-8601。
    #[serde(rename = "createTime")]
    pub create_time: String,
}

/// `void_envelope` 请求体。作废一个签署 envelope，`reason` 为必填的作废原因。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoidEnvelopeRequest {
    /// 作废原因（必填，随 JSON body 提交）。
    pub reason: String,
}
