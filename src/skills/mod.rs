//! 技能商店 / 技能生产器 / 统一工具域。
//!
//! 对齐 `skills/index.ts`。类型在 [`types`]；业务方法经 declaration-merging 模式
//! 分散在 [`skills`] / [`tools`] 的 `impl Client` 块（无 side-effect import）。

#[allow(clippy::module_inception)]
pub mod skills;
pub mod tools;
pub mod types;

pub use skills::SkillDownload;
pub use types::{
    CertificationStatus, GenerateSkillRequest, GenerateSkillResult, OptimizeSkillRequest,
    OptimizeSkillResult, SkillBrowseListResponse, SkillBrowseResponse, SkillStoreItem,
    SkillStoreListItem, SkillStoreQuery, SkillSummary, ToolListResponse, ToolProvider, ToolView,
};
