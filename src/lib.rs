//! # acosmi-sdk
//!
//! Acosmi Rust SDK — 模型网关（双格式 Anthropic + OpenAI）、Agent Run Gateway 与
//! Compliance（电子证据、时间章、报告、签署 envelope）统一客户端。
//!
//! 端口自 [`acosmi-sdk-ts`](https://github.com/acosmi/sdk-ts) v2.8.0（事实标准主实现）。
//! 跨语言契约（snake_case wire-format / 符号名对齐 / bug-for-bug 行为）见父仓
//! `docs/jihua/sdk-rust-port-plan-2026-06-19.md`。
//!
//! ## 双格式红线
//!
//! `AnthropicAdapter` + `OpenAIAdapter` 等地位（对应两个不同下游产品），恒编译、不可降级。
//!
//! ## 运行时
//!
//! 仅原生 `tokio` + `reqwest`（rustls TLS）。流式走 `impl Stream`，取消走
//! `tokio_util::CancellationToken`。
//!
//! ## 模块（随分阶段端口逐步填充）
//!
//! 各业务域 module 是该域对外切片的单一真相源；`lib.rs` 经 `pub use` 对齐
//! `index.ts` 的逐域 re-export。

#![forbid(unsafe_code)]

/// SDK 版本（对齐 npm `@acosmi/sdk-ts` 主线 / `Cargo.toml` package.version）。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

mod macros;

// === 业务域 module（P1→P8 分阶段填充）===
// 已落地：shared（错误体系 + 跨域 DTO）/ core（http/retry/store/client 骨架）/ auth（types 前置）。
// 待加入：auth 全量(P2) / models(+adapters)(P3) / chat+SSE(P4) / billing/skills/
// notifications/agent_runs(P5) / compliance/support(P6) / 商品化(P7) / sanitize(P8)。
pub mod auth;
pub mod core;
pub mod shared;

// 逐域 re-export（对齐 index.ts 单一真源）。方法名 snake_case，类型名 PascalCase 保留跨语言锚点。
pub use crate::auth::{
    all_scopes, chat_bridge_scopes, code_challenge, commerce_scopes,
    complete_web_authorization_request, create_web_authorization_request, discover,
    discover_web_oauth_metadata, discover_with_profile, exchange_code, exchange_code_with_expiry,
    generate_code_verifier, generate_state, is_invalid_grant_error, is_ssl_error,
    is_valid_token_set, model_scopes, new_token_set, refresh_token, register,
    register_web_oauth_client, remote_control_scopes, resolve_success_redirect, revoke_token,
    skill_scopes, token_set_is_expired, AuthorizeResult, ClientRegistration,
    CreateWebAuthorizationRequestOptions, LoginEvent, LoginOptions, OAuthTokenEndpointError,
    RegisterWebOAuthClientOptions, ServerMetadata, TokenResponse, TokenSet,
    WebAuthorizationCallbackParams, WebAuthorizationPending, WebAuthorizationRequest,
};
pub use crate::core::{
    Client, Config, FileTokenStore, InMemoryTokenStore, TokenStore, DEFAULT_GATEWAY_BASE_URL,
};
pub use shared::{Error, Result};

#[cfg(test)]
mod scaffold_tests {
    use super::*;

    #[test]
    fn version_is_wired() {
        assert_eq!(VERSION, "2.8.0");
    }
}
