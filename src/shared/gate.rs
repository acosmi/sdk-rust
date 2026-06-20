//! 跨域统一 gate / capability / step-up / preflight 原语。端口自 `shared/gate.ts`。
//!
//! 只沉淀共享形态原语；真实 gate / preflight 查询命名空间待后端契约就绪。

use super::operation::OperationId;
use super::retry_advice::RetryAdvice;
use crate::macros::open_string_union;
use serde::{Deserialize, Serialize};

open_string_union! {
    /// 高风险 / 收费动作的 gate 状态机。开放联合，后端保留新增空间。
    /// 拿不到能力时 fail-closed 返回 `unknown`。
    FeatureGateState {
        EXECUTABLE => "executable",
        SCOPE_MISSING => "scope_missing",
        NOT_PROVISIONED => "not_provisioned",
        QUOTA_EXCEEDED => "quota_exceeded",
        STEP_UP_REQUIRED => "step_up_required",
        GATE_CLOSED => "gate_closed",
        UNKNOWN => "unknown",
    }
}

/// 配额快照。`FeatureGateStatus.quota` 与 `BillingPreflightResult` 复用。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GateQuota {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<i64>,
    /// 配额单位（如 `etu` / `count`）；后端可省略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// step-up（高风险动作二次验证）状态。字段形态先行沉淀。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepUpStatus {
    pub required: bool,
    pub satisfied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// 满足态过期时间（ISO 8601）；未满足时省略。
    #[serde(rename = "expiresAt", default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// gate / capability 查询结果。fail-closed：拿不到时 `executable=false`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureGateStatus {
    pub executable: bool,
    pub state: FeatureGateState,
    #[serde(
        rename = "requiredScopes",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub required_scopes: Option<Vec<String>>,
    #[serde(
        rename = "requiredStepUp",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub required_step_up: Option<bool>,
    #[serde(
        rename = "missingEntitlements",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub missing_entitlements: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota: Option<GateQuota>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(
        rename = "retryAdvice",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub retry_advice: Option<RetryAdvice>,
    #[serde(
        rename = "operationId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub operation_id: Option<OperationId>,
}

/// 收费 / 高风险动作的 billing preflight 结果。
///
/// 金额字段 `estimated_charge` 为 `String`（json.Number 端口，金融精度红线，见方案 §3 阵营 c）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingPreflightResult {
    pub executable: bool,
    /// 预估扣费金额；`String` 表示，避免浮点精度丢失。
    #[serde(
        rename = "estimatedCharge",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub estimated_charge: Option<String>,
    /// 计费单位 / 币种（如 `etu` / `CNY`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(
        rename = "requiredScopes",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub required_scopes: Option<Vec<String>>,
    #[serde(
        rename = "requiredStepUp",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub required_step_up: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota: Option<GateQuota>,
    #[serde(
        rename = "retryAdvice",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub retry_advice: Option<RetryAdvice>,
    #[serde(
        rename = "preflightId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub preflight_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
