//! Agent Runs 公开 SDK 协议类型。端口自 `agent-runs/types.ts`。
//!
//! 公开 API camelCase（这里以 PascalCase 类型 + snake wire 字段表达）；HTTP wire 为
//! snake_case，转换在 `client.rs`。
//!
//! 🔴 **`AgentRunStreamEvent` 未知 type → Error 变体注入流**（不丢；与
//! `remote_control::parse_remote_control_event` 未知→None 静默丢弃**相反**，两套独立解析）。

use crate::agent_runs::remote_control::{
    AdapterKind, PermissionPolicy, RunnerKind, WorkspacePolicy,
};
use serde_json::Value;

/// run 状态。对应 TS `AgentRunStatus`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentRunStatus {
    /// 默认 / 未知 wire 值（对齐 TS `toStatus` 默认分支）。
    #[default]
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl AgentRunStatus {
    /// wire string → 状态（未知 → Queued，对齐 TS `toStatus` 默认分支）。
    pub fn from_wire(s: Option<&str>) -> Self {
        match s {
            Some("running") => Self::Running,
            Some("completed") => Self::Completed,
            Some("failed") => Self::Failed,
            Some("cancelled") => Self::Cancelled,
            _ => Self::Queued,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// 远控运行时选择器（Phase 3 / 契约 §3）。
pub const AGENT_RUN_RUNTIME_STANDARD: &str = "standard";
pub const AGENT_RUN_RUNTIME_CRABCODE_REMOTE: &str = "crabcode_remote";

/// run metadata 约定键（契约 §18.3 r4）。
pub const AGENT_RUN_META_TITLE: &str = "title";
pub const AGENT_RUN_META_WORKSPACE: &str = "workspace";

/// 本地上下文策略。对应 TS `AgentRunLocalContextPolicy`。
#[derive(Debug, Clone, Default)]
pub struct AgentRunLocalContextPolicy {
    pub enabled: Option<bool>,
    pub readonly: Option<bool>,
    pub max_bytes: Option<i64>,
    pub allowed_tools: Option<Vec<String>>,
}

/// 产物策略。对应 TS `AgentRunArtifactPolicy`。
#[derive(Debug, Clone, Default)]
pub struct AgentRunArtifactPolicy {
    pub enabled: Option<bool>,
    pub max_files: Option<i64>,
}

/// 创建请求。对应 TS `AgentRunCreateRequest`（公开 camelCase；wire 转换在 client）。
#[derive(Debug, Clone, Default)]
pub struct AgentRunCreateRequest {
    pub app_id: String,
    pub mode: Option<String>,
    pub session_id: Option<String>,
    pub input: String,
    pub messages: Option<Vec<Value>>,
    pub model: Option<String>,
    pub active_skill_ids: Option<Vec<String>>,
    pub knowledge_base_ids: Option<Vec<String>>,
    pub metadata: Option<std::collections::HashMap<String, String>>,
    pub local_context_policy: Option<AgentRunLocalContextPolicy>,
    pub artifact_policy: Option<AgentRunArtifactPolicy>,

    // Phase 3+ remote-control 扩展（契约 §3 / ADR-2）。
    pub runtime: Option<String>,
    pub runner: Option<RunnerKind>,
    pub adapter: Option<AdapterKind>,
    pub permission_policy: Option<PermissionPolicy>,
    pub workspace_policy: Option<WorkspacePolicy>,
    /// BYO 模型密钥公开引用（契约 §18.2；仅远控 runtime + runner='cloud'）。
    pub byok_credential_ref: Option<String>,
}

/// agent run view（公开形态）。对应 TS `AgentRun`。
#[derive(Debug, Clone, Default)]
pub struct AgentRun {
    pub run_id: String,
    pub session_id: String,
    pub app_id: Option<String>,
    pub mode: Option<String>,
    pub status: Option<AgentRunStatus>,
    pub created_at: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error: Option<AgentRunErrorPayload>,
    pub metadata: Option<std::collections::HashMap<String, String>>,
    // 远控 view 元数据（Phase 5C；标准 run 为 'standard'/缺省）。view 永不携带 token/policy/messages。
    pub runtime: Option<String>,
    pub runner: Option<String>,
    pub adapter: Option<String>,
}

/// `list()` 过滤/分页参数。对应 TS `AgentRunListOptions`。
#[derive(Debug, Clone, Default)]
pub struct AgentRunListOptions {
    pub runtime: Option<String>,
    pub status: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

/// `list()` 结果。对应 TS `AgentRunListResult`。
#[derive(Debug, Clone)]
pub struct AgentRunListResult {
    pub records: Vec<AgentRun>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

/// 创建响应。对应 TS `AgentRunCreateResponse`。
#[derive(Debug, Clone, Default)]
pub struct AgentRunCreateResponse {
    pub run_id: String,
    pub session_id: String,
    pub status: AgentRunStatus,
}

/// 产物。对应 TS `AgentRunArtifact`。
#[derive(Debug, Clone, Default)]
pub struct AgentRunArtifact {
    pub id: String,
    pub filename: String,
    pub content_type: Option<String>,
    pub size: Option<i64>,
    pub r#type: Option<String>,
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

/// 下载结果。对应 TS `AgentRunDownload`。
#[derive(Debug, Clone)]
pub struct AgentRunDownload {
    pub data: Vec<u8>,
    pub filename: String,
    pub content_type: Option<String>,
}

/// usage（开放形态：已知字段 + raw passthrough）。对应 TS `AgentRunUsage`。
#[derive(Debug, Clone, Default)]
pub struct AgentRunUsage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_create_tokens: Option<i64>,
    pub exact: Option<bool>,
    pub source: Option<String>,
    /// 原始 wire 对象（TS `[key:string]:unknown` 扩展）。
    pub raw: Value,
}

/// settlement（开放形态）。对应 TS `AgentRunSettlement`。
#[derive(Debug, Clone, Default)]
pub struct AgentRunSettlement {
    pub request_id: Option<String>,
    pub status: Option<String>,
    pub consume_status: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_create_tokens: Option<i64>,
    pub token_remaining: Option<i64>,
    pub call_remaining: Option<i64>,
    pub retry_queued: Option<bool>,
    pub exact: Option<bool>,
    pub raw: Value,
}

/// 错误载荷。对应 TS `AgentRunErrorPayload`。
#[derive(Debug, Clone, Default)]
pub struct AgentRunErrorPayload {
    pub code: Option<String>,
    pub message: String,
    pub stage: Option<String>,
    pub retryable: Option<bool>,
    pub raw: Option<Value>,
}

/// 本地工具回写结果。对应 TS `AgentRunLocalToolResult`。
#[derive(Debug, Clone, Default)]
pub struct AgentRunLocalToolResult {
    pub request_id: String,
    pub ok: bool,
    pub content: Option<Value>,
    pub error: Option<String>,
}

/// stream options。对应 TS `AgentRunStreamOptions`。
#[derive(Debug, Clone)]
pub struct AgentRunStreamOptions {
    /// 默认 true：error 事件解析后转 `Error::AgentRunStream` 抛出。
    pub throw_on_error: bool,
}

impl Default for AgentRunStreamOptions {
    fn default() -> Self {
        Self {
            throw_on_error: true,
        }
    }
}

/// `run()` options（继承 stream options）。对应 TS `AgentRunRunOptions`。
pub type AgentRunRunOptions = AgentRunStreamOptions;

/// `run_with_local_tools()` options。对应 TS `AgentRunWithLocalToolsOptions`。
#[derive(Debug, Clone)]
pub struct AgentRunWithLocalToolsOptions {
    pub throw_on_error: bool,
    /// 本地工具回调硬超时（毫秒）；缺省 30_000。
    pub timeout_ms: u64,
}

impl Default for AgentRunWithLocalToolsOptions {
    fn default() -> Self {
        Self {
            throw_on_error: true,
            timeout_ms: 30_000,
        }
    }
}

/// SSE 事件 union（扁平判别符 enum，恰镜像 TS discriminated union）。对应 TS `AgentRunStreamEvent`。
///
/// 🔴 未知 type 在内部 SSE 流解析器中映射为 [`AgentRunStreamEvent::Error`]
/// （`code="unknown_event"`），**注入流不丢**。
#[derive(Debug, Clone)]
pub enum AgentRunStreamEvent {
    RunStarted {
        run_id: String,
        session_id: String,
    },
    Status {
        status: String,
        message: Option<String>,
    },
    TextDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        input: Option<Value>,
    },
    ToolResult {
        id: String,
        name: Option<String>,
        result: Option<Value>,
        error: Option<String>,
    },
    LocalToolRequest {
        request_id: String,
        name: String,
        input: Value,
    },
    Artifact {
        artifact: AgentRunArtifact,
    },
    Sources {
        sources: Value,
    },
    Usage {
        usage: AgentRunUsage,
    },
    Settle {
        settlement: AgentRunSettlement,
    },
    Error {
        error: AgentRunErrorPayload,
    },
    Done {
        run_id: String,
        status: String,
    },
}

impl AgentRunStreamEvent {
    /// 事件判别符（对齐 TS `event.type`）。
    pub fn type_str(&self) -> &'static str {
        match self {
            Self::RunStarted { .. } => "run_started",
            Self::Status { .. } => "status",
            Self::TextDelta { .. } => "text_delta",
            Self::ReasoningDelta { .. } => "reasoning_delta",
            Self::ToolCall { .. } => "tool_call",
            Self::ToolResult { .. } => "tool_result",
            Self::LocalToolRequest { .. } => "local_tool_request",
            Self::Artifact { .. } => "artifact",
            Self::Sources { .. } => "sources",
            Self::Usage { .. } => "usage",
            Self::Settle { .. } => "settle",
            Self::Error { .. } => "error",
            Self::Done { .. } => "done",
        }
    }
}
