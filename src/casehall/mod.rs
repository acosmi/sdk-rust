//! 法律案件咨询 namespace：律师库 / 案件线索 / 咨询 / 法律服务订单 & SKU / 律师执业证自查。
//!
//! 对齐 `casehall/index.ts`。与 tk-dist `yudao-module-casehall` 同源；admin 板块 9 模块由
//! admin UI 直连，不在 SDK 边界。业务方法经 declaration-merging 模式落在 [`client`] 的 `impl Client` 块。

pub mod client;
pub mod types;

pub use types::{
    BookConsultationRequest, BookConsultationResult, CaseLead, CaseLeadIdResult, CaseMatter,
    LawyerCredentialMyView, LawyerSummary, LegalBenefitType, LegalConsultation, LegalServiceOrder,
    LegalServiceSku, LegalSkuCode, ListLawyersParams, SubmitCaseLeadRequest,
};
