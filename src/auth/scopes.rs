//! OAuth scope 常量 + 组合函数。端口自 `auth/scopes.ts`（其端口自 `acosmi-sdk-go/scopes.go`）。
//!
//! 分组 Scope（V2: 10→3 合并），与后端 `DesktopOAuthScopes` 保持一致。
//! 三处必须字面量一致，任一变更需同步另外两处：
//!   - Go:   `nexus-v4/backend/internal/handler/desktop_oauth.go` DesktopOAuthScopes
//!   - Java: `tk-dist/yudao-module-compliance-api` ComplianceScopes
//!   - TS:   `auth/scopes.ts` + `compliance/scopes.ts`
//!
//! 高风险分组 scope（`remote_control` + `chat_bridge`）镜像自 Go DesktopOAuthScopes，
//! **均不进 [`all_scopes`]**：服务端 ScopeExpansion 各自展开为 3 个子 scope；调用方须显式申请，
//! 避免桌面登录自动获得远控 / 凭证管理权限。

// === 核心分组 scope ===

/// 模型服务：模型调用 + 流量包 + 权益。
pub const SCOPE_AI: &str = "ai";
/// 技能与工具：技能商店 + 工具列表 + 执行。
pub const SCOPE_SKILLS: &str = "skills";
/// 账户信息：个人资料 + 钱包余额 + 交易记录。
pub const SCOPE_ACCOUNT: &str = "account";

// === 远程控制 CrabCode 多接入面专用 scope（高风险，不进 all_scopes）===

/// 远程控制能力（分组 scope，自包含展开到 3 个子 scope）。高风险，不进 [`all_scopes`]。
pub const SCOPE_REMOTE_CONTROL: &str = "remote_control";
/// 创建 / 取消 / stream AgentRun。
pub const SCOPE_REMOTE_CONTROL_AGENT_RUN: &str = "remote_control:agent-run";
/// 远程会话 lifecycle 控制（cancel / interrupt / kill）。
pub const SCOPE_REMOTE_CONTROL_SESSION_CONTROL: &str = "remote_control:session-control";
/// 代表用户提交远控权限审批响应（allow / deny / timeout）。
pub const SCOPE_REMOTE_CONTROL_PERMISSION_RESPONSE: &str = "remote_control:permission-response";

// === 第三方聊天集成桥接 scope（高风险，不进 all_scopes）===

/// 第三方聊天集成桥接（分组 scope，服务端 ScopeExpansion 展开为 read/write/rotate）。
/// 凭证管理高风险，不进 [`all_scopes`]。
pub const SCOPE_CHAT_BRIDGE: &str = "chat_bridge";
/// 查询 integration / session / 凭证元数据（仅 ref+fingerprint）。
pub const SCOPE_CHAT_BRIDGE_READ: &str = "chat_bridge:read";
/// 创建 / 更新 integration 与凭证。
pub const SCOPE_CHAT_BRIDGE_WRITE: &str = "chat_bridge:write";
/// 轮换 / 吊销凭证（高风险）。
pub const SCOPE_CHAT_BRIDGE_ROTATE: &str = "chat_bridge:rotate";

// === 旧细粒度 scope（deprecated，保留向后兼容；新代码请用分组 scope）===

