//! sanitize/defensive — 端口自 `acosmi-sdk-ts/src/sanitize/defensive.ts`
//! （其本身端口自 `acosmi-sdk-go/sanitize/defensive.go`）。
//!
//! 对已经解析为 `Vec<Value>` 的 messages 做底线防御：深度校验 + 体积校验 + deny-list 剥除 +
//! `tool_use_id` 联动剥除 `tool_result`。
//!
//! 入参形态：messages 的每个元素预期是 object（含 role / content）；content 为 string（plain
//! text）或数组（block 数组）。形态异常的元素原样透传，不抛错（bug-for-bug 宽容）。
//!
//! 通过返回新 messages（可能 block 数组被缩减）；早失败返 `Err`（调用方应放弃本次请求）。

use super::config::{MinimalSanitizeConfig, SanitizeError};
use super::history::drop_blocks;
use super::types::BlockType;
use serde_json::Value;
use std::collections::HashSet;

/// sanitize 主函数（同步纯函数）。对应 TS `sanitize`。
///
/// - 深度超限 → [`SanitizeError::HistoryTooDeep`]；
/// - 媒体超限 → [`SanitizeError::Size`]；
/// - deny-list 命中 → 剥除（含 `tool_use_id` 联动）；
/// - 形态异常元素原样透传不抛。
pub fn sanitize(
    messages: Vec<Value>,
    cfg: &MinimalSanitizeConfig,
) -> Result<Vec<Value>, SanitizeError> {
    if cfg.max_messages_turns.unwrap_or(0) > 0
        && messages.len() as u64 > cfg.max_messages_turns.unwrap_or(0)
    {
        return Err(SanitizeError::HistoryTooDeep);
    }

    // 体积校验：先扫一遍，任何违规直接早失败（不修改 messages）。
    if cfg.max_image_bytes.unwrap_or(0) > 0
        || cfg.max_video_bytes.unwrap_or(0) > 0
        || cfg.max_pdf_bytes.unwrap_or(0) > 0
    {
        check_media_sizes(&messages, cfg)?;
    }

    // deny-list 剥除 + tool_use_id 联动。
    if !cfg.permanent_deny_blocks.is_empty() {
        let deny_set: HashSet<&'static str> = cfg
            .permanent_deny_blocks
            .iter()
            .map(|b| b.as_str())
            .collect();
        let messages = drop_blocks(messages, &|block| {
            block
                .get("type")
                .and_then(Value::as_str)
                .map(|t| deny_set.contains(t))
                .unwrap_or(false)
        });
        return Ok(messages);
    }

    Ok(messages)
}

/// 遍历所有 block，对 base64 内联 image/video/document 类型校验解码后字节数。
/// URL 版无法本地量体积，跳过（交网关把关）。违规返 [`SanitizeError::Size`]。
fn check_media_sizes(messages: &[Value], cfg: &MinimalSanitizeConfig) -> Result<(), SanitizeError> {
    for msg in messages {
        let Some(content) = msg
            .as_object()
            .and_then(|o| o.get("content"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for raw in content {
            let Some(block) = raw.as_object() else {
                continue;
            };
            let Some(bt) = block.get("type").and_then(Value::as_str) else {
                continue;
            };

            let (block_type, limit) = match bt {
                "image" => (BlockType::Image, cfg.max_image_bytes.unwrap_or(0)),
                "video" => (BlockType::Video, cfg.max_video_bytes.unwrap_or(0)),
                "document" => (BlockType::Document, cfg.max_pdf_bytes.unwrap_or(0)),
                _ => continue,
            };
            if limit == 0 {
                continue;
            }

            let data = extract_base64_data(block);
            if data.is_empty() {
                continue; // URL 版或形态异常，跳过。
            }
            let actual = base64_decoded_len(data);
            if actual > limit {
                return Err(SanitizeError::Size {
                    block_type,
                    actual,
                    limit,
                });
            }
        }
    }
    Ok(())
}

/// 从 Anthropic block 结构中抽 base64 data 字段。对应 TS `extractBase64Data`。
///
/// 形态：`{source:{type:"base64", data:"..."}}`（image/video/document 同构）。
/// 若是 URL 版（`source.type="url"`）或缺字段，返回 `""`。
/// 防御性：某些上游把 `"data:image/jpeg;base64,..."` 整串塞进 data，兜底去掉前缀。
fn extract_base64_data(block: &serde_json::Map<String, Value>) -> &str {
    let Some(src) = block.get("source").and_then(Value::as_object) else {
        return "";
    };
    if src.get("type").and_then(Value::as_str) != Some("base64") {
        return "";
    }
    let Some(data) = src.get("data").and_then(Value::as_str) else {
        return "";
    };
    // 防御性去前缀：含 "base64," 时取其后段（兜底非标准 data: URL 整串）。
    match data.find("base64,") {
        Some(i) => &data[i + "base64,".len()..],
        None => data,
    }
}

/// Go `base64.StdEncoding.DecodedLen` 等价：`floor(n*3/4)` 减去 padding 计数。对应 TS
/// `base64DecodedLen`。`n` = 编码字符串长度（已剥 `data:...;base64,` 前缀）。
fn base64_decoded_len(b64: &str) -> u64 {
    let bytes = b64.as_bytes();
    let n = bytes.len();
    let mut pad: u64 = 0;
    if n >= 1 && bytes[n - 1] == b'=' {
        pad += 1;
    }
    if n >= 2 && bytes[n - 2] == b'=' {
        pad += 1;
    }
    ((n as u64) * 3 / 4).saturating_sub(pad)
}
