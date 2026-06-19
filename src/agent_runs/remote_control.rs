//! Acosmi 远程控制 CrabCode 多接入面 — SDK 类型契约。端口自 `agent-runs/remote-control.ts`。
//!
//! 契约源: docs/audit/sdk-remote-control-contract-2026-05-27.md §3-§4 (Phase 0 frozen)。
//!
//! 设计纪律:
//!   - HTTP wire 字段 snake_case; 公开 API camelCase（此处 PascalCase enum + snake wire）;
//!   - [`parse_remote_control_event`] 负责 wire→TS 翻译，**不识别的 type 返回 None**
//!     （🔴 与 `types::AgentRunStreamEvent` 未知→Error 注入流**相反**，两套独立解析）;
//!   - 11 个 event type 严格对齐契约 §4，任何新增需先改契约文档。

use crate::macros::open_string_union;
use serde_json::Value;

// =============================================================================
// AdapterKind / RunnerKind（契约 §3 placement 矩阵）
// =============================================================================

open_string_union! {
    /// 接入适配器（契约 §3）。闭集，但保 open-union 以容忍上游扩展。
    AdapterKind {
        REMOTE_IO => "remote_io",
        APP_SERVER_TCP_WS => "app_server_tcp_ws",
        BRIDGE_CCR => "bridge_ccr",
        APP_SERVER_UDS => "app_server_uds",
        STDIO_STREAM_JSON => "stdio_stream_json",
        TAURI_MANAGED_APP_SERVER => "tauri_managed_app_server",
    }
}

open_string_union! {
    /// 运行位置（契约 §3）。
    RunnerKind {
        CLOUD => "cloud",
        DESKTOP => "desktop",
        LOCAL_EMBEDDED => "local_embedded",
    }
}

// =============================================================================
// Workspace / Permission Policy（与 Go side 字段名一致）
// =============================================================================

/// 工作区策略。对应 TS `WorkspacePolicy`。
#[derive(Debug, Clone, Default)]
pub struct WorkspacePolicy {
    pub read_only: Option<bool>,
    pub allowed_paths: Option<Vec<String>>,
    pub denied_paths: Option<Vec<String>>,
    pub max_bytes: Option<i64>,
}

/// 权限策略。对应 TS `PermissionPolicy`。
#[derive(Debug, Clone, Default)]
pub struct PermissionPolicy {
    pub shell_allowed: Option<bool>,
    pub shell_deny_list: Option<Vec<String>>,
    pub network_allowed: Option<bool>,
    pub write_allowed: Option<bool>,
    pub approval_timeout_ms: Option<i64>,
    pub required_actors: Option<Vec<String>>,
}

// =============================================================================
// Event union（契约 §4 — 11 种事件，严禁扩展）
// =============================================================================

/// 远控 SSE 事件 union（契约 §4 — 11 种）。对应 TS `RemoteControlEvent`。
#[derive(Debug, Clone)]
pub enum RemoteControlEvent {
    TextDelta {
        index: i64,
        text: String,
    },
    ReasoningDelta {
        index: i64,
        text: String,
    },
    ToolCall {
        tool_call_id: String,
        name: String,
        input: Option<Value>,
        source: Option<String>,
    },
    ToolResult {
        tool_call_id: String,
        ok: bool,
        output: Option<Value>,
        error: Option<String>,
    },
    PermissionRequest {
        request_id: String,
        kind: String,
        payload: Option<Value>,
        deadline_ms: Option<i64>,
    },
    PermissionResult {
        request_id: String,
        decision: String,
        actor: Option<String>,
        decided_at: Option<String>,
    },
    Usage {
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        cache_read: Option<i64>,
        cache_create: Option<i64>,
        exact: Option<bool>,
    },
    Settle {
        status: String,
        billed: Option<bool>,
    },
    Status {
        phase: String,
        message: Option<String>,
    },
    Error {
        code: String,
        message: String,
        retryable: Option<bool>,
        kind: Option<String>,
    },
    Done {
        reason: String,
        run_id: String,
        final_status: String,
    },
}

