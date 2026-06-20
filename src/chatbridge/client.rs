//! 第三方聊天 bridge 控制面客户端（Phase 7B）。端口自 `chatbridge/client.ts`
//! （getter 子客户端：TS `Object.defineProperty` + WeakMap → `fn chat_bridge(&self)`）。
//!
//! 端点：`/api/v4/chat-bridge/*`（nexus-v4 registerChatBridgeRoutes）。
//!
//! **双平面 wire（契约 §12）**：
//!   - **请求体字段 = snake_case**（app_id / workspace_id / config_json / secret_kind / new_plaintext ...）；
//!   - **响应 data = model 直接序列化的 camelCase**（appId / credentialRef / secretKind ...，
//!     与本域 [`types`](super::types) 逐字一致）。
//!     出站请求体经本模块手工映射 camelCase 入参 → snake_case wire。
//!
//! **安全红线（契约 §6 Secret zero-knowledge）**：
//!   - 任何端点不回 plaintext / ciphertext（model Ciphertext/ConfigJSON json:"-"）；
//!   - 创建/轮换的明文只在请求体出现一次，SDK 不缓存不打印（plaintext 入参只进 [`serde_json::json!`]
//!     请求体，永不存字段、永不 Display）；
//!   - 跨租户引用服务端一律 404（不暴露存在性）。

use super::types::{
    as_credential_ref, ChatCredentialPublic, ChatIntegration, IntegrationStatus, Platform, Region,
};
use crate::billing::entitlements::urlencoding;
use crate::core::client::Client;
use crate::shared::{ApiResponse, Result};
use serde::Deserialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;

/// `create_integration()` 请求（wire snake_case 由 SDK 转换）。对应 TS `CreateIntegrationRequest`。
#[derive(Debug, Clone)]
pub struct CreateIntegrationRequest {
    /// 绑定的 Acosmi app。
    pub app_id: String,
    pub platform: Platform,
    pub region: Region,
    /// 平台 workspace/企业 原始 ID；服务端只存 SHA256 hash。
    pub workspace_id: Option<String>,
    /// 平台 bot 原始 ID；服务端只存 SHA256 hash。
    pub bot_id: Option<String>,
    /// 仅非敏感配置（rate limit / feature toggles）；**严禁含 secret**。
    pub config_json: Option<String>,
}

/// `store_credential()` 请求 —— 明文一次性提交。对应 TS `StoreCredentialRequest`。
///
/// **安全**：`plaintext` 仅在出站请求体出现一次，加密落库后即弃；本结构体不被任何响应/缓存持有。
#[derive(Debug, Clone)]
pub struct StoreCredentialRequest {
    /// 凭证种类（平台相关，例：app_secret / signing_secret / bot_token）。
    pub secret_kind: String,
    /// 凭证明文；加密落库后即弃，响应只回 masked 视图。
    pub plaintext: String,
    pub region: Option<Region>,
    pub platform: Option<Platform>,
}

/// 列表端点包装（`{items?: [...]}`）。chatbridge GET 列表用。
#[derive(Debug, Deserialize)]
struct ItemsWrapper<T> {
    #[serde(default = "Vec::new")]
    items: Vec<T>,
}

/// SDK-facing 第三方聊天 bridge 控制面子客户端。对应 TS `ChatBridgeClient`。
///
/// 经 [`Client::chat_bridge`] getter 获取（无状态，持 [`Client`] clone）。
pub struct ChatBridgeClient {
    client: Client,
}

impl Client {
    /// 第三方聊天 bridge 控制面（Phase 7B；契约 §6/§12/ADR-8）。对应 TS `client.chatBridge` getter。
    ///
    /// 守卫（最小授权三档，chat_bridge 不进 all_scopes）：
    ///   - chat_bridge:read   → list_integrations / get_integration / list_credentials
    ///   - chat_bridge:write  → create_integration / update_integration_status / store_credential
    ///   - chat_bridge:rotate → rotate_credential / revoke_credential（高风险）
    pub fn chat_bridge(&self) -> ChatBridgeClient {
        ChatBridgeClient {
            client: self.clone(),
        }
    }
}

impl ChatBridgeClient {
    // ---------------------------------------------------------------------------
    // Integration 管理面
    // ---------------------------------------------------------------------------

