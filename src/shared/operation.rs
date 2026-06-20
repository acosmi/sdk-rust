//! 跨域统一操作（operation projection）原语。端口自 `shared/operation.ts`。
//!
//! 相位说明：TS `operation.ts` 把 `ProviderRequestStatus` 定义为
//! `compliance/provider/types.ts` 的 `ComplianceProviderRequestStatus` 的别名。
//! 为避免 Rust 中 shared → compliance 的前向依赖（循环），**本 crate 把
//! `ProviderRequestStatus` 的定义点放在 shared**，由 P6 `compliance::provider`
//! 反向 `pub use ... as ComplianceProviderRequestStatus`（定义点与 TS 相反，名字保留两个）。

use crate::macros::open_string_union;

/// 统一操作关联键。贯通控制台 / API / MCP / CrabCode / scheduler 各来源。
pub type OperationId = String;

/// 幂等键。作用域 = tenant + principal/client + action + product。
pub type IdempotencyKey = String;

/// 幂等键 HTTP header 名 —— 全 SDK 写接口的单一真相源。
pub const IDEMPOTENCY_KEY_HEADER: &str = "Idempotency-Key";

open_string_union! {
    /// 操作来源。开放联合 —— 平台侧 5 种固定值 + 合规域自定义来源。
    OperationSource {
        CONSOLE => "console",
        API => "api",
        SCHEDULER => "scheduler",
        CRABCODE => "crabcode",
        MCP => "mcp",
    }
}

open_string_union! {
    /// 操作状态机。开放联合，后端保留新增空间。
    OperationStatus {
        PENDING => "pending",
        RUNNING => "running",
        RETRYING => "retrying",
        AWAITING_CALLBACK => "awaiting_callback",
        AWAITING_VERIFY => "awaiting_verify",
        SUCCEEDED => "succeeded",
        FAILED => "failed",
        CANCELED => "canceled",
        UNKNOWN => "unknown",
    }
}

open_string_union! {
    /// 本地 verify 状态（与 provider 状态正交）。
    VerifyStatus {
        PENDING => "pending",
        VERIFIED => "verified",
        FAILED => "failed",
        SKIPPED => "skipped",
        UNKNOWN => "unknown",
    }
}

open_string_union! {
    /// Provider request 状态。定义点在 shared（见模块说明）；
    /// P6 compliance/provider 以 `ComplianceProviderRequestStatus` 复用同一类型。
    ProviderRequestStatus {
        PENDING => "PENDING",
        SUCCESS => "SUCCESS",
        FAILED => "FAILED",
        UNKNOWN => "UNKNOWN",
        RETRYING => "RETRYING",
    }
}
