//! 鉴权 / 身份域类型。端口自 `auth/types.ts`。
//!
//! P1 前置 `TokenSet` 及其形状校验（`core::store` / `core::client` 依赖）；
//! P2 补 `token_set_is_expired`（ISO 8601 日期解析 + 30s 偏移）。

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

/// token 是否已过期（提前 30 秒视为过期）。对应 TS `tokenSetIsExpired`。
///
/// bug-for-bug：非法 / 空 `expires_at`（空串 / `"not-a-date"` 等无法解析为 ISO 8601）
/// 视为**已过期**（返 `true`），触发 refresh / 重登 —— 而非把坏 token 当作"未过期"长期复用。
///
/// `newTokenSet` 始终产出 `toISOString()`（UTC `Z` 后缀），故解析按 RFC3339 / ISO 8601。
pub fn token_set_is_expired(t: &TokenSet) -> bool {
    use chrono::{DateTime, Utc};
    match DateTime::parse_from_rfc3339(&t.expires_at) {
        Ok(expires_at) => {
            // 提前 30 秒视为过期（与 TS `Date.now() > expiresAt - 30_000` 一致）。
            Utc::now() > expires_at.with_timezone(&Utc) - chrono::Duration::seconds(30)
        }
        // 非法 / 空 expires_at → 视为已过期。
        Err(_) => true,
    }
}

/// 运行时校验任意 JSON 值是否为合法 `TokenSet` 形状（所有 6 字段都是 string）。
///
/// 对应 TS `isValidTokenSet`。serde 反序列化到非可选 `String` 字段天然等价该校验：
/// 缺字段 / 类型错 → `Err` → 视为无 token。
pub fn is_valid_token_set(x: &serde_json::Value) -> bool {
    serde_json::from_value::<TokenSet>(x.clone()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn token_with_expiry(expires_at: &str) -> TokenSet {
        TokenSet {
            access_token: "at".into(),
            refresh_token: "rt".into(),
            expires_at: expires_at.into(),
            scope: "ai".into(),
            client_id: "cid".into(),
            server_url: "https://acosmi.com".into(),
        }
    }

    #[test]
    fn expired_when_in_the_past() {
        let past = (Utc::now() - Duration::hours(1)).to_rfc3339();
        assert!(token_set_is_expired(&token_with_expiry(&past)));
    }

    #[test]
    fn not_expired_when_well_in_the_future() {
        let future = (Utc::now() + Duration::hours(1)).to_rfc3339();
        assert!(!token_set_is_expired(&token_with_expiry(&future)));
    }

    #[test]
    fn expired_within_30s_skew_window() {
        // 距过期仅 10s（<30s 提前量）→ 视为已过期。
        let soon = (Utc::now() + Duration::seconds(10)).to_rfc3339();
        assert!(token_set_is_expired(&token_with_expiry(&soon)));
    }

    #[test]
    fn not_expired_just_outside_30s_skew_window() {
        // 距过期 60s（>30s 提前量）→ 未过期。
        let later = (Utc::now() + Duration::seconds(60)).to_rfc3339();
        assert!(!token_set_is_expired(&token_with_expiry(&later)));
    }

    #[test]
    fn illegal_expires_at_treated_as_expired() {
        assert!(token_set_is_expired(&token_with_expiry("")));
        assert!(token_set_is_expired(&token_with_expiry("not-a-date")));
        // 'Z' 后缀 UTC（newTokenSet 的实际产出格式）能解析。
        let z = "2099-01-01T00:00:00Z";
        assert!(!token_set_is_expired(&token_with_expiry(z)));
    }
}
