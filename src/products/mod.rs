//! 商品中心域：公开在售商品（按族 / 按 slug）。
//!
//! 对齐 `products/index.ts`。业务方法经 declaration-merging 模式落在 [`client`] 的 `impl Client` 块。

pub mod client;
pub mod types;

pub use types::{Audience, BillingMode, Product, ProductFamily, RegionScope};
