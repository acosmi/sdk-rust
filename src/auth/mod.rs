//! 鉴权 / 身份域。端口自 `auth/`。
//!
//! P1 含 `types`（`TokenSet` 等基础类型，供 core 依赖）；
//! P2 补 OAuth 全流程（`auth`：discover / register / PKCE / exchange / refresh / revoke /
//! Web OAuth 原语 / loopback authorize）+ `scopes`（scope 常量 + 组合函数）+
//! `types::token_set_is_expired`。

// `auth/auth.rs` 镜像 TS `auth/auth.ts` 文件结构（方案 §2：`src/auth/{auth,scopes,types}.rs`），
// 跨语言符号对照锚点；module inception 是刻意的命名对齐，非组织失误。
#[allow(clippy::module_inception)]
pub mod auth;
pub mod scopes;
pub mod types;

// === 类型（对齐 types.ts barrel）===
pub use types::{
    is_valid_token_set, token_set_is_expired, ClientRegistration, ServerMetadata, TokenResponse,
    TokenSet,
};

// === Auth helpers（对齐 auth/index.ts barrel；含 TS barrel 漏掉的 exchange_code_with_expiry）===
pub use auth::{
    code_challenge,
    complete_web_authorization_request,
    create_web_authorization_request,
    discover,
    discover_web_oauth_metadata,
    discover_with_profile,
    exchange_code,
    exchange_code_with_expiry,
    generate_code_verifier,
    generate_state,
    is_invalid_grant_error,
    is_ssl_error,
    new_token_set,
    refresh_token,
    register,
    register_web_oauth_client,
    resolve_success_redirect,
    revoke_token,
    AuthorizeResult,
    CreateWebAuthorizationRequestOptions,
    LoginEvent,
    LoginOptions,
    OAuthTokenEndpointError,
    RegisterWebOAuthClientOptions,
    WebAuthorizationCallbackParams,
    WebAuthorizationPending,
    WebAuthorizationRequest,
    ERR_AUTH_DENIED,
    ERR_BROWSER_OPEN,
    ERR_DISCOVERY,
    ERR_REGISTRATION,
    ERR_SSL_PROXY,
    ERR_STATE_MISMATCH,
    ERR_TIMEOUT,
    ERR_TOKEN_EXCHANGE,
    // OAuthMetadataProfile (auth 版：discover profile) 与 client::OAuthMetadataProfile 同名不同型，
    // 经 module 路径区分（auth::auth::OAuthMetadataProfile），不在此 re-export 以避免顶层歧义。
    // 事件 / 错误码常量。
    EVENT_AUTH_URL,
    EVENT_COMPLETE,
    EVENT_ERROR,
};

#[cfg(feature = "desktop-loopback")]
pub use auth::authorize;

// === Scopes（对齐 scopes.ts barrel）===
pub use scopes::{
    all_scopes, chat_bridge_scopes, commerce_scopes, model_scopes, remote_control_scopes,
    skill_scopes, SCOPE_ACCOUNT, SCOPE_AI, SCOPE_CHAT_BRIDGE, SCOPE_CHAT_BRIDGE_READ,
    SCOPE_CHAT_BRIDGE_ROTATE, SCOPE_CHAT_BRIDGE_WRITE, SCOPE_REMOTE_CONTROL,
    SCOPE_REMOTE_CONTROL_AGENT_RUN, SCOPE_REMOTE_CONTROL_PERMISSION_RESPONSE,
    SCOPE_REMOTE_CONTROL_SESSION_CONTROL, SCOPE_SKILLS,
};
