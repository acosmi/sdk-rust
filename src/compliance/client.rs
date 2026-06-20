//! SDK-facing Compliance 域子客户端。端口自 `compliance/client.ts`
//! （getter 子客户端 + 独立 base URL `{server_url}/admin-api`，不复用 `/api/v4` 路由）。
//!
//! 设计原则（严格）：
//!   - 走独立 base URL（[`Client::compliance_url`]），不复用 `/api/v4` 路径。
//!   - GET 认证读走 retry_on_401=true（与 agent-runs 同节奏，幂等安全）。
//!   - 公开 verify 走 public_read：无 token 匿名请求，有 token 附带 Authorization 以保留审计
//!     上下文；public 端点不应要求认证 → 401 不触发 force_refresh，也不做 refresh replay。
//!   - POST 写操作走专属 helper：发送前 ensure_token 一次确保 token fresh；遇到 401 直接抛
//!     HTTPError，不自动重放 —— compliance 写接口可能在 provider 侧落幂等写入，SDK 自动重放
//!     会导致 provider 重复请求 / 重复扣费风险。
//!   - 写操作支持 Idempotency-Key header；调用方应持久化 key，重试 / 恢复时复用。
//!   - SDK 不传 provider 字段；服务端按配置选择 provider。

use crate::compliance::errors::{
    classify_compliance_error, is_compliance_business_error, ComplianceErrorInfo,
    ComplianceErrorKey,
};
use crate::compliance::evidence::{
    CreateEvidenceAssetRequest, EvidenceAsset, EvidenceAssetPageItem, EvidencePackage,
    EvidencePackagePageItem, ListEvidenceAssetsRequest, ListEvidencePackagesRequest,
    PublicEvidenceVerifyResult,
};
use crate::compliance::operation::{
    ComplianceCapability, ListOperationsRequest, OperationDetail, OperationPageItem,
};
use crate::compliance::provider::{ComplianceProviderRequestStatus, ProviderRequestStatusView};
use crate::compliance::report::{
    ComplianceReport, CreateReportRequest, ListReportsRequest, ReportDownload, ReportPageItem,
};
use crate::compliance::seal_approval::{
    ApproveSealApprovalQuery, CancelSealApprovalQuery, ListSealApprovalsRequest,
    ListSealUsesRequest, RejectSealApprovalQuery, SealApproval, SealApprovalPageItem,
    SealUsePageItem, SubmitSealApprovalRequest,
};
use crate::compliance::signing::{
    CreateH5SigningUrlRequest, CreateSigningEnvelopeRequest, EnvelopeContractItem,
    ListSigningEnvelopesRequest, SignEnvelopeRequest, SigningEnvelope, SigningEnvelopePageItem,
    VoidEnvelopeRequest,
};
use crate::compliance::template::{
    ContractTemplatePageItem, ContractTemplateResp, ContractTemplateVersion,
    CreateContractTemplateRequest, ListContractTemplatesRequest, UpdateContractTemplateRequest,
    UploadContractTemplatePdfRequest,
};
use crate::compliance::timestamp::{
    ComplianceTimestampVerificationStatus, IssueTimestampRequest, ListTimestampsRequest,
    TimestampPageItem, TimestampToken, TimestampVerifyResult, TsaProvider, TsaStats,
    VerifyTimestampRequest,
};
use crate::compliance::types::{CompliancePollOptions, ComplianceWriteOptions};
use crate::core::client::Client;
use crate::core::http::{
    parse_http_error_with_retry_after, read_limited_text, DEFAULT_JSON_TIMEOUT_MS,
    MAX_ERROR_BODY_SIZE,
};
use crate::shared::pagination::PageRequest;
use crate::shared::{ApiResponse, Error, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// compliance 业务方法默认轮询参数。
const DEFAULT_POLL_TIMEOUT_MS: u64 = 60_000;
const DEFAULT_POLL_INITIAL_INTERVAL_MS: u64 = 1_000;
const DEFAULT_POLL_MAX_INTERVAL_MS: u64 = 5_000;
const DEFAULT_POLL_MULTIPLIER: f64 = 1.5;

/// CompliancePollError 分类。对应 TS union `'timeout' | 'terminal_failure' | 'step_up_required' | 'unknown'`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompliancePollErrorKind {
    Timeout,
    TerminalFailure,
    StepUpRequired,
    Unknown,
}

/// 当 compliance polling 检测到终态失败 / 已过 timeout / step-up 时返回。对应 TS
/// `CompliancePollError`。
///
/// 分类逻辑：终态失败优先于 timeout；timeout 不算 retryable，调用方必须用新 idempotency-key
/// 重新发起整条链路。
#[derive(Debug, Clone)]
pub struct CompliancePollError {
    pub message: String,
    pub kind: CompliancePollErrorKind,
    pub last_info: Option<ComplianceErrorInfo>,
}

impl std::fmt::Display for CompliancePollError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CompliancePollError {}

impl CompliancePollError {
    fn new(
        message: impl Into<String>,
        kind: CompliancePollErrorKind,
        last_info: Option<ComplianceErrorInfo>,
    ) -> Self {
        Self {
            message: message.into(),
            kind,
            last_info,
        }
    }
}

/// poll 一次的分类决策。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PollDecision {
    Continue,
    Done,
    Failed,
}

/// SDK-facing compliance 子客户端。对应 TS `ComplianceClient`。
///
/// 经 [`Client::compliance`] getter 获取（无状态，持 [`Client`] clone）。
pub struct ComplianceClient {
    client: Client,
}

impl ComplianceClient {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    // =========================================================================
    // Evidence Asset
    // =========================================================================