impl RemoteControlEvent {
    pub fn type_str(&self) -> &'static str {
        match self {
            Self::TextDelta { .. } => "text_delta",
            Self::ReasoningDelta { .. } => "reasoning_delta",
            Self::ToolCall { .. } => "tool_call",
            Self::ToolResult { .. } => "tool_result",
            Self::PermissionRequest { .. } => "permission_request",
            Self::PermissionResult { .. } => "permission_result",
            Self::Usage { .. } => "usage",
            Self::Settle { .. } => "settle",
            Self::Status { .. } => "status",
            Self::Error { .. } => "error",
            Self::Done { .. } => "done",
        }
    }
}

// =============================================================================
// Wire → TS 解析
// =============================================================================

fn as_record(v: &Value) -> Option<&serde_json::Map<String, Value>> {
    v.as_object()
}

fn str_field(rec: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    rec.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn num_field(rec: &serde_json::Map<String, Value>, key: &str) -> Option<i64> {
    rec.get(key).and_then(|v| {
        if let Some(i) = v.as_i64() {
            Some(i)
        } else {
            v.as_f64().filter(|f| f.is_finite()).map(|f| f as i64)
        }
    })
}

fn bool_field(rec: &serde_json::Map<String, Value>, key: &str) -> Option<bool> {
    rec.get(key).and_then(|v| v.as_bool())
}

/// 把后端 SSE/JSON wire payload 翻译成强类型 [`RemoteControlEvent`]。对应 TS `parseRemoteControlEvent`。
///
/// - wire 字段 snake_case（text_delta / tool_call_id / permission_request ...）;
/// - **未知 type / 缺失关键字段返回 `None`**（调用方应忽略 + warn）;
/// - 不抛异常: 一律返回 `None`。
///
/// 🔴 与 `types::parse_agent_run_event`（未知 type → Error 事件注入流）**相反** —— 两条
/// SSE 路径独立，不可统一。
pub fn parse_remote_control_event(raw: &Value) -> Option<RemoteControlEvent> {
    let rec = as_record(raw)?;
    let r#type = str_field(rec, "type")?;

    match r#type.as_str() {
        "text_delta" => {
            let index = num_field(rec, "index")?;
            let text = str_field(rec, "text")?;
            Some(RemoteControlEvent::TextDelta { index, text })
        }
        "reasoning_delta" => {
            let index = num_field(rec, "index")?;
            let text = str_field(rec, "text")?;
            Some(RemoteControlEvent::ReasoningDelta { index, text })
        }
        "tool_call" => {
            let tool_call_id =
                str_field(rec, "tool_call_id").or_else(|| str_field(rec, "toolCallId"))?;
            let name = str_field(rec, "name")?;
            Some(RemoteControlEvent::ToolCall {
                tool_call_id,
                name,
                input: rec.get("input").cloned(),
                source: str_field(rec, "source"),
            })
        }
        "tool_result" => {
            let tool_call_id =
                str_field(rec, "tool_call_id").or_else(|| str_field(rec, "toolCallId"))?;
            let ok = bool_field(rec, "ok")?;
            Some(RemoteControlEvent::ToolResult {
                tool_call_id,
                ok,
                output: rec.get("output").cloned(),
                error: str_field(rec, "error"),
            })
        }
        "permission_request" => {
            let request_id =
                str_field(rec, "request_id").or_else(|| str_field(rec, "requestId"))?;
            let kind = str_field(rec, "kind")?;
            Some(RemoteControlEvent::PermissionRequest {
                request_id,
                kind,
                payload: rec.get("payload").cloned(),
                deadline_ms: num_field(rec, "deadline_ms").or_else(|| num_field(rec, "deadlineMs")),
            })
        }
        "permission_result" => {
            let request_id =
                str_field(rec, "request_id").or_else(|| str_field(rec, "requestId"))?;
            let decision = str_field(rec, "decision")?;
            Some(RemoteControlEvent::PermissionResult {
                request_id,
                decision,
                actor: str_field(rec, "actor"),
                decided_at: str_field(rec, "decided_at").or_else(|| str_field(rec, "decidedAt")),
            })
        }
        "usage" => Some(RemoteControlEvent::Usage {
            input_tokens: num_field(rec, "input_tokens").or_else(|| num_field(rec, "inputTokens")),
            output_tokens: num_field(rec, "output_tokens")
                .or_else(|| num_field(rec, "outputTokens")),
            cache_read: num_field(rec, "cache_read").or_else(|| num_field(rec, "cacheRead")),
            cache_create: num_field(rec, "cache_create").or_else(|| num_field(rec, "cacheCreate")),
            exact: bool_field(rec, "exact"),
        }),
        "settle" => {
            let status = str_field(rec, "status")?;
            Some(RemoteControlEvent::Settle {
                status,
                billed: bool_field(rec, "billed"),
            })
        }
        "status" => {
            let phase = str_field(rec, "phase")?;
            Some(RemoteControlEvent::Status {
                phase,
                message: str_field(rec, "message"),
            })
        }
        "error" => {
            let code = str_field(rec, "code")?;
            let message = str_field(rec, "message")?;
            Some(RemoteControlEvent::Error {
                code,
                message,
                retryable: bool_field(rec, "retryable"),
                kind: str_field(rec, "kind"),
            })
        }
        "done" => {
            let reason = str_field(rec, "reason")?;
            let run_id = str_field(rec, "run_id").or_else(|| str_field(rec, "runId"))?;
            let final_status =
                str_field(rec, "final_status").or_else(|| str_field(rec, "finalStatus"))?;
            Some(RemoteControlEvent::Done {
                reason,
                run_id,
                final_status,
            })
        }
        // 🔴 未知 type → None 静默丢弃（与 AgentRunStreamEvent 未知→Error 相反）。
        _ => None,
    }
}

