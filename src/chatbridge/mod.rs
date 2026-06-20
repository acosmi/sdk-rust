//! Chat Bridge 域（Phase 7B）：第三方聊天平台集成 + 凭证 vault 控制面。
//!
//! 对齐 `chatbridge/index.ts`。暴露 read-only metadata + ChannelEvent types + 7 平台/区域/状态
//! 枚举与 type guards + [`ChatBridgeClient`]（integration/credential 管理面 CRUD；经
//! [`crate::Client::chat_bridge`] getter 子客户端访问）。
//!
//! **安全红线（secret 零导出）**：平台 secret（plaintext / token / signing key）永不出现在本
//! barrel 导出 —— `store_credential` / `rotate_credential` 的明文只进请求体一次（仅
//! [`StoreCredentialRequest::plaintext`] 等**写入侧入参**字段承载，无任何响应/元数据类型携带
//! plaintext/ciphertext）；响应恒为 masked 视图（[`ChatCredentialPublic`] 只含 ref+fingerprint）。
//! [`CredentialRef`] 是 `#[serde(transparent)]` 透明 newtype，承载公开 ref 而非 secret。

pub mod client;
pub mod types;

// === 类型契约（read-only metadata + ChannelEvents + 枚举 + type guards）===
// 注：不导出任何承载 secret 的响应/元数据类型；plaintext 仅在写入侧入参结构体出现。
pub use types::{
    as_credential_ref, is_channel_inbound_event, is_integration_status, is_platform, is_region,
    BridgeThreadRef, ChannelAttachment, ChannelCard, ChannelCardAction, ChannelInboundEvent,
    ChannelOutboundEvent, ChatBridgeSession, ChatCredentialPublic, ChatIntegration, ChatThread,
    CredentialRef, IntegrationStatus, Platform, Region, ALL_INTEGRATION_STATUS, ALL_PLATFORMS,
    ALL_REGIONS,
};

// === 子客户端 + 写入侧请求入参（plaintext 一次性提交，不缓存不导出为元数据）===
pub use client::{ChatBridgeClient, CreateIntegrationRequest, StoreCredentialRequest};
