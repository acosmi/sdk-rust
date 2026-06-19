//! SDK-safe 用印审批公共领域类型。端口自 `compliance/seal-approval/types.ts`。
//!
//! 设计原则见 `compliance/evidence/mod.rs` 顶部说明。

use crate::shared::pagination::PageRequest;
use serde::{Deserialize, Serialize};

// =============================================================================
// Seal Approval
// =============================================================================

/// 用印审批视图。
///
/// 🔴 同名字段分歧（方案 §3）：本视图 `seal_id` 是 `Option<i64>`（与 provider 域 String 分歧）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealApproval {
    pub id: i64,
    #[serde(
        rename = "envelopeId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub envelope_id: Option<i64>,
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
    #[serde(
        rename = "hashAlgorithm",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub hash_algorithm: Option<String>,
    #[serde(rename = "sealId", default, skip_serializing_if = "Option::is_none")]
    pub seal_id: Option<i64>,
    #[serde(
        rename = "applicantUserId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub applicant_user_id: Option<String>,
    #[serde(
        rename = "approverUserId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub approver_user_id: Option<String>,
    #[serde(
        rename = "transactorId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub transactor_id: Option<i64>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(rename = "expiresAt", default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub status: String,
    #[serde(
        rename = "approvedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub approved_at: Option<String>,
    #[serde(
        rename = "rejectedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rejected_at: Option<String>,
    #[serde(
        rename = "canceledAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub canceled_at: Option<String>,
}

/// 提交审批；provider 侧字段由后端归一，SDK 调用方不传。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubmitSealApprovalRequest {
    #[serde(
        rename = "envelopeId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub envelope_id: Option<i64>,
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
    #[serde(
        rename = "hashAlgorithm",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub hash_algorithm: Option<String>,
    #[serde(rename = "sealId", default, skip_serializing_if = "Option::is_none")]
    pub seal_id: Option<i64>,
    #[serde(
        rename = "approverUserId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub approver_user_id: Option<String>,
    #[serde(
        rename = "transactorId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub transactor_id: Option<i64>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// 审批通过 query 参数。
#[derive(Debug, Clone, Default)]
pub struct ApproveSealApprovalQuery {
    pub expires_at: Option<String>,
    pub note: Option<String>,
}

/// 审批拒绝 query 参数。
#[derive(Debug, Clone, Default)]
pub struct RejectSealApprovalQuery {
    pub reason: Option<String>,
}

/// 审批取消 query 参数。
#[derive(Debug, Clone, Default)]
pub struct CancelSealApprovalQuery {
    pub reason: Option<String>,
}

// =============================================================================
// List / Page (compliance gateway S1 — gap-register U-1)
// =============================================================================

/// 用印审批分页【列表项】视图。对应后端 G1 `SealApprovalPageItem`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealApprovalPageItem {
    pub id: i64,
    #[serde(
        rename = "envelopeId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub envelope_id: Option<i64>,
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
    #[serde(
        rename = "hashAlgorithm",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub hash_algorithm: Option<String>,
    #[serde(rename = "sealId", default, skip_serializing_if = "Option::is_none")]
    pub seal_id: Option<i64>,
    #[serde(
        rename = "applicantUserId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub applicant_user_id: Option<String>,
    #[serde(
        rename = "approverUserId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub approver_user_id: Option<String>,
    #[serde(
        rename = "transactorId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub transactor_id: Option<i64>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(rename = "expiresAt", default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub status: String,
    #[serde(
        rename = "approvedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub approved_at: Option<String>,
    #[serde(
        rename = "rejectedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rejected_at: Option<String>,
    #[serde(
        rename = "canceledAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub canceled_at: Option<String>,
    /// 创建时间 ISO-8601。
    #[serde(rename = "createTime")]
    pub create_time: String,
}

/// `list_seal_approvals` 请求参数。
#[derive(Debug, Clone, Default)]
pub struct ListSealApprovalsRequest {
    pub page: PageRequest,
    /// 审批状态过滤。
    pub status: Option<String>,
    pub create_time_start: Option<String>,
    pub create_time_end: Option<String>,
}

// =============================================================================
// Seal Use — List / Page (compliance gateway S6 — gap-register U-4)
// =============================================================================

/// 用印执行记录分页【列表项】视图。对应后端 G6 `SealUsePageItem`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealUsePageItem {
    pub id: i64,
    #[serde(rename = "envelopeId")]
    pub envelope_id: i64,
    #[serde(rename = "contractId")]
    pub contract_id: i64,
    #[serde(rename = "sealId")]
    pub seal_id: i64,
    /// 用印执行状态。
    #[serde(rename = "usageStatus")]
    pub usage_status: String,
    /// 签署位置类型（坐标 / 关键字 / 域字段等）。
    #[serde(
        rename = "signLocationType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sign_location_type: Option<String>,
    /// 调起时间 ISO-8601。
    #[serde(rename = "invokedAt", default, skip_serializing_if = "Option::is_none")]
    pub invoked_at: Option<String>,
    /// 成功落章时间 ISO-8601。
    #[serde(
        rename = "consumedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub consumed_at: Option<String>,
    /// 失败时的错误原因（如有）。
    #[serde(
        rename = "failureReason",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub failure_reason: Option<String>,
    /// 创建时间 ISO-8601。
    #[serde(rename = "createTime")]
    pub create_time: String,
}

/// `list_seal_uses` 请求参数。
#[derive(Debug, Clone, Default)]
pub struct ListSealUsesRequest {
    pub page: PageRequest,
    /// 印章 id 过滤。
    pub seal_id: Option<i64>,
    /// 签署 envelope id 过滤。
    pub envelope_id: Option<i64>,
    /// 用印执行状态过滤。
    pub usage_status: Option<String>,
    pub create_time_start: Option<String>,
    pub create_time_end: Option<String>,
}
