//! # acosmi-sdk
//!
//! Acosmi Rust SDK — 模型网关（双格式 Anthropic + OpenAI）、Agent Run Gateway 与
//! Compliance（电子证据、时间章、报告、签署 envelope）统一客户端。
//!
//! 端口自 [`acosmi-sdk-ts`](https://github.com/acosmi/sdk-ts) v2.8.0（事实标准主实现）。
//! 跨语言契约（snake_case wire-format / 符号名对齐 / bug-for-bug 行为）见
//! `docs/开发与发布手册.md` §5。
//!
//! ## 双格式红线
//!
//! `AnthropicAdapter` + `OpenAIAdapter` 等地位（对应两个不同下游产品），恒编译、不可降级。
//!
//! ## 运行时
//!
//! 仅原生 `tokio` + `reqwest`（rustls TLS）。流式走 `impl Stream`，取消走
//! `tokio_util::CancellationToken`。
//!
//! ## 模块（随分阶段端口逐步填充）
//!
//! 各业务域 module 是该域对外切片的单一真相源；`lib.rs` 经 `pub use` 对齐
//! `index.ts` 的逐域 re-export。

#![forbid(unsafe_code)]

/// SDK 版本（对齐 npm `@acosmi/sdk-ts` 主线 / `Cargo.toml` package.version）。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

mod macros;

// === 业务域 module（P1→P8 分阶段填充）===
// 已落地：shared（错误体系 + 跨域 DTO）/ core（http/retry/store/client 骨架）/ auth（types 前置）。
// 待加入：auth 全量(P2) / models(+adapters)(P3) / chat+SSE(P4) / billing/skills/
// notifications/agent_runs(P5) / compliance/support(P6) / 商品化(P7) / sanitize(P8)。
pub mod agent_runs;
pub mod auth;
pub mod billing;
pub mod casehall;
pub mod chatbridge;
pub mod compliance;
pub mod core;
pub mod enterprise;
pub mod finance;
pub mod models;
pub mod notifications;
pub mod pricing;
pub mod products;
/// Provider 无关的历史消息清洗子包（feature `sanitize`，默认开；对齐 npm 子路径 `./sanitize`，
/// index.ts `export * as sanitize`）。经 [`core::sanitize_bridge`] 与 `Client` 接通。
#[cfg(feature = "sanitize")]
pub mod sanitize;
pub mod shared;
pub mod skills;
pub mod subscription;
pub mod support;

