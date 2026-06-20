//! Client 内部 HTTP 辅助。端口自 `core/http.ts`（其端口自 `acosmi-sdk-go/client.go`）。
//!
//! 含：安全限制常量 / `parse_http_error`（Anthropic + OpenAI 错误体）/ `classify_transport`
//! （NetworkError 分类）/ `parse_stream_error`（三 schema）/ 订单状态判定 / SSE 行扫描器 / 限长读取。

use crate::shared::errors::{HttpError, NetworkError, Result, StreamError};
use async_stream::try_stream;
use bytes::Bytes;
use futures::Stream;
use futures::StreamExt;

// =============================================================================
// 安全限制常量（端口自 client.go 安全限制常量段）
// =============================================================================

/// 50MB —— 技能 ZIP 包最大下载体积。
pub const MAX_DOWNLOAD_SIZE: usize = 50 * 1024 * 1024;
/// 1MB —— 错误响应体最大读取量。
pub const MAX_ERROR_BODY_SIZE: usize = 1024 * 1024;
/// 1MB —— SSE 单行最大长度（大 JSON chunk）。
pub const MAX_SSE_LINE_SIZE: usize = 1024 * 1024;
/// 模型列表缓存有效期（毫秒）。
pub const MODEL_CACHE_TTL_MS: u64 = 5 * 60 * 1000;
/// 非流式 JSON 子请求默认超时（毫秒），对应 TS `doJSONFull*` 内 `withRequestTimeout(30_000,...)`。
pub const DEFAULT_JSON_TIMEOUT_MS: u64 = 30_000;
/// chat / messages / 媒体生成 per-request 超时（毫秒，11min），对应 TS `CHAT_REQUEST_TIMEOUT_MS`。
/// 容纳 DeepSeek 等上游"首字节前 10min 保活"窗口。
pub const CHAT_REQUEST_TIMEOUT_MS: u64 = 11 * 60 * 1000;
/// V29 系数缓存（TTL 8s）。
pub const COEF_CACHE_TTL_MS: u64 = 8 * 1000;

// =============================================================================
// HTTP 错误解析
// =============================================================================

/// 解析 HTTP 错误响应体，兼容 Anthropic 和 OpenAI 错误格式。
/// - Anthropic：`{"type":"error","error":{"type":"...","message":"..."}}`
/// - OpenAI：`{"error":{"message":"...","type":"...","code":"..."}}`
/// - 通用回退：`HTTP {status}: {body}`
pub fn parse_http_error(status_code: u16, body: &str) -> HttpError {
    parse_http_error_with_retry_after(status_code, body, 0)
}

/// 同 [`parse_http_error`]，额外携带已从 `Retry-After` 头解析的秒数（0 = 无）。
pub fn parse_http_error_with_retry_after(
    status_code: u16,
    body: &str,
    retry_after: i64,
) -> HttpError {
    if body.is_empty() {
        return HttpError::new(status_code, "", "", retry_after, "");
    }

    let mut type_ = String::new();
    let mut message = String::new();
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(err_obj) = obj.get("error").filter(|v| v.is_object()) {
            if let Some(m) = err_obj.get("message").and_then(|v| v.as_str()) {
                message = m.to_string();
            }
            if let Some(t) = err_obj.get("type").and_then(|v| v.as_str()) {
                type_ = t.to_string();
            }
        }
    }
    // 非 JSON → body 原样保留（type/message 空）。
    HttpError::new(status_code, type_, message, retry_after, body)
}

// =============================================================================
// 网络错误分类
// =============================================================================

/// 包装传输层 `reqwest::Error` 为 `NetworkError`，便于 retry policy 判定。
///
/// - 超时（`is_timeout`）→ `timeout=true`
/// - 连接重置 / EOF / broken pipe / 连接失败 → `eof=true`
/// - 其它：`timeout`/`eof` 均 false（不重试）
pub fn classify_transport(op: &str, url: &str, err: &reqwest::Error) -> NetworkError {
    let mut ne = NetworkError::new(op, url, err.to_string());
    if err.is_timeout() {
        ne.timeout = true;
        return ne;
    }
    if err.is_connect() {
        ne.eof = true;
        return ne;
    }
    let msg = err.to_string().to_lowercase();
    if msg.contains("connection reset")
        || msg.contains("eof")
        || msg.contains("broken pipe")
        || msg.contains("connection closed")
    {
        ne.eof = true;
    }
    ne
}

// =============================================================================
// Stream Error 解析
// =============================================================================

