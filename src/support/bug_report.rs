//! 支持 / 反馈域：CrabCode CLI bug 报告。端口自 `support/bug-report.ts`
//! （注入 `submit_bug_report` / `get_bug_report` 到 [`Client`]，declaration-merging 模式）。
//!
//! V30 CrabCode CLI bug 报告端点封装：
//!   - POST /api/v4/crabcode_cli_feedback  —— Bearer JWT (account scope)，限流 20/h/user
//!   - GET  /api/v4/crabcode/bug/:bug_id   —— 公开（无 auth），限流 60/min/IP
//!
//! 设计要点：
//!   - report_data 用 `serde_json::Value`（调用方任意 JSON），后端只解析为 map 做脱敏 + 字段
//!     抽取，不做严格 schema 校验。
//!   - 服务端兜底脱敏 6 类正则；调用方无须自行做密钥过滤。
//!   - 公开 GET 端点走 `do_public_json_full`，无 token 也能调。

use crate::core::client::Client;
use crate::shared::{ApiResponse, Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;

/// POST /api/v4/crabcode_cli_feedback 返回体。对应 TS `BugReportResult`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BugReportResult {
    /// 服务端生成的 UUID（写入 GitHub Issue body 用）。
    pub feedback_id: String,
    /// 公开页链接，形如 `https://<base>/chat/crabcode/bug/<uuid>`。
    pub detail_url: String,
}

/// GET /api/v4/crabcode/bug/:id 返回体（公开 ViewModel）。对应 TS `BugView`。
///
/// errors / transcript / extras 用 `Vec<Value>` / `HashMap<String, Value>`：客户端 reportData
/// schema 会随版本变，SDK 不强 typed。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BugView {
    pub id: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(rename = "messageCount")]
    pub message_count: i64,
    #[serde(rename = "hasErrors")]
    pub has_errors: bool,
    pub status: String,
    #[serde(
        rename = "clientDatetime",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub client_datetime: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<HashMap<String, Value>>,
}

impl Client {
    /// 上报一份 CrabCode bug 报告。对应 TS `submitBugReport`。
    ///
    /// `report_data` 是任意 JSON 可编码值，后端只解析为 map 做脱敏 + 字段抽取。
    ///
    /// 错误：
    ///   - [`Error::Http`] 401 —— token 过期（内部已做一次 refresh + retry，仍 401 抛出）。
    ///   - [`Error::Http`] 403 type="permission_error"（含 "Custom data retention settings"）
    ///     —— 用户所在组织 ZDR，拒绝收集。
    ///   - [`Error::Http`] 400 type="invalid_request_error" —— content 不是合法 JSON。
    ///   - [`Error::Http`] 429 —— 限流 20/h/user。
    ///   - [`Error::Network`] —— 传输层错误。
    pub async fn submit_bug_report(
        &self,
        report_data: &Value,
        signal: Option<CancellationToken>,
    ) -> Result<BugReportResult> {
        if report_data.is_null() {
            return Err(Error::other("acosmi: reportData required"));
        }
        let content_str = serde_json::to_string(report_data)
            .map_err(|e| Error::other(format!("acosmi: marshal reportData: {e}")))?;
        // 对应 TS doJSON<APIResponse<BugReportResult>>('POST', ...) 后 .data。
        let body = json!({ "content": content_str }).to_string();
        let (env, _) = self
            .do_json_full::<ApiResponse<BugReportResult>>(
                reqwest::Method::POST,
                "/crabcode_cli_feedback",
                Some(&body),
                signal,
            )
            .await?;
        let env = env.ok_or_else(|| Error::other("/crabcode_cli_feedback: empty response body"))?;
        if let Some(err) = env.business_error() {
            return Err(err);
        }
        Ok(env.data)
    }

    /// 取公开 ViewModel（无需 auth，任意人凭 ID 可读）。对应 TS `getBugReport`。
    ///
    /// 错误：
    ///   - [`Error::Http`] 404 —— bug 不存在或被软删。
    ///   - [`Error::Http`] 429 —— 限流 60/min/IP。
    ///   - [`Error::Network`] —— 传输层错误。
    pub async fn get_bug_report(
        &self,
        bug_id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<BugView> {
        let trimmed = bug_id.trim();
        if trimmed.is_empty() {
            return Err(Error::other("acosmi: bugID required"));
        }
        // 公开端点 —— 不强制 token（未登录 / token 过期场景下也能查）。
        let env: ApiResponse<BugView> = self
            .do_public_json_full(&format!("/crabcode/bug/{trimmed}"), signal)
            .await?;
        if let Some(err) = env.business_error() {
            return Err(err);
        }
        Ok(env.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bug_report_result_wire_snake_case() {
        // feedback_id / detail_url 是 snake_case（端点裸字段，非 ApiResponse 信封）。
        let r: BugReportResult = serde_json::from_str(
            r#"{"feedback_id":"uuid-1","detail_url":"https://x/chat/crabcode/bug/uuid-1"}"#,
        )
        .unwrap();
        assert_eq!(r.feedback_id, "uuid-1");
        assert_eq!(r.detail_url, "https://x/chat/crabcode/bug/uuid-1");
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["feedback_id"], "uuid-1");
        assert_eq!(back["detail_url"], "https://x/chat/crabcode/bug/uuid-1");
    }

    #[test]
    fn bug_view_camel_case_wire() {
        let v: BugView = serde_json::from_str(
            r#"{"id":"b1","description":"d","messageCount":3,"hasErrors":true,"status":"open","createdAt":"2026-05-01T00:00:00","extras":{"k":"v"}}"#,
        )
        .unwrap();
        assert_eq!(v.id, "b1");
        assert_eq!(v.message_count, 3);
        assert!(v.has_errors);
        assert_eq!(v.created_at, "2026-05-01T00:00:00");
        assert!(v.extras.is_some());
    }
}
