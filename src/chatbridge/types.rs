//! Acosmi 第三方聊天 bridge 控制面 —— SDK 类型契约（Phase 7）。
//!
//! 契约源：`docs/audit/sdk-remote-control-contract-2026-05-27.md` §6（Secret zero-knowledge）
//! + §7 + §12 + ADR-8。
//!
//! 设计纪律（安全红线）：
//!   - chatbridge 资源 **响应 data = camelCase**（与 model/chat_bridge.go json tag 逐字一致）；
//!     **请求体 = snake_case**（handler ShouldBindJSON tag），两平面在 [`client`](super::client) 手工映射；
//!   - 平台 secret（plaintext / token / signing key）**永不出现在 SDK 公共导出**；
//!     SDK 只见 read-only metadata + [`CredentialRef`] + fingerprint + ChannelEvents；
//!   - 7 个平台严格枚举（闭 union）；
//!   - 平台原始 thread / sender / workspace / bot ID 一律 **hash 字段**入 SDK，永不持原值。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =============================================================================
// Platform / Region / IntegrationStatus（闭 union → enum + serde rename）
// =============================================================================

/// 平台（闭 union，与 Go `service/chatbridge/types.go` 对齐）。TS `Platform`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Platform {
    #[serde(rename = "feishu")]
    Feishu,
    #[serde(rename = "wecom")]
    Wecom,
    #[serde(rename = "dingtalk")]
    Dingtalk,
    #[serde(rename = "slack")]
    Slack,
    #[serde(rename = "teams")]
    Teams,
    #[serde(rename = "telegram")]
    Telegram,
    #[serde(rename = "whatsapp")]
    Whatsapp,
}

impl Platform {
    /// wire 字符串值。
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Feishu => "feishu",
            Platform::Wecom => "wecom",
            Platform::Dingtalk => "dingtalk",
            Platform::Slack => "slack",
            Platform::Teams => "teams",
            Platform::Telegram => "telegram",
            Platform::Whatsapp => "whatsapp",
        }
    }
}

/// 区域（闭 union）。TS `Region`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Region {
    #[serde(rename = "cn")]
    Cn,
    #[serde(rename = "intl")]
    Intl,
}

impl Region {
    pub fn as_str(self) -> &'static str {
        match self {
            Region::Cn => "cn",
            Region::Intl => "intl",
        }
    }
}

/// 集成状态（闭 union）。TS `IntegrationStatus`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntegrationStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "suspended")]
    Suspended,
    #[serde(rename = "revoked")]
    Revoked,
}

impl IntegrationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            IntegrationStatus::Pending => "pending",
            IntegrationStatus::Active => "active",
            IntegrationStatus::Suspended => "suspended",
            IntegrationStatus::Revoked => "revoked",
        }
    }
}

// =============================================================================
// CredentialRef newtype（branded type → #[serde(transparent)] 透明 String 新类型）
// =============================================================================

/// CredentialRef —— chat-bridge 凭证公开引用（`cred_<22 char base32>`）。
///
/// 透明 newtype 防止 plaintext secret 字符串被误传到需要 CredentialRef 的位置。
/// SDK 调用方应仅持有 `CredentialRef`，**永不持 plaintext**。
///
/// **安全**：本类型只承载公开 ref（非 secret）；构造经 [`as_credential_ref`]，不做合法性校验
/// （bug-for-bug，与 TS `asCredentialRef` 一致）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CredentialRef(String);

impl CredentialRef {
    /// 借出底层 ref 字符串（公开值，非 secret）。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CredentialRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 把任意字符串安全包装成 [`CredentialRef`]；不做合法性校验（调用方自判 `cred_` 前缀）。
/// 对应 TS `asCredentialRef`。
pub fn as_credential_ref(s: impl Into<String>) -> CredentialRef {
    CredentialRef(s.into())
}

// =============================================================================
// Channel event 公共类型（read-only events；secret 永不出现）
// =============================================================================

/// 入站附件 / 出站文件附件（ADR-8：`url` 必须是 Acosmi 内部存储 URL，非平台原始签名 URL）。
/// TS `ChannelAttachment`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelAttachment {
    pub kind: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    #[serde(
        rename = "contentType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub content_type: Option<String>,
}

