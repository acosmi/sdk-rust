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

// === 业务域 module（P1→P8 分阶段填充）===
// 当前为 P0 脚手架：仅骨架与版本常量，保证 cargo build/clippy/test 全绿。
// P1 起依次加入：shared / core / auth / models(+adapters) / billing / skills /
// notifications / agent_runs / compliance / support / subscription / pricing /
// products / casehall / enterprise / finance / chatbridge / sanitize。

#[cfg(test)]
mod scaffold_tests {
    use super::*;

    #[test]
    fn version_is_wired() {
        assert_eq!(VERSION, "2.8.0");
    }
}
