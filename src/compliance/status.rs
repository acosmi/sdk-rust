//! 合规域稳定 API 契约（前端可见的最小集合）。端口自 `compliance/status.ts`。
//!
//! 设计原则：
//!   - SDK / 插件 / SaaS 不感知下游履约通道材料或内部流水字段。本文件只暴露面向用户的
//!     状态 / 错误码语义。
//!   - 本文件导出的错误码常量是 **SDK 内部 symbolic key**，不是 Java 服务端的 wire 错误码。
//!     Java 后端通过 ErrorCodeConstants 暴露的是 1-031-xxx-xxx 数值码；wire 上读到的是
//!     `{code, message}`。前端要做分类判断时用 `compliance::errors::classify_compliance_error`
//!     拿到 `ComplianceErrorInfo.key`，再按本文件的 symbolic 常量 match。
//!   - 业务入口在后端层关闭时，前端拿到 `ENVELOPE_GATE_CLOSED` / `PROVIDER_NOT_CONFIGURED`
//!     必须展示"功能未开放"，不得自行重试或绕过。

use crate::macros::open_string_union;

open_string_union! {
    /// Compliance envelope 稳定业务状态。字面量与 Java `EnvelopeStatusEnum.name()` 一致。
    ComplianceEnvelopeStatus {
        DRAFT => "DRAFT",
        CONTRACT_READY => "CONTRACT_READY",
        APPROVED => "APPROVED",
        SIGN_PENDING => "SIGN_PENDING",
        /// provider 报告成功 / callback 已到，但本地证据链尚未就绪 —— 不可承诺扣费。
        PENDING_EVIDENCE => "PENDING_EVIDENCE",
        /// provider unknown/retrying / 本地 verify 失败 —— 进入人工对账。
        PENDING_RECONCILIATION => "PENDING_RECONCILIATION",
        SUCCESS => "SUCCESS",
        FAILED => "FAILED",
        CANCELED => "CANCELED",
    }
}

open_string_union! {
    /// Compliance 用印审批稳定状态。
    ComplianceSealApprovalStatus {
        DRAFT => "DRAFT",
        SUBMITTED => "SUBMITTED",
        APPROVED => "APPROVED",
        REJECTED => "REJECTED",
        CANCELED => "CANCELED",
        USED => "USED",
        EXPIRED => "EXPIRED",
    }
}

open_string_union! {
    /// Provider 业务状态（前端可见的脱敏视图，不暴露下游内部字段）。
    ComplianceProviderStatus {
        PENDING => "pending",
        SUCCESS => "success",
        FAILED => "failed",
        /// 不可承诺扣费；客户必须等查询/对账完成。
        UNKNOWN => "unknown",
        /// 不可承诺扣费。
        RETRYING => "retrying",
    }
}

open_string_union! {
    /// 计费 / 履约的对外稳定状态视图。distribution compliance-billing 内部 API
    /// (quote/reserve/commit/cancel/reconcile) 不暴露给前端 —— 前端只能看见这 4 个语义。
    ComplianceBillingDisplayStatus {
        /// 已预占，待 provider 成功 + 本地验证。
        RESERVED => "reserved",
        /// 已确认扣费。
        COMMITTED => "committed",
        /// 已取消。
        CANCELED => "canceled",
        /// 等待人工对账，扣费状态未确认。
        PENDING_RECONCILIATION => "pending_reconciliation",
    }
}

// ===========================================================================
// 错误码（字面量与 Java ErrorCodeConstants 一致）。SDK 内部 symbolic key，非 wire 字段。
// ===========================================================================

