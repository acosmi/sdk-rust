//! Anthropic 原生格式 adapter。端口自 `models/adapters/anthropic.ts`
//! （其端口自 `acosmi-sdk-go/adapter_anthropic.go`）。
//!
//! 包含 `build_betas()` 调用、serverTools 合入、extraBody 透传（denylist 跳过 SDK 管理字段）。

use crate::models::betas::build_betas;
use crate::models::types::{
    ChatRequest, ChatResponse, ModelCapabilities, StreamEvent, THINKING_HIGH_MIN_MAX_TOKENS,
    THINKING_MAX, THINKING_MAX_FALLBACK_MAX_TOKENS, THINKING_OFF,
};
use crate::shared::errors::{Error, Result};
use serde_json::{json, Map, Value};

/// SDK 在 build_request_body 中写入并管理的请求体字段（精确 body key）。
/// extraBody 中若出现同名 key 会覆盖 SDK 计算结果，故透传时强制跳过这些 key。
const ANTHROPIC_SDK_MANAGED_BODY_KEYS: &[&str] =
    &["thinking", "effort", "max_tokens", "temperature", "betas"];

/// 构建 Anthropic 格式请求体。逻辑等同原 buildChatRequest，含完整 betas/tools/serverTools/extraBody。
pub fn build_request_body(caps: &ModelCapabilities, req: &ChatRequest) -> Map<String, Value> {
    let mut body: Map<String, Value> = Map::new();

    // 消息：rawMessages 优先于 messages。
    if let Some(raw) = &req.raw_messages {
        body.insert("messages".to_string(), raw.clone());
    } else if req
        .messages
        .as_ref()
        .map(|m| !m.is_empty())
        .unwrap_or(false)
    {
        body.insert(
            "messages".to_string(),
            serde_json::to_value(req.messages.as_ref().unwrap()).unwrap_or(Value::Null),
        );
    }

    body.insert("stream".to_string(), Value::Bool(req.stream == Some(true)));

    if let Some(mt) = req.max_tokens {
        if mt > 0 {
            body.insert("max_tokens".to_string(), Value::from(mt));
        }
    }
    if let Some(system) = &req.system {
        body.insert("system".to_string(), system.clone());
    }
    if let Some(temp) = req.temperature {
        body.insert("temperature".to_string(), json!(temp));
    }

    // ── Thinking + Effort 组装 ──
    let level_active = req
        .thinking
        .as_ref()
        .and_then(|t| t.level.as_deref())
        .map(|l| !l.is_empty())
        .unwrap_or(false);
    if level_active {
        // Level 模式：SDK 接管 thinking + effort + maxTokens。
        resolve_thinking_level(&mut body, req, caps);
    } else {
        // 兼容模式：调用方自己拼（保持 v0.8.0 行为）。
        if let Some(thinking) = &req.thinking {
            body.insert(
                "thinking".to_string(),
                serde_json::to_value(thinking).unwrap_or(Value::Null),
            );
        }
        if let Some(effort) = &req.effort {
            body.insert(
                "effort".to_string(),
                serde_json::to_value(effort).unwrap_or(Value::Null),
            );
        }
    }

    // ── Metadata + v1.6.0 endUserId 合并 ──
    // caller 显式 metadata 键（含 user_id）永远优先；endUserId 仅在 metadata 无 user_id 键时填入。
    let has_end_user = req
        .end_user_id
        .as_deref()
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if req.metadata.is_some() || has_end_user {
        let mut meta: Map<String, Value> = Map::new();
        if let Some(m) = &req.metadata {
            for (k, v) in m {
                meta.insert(k.clone(), Value::String(v.clone()));
            }
        }
        if has_end_user && !meta.contains_key("user_id") {
            meta.insert(
                "user_id".to_string(),
                Value::String(req.end_user_id.clone().unwrap()),
            );
        }
        body.insert("metadata".to_string(), Value::Object(meta));
    }

    // ── 合入 tools + serverTools ──
    let mut all_tools: Vec<Value> = Vec::new();
    // tools 透传 —— Go 侧 json.Marshal+Unmarshal 深拷贝，TS/Rust 直接拼接。
    // 仅当为数组时逐元素 push（与 Go/TS silent ignore 非数组行为一致）。
    if let Some(Value::Array(arr)) = &req.tools {
        for t in arr {
            all_tools.push(t.clone());
        }
    }
    if let Some(server_tools) = &req.server_tools {
        for st in server_tools {
            let mut schema: Map<String, Value> = Map::new();
            schema.insert("type".to_string(), Value::String(st.r#type.clone()));
            schema.insert("name".to_string(), Value::String(st.name.clone()));
            if let Some(cfg) = &st.config {
                for (k, v) in cfg {
                    schema.insert(k.clone(), v.clone());
                }
            }
            all_tools.push(Value::Object(schema));
        }
    }
    if !all_tools.is_empty() {
        body.insert("tools".to_string(), Value::Array(all_tools));
    }

    // ── 推理控制（非 Level 模式时的透传）──
    if let Some(speed) = &req.speed {
        if !speed.is_empty() {
            body.insert("speed".to_string(), Value::String(speed.clone()));
        }
    }
    if let Some(oc) = &req.output_config {
        body.insert(
            "output_config".to_string(),
            serde_json::to_value(oc).unwrap_or(Value::Null),
        );
    }

    // ── Beta 自动组装 ──
    let betas = build_betas(caps, req);
    if !betas.is_empty() {
        body.insert(
            "betas".to_string(),
            Value::Array(betas.into_iter().map(Value::String).collect()),
        );
    }

    // ── 透传 extraBody ──
    // Level 模式下 thinking/effort/max_tokens/temperature/betas 已由 SDK 管理；extraBody 中
    // 若含这些 key 会覆盖 SDK 计算结果 —— 强制跳过并告警，保留透传其它字段的能力。
    if let Some(extra) = &req.extra_body {
        for (k, v) in extra {
            if ANTHROPIC_SDK_MANAGED_BODY_KEYS.contains(&k.as_str()) {
                eprintln!(
                    "acosmi-sdk: extraBody key \"{k}\" is SDK-managed and was ignored \
                     (use the dedicated request field instead)."
                );
                continue;
            }
            body.insert(k.clone(), v.clone());
        }
    }

    body
}

/// 解析 Anthropic 格式同步响应。兼容 APIResponse 包装 `{"code":0,"data":{...}}` 和裸 JSON。
pub fn parse_response(body_input: &[u8]) -> Result<ChatResponse> {
    let body_str = String::from_utf8_lossy(body_input);

    // 尝试 APIResponse 包装；data 非空时 code!=0 抛 BusinessError，否则剥出 data 作 raw。
    let raw: String = match serde_json::from_str::<Value>(&body_str) {
        Ok(wrapper) => {
            let data = wrapper.get("data");
            if let Some(data) = data.filter(|v| !v.is_null()) {
                let code = wrapper.get("code").and_then(|v| v.as_i64()).unwrap_or(0);
                if code != 0 {
                    let message = wrapper
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    return Err(Error::business(code, message));
                }
                data.to_string()
            } else {
                // 无 wrapper.data 形态 —— 用原始 bodyStr。
                body_str.to_string()
            }
        }
        // 非 JSON —— 用原始 bodyStr。
        Err(_) => body_str.to_string(),
    };

    let mut resp: ChatResponse = serde_json::from_str(&raw)
        .map_err(|e| Error::other(format!("decode anthropic response: {e}")))?;
    resp.token_remaining = -1;
    resp.call_remaining = -1;
    resp.model_token_remaining = -1;
    resp.model_token_remaining_etu = -1;
    Ok(resp)
}

/// 解析 Anthropic SSE 行。`[DONE]` 哨兵 → done（Nexus Gateway ChatStream 追加）。
pub fn parse_stream_line(event_type: &str, data: &str) -> (StreamEvent, bool) {
    if data == "[DONE]" {
        return (StreamEvent::default(), true);
    }
    (
        StreamEvent {
            event: event_type.to_string(),
            data: data.to_string(),
            ..Default::default()
        },
        false,
    )
}

/// 根据 ThinkingConfig.level 自动组装请求参数。
///
/// off  → thinking=disabled，不设 effort，不动 maxTokens。
/// high → thinking=adaptive，effort=high，maxTokens 至少 32K。
/// max  → thinking=adaptive，effort=max，maxTokens 拉到模型上限。
/// 旧模型不支持 adaptive 时回退 enabled + budget_tokens = maxTokens - 1。
pub fn resolve_thinking_level(
    body: &mut Map<String, Value>,
    req: &ChatRequest,
    caps: &ModelCapabilities,
) {
    let level = req
        .thinking
        .as_ref()
        .and_then(|t| t.level.clone())
        .unwrap_or_default();

    // ── off ──
    if level == THINKING_OFF {
        body.insert("thinking".to_string(), json!({ "type": "disabled" }));
        return;
    }

    // ── 模型不支持任何形式的 thinking → 不动 maxTokens，直接返回 ──
    if !caps.supports_adaptive_thinking && !caps.supports_thinking {
        return;
    }

    // ── 确定 maxTokens ──
    let mut max_tokens = req.max_tokens.unwrap_or(0);
    if max_tokens <= 0 {
        max_tokens = THINKING_HIGH_MIN_MAX_TOKENS;
    }

    if level == THINKING_MAX {
        let mut model_max = caps.max_output_tokens;
        if model_max <= 0 {
            model_max = THINKING_MAX_FALLBACK_MAX_TOKENS;
        }
        if max_tokens < model_max {
            max_tokens = model_max;
        }
    } else if max_tokens < THINKING_HIGH_MIN_MAX_TOKENS {
        max_tokens = THINKING_HIGH_MIN_MAX_TOKENS;
    }
    body.insert("max_tokens".to_string(), Value::from(max_tokens));

    // ── thinking ──
    // adaptive 优先（Claude 4.x）；旧模型回退 enabled + full budget。
    let display = req.thinking.as_ref().and_then(|t| t.display.clone());
    if caps.supports_adaptive_thinking {
        let mut thinking: Map<String, Value> = Map::new();
        thinking.insert("type".to_string(), Value::String("adaptive".to_string()));
        if let Some(d) = &display {
            if !d.is_empty() {
                thinking.insert("display".to_string(), Value::String(d.clone()));
            }
        }
        body.insert("thinking".to_string(), Value::Object(thinking));
    } else if caps.supports_thinking {
        let mut budget = max_tokens - 1;
        if budget < 1024 {
            budget = 1024;
        }
        let mut thinking: Map<String, Value> = Map::new();
        thinking.insert("type".to_string(), Value::String("enabled".to_string()));
        thinking.insert("budget_tokens".to_string(), Value::from(budget));
        if let Some(d) = &display {
            if !d.is_empty() {
                thinking.insert("display".to_string(), Value::String(d.clone()));
            }
        }
        body.insert("thinking".to_string(), Value::Object(thinking));
    }

    // ── effort ──
    // 仅支持 effort 的模型发送此参数。
    if caps.supports_effort {
        let mut effort_level = "high";
        if level == THINKING_MAX && caps.supports_max_effort {
            effort_level = "max";
        }
        body.insert("effort".to_string(), json!({ "level": effort_level }));
    }

    // ── API 约束：thinking 与 temperature 互斥 ──
    body.remove("temperature");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::types::{ChatMessage, ThinkingConfig};
    use serde_json::json;

    fn caps() -> ModelCapabilities {
        ModelCapabilities::default()
    }

    #[test]
    fn build_request_body_key_fields() {
        let req = ChatRequest {
            messages: Some(vec![ChatMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            }]),
            stream: Some(true),
            max_tokens: Some(100),
            system: Some(json!("you are helpful")),
            temperature: Some(0.5),
            end_user_id: Some("u-123".to_string()),
            ..Default::default()
        };
        let body = build_request_body(&caps(), &req);
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["max_tokens"], json!(100));
        assert_eq!(body["system"], json!("you are helpful"));
        assert_eq!(body["temperature"], json!(0.5));
        // endUserId → metadata.user_id
        assert_eq!(body["metadata"]["user_id"], json!("u-123"));
        assert!(body.get("messages").is_some());
    }

    #[test]
    fn extra_body_denylist_skips_sdk_managed_keys() {
        let mut extra = serde_json::Map::new();
        extra.insert("thinking".to_string(), json!({"type": "evil"}));
        extra.insert("max_tokens".to_string(), json!(999999));
        extra.insert("custom_field".to_string(), json!("kept"));
        let req = ChatRequest {
            max_tokens: Some(50),
            extra_body: Some(extra),
            ..Default::default()
        };
        let body = build_request_body(&caps(), &req);
        // SDK-managed keys 不被 extraBody 覆盖。
        assert_eq!(body["max_tokens"], json!(50));
        assert!(body.get("thinking").is_none());
        // 非管理字段透传。
        assert_eq!(body["custom_field"], json!("kept"));
    }

    #[test]
    fn caller_metadata_user_id_wins_over_end_user_id() {
        let mut meta = std::collections::BTreeMap::new();
        meta.insert("user_id".to_string(), "explicit".to_string());
        let req = ChatRequest {
            metadata: Some(meta),
            end_user_id: Some("derived".to_string()),
            ..Default::default()
        };
        let body = build_request_body(&caps(), &req);
        assert_eq!(body["metadata"]["user_id"], json!("explicit"));
    }

    #[test]
    fn parse_stream_line_done() {
        let (ev, done) = parse_stream_line("", "[DONE]");
        assert!(done);
        assert_eq!(ev.event, "");
        let (ev2, done2) = parse_stream_line("content_block_delta", "{}");
        assert!(!done2);
        assert_eq!(ev2.event, "content_block_delta");
    }

    #[test]
    fn thinking_level_off_disables_and_keeps_temperature_path() {
        let mut c = caps();
        c.supports_adaptive_thinking = true;
        let req = ChatRequest {
            temperature: Some(0.7),
            thinking: Some(ThinkingConfig {
                level: Some("off".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let body = build_request_body(&c, &req);
        assert_eq!(body["thinking"], json!({"type": "disabled"}));
        // off 分支不删 temperature。
        assert_eq!(body["temperature"], json!(0.7));
    }

    #[test]
    fn thinking_level_high_adaptive_drops_temperature() {
        let mut c = caps();
        c.supports_adaptive_thinking = true;
        c.supports_effort = true;
        let req = ChatRequest {
            temperature: Some(0.7),
            thinking: Some(ThinkingConfig {
                level: Some("high".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let body = build_request_body(&c, &req);
        assert_eq!(body["thinking"]["type"], json!("adaptive"));
        assert_eq!(body["max_tokens"], json!(32_000));
        assert_eq!(body["effort"]["level"], json!("high"));
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn parse_response_unwraps_business_error() {
        let wrapped = br#"{"code":500,"message":"boom","data":{"id":"x"}}"#;
        let err = parse_response(wrapped).unwrap_err();
        match err {
            Error::Business { code, message } => {
                assert_eq!(code, 500);
                assert_eq!(message, "boom");
            }
            other => panic!("expected Business, got {other:?}"),
        }
    }

    #[test]
    fn parse_response_bare_json_fills_sentinels() {
        let bare = br#"{"id":"m1","type":"message","model":"x","role":"assistant","content":[],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":2}}"#;
        let resp = parse_response(bare).unwrap();
        assert_eq!(resp.token_remaining, -1);
        assert_eq!(resp.call_remaining, -1);
        assert_eq!(resp.model_token_remaining, -1);
        assert_eq!(resp.model_token_remaining_etu, -1);
    }
}
