//! Agent Run Gateway 域：agent runs（create/list/get/cancel/stream/run + 本地工具编排）
//! + remote-control（11-event union + 审批回写 / 中途消息 / session-control）+ BYOK 密钥管理面。
//!
//! 对齐 `agent-runs/index.ts`。
//!
//! ## 🔴 两条 SSE 未知事件处理相反（方案 §4.1 红线）
//! - [`types::AgentRunStreamEvent`] 未知 type → **Error 事件注入流**（`code="unknown_event"`，不丢）;
//! - [`remote_control::parse_remote_control_event`] 未知 type → **None 静默丢弃**（warn-only）。
//!
//! 两套解析独立，不可统一。
//!
//! ## 子客户端 getter（方案 §4.2）
//! [`AgentRunsClient`]（[`crate::Client::agent_runs`]）/ [`CrabCodeByokClient`]
//! （[`crate::Client::crabcode_byok`]）—— 无状态，持 [`crate::Client`] clone。

pub mod byok;
#[allow(clippy::module_inception)]
pub mod client;
pub mod remote_control;
pub mod types;

pub use byok::{
    ByokCreateRequest, ByokCredential, CrabCodeByokClient, BYOK_PROVIDER_ANTHROPIC,
    BYOK_PROVIDER_CUSTOM, BYOK_PROVIDER_DASHSCOPE, BYOK_PROVIDER_DEEPSEEK, BYOK_PROVIDER_OPENAI,
    BYOK_PROVIDER_VOLCENGINE, BYOK_PROVIDER_ZHIPU, BYOK_STATUS_ACTIVE, BYOK_STATUS_REVOKED,
};
pub use client::{AgentRunsClient, LocalToolContext};
pub use remote_control::{
    is_terminal_remote_event, parse_remote_control_event, AdapterKind, PermissionPolicy,
    RemoteControlEvent, RemotePermissionResultRequest, RemoteSessionTokenGrant,
    RemoteUserMessageAck, RemoteUserMessageRequest, RunnerKind, WorkspacePolicy,
    REMOTE_PERMISSION_DECISION_APPROVED, REMOTE_PERMISSION_DECISION_REJECTED,
};
pub use types::{
    AgentRun, AgentRunArtifact, AgentRunArtifactPolicy, AgentRunCreateRequest,
    AgentRunCreateResponse, AgentRunDownload, AgentRunErrorPayload, AgentRunListOptions,
    AgentRunListResult, AgentRunLocalContextPolicy, AgentRunLocalToolResult, AgentRunRunOptions,
    AgentRunSettlement, AgentRunStatus, AgentRunStreamEvent, AgentRunStreamOptions, AgentRunUsage,
    AgentRunWithLocalToolsOptions, AGENT_RUN_META_TITLE, AGENT_RUN_META_WORKSPACE,
    AGENT_RUN_RUNTIME_CRABCODE_REMOTE, AGENT_RUN_RUNTIME_STANDARD,
};
