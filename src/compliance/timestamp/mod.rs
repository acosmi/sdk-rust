//! SDK-safe 时间章公共领域类型。端口自 `compliance/timestamp/types.ts`。
//!
//! 设计原则见 `compliance/evidence/mod.rs` 顶部说明。

use crate::compliance::evidence::ComplianceHashAlgorithm;
use crate::macros::open_string_union;
use crate::shared::pagination::PageRequest;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =============================================================================
// Timestamp
// =============================================================================

open_string_union! {
    /// 时间章校验状态。开放联合，后端保留新增空间。
    ComplianceTimestampVerificationStatus {
        PENDING => "PENDING",
        VERIFIED => "VERIFIED",
        FAILED => "FAILED",
        LOCAL_VERIFY_FAILED => "LOCAL_VERIFY_FAILED",
        UNKNOWN => "UNKNOWN",
        RETRYING => "RETRYING",
    }
}

/// 时间章 token 对外视图（不含 provider/object/tsa 内部字段）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimestampToken {
    pub id: i64,
    #[serde(rename = "assetId")]
    pub asset_id: i64,
    #[serde(rename = "policyOid", default, skip_serializing_if = "Option::is_none")]
    pub policy_oid: Option<String>,
    #[serde(
        rename = "serialNumber",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub serial_number: Option<String>,
    /// gen_time ISO-8601；UNKNOWN/PENDING 状态可能为 null。
    #[serde(rename = "genTime", default, skip_serializing_if = "Option::is_none")]
    pub gen_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accuracy: Option<String>,
    #[serde(rename = "verificationStatus")]
    pub verification_status: ComplianceTimestampVerificationStatus,
    #[serde(
        rename = "verifiedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub verified_at: Option<String>,
    #[serde(
        rename = "verificationError",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub verification_error: Option<String>,
}

/// 申请时间章请求。`provider` 字段已从服务端契约下线；SDK 永远不传。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IssueTimestampRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "mimeType", default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(rename = "hashAlgorithm")]
    pub hash_algorithm: ComplianceHashAlgorithm,
    /// 客户端声明 digest（hex）；contentBase64 非空时作为校验值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(
        rename = "contentBase64",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub content_base64: Option<String>,
}

/// verify 请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyTimestampRequest {
    #[serde(rename = "tokenId")]
    pub token_id: i64,
}

/// verify 结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimestampVerifyResult {
    pub passed: bool,
    pub reason: String,
}

// =============================================================================
// List / Page (compliance gateway S1 — gap-register U-1)
// =============================================================================

/// 时间章分页【列表项】视图。对应后端 G1 `TimestampPageItem`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimestampPageItem {
    pub id: i64,
    #[serde(rename = "assetId")]
    pub asset_id: i64,
    #[serde(rename = "policyOid", default, skip_serializing_if = "Option::is_none")]
    pub policy_oid: Option<String>,
    #[serde(
        rename = "serialNumber",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub serial_number: Option<String>,
    #[serde(rename = "genTime", default, skip_serializing_if = "Option::is_none")]
    pub gen_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accuracy: Option<String>,
    #[serde(rename = "verificationStatus")]
    pub verification_status: ComplianceTimestampVerificationStatus,
    #[serde(
        rename = "verifiedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub verified_at: Option<String>,
    #[serde(
        rename = "verificationError",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub verification_error: Option<String>,
    /// 创建时间 ISO-8601。
    #[serde(rename = "createTime")]
    pub create_time: String,
}

/// `list_timestamps` 请求参数。
#[derive(Debug, Clone, Default)]
pub struct ListTimestampsRequest {
    pub page: PageRequest,
    /// 时间章 provider 过滤（`TsaProviderEnum.name()` 之类）。
    pub provider: Option<String>,
    /// 校验状态过滤。
    pub verification_status: Option<String>,
    pub create_time_start: Option<String>,
    pub create_time_end: Option<String>,
}

// =============================================================================
// TSA readonly views (compliance gateway S3 — gap-register U-7)
// =============================================================================

/// 时间章授权机构（TSA）provider 视图。对应后端 G3 `TsaProviderVO`。只读：不含 provider
/// 端点、凭证、证书或其它内部接入材料。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsaProvider {
    /// provider 名称（如 `TsaProviderEnum.name()`）。
    pub name: String,
    /// provider 所处环境（如 `production` / `sandbox`）。
    pub environment: String,
    /// 该 provider 当前是否可用。
    pub available: bool,
}

/// 时间章统计视图。对应后端 G3 `TsaStatsVO`。只读聚合：时间章总数 + 按校验状态分桶的计数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsaStats {
    /// 时间章总数。
    pub total: i64,
    /// 按校验状态分桶的计数。键为校验状态枚举名（如 `VERIFIED` / `PENDING` / `FAILED`）。
    #[serde(rename = "byVerificationStatus")]
    pub by_verification_status: HashMap<String, i64>,
}