/// 出站交互卡片按钮 / 自由输入。TS `ChannelCardAction`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelCardAction {
    /// 'approve' | 'reject' | 'cancel' | 'free_text' | 其他 bridge 自定义。
    pub kind: String,
    /// bridge 侧稳定 action id（映射回原 requestId 幂等键）。
    pub id: String,
    pub label: String,
}

/// 出站交互卡片（permission / tool_status / done / error）。TS `ChannelCard`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelCard {
    pub kind: String,
    pub title: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<ChannelCardAction>>,
}

/// bridge runtime 视角的入站平台消息。TS `ChannelInboundEvent`。
///
/// 安全约束：`thread_hash` / `sender_hash` 是 SHA256(平台原始 ID)，不允许直接是平台 ID。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelInboundEvent {
    pub platform: Platform,
    #[serde(rename = "threadHash")]
    pub thread_hash: String,
    #[serde(
        rename = "senderHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sender_hash: Option<String>,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<ChannelAttachment>>,
    #[serde(rename = "messageId", default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// ISO-8601。
    #[serde(
        rename = "receivedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub received_at: Option<String>,
    /// 仅含非敏感字段（locale, message_kind），严禁含 plaintext secret。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
}

/// bridge runtime 出站平台消息 / 卡片。TS `ChannelOutboundEvent`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelOutboundEvent {
    #[serde(rename = "threadHash")]
    pub thread_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cards: Option<Vec<ChannelCard>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
}

/// bridge runtime 内部 thread 引用（Acosmi 三元组）。永不携带 platform secret / 原始平台 ID。
/// TS `BridgeThreadRef`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeThreadRef {
    #[serde(rename = "threadId")]
    pub thread_id: String,
    #[serde(rename = "runId", default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(
        rename = "remoteSessionId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub remote_session_id: Option<String>,
    #[serde(rename = "tenantId")]
    pub tenant_id: String,
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "appId")]
    pub app_id: String,
}

// =============================================================================
// Public 只读视图（admin/管理后台 GET 用；严禁含 Ciphertext / Plaintext）
// =============================================================================

/// 平台安装记录的 SDK 只读视图（响应 camelCase，契约 §12）。TS `ChatIntegration`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatIntegration {
    pub id: String,
    #[serde(rename = "tenantId")]
    pub tenant_id: String,
    #[serde(rename = "appId")]
    pub app_id: String,
    pub platform: Platform,
    pub region: Region,
    #[serde(
        rename = "workspaceIdHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub workspace_id_hash: Option<String>,
    #[serde(rename = "botIdHash", default, skip_serializing_if = "Option::is_none")]
    pub bot_id_hash: Option<String>,
    pub status: IntegrationStatus,
    /// **@deprecated** 服务端从不返回此字段（model ConfigJSON json:"-"，防 secret 误入后整体
    /// 不外发）—— 读取恒为 `None`。写入走 `create_integration({config_json})`。
    #[serde(
        rename = "configJson",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub config_json: Option<String>,
    #[serde(
        rename = "installedByUserId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub installed_by_user_id: Option<String>,
    #[serde(
        rename = "lastUsedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub last_used_at: Option<String>,
    #[serde(rename = "createdAt", default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(rename = "updatedAt", default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// 凭证的 SDK 只读视图（**绝不含 ciphertext / plaintext**）。TS `ChatCredentialPublic`。