/// 仅 `done` 与 `settle` 终结一个 stream（契约 §4：`error` 非终结）。对应 TS `isTerminalRemoteEvent`。
pub fn is_terminal_remote_event(ev: &RemoteControlEvent) -> bool {
    matches!(
        ev,
        RemoteControlEvent::Done { .. } | RemoteControlEvent::Settle { .. }
    )
}

// =============================================================================
// 远控管理面请求/响应类型（契约 §18.1 附录 A; Phase 5B/5C）
// =============================================================================

/// C 端/平台可下达的 permission 决策（契约 §14）。仅 `approved` | `rejected`。
pub const REMOTE_PERMISSION_DECISION_APPROVED: &str = "approved";
pub const REMOTE_PERMISSION_DECISION_REJECTED: &str = "rejected";

/// `submit_permission_result()` 请求 —— 回写 permission_request 的决策。
#[derive(Debug, Clone, Default)]
pub struct RemotePermissionResultRequest {
    /// 对应 permission_request 事件的 requestId。
    pub request_id: String,
    /// `approved` | `rejected`（见上常量）。
    pub decision: String,
    /// 可选决策理由（透传给 CrabCode control_response）。
    pub reason: Option<String>,
}

/// `submit_user_message()` 请求 —— 会话中途追加用户消息（Phase 5C 输入框）。
#[derive(Debug, Clone, Default)]
pub struct RemoteUserMessageRequest {
    /// 消息正文; 服务端上限 64KB。Role 由服务端硬编码 'user'（契约 §6 #5 防注入）。
    pub content: String,
    /// 幂等键; 缺省由服务端生成并在 ack 中返回。
    pub request_id: Option<String>,
}

/// `submit_user_message()` 确认。
#[derive(Debug, Clone, Default)]
pub struct RemoteUserMessageAck {
    pub ok: bool,
    /// 服务端最终采用的幂等键。
    pub request_id: String,
}

/// `reveal_remote_token()` 响应 —— desktop launcher 一次性 session token（Phase 5B; 仅 runner='desktop'）。
///
/// 安全红线（契约 §6）: token 一次性消费（重复调用 409），TTL ≤ 1h; 永不落浏览器存储，
/// 应由 native 层接收后只注入 CrabCode 子进程 env。
#[derive(Debug, Clone, Default)]
pub struct RemoteSessionTokenGrant {
    pub access_token: String,
    /// CrabCode 回连的 RemoteIO WS 完整地址。
    pub session_url: String,
    pub tenant_id: String,
    /// 用户在 metadata.workspace 声明的期望项目目录（契约 §18.3 r4）; 缺省 None。
    pub workspace: Option<String>,
}
