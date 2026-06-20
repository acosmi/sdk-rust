//! SDK-safe compliance capability + operation projection 公共领域类型。端口自
//! `compliance/operation/types.ts`。
//!
//! 设计原则见 `compliance/evidence/mod.rs` 顶部说明。
//!
//! 对接后端 compliance gateway S2 / G2 契约（gap-register U-5 / U-6）：
//!   - `GET /compliance/capabilities`    → `Vec<ComplianceCapability>`
//!   - `GET /compliance/operations/page` → `PageResult<OperationPageItem>`
//!   - `GET /compliance/operations/{id}` → `OperationDetail`
//!
//! `state` 字段【复用】跨域 shared 的 `FeatureGateState`（不另造同名近似类型）。

use crate::shared::gate::FeatureGateState;
use crate::shared::pagination::PageRequest;
use serde::{Deserialize, Serialize};

// =============================================================================
// Capability（feature gate 能力查询，gap-register U-6）
// =============================================================================

/// 单个 compliance 高风险 / 收费动作的能力闸门视图。对应后端 G2 `CapabilityVO`。
///
/// 拿不到能力时调用方必须 fail-closed（视为 `executable=false`）。`state` 复用跨域
/// [`FeatureGateState`] 开放联合 —— 后端保留新增空间。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceCapability {
    /// 动作标识（如 `signEnvelope` / `publishReport`）。
    pub action: String,
    /// 该动作当前是否可执行。fail-closed：拿不到能力时按 false 处理。
    pub executable: bool,
    /// 不可执行的具体状态。
    pub state: FeatureGateState,
    /// 该动作所需的 OAuth scope。
    #[serde(rename = "requiredScopes")]
    pub required_scopes: Vec<String>,
    /// 该动作是否需要 step-up（高风险动作二次验证）。
    #[serde(rename = "requiredStepUp")]
    pub required_step_up: bool,
    /// 人类可读原因（诊断 / 展示）。
    pub reason: String,
}

// =============================================================================
// Operation Projection（操作投影，gap-register U-5）
// =============================================================================

/// 操作投影共享字段基座。`OperationPageItem` / `OperationDetail` 经 `#[serde(flatten)]`
/// 共享 —— **不做 type-alias 合并**（详情视图后端可追加字段而不破坏列表项契约）。
///
/// 🔴 同名字段分歧（方案 §3）：本基座 `seal_id` 是 `Option<i64>`，与 provider 域
/// `ProviderRequestStatusView.seal_id`（`Option<String>`）不同。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationBase {
    /// 行 id（数值主键）。
    pub id: i64,
    /// 操作幂等键（跨来源统一关联键）。
    #[serde(rename = "operationId")]
    pub operation_id: String,
    /// 操作状态。
    pub status: String,
    /// 是否终态。
    pub terminal: bool,
    /// 当前状态是否允许 SDK 安全重试。
    pub retryable: bool,
    /// 已尝试次数。
    #[serde(rename = "attemptCount")]
    pub attempt_count: i64,
    /// 关联业务编号。
    #[serde(
        rename = "businessNo",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub business_no: Option<String>,
    /// 关联合同编号。
    #[serde(
        rename = "contractNo",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contract_no: Option<String>,
    /// 关联印章 id。🔴 operation 域 `seal_id` 是 `Option<i64>`（与 provider 域 String 分歧）。
    #[serde(rename = "sealId", default, skip_serializing_if = "Option::is_none")]
    pub seal_id: Option<i64>,
    /// 对账状态。
    #[serde(
        rename = "reconciliationStatus",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub reconciliation_status: Option<String>,
    /// 下次重试时间 ISO-8601。
    #[serde(
        rename = "nextRetryAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub next_retry_at: Option<String>,
    /// 请求发起时间 ISO-8601。
    #[serde(
        rename = "requestedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub requested_at: Option<String>,
    /// provider 响应时间 ISO-8601。
    #[serde(
        rename = "respondedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub responded_at: Option<String>,
    /// 创建时间 ISO-8601。
    #[serde(rename = "createTime")]
    pub create_time: String,
}

/// compliance 操作投影【列表项】视图。对应后端 G2 `OperationPageItem`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationPageItem {
    #[serde(flatten)]
    pub base: OperationBase,
}

/// compliance 操作投影【详情】视图。对应后端 G2 `OperationDetail`。
///
/// 当前与 [`OperationPageItem`] 字段一致 —— 单独成类型（不 type-alias 合并）以便后端在详情
/// 视图追加字段时不破坏列表项契约。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationDetail {
    #[serde(flatten)]
    pub base: OperationBase,
}

/// `list_operations` 请求参数。
#[derive(Debug, Clone, Default)]
pub struct ListOperationsRequest {
    pub page: PageRequest,
    /// 操作状态过滤。
    pub status: Option<String>,
    pub create_time_start: Option<String>,
    pub create_time_end: Option<String>,
}