///
/// 仅暴露公开字段：`credential_ref` / `fingerprint` / `key_id` / `version` / `status`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCredentialPublic {
    #[serde(rename = "credentialRef")]
    pub credential_ref: CredentialRef,
    #[serde(rename = "integrationId")]
    pub integration_id: String,
    pub platform: Platform,
    pub region: Region,
    #[serde(rename = "secretKind")]
    pub secret_kind: String,
    pub fingerprint: String,
    #[serde(rename = "keyId")]
    pub key_id: String,
    pub version: i64,
    /// 开放 union（'active'|'rotating'|'revoked'|任意 string 兜底）。
    pub status: String,
    #[serde(
        rename = "lastUsedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub last_used_at: Option<String>,
    #[serde(rename = "rotatedAt", default, skip_serializing_if = "Option::is_none")]
    pub rotated_at: Option<String>,
    #[serde(rename = "createdAt", default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(rename = "updatedAt", default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// 平台 thread 到 Acosmi session 的映射（SDK 只读视图）。永不携带原始 thread/sender ID。
/// TS `ChatThread`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatThread {
    pub id: String,
    #[serde(rename = "tenantId")]
    pub tenant_id: String,
    #[serde(rename = "integrationId")]
    pub integration_id: String,
    #[serde(rename = "platformThreadHash")]
    pub platform_thread_hash: String,
    pub platform: Platform,
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "appId")]
    pub app_id: String,
    #[serde(rename = "sessionId", default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(rename = "lastRunId", default, skip_serializing_if = "Option::is_none")]
    pub last_run_id: Option<String>,
    #[serde(
        rename = "senderHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sender_hash: Option<String>,
    #[serde(
        rename = "lastInboundAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub last_inbound_at: Option<String>,
    #[serde(
        rename = "lastOutboundAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub last_outbound_at: Option<String>,
}

/// 一次 bridge runtime 会话（SDK 只读视图）。TS `ChatBridgeSession`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatBridgeSession {
    pub id: String,
    #[serde(rename = "tenantId")]
    pub tenant_id: String,
    #[serde(rename = "threadId")]
    pub thread_id: String,
    #[serde(rename = "integrationId")]
    pub integration_id: String,
    #[serde(rename = "runId", default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(
        rename = "remoteSessionId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub remote_session_id: Option<String>,
    /// 开放 union（'created'|'routing'|'active'|'paused'|'closed'|'errored'|任意 string）。
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,
    #[serde(
        rename = "lastFrameAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub last_frame_at: Option<String>,
    #[serde(rename = "closedAt", default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<String>,
    #[serde(
        rename = "disconnectReason",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub disconnect_reason: Option<String>,
    #[serde(rename = "createdAt", default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(rename = "updatedAt", default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

// =============================================================================
// 平台 / 区域 / 状态 常量集合 + runtime type guards
// =============================================================================

/// 全部平台（运行时校验用）。对应 TS `ALL_PLATFORMS`。
pub const ALL_PLATFORMS: &[Platform] = &[
    Platform::Feishu,
    Platform::Wecom,
    Platform::Dingtalk,
    Platform::Slack,
    Platform::Teams,
    Platform::Telegram,
    Platform::Whatsapp,
];

/// 全部区域。对应 TS `ALL_REGIONS`。
pub const ALL_REGIONS: &[Region] = &[Region::Cn, Region::Intl];

/// 全部集成状态。对应 TS `ALL_INTEGRATION_STATUS`。
pub const ALL_INTEGRATION_STATUS: &[IntegrationStatus] = &[
    IntegrationStatus::Pending,
    IntegrationStatus::Active,
    IntegrationStatus::Suspended,
    IntegrationStatus::Revoked,
];

/// runtime type guard：字符串是否合法 [`Platform`]。对应 TS `isPlatform`。
pub fn is_platform(v: &str) -> bool {
    ALL_PLATFORMS.iter().any(|p| p.as_str() == v)
}

/// runtime type guard：字符串是否合法 [`Region`]。对应 TS `isRegion`。
pub fn is_region(v: &str) -> bool {
    ALL_REGIONS.iter().any(|r| r.as_str() == v)
}

