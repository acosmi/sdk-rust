//! sanitize_bridge — 端口自 `acosmi-sdk-ts/src/core/sanitize-bridge.ts`
//! （其本身端口自 `acosmi-sdk-go/sanitize_bridge.go`）。
//!
//! 主包与 sanitize 子包的胶水层。sanitize 子包定义了与 provider 无关的 block 处理工具，
//! 不依赖主包；本文件把 [`ChatRequest`] 这种主包类型归一化为 sanitize 能处理的
//! `Vec<Value>` 形态，并提供 Client 级别的可配置钩子（[`Client::set_defensive_sanitize`] /
//! [`Client::set_auto_strip_ephemeral_history`]）。
//!
//! 调用时机：`build_chat_request` 开头（每次 chat/chat_stream/chat_messages*）。
//! 未配置时零开销（`Client::apply_request_sanitizers`（内部）首行 early-return）。
//!
//! feature `sanitize` 门控（默认开）。

use super::client::Client;
use crate::models::types::ChatRequest;
use crate::sanitize::{sanitize, strip_ephemeral, MinimalSanitizeConfig, SanitizeError};
use crate::shared::Result;
use serde_json::Value;

impl Client {
    /// 配置请求前的底线防御（体积 / deny-list / 深度）。传 `None`（或空 cfg `is_empty()`）
    /// 关闭。并发安全，可在任意时间调用。对应 TS `setDefensiveSanitize`。
    ///
    /// TS 侧传 `{}`（全零字段）即关闭；Rust 侧将空 cfg 归一为 `None`（等价语义 + 维持
    /// `apply_request_sanitizers` 的零开销 early-return）。
    pub fn set_defensive_sanitize(&self, cfg: MinimalSanitizeConfig) {
        let stored = if cfg.is_empty() { None } else { Some(cfg) };
        *self.inner().defensive_cfg.write().unwrap() = stored;
    }

    /// 开启后，每次请求前 SDK 会从 `raw_messages` 中剥除带 `acosmi_ephemeral:true` 标记的 block，
    /// 并联动剥除引用已剥 tool_use 的 tool_result，避免 provider 报 "tool_use_id 不存在"。
    /// 对应 TS `setAutoStripEphemeralHistory`。
    pub fn set_auto_strip_ephemeral_history(&self, on: bool) {
        self.inner()
            .auto_strip_ephemeral
            .store(on, std::sync::atomic::Ordering::Relaxed);
    }

    /// 在 `build_chat_request` 开头调用，把配置化的防御策略应用到 `req`。未配置时立即返回，
    /// 零开销。失败时返 `Err` —— 调用方应放弃本次请求。对应 TS `applyRequestSanitizers`。
    pub(crate) fn apply_request_sanitizers(&self, req: &mut ChatRequest) -> Result<()> {
        let cfg = self.inner().defensive_cfg.read().unwrap().clone();
        let strip = self
            .inner()
            .auto_strip_ephemeral
            .load(std::sync::atomic::Ordering::Relaxed);

        if cfg.is_none() && !strip {
            return Ok(());
        }

        // rawMessages 分支：归一化 → sanitize →（可选）strip → 写回。
        if let Some(raw) = req.raw_messages.take() {
            let mut msgs = normalize_raw_messages(raw).map_err(|e| {
                crate::shared::Error::other(format!("sanitize: normalize raw messages: {e}"))
            })?;
            if let Some(c) = &cfg {
                msgs = sanitize(msgs, c)?;
            }
            if strip {
                msgs = strip_ephemeral(msgs);
            }
            req.raw_messages = Some(Value::Array(msgs));
            return Ok(());
        }

        // 纯 messages 分支：block 级操作不适用，只做深度校验。
        if let Some(c) = &cfg {
            let max = c.max_messages_turns.unwrap_or(0);
            let len = req.messages.as_ref().map(|m| m.len()).unwrap_or(0) as u64;
            if max > 0 && len > max {
                return Err(SanitizeError::HistoryTooDeep.into());
            }
        }

        Ok(())
    }
}

/// 把任意形态的 `raw_messages` 归一为 `Vec<Value>`。对应 TS `normalizeRawMessages`。
///
/// 已是 JSON 数组时直接拆出；其他形态报错（"raw messages must be a JSON array"）。
/// `ChatRequest.raw_messages` 在 Rust 侧已是 `serde_json::Value`，故无需额外 JSON roundtrip。
fn normalize_raw_messages(rm: Value) -> std::result::Result<Vec<Value>, String> {
    match rm {
        Value::Array(a) => Ok(a),
        _ => Err("raw messages must be a JSON array".to_string()),
    }
}

