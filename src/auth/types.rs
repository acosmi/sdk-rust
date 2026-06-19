//! 鉴权 / 身份域类型。端口自 `auth/types.ts`。
//!
//! 相位说明：P1 仅前置 `TokenSet` 及其形状校验（`core::store` / `core::client` 依赖）。
//! `token_set_is_expired`（需 ISO 8601 日期解析 + 30s 偏移）及 OAuth 流程其余部分待 P2。

use serde::{Deserialize, Serialize};

/// OAuth Authorization Server 元数据（RFC 8414）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerMetadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub revocation_endpoint: String,
    pub registration_endpoint: String,
    pub scopes_supported: Vec<String>,
}

/// OAuth token 响应。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// 持久化 token 对。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: String,
    /// ISO 8601 格式。
    pub expires_at: String,
    pub scope: String,
    pub client_id: String,
    pub server_url: String,
}

/// 动态注册响应。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientRegistration {
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
}

/// 运行时校验任意 JSON 值是否为合法 `TokenSet` 形状（所有 6 字段都是 string）。
///
/// 对应 TS `isValidTokenSet`。serde 反序列化到非可选 `String` 字段天然等价该校验：
/// 缺字段 / 类型错 → `Err` → 视为无 token。
pub fn is_valid_token_set(x: &serde_json::Value) -> bool {
    serde_json::from_value::<TokenSet>(x.clone()).is_ok()
}