/// 旧细粒度 scope，保留向后兼容。
#[deprecated(note = "旧细粒度 scope，新代码请用分组 SCOPE_AI")]
pub const SCOPE_MODELS: &str = "models";
/// 旧细粒度 scope，保留向后兼容。
#[deprecated(note = "旧细粒度 scope，新代码请用分组 SCOPE_AI")]
pub const SCOPE_MODELS_CHAT: &str = "models:chat";
/// 旧细粒度 scope，保留向后兼容。
#[deprecated(note = "旧细粒度 scope，新代码请用分组 SCOPE_AI")]
pub const SCOPE_ENTITLEMENTS: &str = "entitlements";
/// 旧细粒度 scope，保留向后兼容。
#[deprecated(note = "旧细粒度 scope，新代码请用分组 SCOPE_AI")]
pub const SCOPE_TOKEN_PACKAGES: &str = "token-packages";
/// 旧细粒度 scope，保留向后兼容。
#[deprecated(note = "旧细粒度 scope，新代码请用分组 SCOPE_SKILLS")]
pub const SCOPE_SKILL_STORE: &str = "skill_store";
/// 旧细粒度 scope，保留向后兼容。
#[deprecated(note = "旧细粒度 scope，新代码请用分组 SCOPE_SKILLS")]
pub const SCOPE_TOOLS: &str = "tools";
/// 旧细粒度 scope，保留向后兼容。
#[deprecated(note = "旧细粒度 scope，新代码请用分组 SCOPE_SKILLS")]
pub const SCOPE_TOOLS_EXECUTE: &str = "tools:execute";
/// 旧细粒度 scope，保留向后兼容。
#[deprecated(note = "旧细粒度 scope，新代码请用分组 SCOPE_ACCOUNT")]
pub const SCOPE_WALLET: &str = "wallet";
/// 旧细粒度 scope，保留向后兼容。
#[deprecated(note = "旧细粒度 scope，新代码请用分组 SCOPE_ACCOUNT")]
pub const SCOPE_WALLET_READONLY: &str = "wallet:readonly";
/// 旧细粒度 scope，保留向后兼容。
#[deprecated(note = "旧细粒度 scope，新代码请用分组 SCOPE_ACCOUNT")]
pub const SCOPE_PROFILE: &str = "profile";

/// 全部分组 scope（推荐）。**不含** remote_control / chat_bridge（高风险须显式申请）。
pub fn all_scopes() -> Vec<String> {
    vec![
        SCOPE_AI.to_string(),
        SCOPE_SKILLS.to_string(),
        SCOPE_ACCOUNT.to_string(),
    ]
}

/// 模型服务相关 scope。
pub fn model_scopes() -> Vec<String> {
    vec![SCOPE_AI.to_string()]
}

/// 商城 / 钱包 scope。
pub fn commerce_scopes() -> Vec<String> {
    vec![SCOPE_AI.to_string(), SCOPE_ACCOUNT.to_string()]
}

/// 技能 / 工具 scope。
pub fn skill_scopes() -> Vec<String> {
    vec![SCOPE_SKILLS.to_string()]
}

/// 远程控制 scope（推荐）。仅返回分组 `SCOPE_REMOTE_CONTROL`，服务端展开为 3 子 scope。
/// 调用方需显式申请，[`all_scopes`] 不含本项（避免桌面登录自动获权）。
pub fn remote_control_scopes() -> Vec<String> {
    vec![SCOPE_REMOTE_CONTROL.to_string()]
}

/// 聊天桥接 scope（推荐）。仅返回分组 `SCOPE_CHAT_BRIDGE`，服务端展开为 3 子 scope。
/// 调用方需显式申请，[`all_scopes`] 不含本项。
pub fn chat_bridge_scopes() -> Vec<String> {
    vec![SCOPE_CHAT_BRIDGE.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_scopes_is_three_grouped() {
        let s = all_scopes();
        assert_eq!(s, vec!["ai", "skills", "account"]);
    }

    #[test]
    fn high_risk_scopes_excluded_from_all() {
        let s = all_scopes();
        assert!(!s.contains(&SCOPE_REMOTE_CONTROL.to_string()));
        assert!(!s.contains(&SCOPE_CHAT_BRIDGE.to_string()));
    }

    #[test]
    fn commerce_scopes_combine_ai_and_account() {
        assert_eq!(commerce_scopes(), vec!["ai", "account"]);
    }

    #[test]
    fn grouped_scope_literals_match_backend() {
        assert_eq!(SCOPE_AI, "ai");
        assert_eq!(SCOPE_SKILLS, "skills");
        assert_eq!(SCOPE_ACCOUNT, "account");
        assert_eq!(SCOPE_REMOTE_CONTROL, "remote_control");
        assert_eq!(SCOPE_CHAT_BRIDGE, "chat_bridge");
    }
}
