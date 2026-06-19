//! SDK-safe provider request 公共领域类型。端口自 `compliance/provider/types.ts`。
//!
//! 设计原则见 `compliance/evidence/mod.rs` 顶部说明。
//!
//! 相位说明（执行日志相位 #2）：TS `ComplianceProviderRequestStatus` 是定义点，而 shared
//! `operation.ts` 的 `ProviderRequestStatus` 是其别名。Rust 中为避免 shared → compliance 的
//! 前向依赖，**定义点放在 `shared::operation::ProviderRequestStatus`**，本模块反向
//! `pub use ... as ComplianceProviderRequestStatus`（定义点与 TS 相反，两名都保留）。

use crate::shared::operation::ProviderRequestStatus;
use serde::{Deserialize, Serialize};

/// Provider request 状态（前端可见）。定义点在 `shared::operation`（见模块说明）。
pub use crate::shared::operation::ProviderRequestStatus as ComplianceProviderRequestStatus;

// =============================================================================
// Provider Request
// =============================================================================

/// Provider 请求状态视图。
///
/// 🔴 同名字段分歧（方案 §3）：本视图的 `seal_id` 是 `Option<String>`，与
/// operation / seal-approval 域的 `Option<i64>` 不同 —— 逐字段查证，不可统一。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRequestStatusView {
    pub id: i64,
    pub status: ProviderRequestStatus,
    /// SUCCESS / FAILED 终态。
    pub terminal: bool,
    /// 当前状态是否允许 SDK 安全重试请求（仅对 RETRYING 为 true）。
    pub retryable: bool,
    #[serde(
        rename = "businessNo",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub business_no: Option<String>,
    #[serde(
        rename = "contractNo",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contract_no: Option<String>,
    /// 🔴 provider 域 `seal_id` 是 `Option<String>`（与 operation/seal-approval 的 i64 分歧）。
    #[serde(rename = "sealId", default, skip_serializing_if = "Option::is_none")]
    pub seal_id: Option<String>,
    #[serde(
        rename = "attemptCount",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub attempt_count: Option<i64>,
    #[serde(
        rename = "reconciliationStatus",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub reconciliation_status: Option<String>,
    #[serde(
        rename = "nextRetryAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub next_retry_at: Option<String>,
    #[serde(
        rename = "requestedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub requested_at: Option<String>,
    #[serde(
        rename = "respondedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub responded_at: Option<String>,
}