/// 从 failed/error 事件 JSON 中提取结构化错误（兼容三种 schema，按优先级）。
/// 对应 TS `parseStreamError`。
pub fn parse_stream_error(data: &str) -> StreamError {
    let payload: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => {
            return StreamError {
                raw_error: data.to_string(),
                ..Default::default()
            }
        }
    };

    let mut code = payload
        .get("errorCode")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let stage = payload
        .get("stage")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut message = payload
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let retryable = payload
        .get("retryable")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut raw_error = String::new();

    if let Some(err_val) = payload.get("error") {
        if let Some(s) = err_val.as_str() {
            raw_error = s.to_string();
        } else if err_val.is_object() {
            raw_error = err_val.to_string();
            // 私有 message 空时，用 Anthropic error.message 兜底。
            if message.is_empty() {
                if let Some(m) = err_val.get("message").and_then(|v| v.as_str()) {
                    message = m.to_string();
                }
            }
            // errorCode 空 + Anthropic error.type 非空时，兜底用 type 作 code。
            if code.is_empty() {
                if let Some(t) = err_val.get("type").and_then(|v| v.as_str()) {
                    code = t.to_string();
                }
            }
        }
    }

    StreamError {
        code,
        stage,
        user_message: message,
        raw_error,
        retryable,
    }
}

// =============================================================================
// Order 状态判定
// =============================================================================

/// 订单是否成功终态。对应 TS `isOrderSuccess`。
pub fn is_order_success(status: &str) -> bool {
    matches!(status, "PAID" | "SUCCESS" | "COMPLETED")
}

/// 订单是否到达终态（成功或失败）。对应 TS `isOrderTerminal`。
pub fn is_order_terminal(status: &str) -> bool {
    matches!(
        status,
        "PAID"
            | "SUCCESS"
            | "COMPLETED"
            | "FAILED"
            | "CANCELLED"
            | "CLOSED"
            | "EXPIRED"
            | "REFUNDED"
    )
}

// =============================================================================
// SSE 行迭代器（替代 Go bufio.Scanner / TS iterSSELines）
// =============================================================================

/// 把字节流（reqwest `Response::bytes_stream()`）转为按行产出的异步流。
///
/// - 单行 1MB 硬上限（与 [`MAX_SSE_LINE_SIZE`] 对齐）—— 超长行报错。
/// - 去除行尾 `\r`（CRLF）。
/// - flush 末行（无结尾 `\n`）。
pub fn iter_sse_lines<S>(body: S) -> impl Stream<Item = Result<String>>
where
    S: Stream<Item = reqwest::Result<Bytes>>,
{
    iter_sse_lines_with_cap(body, MAX_SSE_LINE_SIZE)
}

/// [`iter_sse_lines`] 带自定义单行上限。
pub fn iter_sse_lines_with_cap<S>(
    body: S,
    max_line_bytes: usize,
) -> impl Stream<Item = Result<String>>
where
    S: Stream<Item = reqwest::Result<Bytes>>,
{
    try_stream! {
        futures::pin_mut!(body);
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = body.next().await {
            let chunk = chunk?;
            buf.extend_from_slice(&chunk);

            // 按 \n 切行。
            while let Some(nl) = buf.iter().position(|&b| b == b'\n') {
                let mut line: Vec<u8> = buf.drain(..=nl).collect();
                line.pop(); // 去掉 \n
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                yield String::from_utf8_lossy(&line).into_owned();
            }
            if buf.len() > max_line_bytes {
                Err(crate::shared::errors::Error::other(format!(
                    "SSE line exceeds {max_line_bytes} bytes"
                )))?;
            }
        }
        // 末行（无结尾 \n）。
        if !buf.is_empty() {
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            if !buf.is_empty() {
                yield String::from_utf8_lossy(&buf).into_owned();
            }
        }
    }
}

// =============================================================================
// 限长读取（替代 Go io.LimitReader）
// =============================================================================

/// 读取字节流但限制最大字节数，超限丢弃尾部。对应 TS `readLimited`。
pub async fn read_limited<S>(body: S, max_bytes: usize) -> Result<Vec<u8>>
where
    S: Stream<Item = reqwest::Result<Bytes>>,
{
    futures::pin_mut!(body);
    let mut out: Vec<u8> = Vec::new();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        if out.len() + chunk.len() > max_bytes {
            let remain = max_bytes - out.len();
            if remain > 0 {
                out.extend_from_slice(&chunk[..remain]);
            }
            break;
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// 读取字节流 + UTF-8 解码，限制最大字节数。对应 TS `readLimitedText`。
pub async fn read_limited_text<S>(body: S, max_bytes: usize) -> Result<String>
where
    S: Stream<Item = reqwest::Result<Bytes>>,
{
    let buf = read_limited(body, max_bytes).await?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}
