//! 企业席位（P6a）namespace：企业组织 / 成员 / 订阅 + 席位 / 用量报表 / KYC 自查。
//!
//! 对齐 `enterprise/index.ts`。admin 板块 9 controllers 由 admin UI 直连；SDK 暴露登录态 +
//! 跨身份共用端点。业务方法经 declaration-merging 模式落在 [`client`] 的 `impl Client` 块。

pub mod client;
pub mod types;

pub use types::{
    AssignSeatRequest, EnterpriseKycMyStatusView, EnterpriseMember, EnterpriseSummary,
    InviteMemberRequest, MemberRole, OrgConsumeReport, OrgSeat, OrgSubscription,
};