    /// 创建平台集成（chat_bridge:write）。对应 TS `createIntegration`。
    ///
    /// 出站请求体手工映射 camelCase 入参 → **snake_case wire**（app_id / workspace_id / bot_id /
    /// config_json）。
    pub async fn create_integration(
        &self,
        req: &CreateIntegrationRequest,
        signal: Option<CancellationToken>,
    ) -> Result<ChatIntegration> {
        // 请求体 = snake_case（契约 §12 chatbridge 平面）。Option 为 None 时省略字段。
        let body_str = create_integration_wire_body(req).to_string();
        self.client
            .commerce_post::<ChatIntegration>("/chat-bridge/integrations", Some(&body_str), signal)
            .await
    }

    /// 列出本租户集成（chat_bridge:read；masked 视图）。可按 app_id 过滤。对应 TS `listIntegrations`。
    pub async fn list_integrations(
        &self,
        app_id: Option<&str>,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<ChatIntegration>> {
        let path = match app_id {
            Some(a) => format!("/chat-bridge/integrations?app_id={}", urlencoding(a)),
            None => "/chat-bridge/integrations".to_string(),
        };
        // data = `{items?: [...]}`（响应 camelCase）。
        let w: ItemsWrapper<ChatIntegration> = self.client.commerce_get(&path, signal).await?;
        Ok(w.items)
    }

    /// 取单个集成（chat_bridge:read）。不存在/跨租户一律 404。对应 TS `getIntegration`。
    pub async fn get_integration(
        &self,
        id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<ChatIntegration> {
        self.client
            .commerce_get(
                &format!("/chat-bridge/integrations/{}", urlencoding(id)),
                signal,
            )
            .await
    }

    /// 更新集成状态（chat_bridge:write）：pending | active | suspended | revoked。
    /// 对应 TS `updateIntegrationStatus`（PATCH，返回 void）。
    pub async fn update_integration_status(
        &self,
        id: &str,
        status: IntegrationStatus,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        let body = json!({ "status": status.as_str() }).to_string();
        // PATCH —— commerce helper 默认 POST/GET，这里直接走 do_json_full(PATCH)。
        let (env, _) = self
            .client
            .do_json_full::<ApiResponse<serde_json::Value>>(
                reqwest::Method::PATCH,
                &format!("/chat-bridge/integrations/{}/status", urlencoding(id)),
                Some(&body),
                signal,
            )
            .await?;
        if let Some(env) = env {
            if let Some(err) = env.business_error() {
                return Err(err);
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Credential 管理面（vault）
    // ---------------------------------------------------------------------------

    /// 存凭证（chat_bridge:write）—— 明文一次性提交，返回 masked 记录（ref+fingerprint）。
    /// 对应 TS `storeCredential`。
    ///
    /// 出站请求体 = **snake_case**（secret_kind / plaintext / region / platform）。
    /// **plaintext 仅进此处请求体一次**；响应恒为 masked 视图（无 plaintext/ciphertext）。
    pub async fn store_credential(
        &self,
        integration_id: &str,
        req: &StoreCredentialRequest,
        signal: Option<CancellationToken>,
    ) -> Result<ChatCredentialPublic> {
        let body_str = store_credential_wire_body(req).to_string();
        let cred: ChatCredentialPublic = self
            .client
            .commerce_post(
                &format!(
                    "/chat-bridge/integrations/{}/credentials",
                    urlencoding(integration_id)
                ),
                Some(&body_str),
                signal,
            )
            .await?;
        Ok(brand_credential(cred))
    }

    /// 列出集成的凭证（chat_bridge:read；masked，永不含明文/密文）。对应 TS `listCredentials`。
    pub async fn list_credentials(
        &self,
        integration_id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<ChatCredentialPublic>> {
        let w: ItemsWrapper<ChatCredentialPublic> = self
            .client
            .commerce_get(
                &format!(
                    "/chat-bridge/integrations/{}/credentials",
                    urlencoding(integration_id)
                ),
                signal,
            )
            .await?;
        Ok(w.items.into_iter().map(brand_credential).collect())
    }

    /// 轮换凭证（chat_bridge:rotate，高风险）—— ref 不变，fingerprint 更新。对应 TS `rotateCredential`。
    ///
    /// 出站请求体 = **snake_case**（secret_kind / new_plaintext）。new_plaintext 仅出现一次。
    pub async fn rotate_credential(
        &self,
        integration_id: &str,
        secret_kind: &str,
        new_plaintext: &str,
        signal: Option<CancellationToken>,
    ) -> Result<ChatCredentialPublic> {
        let body = json!({
            "secret_kind": secret_kind,
            "new_plaintext": new_plaintext,
        })
        .to_string();
        let cred: ChatCredentialPublic = self
            .client
            .commerce_post(
                &format!(
                    "/chat-bridge/integrations/{}/credentials/rotate",
                    urlencoding(integration_id)
                ),
                Some(&body),
                signal,
            )
            .await?;
        Ok(brand_credential(cred))
    }

    /// 吊销凭证（chat_bridge:rotate）—— 软吊销 + 服务端抹密文。对应 TS `revokeCredential`（返回 void）。
    pub async fn revoke_credential(
        &self,
        credential_ref: &str,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        self.client
            .commerce_post_discard(
                &format!(
                    "/chat-bridge/credentials/{}/revoke",
                    urlencoding(credential_ref)
                ),
                Some("{}"),
                signal,
            )
            .await
    }
}

/// 服务端返回的 credentialRef 是裸 string；出 SDK 边界前统一 brand 成 [`CredentialRef`]
/// （运行时恒等，只改类型识别），防止与 plaintext 字符串互窜。对应 TS `brandCredential`。
fn brand_credential(mut c: ChatCredentialPublic) -> ChatCredentialPublic {
    c.credential_ref = as_credential_ref(c.credential_ref.as_str());
    c
}

/// 构建 create_integration 出站请求体（camelCase 入参 → **snake_case wire**）。
fn create_integration_wire_body(req: &CreateIntegrationRequest) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    body.insert("app_id".into(), json!(req.app_id));
    body.insert("platform".into(), json!(req.platform.as_str()));
    body.insert("region".into(), json!(req.region.as_str()));
    if let Some(w) = &req.workspace_id {
        body.insert("workspace_id".into(), json!(w));
    }
    if let Some(b) = &req.bot_id {
        body.insert("bot_id".into(), json!(b));
    }
    if let Some(c) = &req.config_json {
        body.insert("config_json".into(), json!(c));
    }
    serde_json::Value::Object(body)
}

/// 构建 store_credential 出站请求体（camelCase 入参 → **snake_case wire**）。
/// plaintext 仅在此处进入请求体一次。
fn store_credential_wire_body(req: &StoreCredentialRequest) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    body.insert("secret_kind".into(), json!(req.secret_kind));
    body.insert("plaintext".into(), json!(req.plaintext));
    if let Some(r) = req.region {
        body.insert("region".into(), json!(r.as_str()));
    }
    if let Some(p) = req.platform {
        body.insert("platform".into(), json!(p.as_str()));
    }
    serde_json::Value::Object(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chatbridge::types::{Platform, Region};

    #[test]
    fn create_integration_request_is_snake_case_wire() {
        // 入参 camelCase → 请求体 snake_case（双平面，契约 §12）。
        let req = CreateIntegrationRequest {
            app_id: "app-1".into(),
            platform: Platform::Feishu,
            region: Region::Cn,
            workspace_id: Some("ws-raw".into()),
            bot_id: Some("bot-raw".into()),
            config_json: Some("{\"rate\":1}".into()),
        };
        let body = create_integration_wire_body(&req);
        assert_eq!(body["app_id"], "app-1");
        assert_eq!(body["workspace_id"], "ws-raw");
        assert_eq!(body["bot_id"], "bot-raw");
        assert_eq!(body["config_json"], "{\"rate\":1}");
        assert_eq!(body["platform"], "feishu");
        assert_eq!(body["region"], "cn");
        // 绝不出现 camelCase wire 键。
        assert!(body.get("appId").is_none());
        assert!(body.get("workspaceId").is_none());
        assert!(body.get("configJson").is_none());
    }

    #[test]
    fn store_credential_request_snake_case_plaintext_once() {
        let req = StoreCredentialRequest {
            secret_kind: "bot_token".into(),
            plaintext: "super-secret".into(),
            region: Some(Region::Intl),
            platform: Some(Platform::Slack),
        };
        let body = store_credential_wire_body(&req);
        assert_eq!(body["secret_kind"], "bot_token");
        // plaintext 出现且仅一次（snake_case 键）。
        assert_eq!(body["plaintext"], "super-secret");
        assert_eq!(body["region"], "intl");
        assert_eq!(body["platform"], "slack");
        assert!(body.get("secretKind").is_none());
        // 计数 plaintext 仅 1 个键。
        let count = body
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| k.contains("plaintext"))
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn optional_fields_omitted_when_none() {
        let req = CreateIntegrationRequest {
            app_id: "a".into(),
            platform: Platform::Wecom,
            region: Region::Cn,
            workspace_id: None,
            bot_id: None,
            config_json: None,
        };
        let body = create_integration_wire_body(&req);
        let obj = body.as_object().unwrap();
        assert!(!obj.contains_key("workspace_id"));
        assert!(!obj.contains_key("bot_id"));
        assert!(!obj.contains_key("config_json"));
    }
}
