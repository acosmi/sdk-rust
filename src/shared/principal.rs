//! 跨域统一身份 / 租户 / API client 引用原语。端口自 `shared/principal.ts`。
//!
//! 只沉淀轻量引用（`*Ref`）；完整 tenant / iam 视图待后续命名空间。

use serde::{Deserialize, Serialize};

/// 租户轻量引用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantRef {
    #[serde(rename = "tenantId")]
    pub tenant_id: String,
    /// 展示名；后端可省略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Principal（操作主体）轻量引用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrincipalRef {
    #[serde(rename = "principalId")]
    pub principal_id: String,
    /// 关联用户 id；与 `principal_id` 可能不同。
    #[serde(rename = "userId", default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// 所属租户 id。
    #[serde(rename = "tenantId", default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    /// 展示名；后端可省略。
    #[serde(
        rename = "displayName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub display_name: Option<String>,
}

/// API client 轻量引用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiClientRef {
    #[serde(rename = "clientId")]
    pub client_id: String,
    /// 展示名；后端可省略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}
