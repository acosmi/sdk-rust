//! CrabCode 远控 BYO 模型密钥管理面（契约 §18.2）。端口自 `agent-runs/byok.ts`。
//!
//! 端点: /api/v4/crabcode/byok-credentials（wire snake_case; remote-control 平面）。
//! 守卫: AuthOrDesktopScope("remote_control")。
//!
//! 安全红线（契约 §6 附录 A）:
//!   - 任何端点不回 plaintext / ciphertext; 列表一律 masked 视图;
//!   - credentialRef 是唯一可持有引用 —— 传给 `create_remote_run({ byok_credential_ref })`（仅 runner='cloud'）;
//!   - SDK 不缓存、不打印明文。

use crate::core::client::Client;
use crate::shared::{ApiResponse, Result};
use serde::Deserialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;

/// 允许的第三方提供商（与网关 byokAllowedProviders 对齐）。常量集合（open union）。
pub const BYOK_PROVIDER_ANTHROPIC: &str = "anthropic";
pub const BYOK_PROVIDER_OPENAI: &str = "openai";
pub const BYOK_PROVIDER_DEEPSEEK: &str = "deepseek";
pub const BYOK_PROVIDER_DASHSCOPE: &str = "dashscope";
pub const BYOK_PROVIDER_ZHIPU: &str = "zhipu";
pub const BYOK_PROVIDER_VOLCENGINE: &str = "volcengine";
pub const BYOK_PROVIDER_CUSTOM: &str = "custom";

/// BYOK 密钥状态。
pub const BYOK_STATUS_ACTIVE: &str = "active";
pub const BYOK_STATUS_REVOKED: &str = "revoked";

/// BYOK 密钥 masked 视图 —— 永不含明文/密文。对应 TS `ByokCredential`。
#[derive(Debug, Clone, Default)]
pub struct ByokCredential {
    pub credential_ref: String,
    pub provider: String,
    pub name: Option<String>,
    /// 仅 provider='custom' 时存在（https:// 起始）。
    pub base_url: Option<String>,
    /// 明文指纹（轮换后变化）。
    pub fingerprint: Option<String>,
    pub status: String,
    pub created_at: Option<String>,
    pub last_used_at: Option<String>,
}

/// `create()` 请求。明文一次性提交，服务端加密落库后即弃。对应 TS `ByokCreateRequest`。
#[derive(Debug, Clone, Default)]
pub struct ByokCreateRequest {
    pub provider: String,
    /// API key 明文; ≤4KB，不得含换行。
    pub plaintext: String,
    /// 显示名; ≤100 字符。
    pub name: Option<String>,
    /// 仅 provider='custom' 必填（必须 https://）。
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct WireByokCredential {
    #[serde(default)]
    credential_ref: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    fingerprint: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    last_used_at: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct WireByokList {
    #[serde(default)]
    items: Option<Vec<WireByokCredential>>,
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.is_empty())
}

fn from_wire_byok(v: WireByokCredential) -> ByokCredential {
    ByokCredential {
        credential_ref: v.credential_ref.unwrap_or_default(),
        provider: v.provider.unwrap_or_default(),
        name: non_empty(v.name),
        base_url: non_empty(v.base_url),
        fingerprint: non_empty(v.fingerprint),
        status: v.status.unwrap_or_default(),
        created_at: non_empty(v.created_at),
        last_used_at: non_empty(v.last_used_at),
    }
}

/// CrabCode 远控 BYO 模型密钥管理面子客户端。对应 TS `CrabCodeByokClient`。
///
/// 经 [`Client::crabcode_byok`] getter 获取（无状态，持 [`Client`] clone）。
pub struct CrabCodeByokClient {
    client: Client,
}

impl CrabCodeByokClient {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// 列出调用者自己的密钥（masked; 新→旧，服务端上限 100 条）。对应 TS `list`。
    pub async fn list(&self, signal: Option<CancellationToken>) -> Result<Vec<ByokCredential>> {
        let env: ApiResponse<WireByokList> = self
            .client
            .agent_runs_request_api("GET", "/crabcode/byok-credentials", None, signal, true)
            .await?;
        Ok(env
            .data
            .items
            .unwrap_or_default()
            .into_iter()
            .map(from_wire_byok)
            .collect())
    }

    /// 创建密钥 —— 明文一次性提交，返回 masked 视图。对应 TS `create`。
    pub async fn create(
        &self,
        req: &ByokCreateRequest,
        signal: Option<CancellationToken>,
    ) -> Result<ByokCredential> {
        let body = json!({
            "provider": req.provider,
            "name": req.name,
            "base_url": req.base_url,
            "plaintext": req.plaintext,
        })
        .to_string();
        let env: ApiResponse<WireByokCredential> = self
            .client
            .agent_runs_request_api(
                "POST",
                "/crabcode/byok-credentials",
                Some(&body),
                signal,
                false,
            )
            .await?;
        Ok(from_wire_byok(env.data))
    }

    /// 轮换密钥明文 —— credentialRef 不变，fingerprint 更新。对应 TS `rotate`。
    pub async fn rotate(
        &self,
        credential_ref: &str,
        new_plaintext: &str,
        signal: Option<CancellationToken>,
    ) -> Result<ByokCredential> {
        let path = format!(
            "/crabcode/byok-credentials/{}/rotate",
            crate::billing::entitlements::urlencoding(credential_ref)
        );
        let body = json!({ "new_plaintext": new_plaintext }).to_string();
        let env: ApiResponse<WireByokCredential> = self
            .client
            .agent_runs_request_api("POST", &path, Some(&body), signal, false)
            .await?;
        Ok(from_wire_byok(env.data))
    }

    /// 吊销密钥（软状态 + 服务端抹密文，不可恢复; 幂等）。对应 TS `revoke`。
    pub async fn revoke(
        &self,
        credential_ref: &str,
        signal: Option<CancellationToken>,
    ) -> Result<ByokCredential> {
        let path = format!(
            "/crabcode/byok-credentials/{}/revoke",
            crate::billing::entitlements::urlencoding(credential_ref)
        );
        let env: ApiResponse<WireByokCredential> = self
            .client
            .agent_runs_request_api("POST", &path, Some("{}"), signal, false)
            .await?;
        Ok(from_wire_byok(env.data))
    }
}

impl Client {
    /// CrabCode 远控 BYO 模型密钥管理面（契约 §18.2）。对应 TS `client.crabcodeByok` getter。
    pub fn crabcode_byok(&self) -> CrabCodeByokClient {
        CrabCodeByokClient::new(self.clone())
    }
}
