//! 财务（P7）namespace：退款 / 发票 / 对公转账。
//!
//! 对齐 `finance/index.ts`。与 tk-dist `dist_invoice` / `dist_refund_*` /
//! `dist_corporate_transfer` 表族对接。admin 板块（审批/财务工作台/对账）由 admin UI 直连，
//! 不在 SDK 边界。**金额阵营（§3）**：所有 `*Fen` 字段 = i64 整数分。
//! 业务方法经 declaration-merging 模式落在 [`client`] 的 `impl Client` 块。

pub mod client;
pub mod types;

pub use types::{
    CorporateTransfer, InitiateCorporateTransferInput, InitiateCorporateTransferResult, Invoice,
    InvoiceType, RefundPolicy, RefundProductFamily, RefundRecord, RequestInvoiceInput,
    RequestRefundInput,
};
