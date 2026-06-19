//! SDK-safe 证据报告公共领域类型。端口自 `compliance/report/types.ts`。
//!
//! 设计原则见 `compliance/evidence/mod.rs` 顶部说明。

use crate::shared::pagination::PageRequest;
use serde::{Deserialize, Serialize};

// =============================================================================
// Report
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceReport {
    pub id: i64,
    #[serde(rename = "reportNo")]
    pub report_no: String,
    #[serde(rename = "reportType")]
    pub report_type: String,
    pub status: String,
    #[serde(rename = "assetId", default, skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<i64>,
    #[serde(rename = "packageId", default, skip_serializing_if = "Option::is_none")]
    pub package_id: Option<i64>,
    #[serde(
        rename = "publicUrlToken",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub public_url_token: Option<String>,
    #[serde(
        rename = "publishedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub published_at: Option<String>,
    #[serde(rename = "bodyHash", default, skip_serializing_if = "Option::is_none")]
    pub body_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateReportRequest {
    #[serde(rename = "assetId")]
    pub asset_id: i64,
    #[serde(rename = "packageId")]
    pub package_id: i64,
}

/// 离线复核下载 VO；建议调用方持久化作为长期可重复验证依据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportDownload {
    pub id: i64,
    #[serde(rename = "reportNo")]
    pub report_no: String,
    #[serde(rename = "reportType")]
    pub report_type: String,
    pub status: String,
    #[serde(rename = "bodyHash", default, skip_serializing_if = "Option::is_none")]
    pub body_hash: Option<String>,
    #[serde(
        rename = "publishedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub published_at: Option<String>,
    #[serde(
        rename = "assetEvidenceNo",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub asset_evidence_no: Option<String>,
    #[serde(
        rename = "assetHashAlgorithm",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub asset_hash_algorithm: Option<String>,
    #[serde(
        rename = "assetContentHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub asset_content_hash: Option<String>,
    #[serde(
        rename = "packageManifestHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub package_manifest_hash: Option<String>,
    #[serde(
        rename = "packageHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub package_hash: Option<String>,
    #[serde(
        rename = "packageHashAlgorithm",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub package_hash_algorithm: Option<String>,
    #[serde(
        rename = "timestampSerialNumber",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub timestamp_serial_number: Option<String>,
    #[serde(
        rename = "timestampGenTime",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub timestamp_gen_time: Option<String>,
    #[serde(
        rename = "timestampVerificationStatus",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub timestamp_verification_status: Option<String>,
}

// =============================================================================
// List / Page (compliance gateway S1 — gap-register U-1)
// =============================================================================

/// 证据报告分页【列表项】视图。对应后端 G1 `ReportPageItem`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportPageItem {
    pub id: i64,
    #[serde(rename = "reportNo")]
    pub report_no: String,
    #[serde(rename = "reportType")]
    pub report_type: String,
    pub status: String,
    #[serde(rename = "assetId", default, skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<i64>,
    #[serde(rename = "packageId", default, skip_serializing_if = "Option::is_none")]
    pub package_id: Option<i64>,
    #[serde(
        rename = "publicUrlToken",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub public_url_token: Option<String>,
    #[serde(
        rename = "publishedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub published_at: Option<String>,
    #[serde(rename = "bodyHash", default, skip_serializing_if = "Option::is_none")]
    pub body_hash: Option<String>,
    /// 创建时间 ISO-8601。
    #[serde(rename = "createTime")]
    pub create_time: String,
}

/// `list_reports` 请求参数。
#[derive(Debug, Clone, Default)]
pub struct ListReportsRequest {
    pub page: PageRequest,
    /// 报告状态过滤。
    pub status: Option<String>,
    pub create_time_start: Option<String>,
    pub create_time_end: Option<String>,
}
