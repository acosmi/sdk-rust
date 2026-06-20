//! sanitize 子包 — 端口自 `acosmi-sdk-ts/src/sanitize/`（其本身端口自 `acosmi-sdk-go/sanitize/`）。
//!
//! Provider 无关的 content-block 防御性处理：底线剥除 + 体积/深度早失败。主包经
//! [`crate::core::sanitize_bridge`] 把这些工具与 `Client`/`ChatRequest` 粘合
//! （对应 TS `core/sanitize-bridge.ts`）。
//!
//! feature `sanitize`（默认开）门控；对齐 npm 子路径导出 `./sanitize`（`export * as sanitize`）。

pub mod config;
pub mod defensive;
pub mod history;
pub mod types;

// 对齐 index.ts `export * from`（types/config/defensive/history）。
pub use config::{MinimalSanitizeConfig, SanitizeError};
pub use defensive::sanitize;
pub use history::{drop_blocks, strip_ephemeral, BlockPredicate};
pub use types::{BlockType, DeltaType, EPHEMERAL_MARKER_FIELD};

#[cfg(test)]
mod tests {
    use super::config::{MinimalSanitizeConfig, SanitizeError};
    use super::defensive::sanitize;
    use super::history::{drop_blocks, strip_ephemeral};
    use super::types::BlockType;
    use serde_json::{json, Value};

    fn msgs(v: Value) -> Vec<Value> {
        v.as_array().unwrap().clone()
    }

    // ── 验收② thinking / redacted_thinking 硬豁免 ──────────────────────────────
    #[test]
    fn strip_ephemeral_thinking_hard_exempt() {
        let input = msgs(json!([{
            "role": "assistant",
            "content": [
                { "type": "thinking", "thinking": "deep", "acosmi_ephemeral": true },
                { "type": "redacted_thinking", "data": "x", "acosmi_ephemeral": true },
                { "type": "text", "text": "ephemeral note", "acosmi_ephemeral": true },
                { "type": "text", "text": "kept" }
            ]
        }]));
        let out = strip_ephemeral(input);
        let content = out[0]["content"].as_array().unwrap();
        // thinking + redacted_thinking 保留（硬豁免），ephemeral text 被剥，普通 text 保留。
        let types: Vec<&str> = content
            .iter()
            .map(|b| b["type"].as_str().unwrap())
            .collect();
        assert_eq!(types, vec!["thinking", "redacted_thinking", "text"]);
        assert_eq!(content[2]["text"], "kept");
    }

