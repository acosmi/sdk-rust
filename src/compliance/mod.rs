//! Compliance 域（电子证据 / 时间章 / 报告 / 签署 envelope / 用印审批 / 模板）。
//!
//! 对齐 `compliance/index.ts`。子客户端经 [`Client::compliance`](crate::core::Client::compliance)
//! getter 获取（独立 base URL `{server_url}/admin-api`，不复用 `/api/v4`）。
//!
//! 错误体系分层（方案 §4.5）在 [`errors`]（`ComplianceErrorKey` enum + serde rename，便于
//! retry_advice 编译期穷举映射）。8 子域类型逐个端口（evidence / timestamp / report / signing /
//! seal_approval / provider / operation / template）。
//!
//! provider 子域反向 `pub use shared::operation::ProviderRequestStatus as
//! ComplianceProviderRequestStatus`（定义点在 shared，见执行日志相位 #2）。

pub mod client;
pub mod errors;
pub mod scopes;
pub mod status;
pub mod types;

pub mod evidence;
pub mod operation;
pub mod provider;
pub mod report;
pub mod seal_approval;
pub mod signing;
pub mod template;
pub mod timestamp;

// === 子客户端 + poll error ===
pub use client::{ComplianceClient, CompliancePollError, CompliancePollErrorKind};

// === 错误码分类器 ===
pub use errors::{
    classify_compliance_error, is_compliance_business_error, ComplianceErrorInfo,
    ComplianceErrorKey,
};

// === scopes ===
pub use scopes::{
    compliance_scopes, SCOPE_COMPLIANCE_CONTRACT_SIGNING_READ,
    SCOPE_COMPLIANCE_CONTRACT_SIGNING_WRITE, SCOPE_COMPLIANCE_CONTRACT_TEMPLATE_READ,
    SCOPE_COMPLIANCE_CONTRACT_TEMPLATE_WRITE, SCOPE_COMPLIANCE_EVIDENCE_READ,
    SCOPE_COMPLIANCE_EVIDENCE_WRITE, SCOPE_COMPLIANCE_REPORTS_PUBLISH,
    SCOPE_COMPLIANCE_REPORTS_READ, SCOPE_COMPLIANCE_REPORTS_WRITE,
    SCOPE_COMPLIANCE_SEAL_APPROVAL_APPROVE, SCOPE_COMPLIANCE_SEAL_APPROVAL_REQUEST,
    SCOPE_COMPLIANCE_SEAL_MANAGE, SCOPE_COMPLIANCE_SEAL_USE_EXECUTE,
    SCOPE_COMPLIANCE_TIMESTAMP_ISSUE, SCOPE_COMPLIANCE_TIMESTAMP_VERIFY,
};

// === 状态与领域类型 ===
pub use status::{
    is_billing_confirmable, is_compliance_terminal_error, ComplianceBillingDisplayStatus,
    ComplianceEnvelopeStatus, ComplianceProviderStatus, ComplianceSealApprovalStatus,
    ERR_BILLING_CALLBACK_CANNOT_COMMIT, ERR_BILLING_COMMIT_REQUIRES_LOCAL_VERIFY,
    ERR_BILLING_S2S_FORBIDDEN, ERR_COMPLIANCE_STEP_UP_REQUIRED, ERR_ENVELOPE_GATE_CLOSED,
    ERR_PROVIDER_NOT_CONFIGURED, ERR_PROVIDER_UNKNOWN_NO_RETRY,
    ERR_SEAL_APPROVAL_CONTRACT_HASH_MISMATCH, ERR_SEAL_APPROVAL_EXPIRED,
    ERR_SEAL_APPROVAL_LOCATION_MISMATCH, ERR_SEAL_APPROVAL_NONCE_USED,
    ERR_SEAL_APPROVAL_NOT_APPROVED, ERR_SEAL_APPROVAL_SEAL_MISMATCH,
    ERR_SEAL_APPROVAL_TRANSACTOR_MISMATCH, ERR_SEAL_USE_ALREADY_CONSUMED,
};
pub use types::{CompliancePollOptions, ComplianceWriteOptions};

// === 8 子域类型 re-export（对齐 compliance/index.ts 逐子域 export *）===
pub use evidence::{
    ComplianceAssetType, ComplianceDigestSource, ComplianceHashAlgorithm, CompliancePrivacyLevel,
    CreateEvidenceAssetRequest, EvidenceAsset, EvidenceAssetPageItem, EvidencePackage,
    EvidencePackagePageItem, ListEvidenceAssetsRequest, ListEvidencePackagesRequest,
    PublicEvidenceVerifyResult,
};
pub use operation::{
    ComplianceCapability, ListOperationsRequest, OperationBase, OperationDetail, OperationPageItem,
};
pub use provider::{ComplianceProviderRequestStatus, ProviderRequestStatusView};
pub use report::{
    ComplianceReport, CreateReportRequest, ListReportsRequest, ReportDownload, ReportPageItem,
};
pub use seal_approval::{
    ApproveSealApprovalQuery, CancelSealApprovalQuery, ListSealApprovalsRequest,
    ListSealUsesRequest, RejectSealApprovalQuery, SealApproval, SealApprovalPageItem,
    SealUsePageItem, SubmitSealApprovalRequest,
};
pub use signing::{
    CreateH5SigningUrlRequest, CreateSigningEnvelopeRequest, EnvelopeContractItem,
    ListSigningEnvelopesRequest, SignEnvelopeRequest, SigningEnvelope, SigningEnvelopePageItem,
    VoidEnvelopeRequest,
};
pub use template::{
    ContractTemplateField, ContractTemplateFieldType, ContractTemplatePageItem,
    ContractTemplateResp, ContractTemplateStatus, ContractTemplateVersion,
    CreateContractTemplateRequest, ListContractTemplatesRequest, UpdateContractTemplateRequest,
    UploadContractTemplatePdfRequest,
};
pub use timestamp::{
    ComplianceTimestampVerificationStatus, IssueTimestampRequest, ListTimestampsRequest,
    TimestampPageItem, TimestampToken, TimestampVerifyResult, TsaProvider, TsaStats,
    VerifyTimestampRequest,
};