/// runtime type guard：字符串是否合法 [`IntegrationStatus`]。对应 TS `isIntegrationStatus`。
pub fn is_integration_status(v: &str) -> bool {
    ALL_INTEGRATION_STATUS.iter().any(|s| s.as_str() == v)
}

/// 极简 type guard：任意 JSON 是否为合法入站事件（仅校验 platform/threadHash/content）。
/// 对应 TS `isChannelInboundEvent`（失败返 false，不抛）。
pub fn is_channel_inbound_event(v: &serde_json::Value) -> bool {
    let obj = match v.as_object() {
        Some(o) => o,
        None => return false,
    };
    let platform_ok = obj
        .get("platform")
        .and_then(|p| p.as_str())
        .map(is_platform)
        .unwrap_or(false);
    let thread_hash_ok = obj
        .get("threadHash")
        .and_then(|t| t.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    let content_ok = obj.get("content").map(|c| c.is_string()).unwrap_or(false);
    platform_ok && thread_hash_ok && content_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_ref_is_transparent_newtype() {
        // #[serde(transparent)]：序列化为裸字符串（非 {"0":"..."} 包装）。
        let r = as_credential_ref("cred_abc123");
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v, serde_json::Value::String("cred_abc123".into()));
        // 反序列化也是裸字符串。
        let back: CredentialRef = serde_json::from_str("\"cred_xyz\"").unwrap();
        assert_eq!(back.as_str(), "cred_xyz");
    }

    #[test]
    fn chat_credential_public_response_is_camel_case() {
        // 响应 data = camelCase（contract §12 chatbridge 平面）；credentialRef → CredentialRef。
        let c: ChatCredentialPublic = serde_json::from_str(
            r#"{"credentialRef":"cred_aaa","integrationId":"int1","platform":"slack",
                "region":"intl","secretKind":"bot_token","fingerprint":"fp","keyId":"k1",
                "version":2,"status":"active"}"#,
        )
        .unwrap();
        assert_eq!(c.credential_ref.as_str(), "cred_aaa");
        assert_eq!(c.platform, Platform::Slack);
        assert_eq!(c.region, Region::Intl);
        assert_eq!(c.secret_kind, "bot_token");
        // 安全：结构体无任何 plaintext/ciphertext 字段（编译期保证；此处只断公开字段）。
        assert_eq!(c.version, 2);
    }

    #[test]
    fn integration_response_no_config_json_when_absent() {
        // configJson @deprecated 服务端从不返回 → 恒 None。
        let i: ChatIntegration = serde_json::from_str(
            r#"{"id":"int1","tenantId":"t1","appId":"a1","platform":"feishu",
                "region":"cn","status":"active"}"#,
        )
        .unwrap();
        assert!(i.config_json.is_none());
        assert_eq!(i.platform, Platform::Feishu);
    }

    #[test]
    fn type_guards_validate_strings() {
        assert!(is_platform("slack"));
        assert!(!is_platform("discord"));
        assert!(is_region("cn"));
        assert!(!is_region("us"));
        assert!(is_integration_status("revoked"));
        assert!(!is_integration_status("deleted"));
    }

    #[test]
    fn is_channel_inbound_event_checks_minimal_fields() {
        let ok = serde_json::json!({"platform":"slack","threadHash":"h","content":"hi"});
        assert!(is_channel_inbound_event(&ok));
        // 缺 content。
        let bad = serde_json::json!({"platform":"slack","threadHash":"h"});
        assert!(!is_channel_inbound_event(&bad));
        // 非法平台。
        let bad2 = serde_json::json!({"platform":"x","threadHash":"h","content":"hi"});
        assert!(!is_channel_inbound_event(&bad2));
        // 空 threadHash。
        let bad3 = serde_json::json!({"platform":"slack","threadHash":"","content":"hi"});
        assert!(!is_channel_inbound_event(&bad3));
    }
}