    // ── 验收③ dropBlocks 按 tool_use_id 联动剥 + content 空整条丢 ────────────────
    #[test]
    fn drop_blocks_links_tool_result_and_drops_empty_message() {
        // assistant 轮整条只含一个 ephemeral tool_use → 命中剥除 → content 空 → 整条丢。
        // user 轮的 tool_result 引用该 tool_use_id → 联动剥 → content 空 → 整条丢。
        let input = msgs(json!([
            { "role": "user", "content": [{ "type": "text", "text": "hi" }] },
            {
                "role": "assistant",
                "content": [{ "type": "tool_use", "id": "call_1", "name": "f", "acosmi_ephemeral": true }]
            },
            {
                "role": "user",
                "content": [{ "type": "tool_result", "tool_use_id": "call_1", "content": "r" }]
            }
        ]));
        let out = strip_ephemeral(input);
        // 只剩第一条 user 消息（assistant 与第二条 user 都被整条丢弃）。
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["role"], "user");
        assert_eq!(out[0]["content"][0]["text"], "hi");
    }

    #[test]
    fn drop_blocks_keeps_partial_message_and_unrelated_tool_result() {
        // assistant 轮：ephemeral tool_use(call_1) + 普通 text → tool_use 剥、text 留 → 不丢。
        // user 轮：tool_result(call_1) 联动剥；tool_result(call_2) 无关保留。
        let input = msgs(json!([
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "call_1", "name": "f", "acosmi_ephemeral": true },
                    { "type": "text", "text": "narration" }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_1", "content": "r1" },
                    { "type": "tool_result", "tool_use_id": "call_2", "content": "r2" }
                ]
            }
        ]));
        let out = strip_ephemeral(input);
        assert_eq!(out.len(), 2);
        // assistant 留 text。
        let a = out[0]["content"].as_array().unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0]["type"], "text");
        // user 只留 call_2 的 tool_result。
        let u = out[1]["content"].as_array().unwrap();
        assert_eq!(u.len(), 1);
        assert_eq!(u[0]["tool_use_id"], "call_2");
    }

    #[test]
    fn drop_blocks_predicate_collects_across_message_order() {
        // tool_result 出现在 tool_use 之前也能联动剥（两步收敛，顺序无关）。
        let input = msgs(json!([
            { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "x", "content": "r" }] },
            { "role": "assistant", "content": [{ "type": "tool_use", "id": "x", "name": "f" }] }
        ]));
        // pred = 剥所有 tool_use。
        let out = drop_blocks(input, &|b| {
            b.get("type").and_then(Value::as_str) == Some("tool_use")
        });
        // 两条都被剥空 → 整条丢。
        assert!(out.is_empty());
    }

    // ── 验收④ base64_decoded_len 对齐 Go DecodedLen + 媒体超限 SizeError ──────────
    #[test]
    fn media_size_over_limit_returns_size_error() {
        // "AAAA" = 4 字符无 padding → DecodedLen = 4*3/4 = 3 字节。limit=2 → 超限。
        let input = msgs(json!([{
            "role": "user",
            "content": [{ "type": "image", "source": { "type": "base64", "data": "AAAA" } }]
        }]));
        let cfg = MinimalSanitizeConfig {
            max_image_bytes: Some(2),
            ..Default::default()
        };
        let err = sanitize(input, &cfg).unwrap_err();
        match err {
            SanitizeError::Size {
                block_type,
                actual,
                limit,
            } => {
                assert_eq!(block_type, BlockType::Image);
                assert_eq!(actual, 3);
                assert_eq!(limit, 2);
            }
            other => panic!("expected Size, got {other:?}"),
        }
    }

    #[test]
    fn media_size_data_url_prefix_stripped() {
        // 含 "data:image/png;base64," 前缀 → 剥前缀后量体积。剥后 "AAAA"(4) → 3 字节。
        let input = msgs(json!([{
            "role": "user",
            "content": [{
                "type": "image",
                "source": { "type": "base64", "data": "data:image/png;base64,AAAA" }
            }]
        }]));
        // limit=3 恰不超限（3 > 3 为 false）。
        let cfg_ok = MinimalSanitizeConfig {
            max_image_bytes: Some(3),
            ..Default::default()
        };
        assert!(sanitize(input.clone(), &cfg_ok).is_ok());
        // limit=2 → 超限（证明前缀确实被剥，否则按整串长度会更大也超限但 actual 不同）。
        let cfg_bad = MinimalSanitizeConfig {
            max_image_bytes: Some(2),
            ..Default::default()
        };
        match sanitize(input, &cfg_bad).unwrap_err() {
            SanitizeError::Size { actual, .. } => assert_eq!(actual, 3),
            other => panic!("expected Size, got {other:?}"),
        }
    }

    #[test]
    fn base64_decoded_len_padding_matches_go() {
        // 验证经 sanitize 暴露的 DecodedLen 语义：
        //   "AAA=" (4 chars, 1 pad) → 4*3/4 - 1 = 2 字节。
        let input = msgs(json!([{
            "role": "user",
            "content": [{ "type": "image", "source": { "type": "base64", "data": "AAA=" } }]
        }]));
        let cfg = MinimalSanitizeConfig {
            max_image_bytes: Some(1),
            ..Default::default()
        };
        match sanitize(input, &cfg).unwrap_err() {
            SanitizeError::Size { actual, .. } => assert_eq!(actual, 2),
            other => panic!("expected Size, got {other:?}"),
        }
    }

    #[test]
    fn url_media_skipped() {
        // source.type=url → 无法本地量体积 → 跳过，不抛。
        let input = msgs(json!([{
            "role": "user",
            "content": [{ "type": "image", "source": { "type": "url", "url": "http://x/y.png" } }]
        }]));
        let cfg = MinimalSanitizeConfig {
            max_image_bytes: Some(1),
            ..Default::default()
        };
        assert!(sanitize(input, &cfg).is_ok());
    }

    // ── 验收 深度超限 HistoryTooDeep ────────────────────────────────────────────
    #[test]
    fn history_too_deep() {
        let input = msgs(json!([
            { "role": "user", "content": "a" },
            { "role": "assistant", "content": "b" },
            { "role": "user", "content": "c" }
        ]));
        let cfg = MinimalSanitizeConfig {
            max_messages_turns: Some(2),
            ..Default::default()
        };
        assert_eq!(
            sanitize(input, &cfg).unwrap_err(),
            SanitizeError::HistoryTooDeep
        );
    }

    #[test]
    fn deny_list_strips_block() {
        let input = msgs(json!([{
            "role": "assistant",
            "content": [
                { "type": "thinking", "thinking": "x" },
                { "type": "text", "text": "y" }
            ]
        }]));
        let cfg = MinimalSanitizeConfig {
            permanent_deny_blocks: vec![BlockType::Thinking],
            ..Default::default()
        };
        let out = sanitize(input, &cfg).unwrap();
        let content = out[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
    }

    // ── 验收 形态异常元素透传不抛（bug-for-bug 宽容）──────────────────────────────
    #[test]
    fn malformed_elements_pass_through() {
        // 非 object 元素 / content 为 string / content 缺失 → 原样透传，不抛。
        let input = msgs(json!([
            "not-an-object",
            42,
            { "role": "user", "content": "plain text not array" },
            { "role": "system" }
        ]));
        let cfg = MinimalSanitizeConfig {
            permanent_deny_blocks: vec![BlockType::Image],
            max_image_bytes: Some(1),
            ..Default::default()
        };
        let out = sanitize(input.clone(), &cfg).unwrap();
        assert_eq!(out, input); // 全部原样透传。
    }

    // ── is_empty 零开销判定 ─────────────────────────────────────────────────────
    #[test]
    fn config_is_empty() {
        assert!(MinimalSanitizeConfig::default().is_empty());
        assert!(!MinimalSanitizeConfig {
            max_image_bytes: Some(1),
            ..Default::default()
        }
        .is_empty());
    }
}
