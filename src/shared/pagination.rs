//! 跨域统一分页请求 / 排序原语。端口自 `shared/pagination.ts`。
//!
//! 红线：`PageResult<T>` 是 `YudaoPageResult<T>` 的别名，不开第二套分页结果结构。

use super::api_response::YudaoPageResult;
use serde::{Deserialize, Serialize};

/// 排序方向。wire 上为小写字符串（闭联合）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDirection {
    Asc,
    Desc,
}

/// 跨域统一分页【请求】参数。字段全部可选；SDK 不在客户端做字段校验。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PageRequest {
    /// 1-based 页码。
    #[serde(rename = "pageNo", default, skip_serializing_if = "Option::is_none")]
    pub page_no: Option<i64>,
    /// 每页条数。
    #[serde(rename = "pageSize", default, skip_serializing_if = "Option::is_none")]
    pub page_size: Option<i64>,
    /// 排序字段名（领域字段，由各命名空间文档约定白名单）。
    #[serde(rename = "sortBy", default, skip_serializing_if = "Option::is_none")]
    pub sort_by: Option<String>,
    /// 排序方向；省略时由服务端决定默认值。
    #[serde(
        rename = "sortDirection",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sort_direction: Option<SortDirection>,
}

/// 跨域统一分页【结果】—— 刻意做成 `YudaoPageResult<T>` 的别名（单一分页结构）。
pub type PageResult<T> = YudaoPageResult<T>;
