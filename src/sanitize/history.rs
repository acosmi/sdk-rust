//! sanitize/history — 端口自 `acosmi-sdk-ts/src/sanitize/history.ts`
//! （其本身端口自 `acosmi-sdk-go/sanitize/history.go`）。
//!
//! `drop_blocks` 按谓词从 messages 中剥除 block，并按 `tool_use_id` 联动剥除对应
//! `tool_result`/`mcp_tool_result`（§5 P8 验收③）；content 剥空则整条 message 丢弃。
//! `strip_ephemeral` 剥 `acosmi_ephemeral:true` 标记块，对 `thinking`/`redacted_thinking`
//! **硬豁免**（§5 P8 验收②）。
//!
//! 全程操作 `serde_json::Value` 树。形态异常元素原样透传不抛（bug-for-bug 宽容）。

use super::types::EPHEMERAL_MARKER_FIELD;
use serde_json::Value;
use std::collections::HashSet;

/// 判定一个 block（JSON object）是否应被剥除。对应 TS `BlockPredicate`。
pub type BlockPredicate<'a> = dyn Fn(&serde_json::Map<String, Value>) -> bool + 'a;

/// 按 `pred` 从 messages 中剥除 block，并联动剥除对应的 `tool_result`（user 轮）。
///
/// 收敛两步：先扫收集 `dropped_tool_use_ids`，再整体剥；这样顺序不影响正确性（即使
/// `tool_result` 在 `tool_use` 之前出现也能捕获）。形态异常元素原样透传。content 剥空整条丢。
pub fn drop_blocks(messages: Vec<Value>, pred: &BlockPredicate) -> Vec<Value> {
    let dropped_tool_use_ids = collect_dropped_tool_use_ids(&messages, pred);

    let mut out: Vec<Value> = Vec::with_capacity(messages.len());
    for msg in messages {
        let Some(obj) = msg.as_object() else {
            out.push(msg);
            continue;
        };
        let Some(content) = obj.get("content").and_then(Value::as_array) else {
            out.push(msg);
            continue;
        };

        let (kept, changed) = filter_blocks(content, pred, &dropped_tool_use_ids);
        if !changed {
            out.push(msg);
            continue;
        }

        // content 空了的消息整条丢弃（assistant 本轮全是 ephemeral 块；或 user 轮全是联动剥的
        // tool_result）。避免产生空消息让 provider 报错。
        if kept.is_empty() {
            continue;
        }

        // 浅拷贝 msg，只改 content，避免污染调用方数据。
        let mut new_obj = obj.clone();
        new_obj.insert("content".to_string(), Value::Array(kept));
        out.push(Value::Object(new_obj));
    }
    out
}

/// 第一遍扫描，仅收集"本次被 pred 命中的 tool_use 类 block 的 id"，以便联动剥 tool_result。
fn collect_dropped_tool_use_ids(messages: &[Value], pred: &BlockPredicate) -> HashSet<String> {
    let mut ids: HashSet<String> = HashSet::new();
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
            if !pred(block) {
                continue;
            }
            let Some(t) = block.get("type").and_then(Value::as_str) else {
                continue;
            };
            if t == "tool_use" || t == "server_tool_use" || t == "mcp_tool_use" {
                if let Some(id) = block.get("id").and_then(Value::as_str) {
                    if !id.is_empty() {
                        ids.insert(id.to_string());
                    }
                }
            }
        }
    }
    ids
}

/// 对单条消息的 content 数组剥除 pred 命中的 block，以及 `tool_use_id` 在
/// `dropped_tool_use_ids` 中的 `tool_result`/`mcp_tool_result`。返回 `(新数组, 是否变更)`。
fn filter_blocks(
    content: &[Value],
    pred: &BlockPredicate,
    dropped_tool_use_ids: &HashSet<String>,
) -> (Vec<Value>, bool) {
    let mut kept: Vec<Value> = Vec::with_capacity(content.len());
    let mut changed = false;
    for raw in content {
        let Some(block) = raw.as_object() else {
            kept.push(raw.clone());
            continue;
        };
        if pred(block) {
            changed = true;
            continue;
        }
        // 联动剥 tool_result / mcp_tool_result（仅当 tool_use_id 命中）。
        if !dropped_tool_use_ids.is_empty() {
            let t = block.get("type").and_then(Value::as_str);
            if t == Some("tool_result") || t == Some("mcp_tool_result") {
                if let Some(id) = block.get("tool_use_id").and_then(Value::as_str) {
                    if !id.is_empty() && dropped_tool_use_ids.contains(id) {
                        changed = true;
                        continue;
                    }
                }
            }
        }
        kept.push(raw.clone());
    }
    (kept, changed)
}

/// 从 messages 中剥除带 `acosmi_ephemeral:true` 标记的 block，以及对应的 `tool_result`。
///
/// **硬豁免**（§5 P8 验收②）：`thinking` / `redacted_thinking` 块永不剥，即使携带 ephemeral
/// 标记。理由：Anthropic extended thinking + tool_use 续轮场景下，上游强制要求 assistant 历史中
/// 保留原始 thinking 块，否则返回 "The content[].thinking in the thinking mode must be passed
/// back to the API."。本豁免兜底历史会话与第三方调用方两类已污染场景。
///
/// # Examples
///
/// ```
/// use acosmi::sanitize::strip_ephemeral;
///
/// // 空历史 → 空结果（无 ephemeral 块可剥）。类型由参数推断为 Vec<serde_json::Value>。
/// let cleaned = strip_ephemeral(Vec::new());
/// assert!(cleaned.is_empty());
/// ```
pub fn strip_ephemeral(messages: Vec<Value>) -> Vec<Value> {
    drop_blocks(messages, &|block| {
        match block.get("type").and_then(Value::as_str) {
            // 硬豁免，不可被 ephemeral 标记覆盖。
            Some("thinking") | Some("redacted_thinking") => false,
            _ => block.get(EPHEMERAL_MARKER_FIELD) == Some(&Value::Bool(true)),
        }
    })
}
