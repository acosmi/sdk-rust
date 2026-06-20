//! Anthropic SSE 事件的 content block 元数据解析。端口自 `models/stream-meta.ts`
//! （其端口自 `acosmi-sdk-go/stream_meta.go`）。
//!
//! 把 content_block_start 中的 index / type / acosmi_ephemeral 记入 `block_type_map`，
//! 供 delta/stop 查表回填 [`crate::models::StreamEvent`] 的 blockIndex / blockType / ephemeral。
//!
//! - 惰性解析：只对 3 种 content_block_* 事件解 JSON，其他事件跳过。
//! - 单流 map：由单 SSE 扫描循环拥有，无锁；stop 后删表项，防长流累积。

use std::collections::HashMap;

/// SDK 为单个 content block 缓存的元数据。
#[derive(Debug, Clone, Default)]
pub struct BlockMeta {
    pub r#type: String,
    pub ephemeral: bool,
}

/// 按 Anthropic SSE 事件类型解析 `data`，更新 `block_type_map`，返回 `(index, type, ephemeral)`。
///
/// 仅当事件为 content_block_start / content_block_delta / content_block_stop 时返回非空 type；
/// 其他事件返回空 type，调用方据此判别是否填充 StreamEvent 元数据字段。
/// JSON 失败 → `(0, "", false)`（不抛，对齐 TS catch）。
pub fn extract_anthropic_block_meta(
    event_type: &str,
    data: &str,
    block_type_map: &mut HashMap<i64, BlockMeta>,
) -> (i64, String, bool) {
    match event_type {
        "content_block_start" => {
            #[derive(serde::Deserialize)]
            struct ContentBlock {
                #[serde(default)]
                r#type: Option<String>,
                #[serde(default)]
                acosmi_ephemeral: Option<bool>,
            }
            #[derive(serde::Deserialize)]
            struct Payload {
                #[serde(default)]
                index: Option<i64>,
                #[serde(default)]
                content_block: Option<ContentBlock>,
            }
            let payload: Payload = match serde_json::from_str(data) {
                Ok(p) => p,
                Err(_) => return (0, String::new(), false),
            };
            let index = payload.index.unwrap_or(0);
            let cb = payload.content_block;
            let meta = BlockMeta {
                r#type: cb
                    .as_ref()
                    .and_then(|c| c.r#type.clone())
                    .unwrap_or_default(),
                ephemeral: cb
                    .as_ref()
                    .and_then(|c| c.acosmi_ephemeral)
                    .unwrap_or(false),
            };
            let out = (index, meta.r#type.clone(), meta.ephemeral);
            block_type_map.insert(index, meta);
            out
        }
        "content_block_delta" => {
            let index = parse_index(data);
            let meta = block_type_map.get(&index);
            (
                index,
                meta.map(|m| m.r#type.clone()).unwrap_or_default(),
                meta.map(|m| m.ephemeral).unwrap_or(false),
            )
        }
        "content_block_stop" => {
            let index = parse_index(data);
            let meta = block_type_map.remove(&index);
            (
                index,
                meta.as_ref().map(|m| m.r#type.clone()).unwrap_or_default(),
                meta.as_ref().map(|m| m.ephemeral).unwrap_or(false),
            )
        }
        _ => (0, String::new(), false),
    }
}

/// 解析 `{"index": n}` 的 index 字段；失败/缺失 → 0（对齐 TS catch + `?? 0`）。
fn parse_index(data: &str) -> i64 {
    #[derive(serde::Deserialize)]
    struct Payload {
        #[serde(default)]
        index: Option<i64>,
    }
    serde_json::from_str::<Payload>(data)
        .ok()
        .and_then(|p| p.index)
        .unwrap_or(0)
}
