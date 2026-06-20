//! 定价域：公开业务参数 + 公开模型摘要 + csign 电子认证 SKU（匿名可读）。
//!
//! 对齐 `pricing/index.ts`。业务方法经 declaration-merging 模式落在 [`client`] 的 `impl Client` 块。

pub mod client;
pub mod types;

pub use types::{
    ComplianceBenefitType, ComplianceQuoteResponse, ComplianceSku, PricingConfig,
    PublicModelSummary,
};
