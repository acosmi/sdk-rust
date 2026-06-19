//! nexus-v4 / yudao 标准 API 响应包装。端口自 `shared/api-response.ts`。

use super::errors::Error;
use serde::{Deserialize, Serialize};

/// nexus-v4 标准响应。兼容 yudao 格式（`msg`）和 nexus-v4 格式（`message`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub code: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<String>,
    pub data: T,
}

impl<T> ApiResponse<T> {
    /// 优先返回 `message`，降级到 `msg`（兼容 yudao 透传）。对应 TS `apiResponseGetMessage`。
    pub fn get_message(&self) -> String {
        self.message
            .clone()
            .or_else(|| self.msg.clone())
            .unwrap_or_default()
    }

    /// 检查业务层错误码 —— `code != 0` 时返回 `BusinessError`。对应 TS `apiResponseBusinessError`。
    pub fn business_error(&self) -> Option<Error> {
        if self.code != 0 {
            Some(Error::business(self.code, self.get_message()))
        } else {
            None
        }
    }
}

/// yudao 分页响应格式（tk-dist 代理透传）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YudaoPageResult<T> {
    pub list: Vec<T>,
    pub total: i64,
}
