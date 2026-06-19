//! 跨子域共享的 compliance 公共领域类型。端口自 `compliance/types.ts`。
//!
//! 只放跨多个 compliance 子域、且不专属任一子域的类型。子域专属类型在
//! `compliance/<子域>/types.rs`。
//!
//! `signal`（TS `AbortSignal`）→ Rust `Option<CancellationToken>`，由各方法形参直接承载，
//! 不在 options 结构内携带（取消模型差异，见方案 §4.6）。

/// Compliance 写操作选项；写操作不自动 retry，不自动 401 重放。
///
/// `signal` 不在此结构内（Rust 经方法形参 `Option<CancellationToken>` 承载取消）。
#[derive(Debug, Clone, Default)]
pub struct ComplianceWriteOptions {
    /// 调用方稳定的幂等键。写操作强烈建议持久化；缺省时服务端按 UUID 兜底，但调用方
    /// 在重试 / 恢复时无法对账。
    pub idempotency_key: Option<String>,
}

/// 轮询参数（exponential backoff）。
///
/// `signal` 不在此结构内（Rust 经方法形参 `Option<CancellationToken>` 承载取消）。
#[derive(Debug, Clone, Default)]
pub struct CompliancePollOptions {
    /// 总超时（ms），缺省 60s。
    pub timeout_ms: Option<u64>,
    /// 初始 interval（ms），缺省 1000。
    pub initial_interval_ms: Option<u64>,
    /// interval 倍增上限（ms），缺省 5000。
    pub max_interval_ms: Option<u64>,
    /// 倍增系数，缺省 1.5。
    pub multiplier: Option<f64>,
}