    /// 创建证据资产（写）。对应 TS `createEvidenceAsset`。
    pub async fn create_evidence_asset(
        &self,
        req: &CreateEvidenceAssetRequest,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<EvidenceAsset> {
        self.write(
            "POST",
            "/compliance/evidence/assets",
            Some(req),
            opts,
            signal,
        )
        .await
    }

    /// 读 —— 证据资产详情。对应 TS `getEvidenceAsset`。
    pub async fn get_evidence_asset(
        &self,
        id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<EvidenceAsset> {
        self.read(&format!("/compliance/evidence/assets/{id}"), None, signal)
            .await
    }

    /// 读 —— 证据资产分页列表（S1）。对应 TS `listEvidenceAssets`。
    pub async fn list_evidence_assets(
        &self,
        req: &ListEvidenceAssetsRequest,
        signal: Option<CancellationToken>,
    ) -> Result<crate::shared::pagination::PageResult<EvidenceAssetPageItem>> {
        let mut q = QueryBuilder::new(&req.page);
        q.add("assetType", req.asset_type.as_deref());
        q.add("status", req.status.as_deref());
        q.add("createTimeStart", req.create_time_start.as_deref());
        q.add("createTimeEnd", req.create_time_end.as_deref());
        let path = format!("/compliance/evidence/assets/page{}", q.finish());
        self.read(&path, None, signal).await
    }

    /// 公开 verify。匿名可调用：未 login 时走匿名请求。已 login / 已持有 token 时附带
    /// `Authorization` 以保留审计上下文。对应 TS `verifyEvidencePublic`。
    pub async fn verify_evidence_public(
        &self,
        evidence_no: Option<&str>,
        public_verify_code: Option<&str>,
        signal: Option<CancellationToken>,
    ) -> Result<PublicEvidenceVerifyResult> {
        let mut parts: Vec<String> = Vec::new();
        if let Some(v) = evidence_no.filter(|s| !s.is_empty()) {
            parts.push(format!("evidenceNo={}", urlencode(v)));
        }
        if let Some(v) = public_verify_code.filter(|s| !s.is_empty()) {
            parts.push(format!("publicVerifyCode={}", urlencode(v)));
        }
        let qs = if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        };
        self.public_read(&format!("/compliance/evidence/verify{qs}"), signal)
            .await
    }

    // =========================================================================
    // Timestamp
    // =========================================================================

    /// 申请时间章（写）。SDK 永远不传 provider 字段。对应 TS `issueTimestamp`。
    pub async fn issue_timestamp(
        &self,
        req: &IssueTimestampRequest,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<TimestampToken> {
        self.write("POST", "/compliance/timestamps", Some(req), opts, signal)
            .await
    }

    /// 给已有资产申请时间章（写）。对应 TS `issueTimestampForAsset`。
    pub async fn issue_timestamp_for_asset(
        &self,
        asset_id: i64,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<TimestampToken> {
        self.write::<(), _>(
            "POST",
            &format!("/compliance/evidence/assets/{asset_id}/timestamp"),
            None,
            opts,
            signal,
        )
        .await
    }

    /// 读 —— 时间章 token 详情。对应 TS `getTimestamp`。
    pub async fn get_timestamp(
        &self,
        id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<TimestampToken> {
        self.read(&format!("/compliance/timestamps/{id}"), None, signal)
            .await
    }

    /// 读 —— 时间章分页列表（S1）。对应 TS `listTimestamps`。
    pub async fn list_timestamps(
        &self,
        req: &ListTimestampsRequest,
        signal: Option<CancellationToken>,
    ) -> Result<crate::shared::pagination::PageResult<TimestampPageItem>> {
        let mut q = QueryBuilder::new(&req.page);
        q.add("provider", req.provider.as_deref());
        q.add("verificationStatus", req.verification_status.as_deref());
        q.add("createTimeStart", req.create_time_start.as_deref());
        q.add("createTimeEnd", req.create_time_end.as_deref());
        let path = format!("/compliance/timestamps/page{}", q.finish());
        self.read(&path, None, signal).await
    }

    /// verify —— 本地离线校验已申请的时间章（写）。对应 TS `verifyTimestamp`。
    pub async fn verify_timestamp(
        &self,
        req: &VerifyTimestampRequest,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<TimestampVerifyResult> {
        self.write(
            "POST",
            "/compliance/timestamps/verify",
            Some(req),
            opts,
            signal,
        )
        .await
    }

    /// 轮询到 VERIFIED 终态。对应 TS `waitForTimestampVerified`。
    pub async fn wait_for_timestamp_verified(
        &self,
        id: i64,
        opts: &CompliancePollOptions,
        signal: Option<CancellationToken>,
    ) -> std::result::Result<TimestampToken, CompliancePollError> {
        self.poll(
            || self.get_timestamp(id, signal.clone()),
            |t: &TimestampToken| classify_timestamp(&t.verification_status),
            opts,
            signal.clone(),
            derive_last_info_timestamp,
        )
        .await
    }

    // =========================================================================
    // TSA readonly views (S3 / gap-register U-7)
    // =========================================================================

    /// 读 —— 时间章授权机构（TSA）provider 列表（S3）。对应 TS `listTsaProviders`。
    pub async fn list_tsa_providers(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<TsaProvider>> {
        self.read("/compliance/timestamps/providers", None, signal)
            .await
    }

    /// 读 —— 时间章统计视图（S3）。对应 TS `getTsaStats`。
    pub async fn get_tsa_stats(&self, signal: Option<CancellationToken>) -> Result<TsaStats> {
        self.read("/compliance/timestamps/stats", None, signal)
            .await
    }

    // =========================================================================
    // Evidence Package
    // =========================================================================

    /// 构建证据包（写）。对应 TS `buildEvidencePackage`。
    pub async fn build_evidence_package(
        &self,
        asset_id: i64,
        timestamp_token_id: Option<i64>,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<EvidencePackage> {
        let ts_param = match timestamp_token_id {
            Some(id) => format!("?timestampTokenId={id}"),
            None => String::new(),
        };
        self.write::<(), _>(
            "POST",
            &format!("/compliance/evidence/assets/{asset_id}/packages{ts_param}"),
            None,
            opts,
            signal,
        )
        .await
    }

    /// 读 —— 证据包分页列表（S1）。对应 TS `listEvidencePackages`。
    pub async fn list_evidence_packages(
        &self,
        req: &ListEvidencePackagesRequest,
        signal: Option<CancellationToken>,
    ) -> Result<crate::shared::pagination::PageResult<EvidencePackagePageItem>> {
        let mut q = QueryBuilder::new(&req.page);
        q.add("status", req.status.as_deref());
        q.add("createTimeStart", req.create_time_start.as_deref());
        q.add("createTimeEnd", req.create_time_end.as_deref());
        let path = format!("/compliance/evidence/packages/page{}", q.finish());
        self.read(&path, None, signal).await
    }

    // =========================================================================
    // Report
    // =========================================================================

    /// 创建证据报告（写）。对应 TS `createReport`。
    pub async fn create_report(
        &self,
        req: &CreateReportRequest,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<ComplianceReport> {
        self.write("POST", "/compliance/reports", Some(req), opts, signal)
            .await
    }

    /// 读 —— 报告详情。对应 TS `getReport`。
    pub async fn get_report(
        &self,
        id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<ComplianceReport> {
        self.read(&format!("/compliance/reports/{id}"), None, signal)
            .await
    }

    /// 读 —— 证据报告分页列表（S1）。对应 TS `listReports`。
    pub async fn list_reports(
        &self,
        req: &ListReportsRequest,
        signal: Option<CancellationToken>,
    ) -> Result<crate::shared::pagination::PageResult<ReportPageItem>> {
        let mut q = QueryBuilder::new(&req.page);
        q.add("status", req.status.as_deref());
        q.add("createTimeStart", req.create_time_start.as_deref());
        q.add("createTimeEnd", req.create_time_end.as_deref());
        let path = format!("/compliance/reports/page{}", q.finish());
        self.read(&path, None, signal).await
    }

    /// 发布报告（写，step-up 必须）。对应 TS `publishReport`。
    ///
    /// `@status gated` —— step-up 闸门未闭合前服务端会一致返回 `COMPLIANCE_STEP_UP_REQUIRED`。
    /// SDK 不会自动重试、不伪成功；调用方需引导用户重新做 OAuth introspection 或重新登录后再次
    /// 调用本方法（使用同一 idempotency-key）。
    pub async fn publish_report(
        &self,
        id: i64,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<ComplianceReport> {
        self.write::<(), _>(
            "POST",
            &format!("/compliance/reports/{id}/publish"),
            None,
            opts,
            signal,
        )
        .await
    }

    /// 下载报告（读）。对应 TS `downloadReport`。
    pub async fn download_report(
        &self,
        id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<ReportDownload> {
        self.read(&format!("/compliance/reports/{id}/download"), None, signal)
            .await
    }

    // =========================================================================
    // Signing Envelope
    // =========================================================================

    /// 创建 envelope（写）。返回 envelope id。对应 TS `createSigningEnvelope`。
    pub async fn create_signing_envelope(
        &self,
        req: &CreateSigningEnvelopeRequest,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<i64> {
        self.write(
            "POST",
            "/compliance/signing-envelopes",
            Some(req),
            opts,
            signal,
        )
        .await
    }

    /// 读 —— envelope 详情。对应 TS `getSigningEnvelope`。
    pub async fn get_signing_envelope(
        &self,
        envelope_id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<SigningEnvelope> {
        self.read(
            &format!("/compliance/signing-envelopes/{envelope_id}"),
            None,
            signal,
        )
        .await
    }

    /// 读 —— 签署 envelope 分页列表（S1）。对应 TS `listSigningEnvelopes`。
    pub async fn list_signing_envelopes(
        &self,
        req: &ListSigningEnvelopesRequest,
        signal: Option<CancellationToken>,
    ) -> Result<crate::shared::pagination::PageResult<SigningEnvelopePageItem>> {
        let mut q = QueryBuilder::new(&req.page);
        q.add("status", req.status.as_deref());
        q.add("createTimeStart", req.create_time_start.as_deref());
        q.add("createTimeEnd", req.create_time_end.as_deref());
        let path = format!("/compliance/signing-envelopes/page{}", q.finish());
        self.read(&path, None, signal).await
    }

    /// 正式签署（写，step-up 必须）。对应 TS `signEnvelope`。空体写接口返回 `()`。
    pub async fn sign_envelope(
        &self,
        envelope_id: i64,
        req: &SignEnvelopeRequest,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        self.write_unit(
            "POST",
            &format!("/compliance/signing-envelopes/{envelope_id}/sign"),
            Some(req),
            opts,
            signal,
        )
        .await
    }

    /// 创建 H5 签署短链（写，step-up 必须）。对应 TS `createH5SigningUrl`。
    pub async fn create_h5_signing_url(
        &self,
        envelope_id: i64,
        req: &CreateH5SigningUrlRequest,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<String> {
        self.write(
            "POST",
            &format!("/compliance/signing-envelopes/{envelope_id}/h5-url"),
            Some(req),
            opts,
            signal,
        )
        .await
    }

    /// 同步 provider 状态（写但只读对账）。对应 TS `syncSigningEnvelopeStatus`。
    pub async fn sync_signing_envelope_status(
        &self,
        envelope_id: i64,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        self.write_unit::<()>(
            "POST",
            &format!("/compliance/signing-envelopes/{envelope_id}/sync-provider-status"),
            None,
            opts,
            signal,
        )
        .await
    }

    /// 读 —— envelope 下挂的合同列表（S4）。返回普通数组（非 PageResult）。对应 TS
    /// `listEnvelopeContracts`。
    pub async fn list_envelope_contracts(
        &self,
        envelope_id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<EnvelopeContractItem>> {
        self.read(
            &format!("/compliance/signing-envelopes/{envelope_id}/contracts"),
            None,
            signal,
        )
        .await
    }

    /// 读 —— envelope 关联的 provider 请求列表（S4，复用 OperationPageItem）。对应 TS
    /// `listEnvelopeProviderRequests`。
    pub async fn list_envelope_provider_requests(
        &self,
        envelope_id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<OperationPageItem>> {
        self.read(
            &format!("/compliance/signing-envelopes/{envelope_id}/provider-requests"),
            None,
            signal,
        )
        .await
    }

    /// 作废 envelope（写，S4）。对应 TS `voidEnvelope`。
    pub async fn void_envelope(
        &self,
        envelope_id: i64,
        req: &VoidEnvelopeRequest,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<bool> {
        self.write(
            "POST",
            &format!("/compliance/signing-envelopes/{envelope_id}/void"),
            Some(req),
            opts,
            signal,
        )
        .await
    }

    // =========================================================================
    // Seal Approval
    // =========================================================================

    /// 提交用印审批申请（写）。返回审批 id。对应 TS `submitSealApproval`。
    pub async fn submit_seal_approval(
        &self,
        req: &SubmitSealApprovalRequest,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<i64> {
        self.write(
            "POST",
            "/compliance/seal-approvals",
            Some(req),
            opts,
            signal,
        )
        .await
    }

    /// 审批通过用印申请（写，step-up 必须）。对应 TS `approveSealApproval`。
    pub async fn approve_seal_approval(
        &self,
        id: i64,
        query: &ApproveSealApprovalQuery,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        let mut parts: Vec<String> = Vec::new();
        if let Some(v) = query.expires_at.as_deref().filter(|s| !s.is_empty()) {
            parts.push(format!("expiresAt={}", urlencode(v)));
        }
        if let Some(v) = query.note.as_deref().filter(|s| !s.is_empty()) {
            parts.push(format!("note={}", urlencode(v)));
        }
        let qs = qstr(&parts);
        self.write_unit::<()>(
            "POST",
            &format!("/compliance/seal-approvals/{id}/approve{qs}"),
            None,
            opts,
            signal,
        )
        .await
    }

    /// 审批拒绝用印申请（写）。对应 TS `rejectSealApproval`。
    pub async fn reject_seal_approval(
        &self,
        id: i64,
        query: &RejectSealApprovalQuery,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        let mut parts: Vec<String> = Vec::new();
        if let Some(v) = query.reason.as_deref().filter(|s| !s.is_empty()) {
            parts.push(format!("reason={}", urlencode(v)));
        }
        let qs = qstr(&parts);
        self.write_unit::<()>(
            "POST",
            &format!("/compliance/seal-approvals/{id}/reject{qs}"),
            None,
            opts,
            signal,
        )
        .await
    }

    /// 取消用印申请（写）。对应 TS `cancelSealApproval`。
    pub async fn cancel_seal_approval(
        &self,
        id: i64,
        query: &CancelSealApprovalQuery,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        let mut parts: Vec<String> = Vec::new();
        if let Some(v) = query.reason.as_deref().filter(|s| !s.is_empty()) {
            parts.push(format!("reason={}", urlencode(v)));
        }
        let qs = qstr(&parts);
        self.write_unit::<()>(
            "POST",
            &format!("/compliance/seal-approvals/{id}/cancel{qs}"),
            None,
            opts,
            signal,
        )
        .await
    }

    /// 读 —— pending 用印审批列表（不分页）。对应 TS `listPendingSealApprovals`。
    pub async fn list_pending_seal_approvals(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<SealApproval>> {
        self.read("/compliance/seal-approvals/pending", None, signal)
            .await
    }

    /// 读 —— 用印审批详情。对应 TS `getSealApproval`。
    pub async fn get_seal_approval(
        &self,
        id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<SealApproval> {
        self.read(&format!("/compliance/seal-approvals/{id}"), None, signal)
            .await
    }

    /// 读 —— 用印审批分页列表（S1）。对应 TS `listSealApprovals`。
    pub async fn list_seal_approvals(
        &self,
        req: &ListSealApprovalsRequest,
        signal: Option<CancellationToken>,
    ) -> Result<crate::shared::pagination::PageResult<SealApprovalPageItem>> {
        let mut q = QueryBuilder::new(&req.page);
        q.add("status", req.status.as_deref());
        q.add("createTimeStart", req.create_time_start.as_deref());
        q.add("createTimeEnd", req.create_time_end.as_deref());
        let path = format!("/compliance/seal-approvals/page{}", q.finish());
        self.read(&path, None, signal).await
    }

    // =========================================================================
    // Seal Use (read-only — S6 / gap-register U-4)
    // =========================================================================

    /// 读 —— 用印执行记录分页列表（S6）。对应 TS `listSealUses`。
    pub async fn list_seal_uses(
        &self,
        req: &ListSealUsesRequest,
        signal: Option<CancellationToken>,
    ) -> Result<crate::shared::pagination::PageResult<SealUsePageItem>> {
        let mut q = QueryBuilder::new(&req.page);
        q.add("sealId", req.seal_id.map(|v| v.to_string()).as_deref());
        q.add(
            "envelopeId",
            req.envelope_id.map(|v| v.to_string()).as_deref(),
        );
        q.add("usageStatus", req.usage_status.as_deref());
        q.add("createTimeStart", req.create_time_start.as_deref());
        q.add("createTimeEnd", req.create_time_end.as_deref());
        let path = format!("/compliance/seal-uses/page{}", q.finish());
        self.read(&path, None, signal).await
    }

    // =========================================================================
    // Provider Request (read-only)
    // =========================================================================

    /// 读 —— provider request 状态视图。对应 TS `getProviderRequest`。
    pub async fn get_provider_request(
        &self,
        id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<ProviderRequestStatusView> {
        self.read(&format!("/compliance/provider-requests/{id}"), None, signal)
            .await
    }

    /// 轮询 provider request 到 SUCCESS / FAILED 终态。对应 TS
    /// `waitForProviderRequestTerminal`。
    pub async fn wait_for_provider_request_terminal(
        &self,
        id: i64,
        opts: &CompliancePollOptions,
        signal: Option<CancellationToken>,
    ) -> std::result::Result<ProviderRequestStatusView, CompliancePollError> {
        self.poll(
            || self.get_provider_request(id, signal.clone()),
            |v: &ProviderRequestStatusView| classify_provider_status(&v.status),
            opts,
            signal.clone(),
            derive_last_info_provider,
        )
        .await
    }

    // =========================================================================
    // Capabilities (read-only — S2 / gap-register U-6)
    // =========================================================================

    /// 读 —— compliance 能力闸门列表（S2）。对应 TS `getCapabilities`。
    pub async fn get_capabilities(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<ComplianceCapability>> {
        self.read("/compliance/capabilities", None, signal).await
    }

    /// 读 —— 单个动作的能力闸门视图（便捷方法）。无匹配返回 `None`。对应 TS `getFeatureGate`。
    pub async fn get_feature_gate(
        &self,
        action: &str,
        signal: Option<CancellationToken>,
    ) -> Result<Option<ComplianceCapability>> {
        let caps = self.get_capabilities(signal).await?;
        Ok(caps.into_iter().find(|c| c.action == action))
    }

    // =========================================================================
    // Operation projection (read-only — S2 / gap-register U-5)
    // =========================================================================

    /// 读 —— compliance 操作投影分页列表（S2）。对应 TS `listOperations`。
    pub async fn list_operations(
        &self,
        req: &ListOperationsRequest,
        signal: Option<CancellationToken>,
    ) -> Result<crate::shared::pagination::PageResult<OperationPageItem>> {
        let mut q = QueryBuilder::new(&req.page);
        q.add("status", req.status.as_deref());
        q.add("createTimeStart", req.create_time_start.as_deref());
        q.add("createTimeEnd", req.create_time_end.as_deref());
        let path = format!("/compliance/operations/page{}", q.finish());
        self.read(&path, None, signal).await
    }

    /// 读 —— compliance 操作投影详情（S2）。`id` 为数值行主键。对应 TS `getOperation`。
    pub async fn get_operation(
        &self,
        id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<OperationDetail> {
        self.read(&format!("/compliance/operations/{id}"), None, signal)
            .await
    }

    // =========================================================================
    // Contract Template (S5 — gap-register U-2)
    // =========================================================================

    /// 创建合同模板（写）。对应 TS `createContractTemplate`。
    pub async fn create_contract_template(
        &self,
        req: &CreateContractTemplateRequest,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<ContractTemplateResp> {
        self.write(
            "POST",
            "/compliance/contract-templates",
            Some(req),
            opts,
            signal,
        )
        .await
    }

    /// 更新合同模板（写，仅 DRAFT 状态）。对应 TS `updateContractTemplate`。
    pub async fn update_contract_template(
        &self,
        id: i64,
        req: &UpdateContractTemplateRequest,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<ContractTemplateResp> {
        self.write(
            "POST",
            &format!("/compliance/contract-templates/{id}"),
            Some(req),
            opts,
            signal,
        )
        .await
    }

    /// 删除合同模板（写，仅 DRAFT 状态）。对应 TS `deleteContractTemplate`。
    pub async fn delete_contract_template(
        &self,
        id: i64,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        self.write_unit::<()>(
            "POST",
            &format!("/compliance/contract-templates/{id}/delete"),
            None,
            opts,
            signal,
        )
        .await
    }

    /// 读 —— 合同模板详情。对应 TS `getContractTemplate`。
    pub async fn get_contract_template(
        &self,
        id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<ContractTemplateResp> {
        self.read(
            &format!("/compliance/contract-templates/{id}"),
            None,
            signal,
        )
        .await
    }

    /// 读 —— 合同模板分页列表（S5）。对应 TS `listContractTemplates`。
    pub async fn list_contract_templates(
        &self,
        req: &ListContractTemplatesRequest,
        signal: Option<CancellationToken>,
    ) -> Result<crate::shared::pagination::PageResult<ContractTemplatePageItem>> {
        let mut q = QueryBuilder::new(&req.page);
        q.add("status", req.status.as_deref());
        q.add("createTimeStart", req.create_time_start.as_deref());
        q.add("createTimeEnd", req.create_time_end.as_deref());
        let path = format!("/compliance/contract-templates/page{}", q.finish());
        self.read(&path, None, signal).await
    }

    /// 上传合同模板 PDF（写）。对应 TS `uploadContractTemplatePdf`。
    pub async fn upload_contract_template_pdf(
        &self,
        id: i64,
        req: &UploadContractTemplatePdfRequest,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<ContractTemplateResp> {
        self.write(
            "POST",
            &format!("/compliance/contract-templates/{id}/pdf"),
            Some(req),
            opts,
            signal,
        )
        .await
    }

    /// 发布合同模板（写）。DRAFT → PUBLISHED。对应 TS `publishContractTemplate`。
    pub async fn publish_contract_template(
        &self,
        id: i64,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<ContractTemplateResp> {
        self.write::<(), _>(
            "POST",
            &format!("/compliance/contract-templates/{id}/publish"),
            None,
            opts,
            signal,
        )
        .await
    }

    /// 归档合同模板（写）。PUBLISHED → ARCHIVED。对应 TS `archiveContractTemplate`。
    pub async fn archive_contract_template(
        &self,
        id: i64,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<ContractTemplateResp> {
        self.write::<(), _>(
            "POST",
            &format!("/compliance/contract-templates/{id}/archive"),
            None,
            opts,
            signal,
        )
        .await
    }

    /// 读 —— 合同模板版本快照列表。对应 TS `listContractTemplateVersions`。
    pub async fn list_contract_template_versions(
        &self,
        id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<ContractTemplateVersion>> {
        self.read(
            &format!("/compliance/contract-templates/{id}/versions"),
            None,
            signal,
        )
        .await
    }

    // =========================================================================
    // Error classification (re-export for convenience)
    // =========================================================================

    /// 把一个错误分类为 compliance 视图。非 compliance 段位的 BusinessError 返回 `None`。
    /// 对应 TS `classifyError`。
    pub fn classify_error(&self, err: &Error) -> Option<ComplianceErrorInfo> {
        if !matches!(err, Error::Business { .. }) {
            return None;
        }
        if !is_compliance_business_error(err) {
            return None;
        }
        Some(classify_compliance_error(err))
    }

    // =========================================================================
    // Internal helpers
    // =========================================================================

    /// 读路径：GET / 公开 verify。允许 401 单次刷新后重放（GET 幂等安全）。
    async fn read<T: DeserializeOwned>(
        &self,
        path: &str,
        body: Option<&str>,
        signal: Option<CancellationToken>,
    ) -> Result<T> {
        self.client
            .compliance_execute_json(reqwest::Method::GET, path, body, signal, true, &[])
            .await
    }

    /// 公开读路径：public verify。无 token 匿名；有 token 附 Authorization；401 不刷新重放。
    async fn public_read<T: DeserializeOwned>(
        &self,
        path: &str,
        signal: Option<CancellationToken>,
    ) -> Result<T> {
        self.client.compliance_public_read(path, signal).await
    }

    /// 写路径：POST。发送前 ensure_token 一次；401 → 抛 HTTPError；不重试。
    async fn write<B: Serialize, T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<&B>,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<T> {
        let body_str = match body {
            Some(b) => Some(
                serde_json::to_string(b)
                    .map_err(|e| Error::other(format!("{path}: serialize body: {e}")))?,
            ),
            None => None,
        };
        let m = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|e| Error::other(format!("invalid method {method}: {e}")))?;
        let extra = idempotency_header(opts);
        self.client
            .compliance_execute_json(m, path, body_str.as_deref(), signal, false, &extra)
            .await
    }

    /// 写路径返回 `()`（空体 void 端点）。对应 TS `write<void>`。
    async fn write_unit<B: Serialize>(
        &self,
        method: &str,
        path: &str,
        body: Option<&B>,
        opts: &ComplianceWriteOptions,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        let body_str = match body {
            Some(b) => Some(
                serde_json::to_string(b)
                    .map_err(|e| Error::other(format!("{path}: serialize body: {e}")))?,
            ),
            None => None,
        };
        let m = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|e| Error::other(format!("invalid method {method}: {e}")))?;
        let extra = idempotency_header(opts);
        self.client
            .compliance_execute_unit(m, path, body_str.as_deref(), signal, false, &extra)
            .await
    }

    /// 轮询循环（exponential backoff）。对应 TS `poll`。
    ///
    /// 终态失败优先于 timeout；abort 携带最后状态；timeout 不算 retryable。
    async fn poll<T, F, Fut, C, D>(
        &self,
        mut fetcher: F,
        classify: C,
        opts: &CompliancePollOptions,
        signal: Option<CancellationToken>,
        derive_last: D,
    ) -> std::result::Result<T, CompliancePollError>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T>>,
        C: Fn(&T) -> PollDecision,
        D: Fn(&T) -> ComplianceErrorInfo,
    {
        let timeout_ms = opts.timeout_ms.unwrap_or(DEFAULT_POLL_TIMEOUT_MS);
        let initial = opts
            .initial_interval_ms
            .unwrap_or(DEFAULT_POLL_INITIAL_INTERVAL_MS);
        let max_interval = opts.max_interval_ms.unwrap_or(DEFAULT_POLL_MAX_INTERVAL_MS);
        let multiplier = opts.multiplier.unwrap_or(DEFAULT_POLL_MULTIPLIER);

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let mut interval_ms = initial;
        let mut last_value: Option<T> = None;

        while Instant::now() < deadline {
            if signal.as_ref().map(|s| s.is_cancelled()).unwrap_or(false) {
                return Err(CompliancePollError::new(
                    "compliance poll aborted",
                    CompliancePollErrorKind::Unknown,
                    last_value.as_ref().map(&derive_last),
                ));
            }
            let value = fetcher().await.map_err(|e| {
                CompliancePollError::new(
                    format!("compliance poll fetch failed: {e}"),
                    CompliancePollErrorKind::Unknown,
                    None,
                )
            })?;
            let decision = classify(&value);
            match decision {
                PollDecision::Done => return Ok(value),
                PollDecision::Failed => {
                    return Err(CompliancePollError::new(
                        "compliance poll observed terminal failure",
                        CompliancePollErrorKind::TerminalFailure,
                        Some(derive_last(&value)),
                    ));
                }
                PollDecision::Continue => {}
            }
            last_value = Some(value);

            let remaining = deadline.saturating_duration_since(Instant::now());
            let sleep_ms = interval_ms.min(remaining.as_millis() as u64);
            if sleep_ms == 0 {
                break;
            }
            // 可取消 sleep。
            match &signal {
                Some(cancel) => {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(sleep_ms)) => {}
                        _ = cancel.cancelled() => {
                            return Err(CompliancePollError::new(
                                "compliance poll aborted",
                                CompliancePollErrorKind::Unknown,
                                last_value.as_ref().map(&derive_last),
                            ));
                        }
                    }
                }
                None => tokio::time::sleep(Duration::from_millis(sleep_ms)).await,
            }
            interval_ms = ((interval_ms as f64 * multiplier).floor() as u64).min(max_interval);
        }

        Err(CompliancePollError::new(
            "compliance poll timed out",
            CompliancePollErrorKind::Timeout,
            last_value.as_ref().map(&derive_last),
        ))
    }
}

impl Client {
    /// SDK-facing compliance 域子客户端。对应 TS `client.compliance` getter。
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use acosmi::Client;
    /// # async fn demo(client: &Client) -> Result<(), Box<dyn std::error::Error>> {
    /// let cc = client.compliance();
    /// let stats = cc.get_tsa_stats(None).await?; // TSA 只读统计视图
    /// # let _ = stats; Ok(()) }
    /// ```
    pub fn compliance(&self) -> ComplianceClient {
        ComplianceClient::new(self.clone())
    }

    /// Compliance API URL 拼接。`compliance_base_url` 已配置时直接拼接；未配置时默认
    /// `{server_url}/admin-api` + path，匹配 Java compliance controller 的 `/compliance/...` 路径。
    /// 对应 TS `complianceURL`。不复用 `api_url`（后者强制 `/api/v4`）。
    pub fn compliance_url(&self, path: &str) -> String {
        let base = match self.compliance_base_url() {
            Some(b) => b.to_string(),
            None => format!("{}/admin-api", self.server_url()),
        };
        format!("{base}{path}")
    }

    /// compliance 认证 JSON 请求（read=retry_on_401 / write=不重试）→ `ApiResponse<T>` 解包 → data。
    /// 对应 TS `ComplianceClient.executeJson`。空体成功响应（void 端点除外）→ 强 Err。
    pub(crate) async fn compliance_execute_json<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&str>,
        signal: Option<CancellationToken>,
        retry_on_401: bool,
        extra_headers: &[(String, String)],
    ) -> Result<T> {
        let bytes = self
            .compliance_execute_bytes(method, path, body, signal, retry_on_401, extra_headers)
            .await?;
        if bytes.is_empty() {
            return Err(Error::other(format!("{path}: empty response body")));
        }
        let env: ApiResponse<T> = serde_json::from_slice(&bytes)
            .map_err(|e| Error::other(format!("{path}: decode: {e}")))?;
        if let Some(err) = env.business_error() {
            return Err(err);
        }
        Ok(env.data)
    }

    /// compliance void 写端点：空体合法（返回 `()`）；非空体仍走业务码检查。对应 TS
    /// `write<void>`（`undefined as T`）。
    pub(crate) async fn compliance_execute_unit(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&str>,
        signal: Option<CancellationToken>,
        retry_on_401: bool,
        extra_headers: &[(String, String)],
    ) -> Result<()> {
        let bytes = self
            .compliance_execute_bytes(method, path, body, signal, retry_on_401, extra_headers)
            .await?;
        if bytes.is_empty() {
            return Ok(());
        }
        // 非空体：仍检查业务码（不解 data）。
        let env: ApiResponse<serde_json::Value> = serde_json::from_slice(&bytes)
            .map_err(|e| Error::other(format!("{path}: decode: {e}")))?;
        if let Some(err) = env.business_error() {
            return Err(err);
        }
        Ok(())
    }

    /// compliance 请求底座：派生超时 → ensure_token → Bearer → 单次 do_request →
    /// 401（read）单次 force_refresh 重放 → 非 2xx 抛 HTTPError → 返回 body bytes。
    async fn compliance_execute_bytes(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&str>,
        signal: Option<CancellationToken>,
        retry_on_401: bool,
        extra_headers: &[(String, String)],
    ) -> Result<Vec<u8>> {
        // 非流式 JSON 套默认超时（对应 TS withRequestTimeout(DEFAULT_API_TIMEOUT_MS)）。
        let ctl = self.derive_timeout_token(DEFAULT_JSON_TIMEOUT_MS, signal);
        self.compliance_execute_bytes_inner(
            method,
            path,
            body,
            ctl.as_ref(),
            retry_on_401,
            extra_headers,
            false,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    fn compliance_execute_bytes_inner<'a>(
        &'a self,
        method: reqwest::Method,
        path: &'a str,
        body: Option<&'a str>,
        signal: Option<&'a CancellationToken>,
        retry_on_401: bool,
        extra_headers: &'a [(String, String)],
        retried: bool,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>>> + Send + 'a>> {
        Box::pin(async move {
            let token = self.ensure_token(signal.cloned()).await?;
            let url = self.compliance_url(path);

            let mut headers: Vec<(reqwest::header::HeaderName, String)> = vec![
                (reqwest::header::AUTHORIZATION, format!("Bearer {token}")),
                (reqwest::header::ACCEPT, "application/json".to_string()),
            ];
            if body.is_some() {
                headers.push((
                    reqwest::header::CONTENT_TYPE,
                    "application/json".to_string(),
                ));
            }
            for (k, v) in extra_headers {
                if let Ok(name) = reqwest::header::HeaderName::from_bytes(k.as_bytes()) {
                    headers.push((name, v.clone()));
                }
            }

            let resp = self
                .do_request(method.clone(), &url, &headers, body, signal)
                .await?;

            if resp.status().as_u16() == 401 && retry_on_401 && !retried {
                drop(resp);
                self.force_refresh(signal.cloned())
                    .await
                    .map_err(|e| Error::other(format!("unauthorized and refresh failed: {e}")))?;
                return self
                    .compliance_execute_bytes_inner(
                        method,
                        path,
                        body,
                        signal,
                        retry_on_401,
                        extra_headers,
                        true,
                    )
                    .await;
            }

            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let retry_after = parse_retry_after(resp.headers());
                let text = read_limited_text(resp.bytes_stream(), MAX_ERROR_BODY_SIZE).await?;
                return Err(Error::Http(parse_http_error_with_retry_after(
                    status,
                    &text,
                    retry_after,
                )));
            }

            let bytes = resp
                .bytes()
                .await
                .map_err(|e| Error::other(format!("{path}: read body: {e}")))?;
            Ok(bytes.to_vec())
        })
    }

    /// compliance 公开读：无 token 匿名（ensure_token 失败吞掉）；有 token 附 Authorization；
    /// 401 不刷新重放。对应 TS `ComplianceClient.publicRead`。
    pub(crate) async fn compliance_public_read<T: DeserializeOwned>(
        &self,
        path: &str,
        signal: Option<CancellationToken>,
    ) -> Result<T> {
        let ctl = self.derive_timeout_token(DEFAULT_JSON_TIMEOUT_MS, signal);
        let token = self.ensure_token(ctl.clone()).await.ok();
        let url = self.compliance_url(path);
        let mut headers: Vec<(reqwest::header::HeaderName, String)> =
            vec![(reqwest::header::ACCEPT, "application/json".to_string())];
        if let Some(t) = token.filter(|t| !t.is_empty()) {
            headers.push((reqwest::header::AUTHORIZATION, format!("Bearer {t}")));
        }

        let resp = self
            .do_request(reqwest::Method::GET, &url, &headers, None, ctl.as_ref())
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let retry_after = parse_retry_after(resp.headers());
            let text = read_limited_text(resp.bytes_stream(), MAX_ERROR_BODY_SIZE).await?;
            return Err(Error::Http(parse_http_error_with_retry_after(
                status,
                &text,
                retry_after,
            )));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| Error::other(format!("{path}: read body: {e}")))?;
        if bytes.is_empty() {
            return Err(Error::other(format!("{path}: empty response body")));
        }
        let env: ApiResponse<T> = serde_json::from_slice(&bytes)
            .map_err(|e| Error::other(format!("{path}: decode: {e}")))?;
        if let Some(err) = env.business_error() {
            return Err(err);
        }
        Ok(env.data)
    }
}

// =============================================================================
// poll 分类 + last-info 派生（对齐 TS classifyTimestamp / classifyProviderStatus / deriveLastInfo）
// =============================================================================

fn classify_timestamp(status: &ComplianceTimestampVerificationStatus) -> PollDecision {
    match status.as_str() {
        ComplianceTimestampVerificationStatus::VERIFIED => PollDecision::Done,
        ComplianceTimestampVerificationStatus::FAILED
        | ComplianceTimestampVerificationStatus::LOCAL_VERIFY_FAILED => PollDecision::Failed,
        // PENDING / UNKNOWN / RETRYING / 未知 → continue。
        _ => PollDecision::Continue,
    }
}

fn classify_provider_status(status: &ComplianceProviderRequestStatus) -> PollDecision {
    match status.as_str() {
        ComplianceProviderRequestStatus::SUCCESS => PollDecision::Done,
        ComplianceProviderRequestStatus::FAILED => PollDecision::Failed,
        // PENDING / UNKNOWN / RETRYING / 未知 → continue。
        _ => PollDecision::Continue,
    }
}

/// 从最后一次轮询到的状态视图派生 ComplianceErrorInfo（best-effort 诊断载体，不伪造 code）。
fn derive_last_info_timestamp(t: &TimestampToken) -> ComplianceErrorInfo {
    let status = t.verification_status.as_str();
    let message = t
        .verification_error
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if status.is_empty() {
                "compliance poll: last observed status (no detail)".to_string()
            } else {
                format!("last polled status: {status}")
            }
        });
    last_info(message)
}

fn derive_last_info_provider(v: &ProviderRequestStatusView) -> ComplianceErrorInfo {
    let status = v.status.as_str();
    let message = if status.is_empty() {
        "compliance poll: last observed status (no detail)".to_string()
    } else {
        format!("last polled status: {status}")
    };
    last_info(message)
}

fn last_info(message: String) -> ComplianceErrorInfo {
    ComplianceErrorInfo {
        code: 0,
        message,
        key: ComplianceErrorKey::UnknownComplianceError,
        retryable: false,
        terminal: false,
        step_up_required: false,
    }
}

// =============================================================================
// query / header helpers
// =============================================================================

/// 把分页 + 过滤字段拼成 query string（含前导 `?`，空则返回 `""`）。对应 TS `pageQuery`。
struct QueryBuilder {
    parts: Vec<String>,
}

impl QueryBuilder {
    fn new(page: &PageRequest) -> Self {
        let mut b = QueryBuilder { parts: Vec::new() };
        b.add("pageNo", page.page_no.map(|v| v.to_string()).as_deref());
        b.add("pageSize", page.page_size.map(|v| v.to_string()).as_deref());
        b.add("sortBy", page.sort_by.as_deref());
        if let Some(dir) = &page.sort_direction {
            let s = match dir {
                crate::shared::pagination::SortDirection::Asc => "asc",
                crate::shared::pagination::SortDirection::Desc => "desc",
            };
            b.add("sortDirection", Some(s));
        }
        b
    }

    fn add(&mut self, key: &str, value: Option<&str>) {
        if let Some(v) = value.filter(|s| !s.is_empty()) {
            self.parts.push(format!("{}={}", key, urlencode(v)));
        }
    }

    fn finish(self) -> String {
        qstr(&self.parts)
    }
}

fn qstr(parts: &[String]) -> String {
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
}

fn idempotency_header(opts: &ComplianceWriteOptions) -> Vec<(String, String)> {
    match opts.idempotency_key.as_deref().filter(|s| !s.is_empty()) {
        Some(k) => vec![("Idempotency-Key".to_string(), k.to_string())],
        None => Vec::new(),
    }
}

/// 最小 URL 编码（与 billing entitlements urlencoding 同口径）。复用 crate 内既有实现。
fn urlencode(s: &str) -> String {
    crate::billing::entitlements::urlencoding(s)
}

fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> i64 {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|&s| s > 0)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_builder_skips_empty_and_encodes() {
        let page = PageRequest {
            page_no: Some(2),
            page_size: Some(20),
            ..Default::default()
        };
        let mut q = QueryBuilder::new(&page);
        q.add("status", Some("DRAFT"));
        q.add("createTimeStart", Some("2026-05-01 00:00:00"));
        q.add("nope", None);
        q.add("empty", Some(""));
        let s = q.finish();
        assert!(s.starts_with('?'));
        assert!(s.contains("pageNo=2"));
        assert!(s.contains("pageSize=20"));
        assert!(s.contains("status=DRAFT"));
        // 空格被编码。
        assert!(s.contains("createTimeStart=2026-05-01%2000%3A00%3A00"));
        assert!(!s.contains("nope"));
        assert!(!s.contains("empty"));
    }

    #[test]
    fn query_builder_empty_is_blank() {
        let q = QueryBuilder::new(&PageRequest::default());
        assert_eq!(q.finish(), "");
    }

    #[test]
    fn classify_timestamp_decisions() {
        assert_eq!(
            classify_timestamp(&ComplianceTimestampVerificationStatus::from("VERIFIED")),
            PollDecision::Done
        );
        assert_eq!(
            classify_timestamp(&ComplianceTimestampVerificationStatus::from("FAILED")),
            PollDecision::Failed
        );
        assert_eq!(
            classify_timestamp(&ComplianceTimestampVerificationStatus::from(
                "LOCAL_VERIFY_FAILED"
            )),
            PollDecision::Failed
        );
        assert_eq!(
            classify_timestamp(&ComplianceTimestampVerificationStatus::from("PENDING")),
            PollDecision::Continue
        );
        assert_eq!(
            classify_timestamp(&ComplianceTimestampVerificationStatus::from("WHATEVER")),
            PollDecision::Continue
        );
    }

    #[test]
    fn classify_provider_decisions() {
        assert_eq!(
            classify_provider_status(&ComplianceProviderRequestStatus::from("SUCCESS")),
            PollDecision::Done
        );
        assert_eq!(
            classify_provider_status(&ComplianceProviderRequestStatus::from("FAILED")),
            PollDecision::Failed
        );
        assert_eq!(
            classify_provider_status(&ComplianceProviderRequestStatus::from("RETRYING")),
            PollDecision::Continue
        );
    }

    #[test]
    fn idempotency_header_present_and_absent() {
        let opts = ComplianceWriteOptions {
            idempotency_key: Some("key-1".to_string()),
        };
        assert_eq!(
            idempotency_header(&opts),
            vec![("Idempotency-Key".to_string(), "key-1".to_string())]
        );
        assert!(idempotency_header(&ComplianceWriteOptions::default()).is_empty());
    }

    // 🔴 同名字段分歧（方案 §3）：provider.seal_id=String vs operation/seal-approval.seal_id=i64。
    #[test]
    fn seal_id_divergence_provider_string_vs_operation_i64() {
        // provider 域：seal_id 是 String。
        let pv: ProviderRequestStatusView = serde_json::from_str(
            r#"{"id":1,"status":"PENDING","terminal":false,"retryable":false,"sealId":"SEAL-ABC"}"#,
        )
        .unwrap();
        assert_eq!(pv.seal_id.as_deref(), Some("SEAL-ABC"));

        // operation 域：seal_id 是 i64。
        let op: OperationDetail = serde_json::from_str(
            r#"{"id":2,"operationId":"op-1","status":"running","terminal":false,"retryable":true,"attemptCount":1,"sealId":42,"createTime":"2026-05-01T00:00:00"}"#,
        )
        .unwrap();
        assert_eq!(op.base.seal_id, Some(42));

        // seal-approval 域：seal_id 是 i64。
        let sa: SealApproval =
            serde_json::from_str(r#"{"id":3,"status":"SUBMITTED","sealId":7}"#).unwrap();
        assert_eq!(sa.seal_id, Some(7));
    }

    #[test]
    fn operation_detail_and_page_item_independent_round_trip() {
        let json = r#"{"id":5,"operationId":"op-x","status":"succeeded","terminal":true,"retryable":false,"attemptCount":3,"businessNo":"B1","createTime":"2026-05-01T01:02:03"}"#;
        let item: OperationPageItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.base.operation_id, "op-x");
        assert_eq!(item.base.business_no.as_deref(), Some("B1"));
        let detail: OperationDetail = serde_json::from_str(json).unwrap();
        assert_eq!(detail.base.attempt_count, 3);
        // flatten 序列化 round-trip 不丢字段。
        let back = serde_json::to_value(&item).unwrap();
        assert_eq!(back["operationId"], "op-x");
        assert_eq!(back["attemptCount"], 3);
    }

    #[test]
    fn evidence_asset_wire_round_trip() {
        let json = r#"{"id":1,"evidenceNo":"EV-1","assetType":"CONTRACT","name":"x","hashAlgorithm":"sha256","contentHash":"abcd","digestSource":"CLIENT","privacyLevel":"private","status":"DRAFT"}"#;
        let a: EvidenceAsset = serde_json::from_str(json).unwrap();
        assert_eq!(a.evidence_no, "EV-1");
        assert_eq!(a.asset_type.as_str(), "CONTRACT");
        assert_eq!(a.hash_algorithm.as_str(), "sha256");
        let back = serde_json::to_value(&a).unwrap();
        assert_eq!(back["evidenceNo"], "EV-1");
        assert_eq!(back["assetType"], "CONTRACT");
    }
}
