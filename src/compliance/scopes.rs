//! 合规履约 scope。端口自 `compliance/scopes.ts`（源自 acosmi-sdk-go scopes.go v0.19.0）。
//!
//! 三处必须字面量一致，任一变更需同步另外两处:
//!   - Go:  nexus-v4/backend/internal/handler/desktop_oauth.go DesktopOAuthScopes
//!   - Java: tk-dist/yudao-module-compliance-api ComplianceScopes
//!   - Rust: 本文件 SCOPE_COMPLIANCE_* 常量
//!
//! 调用 compliance API 时必须按这些常量声明所需 scope；不允许通过 ScopeAI /
//! ScopeAccount 隐式获得 compliance 权限。合规 scope 不做分组合并 —— 服务端按细粒度
//! 校验，SDK 也只应按需申请最小集合。

pub const SCOPE_COMPLIANCE_EVIDENCE_READ: &str = "compliance:evidence:read";
pub const SCOPE_COMPLIANCE_EVIDENCE_WRITE: &str = "compliance:evidence:write";

pub const SCOPE_COMPLIANCE_TIMESTAMP_ISSUE: &str = "compliance:timestamp:issue";
pub const SCOPE_COMPLIANCE_TIMESTAMP_VERIFY: &str = "compliance:timestamp:verify";

pub const SCOPE_COMPLIANCE_CONTRACT_SIGNING_READ: &str = "compliance:contract_signing:read";
pub const SCOPE_COMPLIANCE_CONTRACT_SIGNING_WRITE: &str = "compliance:contract_signing:write";

pub const SCOPE_COMPLIANCE_SEAL_MANAGE: &str = "compliance:seal:manage";
pub const SCOPE_COMPLIANCE_SEAL_APPROVAL_REQUEST: &str = "compliance:seal_approval:request";
pub const SCOPE_COMPLIANCE_SEAL_APPROVAL_APPROVE: &str = "compliance:seal_approval:approve";
pub const SCOPE_COMPLIANCE_SEAL_USE_EXECUTE: &str = "compliance:seal_use:execute";

pub const SCOPE_COMPLIANCE_REPORTS_READ: &str = "compliance:reports:read";
pub const SCOPE_COMPLIANCE_REPORTS_WRITE: &str = "compliance:reports:write";
pub const SCOPE_COMPLIANCE_REPORTS_PUBLISH: &str = "compliance:reports:publish";

pub const SCOPE_COMPLIANCE_CONTRACT_TEMPLATE_READ: &str = "compliance:contract_template:read";
pub const SCOPE_COMPLIANCE_CONTRACT_TEMPLATE_WRITE: &str = "compliance:contract_template:write";

/// 全部合规域 scope。OAuth 申请合规权限时使用；谨慎一次性申请全部，推荐按业务最小集合申请。
/// 对应 TS `complianceScopes()`。
pub fn compliance_scopes() -> Vec<&'static str> {
    vec![
        SCOPE_COMPLIANCE_EVIDENCE_READ,
        SCOPE_COMPLIANCE_EVIDENCE_WRITE,
        SCOPE_COMPLIANCE_TIMESTAMP_ISSUE,
        SCOPE_COMPLIANCE_TIMESTAMP_VERIFY,
        SCOPE_COMPLIANCE_CONTRACT_SIGNING_READ,
        SCOPE_COMPLIANCE_CONTRACT_SIGNING_WRITE,
        SCOPE_COMPLIANCE_SEAL_MANAGE,
        SCOPE_COMPLIANCE_SEAL_APPROVAL_REQUEST,
        SCOPE_COMPLIANCE_SEAL_APPROVAL_APPROVE,
        SCOPE_COMPLIANCE_SEAL_USE_EXECUTE,
        SCOPE_COMPLIANCE_REPORTS_READ,
        SCOPE_COMPLIANCE_REPORTS_WRITE,
        SCOPE_COMPLIANCE_REPORTS_PUBLISH,
        SCOPE_COMPLIANCE_CONTRACT_TEMPLATE_READ,
        SCOPE_COMPLIANCE_CONTRACT_TEMPLATE_WRITE,
    ]
}