/// 高风险动作 step-up —— 重新做 OAuth introspection 或重新登录，提升 token 等级后再尝试。
pub const ERR_COMPLIANCE_STEP_UP_REQUIRED: &str = "COMPLIANCE_STEP_UP_REQUIRED";
/// envelope 闸门关闭 —— 审批 gate 等条件未同时就绪，前端展示"功能开放中"。
pub const ERR_ENVELOPE_GATE_CLOSED: &str = "ENVELOPE_GATE_CLOSED";
/// 履约 provider 未配置 —— 受控环境材料缺失时业务路径会一致拒绝，不要前端重试。
pub const ERR_PROVIDER_NOT_CONFIGURED: &str = "PROVIDER_NOT_CONFIGURED";
/// Provider 返回 unknown / retrying —— 客户必须等待对账，前端展示"处理中（人工核对）"。
pub const ERR_PROVIDER_UNKNOWN_NO_RETRY: &str = "PROVIDER_REQUEST_UNKNOWN_NO_RETRY";
/// callback 不能直接确认扣费 —— 后端守门，前端无需处理，记录到审计日志展示即可。
pub const ERR_BILLING_CALLBACK_CANNOT_COMMIT: &str = "BILLING_CALLBACK_CANNOT_COMMIT";
/// provider 成功但本地未验证通过，不允许扣费。
pub const ERR_BILLING_COMMIT_REQUIRES_LOCAL_VERIFY: &str = "BILLING_COMMIT_REQUIRES_LOCAL_VERIFY";
/// distribution compliance-billing 必须 S2S —— 前端误调用时拿到该错误码，禁止 retry。
pub const ERR_BILLING_S2S_FORBIDDEN: &str = "BILLING_S2S_FORBIDDEN";

/// 审批 gate 各类失败语义（前端展示"审批中 / 审批已用 / 审批失效"）。
pub const ERR_SEAL_APPROVAL_NOT_APPROVED: &str = "SEAL_APPROVAL_STATE_NOT_APPROVED";
pub const ERR_SEAL_APPROVAL_EXPIRED: &str = "SEAL_APPROVAL_EXPIRED";
pub const ERR_SEAL_APPROVAL_NONCE_USED: &str = "SEAL_APPROVAL_NONCE_USED";
pub const ERR_SEAL_APPROVAL_CONTRACT_HASH_MISMATCH: &str = "SEAL_APPROVAL_CONTRACT_HASH_MISMATCH";
pub const ERR_SEAL_APPROVAL_SEAL_MISMATCH: &str = "SEAL_APPROVAL_SEAL_MISMATCH";
pub const ERR_SEAL_APPROVAL_LOCATION_MISMATCH: &str = "SEAL_APPROVAL_LOCATION_MISMATCH";
pub const ERR_SEAL_APPROVAL_TRANSACTOR_MISMATCH: &str = "SEAL_APPROVAL_TRANSACTOR_MISMATCH";

/// 用印重复消费防护 —— 前端遇到该错误必须刷新审批单状态，不允许"换一次 nonce 再试"。
pub const ERR_SEAL_USE_ALREADY_CONSUMED: &str = "SEAL_USE_ALREADY_CONSUMED";

/// 判定一个错误码是否属于"需要前端引导用户重新认证/审批"的高级语义，而不是简单 retry。
/// 这类错误前端不允许在 UI 层做自动重试。对应 TS `isComplianceTerminalError`。
pub fn is_compliance_terminal_error(code: &str) -> bool {
    matches!(
        code,
        ERR_ENVELOPE_GATE_CLOSED
            | ERR_PROVIDER_NOT_CONFIGURED
            | ERR_PROVIDER_UNKNOWN_NO_RETRY
            | ERR_BILLING_CALLBACK_CANNOT_COMMIT
            | ERR_BILLING_COMMIT_REQUIRES_LOCAL_VERIFY
            | ERR_BILLING_S2S_FORBIDDEN
            | ERR_SEAL_APPROVAL_NONCE_USED
            | ERR_SEAL_APPROVAL_EXPIRED
            | ERR_SEAL_APPROVAL_CONTRACT_HASH_MISMATCH
            | ERR_SEAL_APPROVAL_SEAL_MISMATCH
            | ERR_SEAL_APPROVAL_LOCATION_MISMATCH
            | ERR_SEAL_APPROVAL_TRANSACTOR_MISMATCH
            | ERR_SEAL_USE_ALREADY_CONSUMED
    )
}

/// "是否允许在 UI 上呈现为'扣费已确认'"的判定。provider 状态为 unknown / retrying / pending
/// 时一律不展示为已确认。对应 TS `isBillingConfirmable`。
pub fn is_billing_confirmable(
    provider_status: Option<&ComplianceProviderStatus>,
    billing_status: Option<&ComplianceBillingDisplayStatus>,
) -> bool {
    provider_status.map(|s| s.as_str()) == Some(ComplianceProviderStatus::SUCCESS)
        && billing_status.map(|s| s.as_str()) == Some(ComplianceBillingDisplayStatus::COMMITTED)
}
