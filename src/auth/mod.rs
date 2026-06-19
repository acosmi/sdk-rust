//! 鉴权 / 身份域。端口自 `auth/`。
//!
//! 相位说明：P1 仅含 `types`（`TokenSet` 等基础类型，供 core 依赖）。
//! OAuth discover / register / exchange / refresh / scopes 等待 P2。

pub mod types;

pub use types::{is_valid_token_set, ClientRegistration, ServerMetadata, TokenResponse, TokenSet};
