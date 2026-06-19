//! 支持 / 反馈域。对齐 `support/index.ts`。
//!
//! 业务方法（`submit_bug_report` / `get_bug_report`）经 declaration-merging 模式注入
//! [`Client`](crate::core::Client) 的 `impl Client` 块（无 side-effect import）。

pub mod bug_report;

pub use bug_report::{BugReportResult, BugView};