// SanitizeError → 顶层 Error（feature `sanitize` 下接通 `?` 链路）。
impl From<SanitizeError> for crate::shared::Error {
    fn from(e: SanitizeError) -> Self {
        crate::shared::Error::other(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Client, Config};
    use crate::sanitize::BlockType;
    use serde_json::json;

    fn client() -> Client {
        Client::new(Config::default()).unwrap()
    }

    // ── 验收 apply_request_sanitizers 接通：未配置零开销 early-return ───────────────
    #[test]
    fn unconfigured_is_noop() {
        let c = client();
        let mut req = ChatRequest {
            raw_messages: Some(json!([{ "role": "user", "content": [
                { "type": "text", "text": "x", "acosmi_ephemeral": true }
            ]}])),
            ..Default::default()
        };
        let before = req.raw_messages.clone();
        c.apply_request_sanitizers(&mut req).unwrap();
        assert_eq!(req.raw_messages, before); // 未配置 → 原样不动。
    }

    // ── strip 分支：开启 auto-strip 剥 ephemeral，thinking 硬豁免 ──────────────────
    #[test]
    fn auto_strip_branch_applies() {
        let c = client();
        c.set_auto_strip_ephemeral_history(true);
        let mut req = ChatRequest {
            raw_messages: Some(json!([{ "role": "assistant", "content": [
                { "type": "thinking", "thinking": "t", "acosmi_ephemeral": true },
                { "type": "text", "text": "ephemeral", "acosmi_ephemeral": true },
                { "type": "text", "text": "kept" }
            ]}])),
            ..Default::default()
        };
        c.apply_request_sanitizers(&mut req).unwrap();
        let content = req.raw_messages.as_ref().unwrap()[0]["content"]
            .as_array()
            .unwrap();
        let types: Vec<&str> = content
            .iter()
            .map(|b| b["type"].as_str().unwrap())
            .collect();
        assert_eq!(types, vec!["thinking", "text"]); // thinking 豁免、ephemeral text 剥、kept 留。
    }

    // ── defensive cfg 分支：媒体超限 → Err（放弃请求）────────────────────────────
    #[test]
    fn defensive_cfg_media_oversize_errors() {
        let c = client();
        c.set_defensive_sanitize(MinimalSanitizeConfig {
            max_image_bytes: Some(2),
            ..Default::default()
        });
        let mut req = ChatRequest {
            raw_messages: Some(json!([{ "role": "user", "content": [
                { "type": "image", "source": { "type": "base64", "data": "AAAA" } }
            ]}])),
            ..Default::default()
        };
        assert!(c.apply_request_sanitizers(&mut req).is_err());
    }

    // ── deny-list 分支生效 ──────────────────────────────────────────────────────
    #[test]
    fn defensive_cfg_deny_list_strips() {
        let c = client();
        c.set_defensive_sanitize(MinimalSanitizeConfig {
            permanent_deny_blocks: vec![BlockType::Image],
            ..Default::default()
        });
        let mut req = ChatRequest {
            raw_messages: Some(json!([{ "role": "user", "content": [
                { "type": "image", "source": { "type": "url", "url": "http://x/y" } },
                { "type": "text", "text": "kept" }
            ]}])),
            ..Default::default()
        };
        c.apply_request_sanitizers(&mut req).unwrap();
        let content = req.raw_messages.as_ref().unwrap()[0]["content"]
            .as_array()
            .unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
    }

    // ── 空 cfg 关闭（is_empty → None）等价语义 ───────────────────────────────────
    #[test]
    fn empty_cfg_disables() {
        let c = client();
        c.set_defensive_sanitize(MinimalSanitizeConfig::default()); // 空 = 关闭。
        let mut req = ChatRequest {
            raw_messages: Some(json!([{ "role": "user", "content": [
                { "type": "text", "text": "x", "acosmi_ephemeral": true }
            ]}])),
            ..Default::default()
        };
        let before = req.raw_messages.clone();
        c.apply_request_sanitizers(&mut req).unwrap();
        assert_eq!(req.raw_messages, before);
    }

    // ── 纯 messages 分支：深度超限 HistoryTooDeep ───────────────────────────────
    #[test]
    fn pure_messages_depth_check() {
        use crate::models::types::ChatMessage;
        let c = client();
        c.set_defensive_sanitize(MinimalSanitizeConfig {
            max_messages_turns: Some(1),
            ..Default::default()
        });
        let mut req = ChatRequest {
            messages: Some(vec![ChatMessage::default(), ChatMessage::default()]),
            ..Default::default()
        };
        assert!(c.apply_request_sanitizers(&mut req).is_err());
    }

    // ── normalize 失败（raw_messages 非数组）→ Err ──────────────────────────────
    #[test]
    fn normalize_non_array_errors() {
        let c = client();
        c.set_auto_strip_ephemeral_history(true);
        let mut req = ChatRequest {
            raw_messages: Some(json!({ "not": "an array" })),
            ..Default::default()
        };
        assert!(c.apply_request_sanitizers(&mut req).is_err());
    }
}