// 逐域 re-export（对齐 index.ts 单一真源）。方法名 snake_case，类型名 PascalCase 保留跨语言锚点。
pub use crate::auth::{
    all_scopes, chat_bridge_scopes, code_challenge, commerce_scopes,
    complete_web_authorization_request, create_web_authorization_request, discover,
    discover_web_oauth_metadata, discover_with_profile, exchange_code, exchange_code_with_expiry,
    generate_code_verifier, generate_state, is_invalid_grant_error, is_ssl_error,
    is_valid_token_set, model_scopes, new_token_set, refresh_token, register,
    register_web_oauth_client, remote_control_scopes, resolve_success_redirect, revoke_token,
    skill_scopes, token_set_is_expired, AuthorizeResult, ClientRegistration,
    CreateWebAuthorizationRequestOptions, LoginEvent, LoginOptions, OAuthTokenEndpointError,
    RegisterWebOAuthClientOptions, ServerMetadata, TokenResponse, TokenSet,
    WebAuthorizationCallbackParams, WebAuthorizationPending, WebAuthorizationRequest,
};
pub use crate::core::{
    ChatUsageEvent, Client, Config, FileTokenStore, FilterStatus, InMemoryTokenStore, TokenStore,
    DEFAULT_GATEWAY_BASE_URL,
};
// === models 域逐项 re-export（对齐 models/index.ts）===
pub use crate::models::{
    anthropic_response_text_content, anthropic_response_thinking_content,
    anthropic_response_tool_use_blocks, bucket_info_is_commercial, bucket_row_is_commercial,
    build_betas, extract_anthropic_block_meta, find_desktop_visual_understanding_model,
    find_first_model_by_input_modality, get_adapter, get_adapter_for_model, is_sse_comment_line,
    model_supports_image_input, model_supports_input_modality, new_openai_stream_converter,
    new_thinking_config, new_web_search_tool, parse_settlement, parse_sources_event, unique_merge,
    validate_end_user_id, zero_model_capabilities, Adapter, AnthropicContentBlock,
    AnthropicResponse, AnthropicUsage, BlockMeta, BucketInfo, BucketRow, ChatContentBlock,
    ChatMessage, ChatRequest, ChatResponse, ChatUsage, EffortConfig, EmbeddingData, EmbeddingInput,
    EmbeddingRequest, EmbeddingResponse, EmbeddingUsage, GeoLoc, ImageGenerationRequest,
    ImageGenerationResponse, InputModality, ManagedModel, ModelCapabilities, MultimodalContent,
    OpenAIChatChoice, OpenAIChatMessage, OpenAIChatResponse, OpenAIFunctionCall,
    OpenAIStreamChoice, OpenAIStreamChunk, OpenAIStreamConverter, OpenAIStreamDelta,
    OpenAIStreamToolCall, OpenAIToolCall, OpenAIUsage, OutputConfig, ProviderFormat, QuotaSummary,
    RerankDocument, RerankQuery, RerankRequest, RerankResponse, RerankResult, ServerTool,
    SourcesEvent, StreamEvent, StreamSettlement, ThinkingConfig, VideoGenerationRequest,
    VideoTaskResponse, WebSearchConfig, WebSearchSource, MAX_END_USER_ID_LENGTH,
    SERVER_TOOL_TYPE_WEB_SEARCH, THINKING_HIGH, THINKING_MAX, THINKING_OFF,
};
// === billing 域逐项 re-export（对齐 billing/index.ts）===
pub use crate::billing::{
    BalanceDetail, BalanceDetailEntitlement, BuyResponse, ConsumeRecord, ConsumeRecordPage,
    EntitlementBalance, EntitlementItem, ModelBucket, ModelByQuotaResponse, ModelCoefficient,
    Order, OrderListItem, OrderStatus, PayPayload, PaymentMethod, TokenPackage, Transaction,
    WalletStats,
};
// === skills 域逐项 re-export（对齐 skills/index.ts）===
pub use crate::skills::{
    CertificationStatus, GenerateSkillRequest, GenerateSkillResult, OptimizeSkillRequest,
    OptimizeSkillResult, SkillBrowseListResponse, SkillBrowseResponse, SkillDownload,
    SkillStoreItem, SkillStoreListItem, SkillStoreQuery, SkillSummary, ToolListResponse,
    ToolProvider, ToolView,
};
// === notifications 域逐项 re-export（对齐 notifications/index.ts）===
pub use crate::notifications::{
    parse_notification_event, DeviceRegistration, Notification, NotificationList,
    NotificationPreference, NotificationUnreadCount, WSConfig, WSEvent,
};
// === agent-runs 域逐项 re-export（对齐 agent-runs/index.ts）===
pub use crate::agent_runs::{
    is_terminal_remote_event, parse_remote_control_event, AdapterKind, AgentRun, AgentRunArtifact,
    AgentRunArtifactPolicy, AgentRunCreateRequest, AgentRunCreateResponse, AgentRunDownload,
    AgentRunErrorPayload, AgentRunListOptions, AgentRunListResult, AgentRunLocalContextPolicy,
    AgentRunLocalToolResult, AgentRunRunOptions, AgentRunSettlement, AgentRunStatus,
    AgentRunStreamEvent, AgentRunStreamOptions, AgentRunUsage, AgentRunWithLocalToolsOptions,
    AgentRunsClient, ByokCreateRequest, ByokCredential, CrabCodeByokClient, LocalToolContext,
    PermissionPolicy, RemoteControlEvent, RemotePermissionResultRequest, RemoteSessionTokenGrant,
    RemoteUserMessageAck, RemoteUserMessageRequest, RunnerKind, WorkspacePolicy,
};
// === compliance 域逐项 re-export（对齐 compliance/index.ts）===
pub use crate::compliance::{
    classify_compliance_error, compliance_scopes, is_billing_confirmable,
    is_compliance_business_error, is_compliance_terminal_error, ApproveSealApprovalQuery,
    CancelSealApprovalQuery, ComplianceBillingDisplayStatus, ComplianceCapability,
    ComplianceClient, ComplianceEnvelopeStatus, ComplianceErrorInfo, ComplianceErrorKey,
    CompliancePollError, CompliancePollErrorKind, CompliancePollOptions,
    ComplianceProviderRequestStatus, ComplianceProviderStatus, ComplianceReport,
    ComplianceSealApprovalStatus, ComplianceWriteOptions, ContractTemplateField,
    ContractTemplateFieldType, ContractTemplatePageItem, ContractTemplateResp,
    ContractTemplateStatus, ContractTemplateVersion, CreateContractTemplateRequest,
    CreateEvidenceAssetRequest, CreateH5SigningUrlRequest, CreateReportRequest,
    CreateSigningEnvelopeRequest, EnvelopeContractItem, EvidenceAsset, EvidenceAssetPageItem,
    EvidencePackage, EvidencePackagePageItem, IssueTimestampRequest, ListContractTemplatesRequest,
    ListEvidenceAssetsRequest, ListEvidencePackagesRequest, ListOperationsRequest,
    ListReportsRequest, ListSealApprovalsRequest, ListSealUsesRequest, ListSigningEnvelopesRequest,
    ListTimestampsRequest, OperationBase, OperationDetail, OperationPageItem,
    ProviderRequestStatusView, PublicEvidenceVerifyResult, RejectSealApprovalQuery, ReportDownload,
    ReportPageItem, SealApproval, SealApprovalPageItem, SealUsePageItem, SignEnvelopeRequest,
    SigningEnvelope, SigningEnvelopePageItem, SubmitSealApprovalRequest, TimestampPageItem,
    TimestampToken, TimestampVerifyResult, TsaProvider, TsaStats, UpdateContractTemplateRequest,
    UploadContractTemplatePdfRequest, VerifyTimestampRequest, VoidEnvelopeRequest,
};
// === support 域逐项 re-export（对齐 support/index.ts）===
pub use crate::support::{BugReportResult, BugView};
// === subscription 域逐项 re-export（对齐 subscription/index.ts）===
pub use crate::subscription::{
    Membership, RolloverPolicy, SubscriptionAudience, SubscriptionPlan, SubscriptionPrecheckResult,
    SubscriptionTier, UserSubscription,
};
// === pricing 域逐项 re-export（对齐 pricing/index.ts）===
pub use crate::pricing::{
    ComplianceBenefitType, ComplianceQuoteResponse, ComplianceSku, PricingConfig,
    PublicModelSummary,
};
// === products 域逐项 re-export（对齐 products/index.ts）===
pub use crate::products::{Audience, BillingMode, Product, ProductFamily, RegionScope};
// === casehall 域逐项 re-export（对齐 casehall/index.ts）===
pub use crate::casehall::{
    BookConsultationRequest, BookConsultationResult, CaseLead, CaseLeadIdResult, CaseMatter,
    LawyerCredentialMyView, LawyerSummary, LegalBenefitType, LegalConsultation, LegalServiceOrder,
    LegalServiceSku, LegalSkuCode, ListLawyersParams, SubmitCaseLeadRequest,
};
// === enterprise 域逐项 re-export（对齐 enterprise/index.ts）===
pub use crate::enterprise::{
    AssignSeatRequest, EnterpriseKycMyStatusView, EnterpriseMember, EnterpriseSummary,
    InviteMemberRequest, MemberRole, OrgConsumeReport, OrgSeat, OrgSubscription,
};
// === finance 域逐项 re-export（对齐 finance/index.ts；所有 *Fen = i64 整数分 §3）===
pub use crate::finance::{
    CorporateTransfer, InitiateCorporateTransferInput, InitiateCorporateTransferResult, Invoice,
    InvoiceType, RefundPolicy, RefundProductFamily, RefundRecord, RequestInvoiceInput,
    RequestRefundInput,
};
// === chatbridge 域逐项 re-export（对齐 chatbridge/index.ts；secret 零导出 §安全红线）===
pub use crate::chatbridge::{
    as_credential_ref, is_channel_inbound_event, is_integration_status, is_platform, is_region,
    BridgeThreadRef, ChannelAttachment, ChannelCard, ChannelCardAction, ChannelInboundEvent,
    ChannelOutboundEvent, ChatBridgeClient, ChatBridgeSession, ChatCredentialPublic,
    ChatIntegration, ChatThread, CreateIntegrationRequest, CredentialRef, IntegrationStatus,
    Platform, Region, StoreCredentialRequest, ALL_INTEGRATION_STATUS, ALL_PLATFORMS, ALL_REGIONS,
};
pub use shared::{Error, Result};

#[cfg(test)]
mod scaffold_tests {
    use super::*;

    #[test]
    fn version_is_wired() {
        assert_eq!(VERSION, "2.10.0");
    }
}
