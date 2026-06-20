//! OpenAI 兼容格式 adapter。端口自 `models/adapters/openai.ts`
//! （其端口自 `acosmi-sdk-go/adapter_openai.go`）。
//!
//! 用于所有非 Anthropic 厂商（DeepSeek, DashScope, Zhipu, Moonshot, VolcEngine 等）。
//! 关键区别：不注入 Anthropic betas / 端点后缀 `/chat` / 流式 `[DONE]` / choices 响应格式。

use crate::models::types::{
    ChatContentBlock, ChatRequest, ChatResponse, ChatUsage, ModelCapabilities, StreamEvent,
    THINKING_HIGH, THINKING_MAX, THINKING_OFF,
};
use crate::models::wire_anthropic::{AnthropicContentBlock, AnthropicResponse, AnthropicUsage};
use crate::models::wire_openai::{OpenAIChatResponse, OpenAIStreamChunk};
use crate::shared::errors::{Error, Result};
use serde_json::{json, Map, Value};

/// 构建 OpenAI 兼容格式请求体。不注入 Anthropic betas，扩展字段以通用 JSON 传递。
pub fn build_request_body(_caps: &ModelCapabilities, req: &ChatRequest) -> Map<String, Value> {
    let mut body: Map<String, Value> = Map::new();

    // ── 消息：直接透传（Gateway 负责最终转换）──
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

    // ── System prompt：透传给 Gateway ──
    if let Some(system) = &req.system {
        body.insert("system".to_string(), system.clone());
    }

    // ── Temperature ──
    if let Some(temp) = req.temperature {
        body.insert("temperature".to_string(), json!(temp));
    }

    // ── Tools：透传原始格式，Gateway adapter 负责格式转换 ──
    if let Some(tools) = &req.tools {
        body.insert("tools".to_string(), tools.clone());
    }

    // ── 扩展字段（v0.13.0：按 OpenAI wire format 直接翻译）──

    // Thinking / Effort → reasoning_effort
    let eff = resolve_openai_reasoning_effort(req);
    if !eff.is_empty() {
        body.insert("reasoning_effort".to_string(), Value::String(eff));
    }

    if let Some(speed) = &req.speed {
        if !speed.is_empty() {
            body.insert("speed".to_string(), Value::String(speed.clone()));
        }
    }

    // outputConfig → response_format
    if let Some(rf) = resolve_openai_response_format(req) {
        body.insert("response_format".to_string(), Value::Object(rf));
    }

    if let Some(m) = &req.metadata {
        body.insert(
            "metadata".to_string(),
            serde_json::to_value(m).unwrap_or(Value::Null),
        );
    }

    // parallel_tool_calls 是 OpenAI 原生字段，无歧义直接写。
    if let Some(ptc) = req.parallel_tool_calls {
        body.insert("parallel_tool_calls".to_string(), Value::Bool(ptc));
    }

    // ── 不注入 Anthropic Betas ──

    // ── 透传 extraBody ──
    if let Some(extra) = &req.extra_body {
        for (k, v) in extra {
            body.insert(k.clone(), v.clone());
        }
    }

    // ── v1.6.0：endUserId → 顶层 body["user_id"]（OpenAI wire 形态）──
    // 优先级最高：在 extraBody 之后写入，即便 caller 通过 extraBody["user_id"] 自填，显式 endUserId 胜出。
    if let Some(uid) = &req.end_user_id {
        if !uid.is_empty() {
            body.insert("user_id".to_string(), Value::String(uid.clone()));
        }
    }

    // ── 流式选项 ──
    if req.stream == Some(true) {
        body.insert(
            "stream_options".to_string(),
            json!({ "include_usage": true }),
        );
    }

    body
}

/// 解析 OpenAI 格式同步响应为 [`ChatResponse`]。兼容 APIResponse 包装和裸 OpenAI JSON。
pub fn parse_response(body_input: &[u8]) -> Result<ChatResponse> {
    let raw = unwrap_api_response(body_input)?;
    let oai: OpenAIChatResponse = serde_json::from_str(&raw)
        .map_err(|e| Error::other(format!("decode openai response: {e}")))?;
    Ok(convert_openai_to_chat_response(&oai))
}

/// 解析 OpenAI SSE 行。`[DONE]` 标记流结束；非 `[DONE]` 行校验是合法 JSON（对齐 Go 行为）。
pub fn parse_stream_line(event_type: &str, data: &str) -> Result<(StreamEvent, bool)> {
    if data == "[DONE]" {
        return Ok((StreamEvent::default(), true));
    }
    // 校验 chunk 是合法 JSON。
    serde_json::from_str::<Value>(data)
        .map_err(|e| Error::other(format!("parse openai stream chunk: {e}")))?;
    Ok((
        StreamEvent {
            event: event_type.to_string(),
            data: data.to_string(),
            ..Default::default()
        },
        false,
    ))
}

/// 把 thinking/effort 翻译成 OpenAI `reasoning_effort` 字段值。空串表示不设置。
pub fn resolve_openai_reasoning_effort(req: &ChatRequest) -> String {
    // effort 优先级最高（本身就是通用级别语义）。
    if let Some(effort) = &req.effort {
        if !effort.level.is_empty() {
            match effort.level.as_str() {
                "low" | "medium" | "high" => return effort.level.clone(),
                // OpenAI 无 max 级别，等价最深 = high。
                "max" => return "high".to_string(),
                _ => {}
            }
        }
    }
    // thinking.level 次之。
    if let Some(thinking) = &req.thinking {
        match thinking.level.as_deref() {
            Some(THINKING_HIGH) => return "high".to_string(),
            Some(THINKING_MAX) => return "high".to_string(),
            Some(THINKING_OFF) => return String::new(),
            _ => {}
        }
    }
    String::new()
}

/// 把 outputConfig 翻译成 OpenAI response_format。返回 `None` 表示不设置。
pub fn resolve_openai_response_format(req: &ChatRequest) -> Option<Map<String, Value>> {
    let oc = req.output_config.as_ref()?;
    match oc.format.as_deref() {
        Some("json_schema") => {
            // OpenAI schema 形态：{type:"json_schema", json_schema:{schema:{...},strict:true}}
            let mut js: Map<String, Value> = Map::new();
            if let Some(schema) = &oc.schema {
                js.insert("schema".to_string(), schema.clone());
            }
            js.insert("strict".to_string(), Value::Bool(true));
            let mut out: Map<String, Value> = Map::new();
            out.insert("type".to_string(), Value::String("json_schema".to_string()));
            out.insert("json_schema".to_string(), Value::Object(js));
            Some(out)
        }
        Some("json_object") => {
            let mut out: Map<String, Value> = Map::new();
            out.insert("type".to_string(), Value::String("json_object".to_string()));
            Some(out)
        }
        Some("") | None => None,
        Some(other) => {
            // 未知 format，原样透传，交 Gateway 处理。
            let mut out: Map<String, Value> = Map::new();
            out.insert("type".to_string(), Value::String(other.to_string()));
            Some(out)
        }
    }
}

/// 剥 APIResponse 包装。data 非空 + code!=0 抛 BusinessError；否则返回 data 字符串或原文。
fn unwrap_api_response(body_input: &[u8]) -> Result<String> {
    let body_str = String::from_utf8_lossy(body_input);
    match serde_json::from_str::<Value>(&body_str) {
        Ok(wrapper) => {
            if let Some(data) = wrapper.get("data").filter(|v| !v.is_null()) {
                let code = wrapper.get("code").and_then(|v| v.as_i64()).unwrap_or(0);
                if code != 0 {
                    let message = wrapper
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    return Err(Error::business(code, message));
                }
                Ok(data.to_string())
            } else {
                Ok(body_str.to_string())
            }
        }
        Err(_) => Ok(body_str.to_string()),
    }
}

/// 将 OpenAI 同步响应转换为 [`ChatResponse`]。
fn convert_openai_to_chat_response(oai: &OpenAIChatResponse) -> ChatResponse {
    let mut resp = ChatResponse {
        id: oai.id.clone(),
        r#type: "message".to_string(),
        model: oai.model.clone(),
        role: "assistant".to_string(),
        content: Vec::new(),
        stop_reason: String::new(),
        usage: ChatUsage {
            input_tokens: oai.usage.prompt_tokens,
            output_tokens: oai.usage.completion_tokens,
            ..Default::default()
        },
        token_remaining: -1,
        call_remaining: -1,
        model_token_remaining: -1,
        model_token_remaining_etu: -1,
    };

    if let Some(choice) = oai.choices.first() {
        // finish_reason 映射。
        resp.stop_reason = map_finish_reason(&choice.finish_reason);

        // thinking content → thinking block。
        if let Some(rc) = &choice.message.reasoning_content {
            if !rc.is_empty() {
                resp.content.push(ChatContentBlock {
                    r#type: "thinking".to_string(),
                    thinking: Some(rc.clone()),
                    ..Default::default()
                });
            }
        }

        // text content → text block。
        if !choice.message.content.is_empty() {
            resp.content.push(ChatContentBlock {
                r#type: "text".to_string(),
                text: Some(choice.message.content.clone()),
                ..Default::default()
            });
        }

        // tool_calls → tool_use blocks。
        if let Some(tcs) = &choice.message.tool_calls {
            for tc in tcs {
                resp.content.push(ChatContentBlock {
                    r#type: "tool_use".to_string(),
                    id: Some(tc.id.clone()),
                    name: Some(tc.function.name.clone()),
                    // OpenAI arguments 是 string，尝试解析为 JSON value（失败保留原串）。
                    input: Some(try_parse_json(&tc.function.arguments)),
                    ..Default::default()
                });
            }
        }
    }

    resp
}

/// finish_reason → Anthropic stop_reason。
fn map_finish_reason(finish_reason: &str) -> String {
    match finish_reason {
        "stop" => "end_turn".to_string(),
        "tool_calls" => "tool_use".to_string(),
        "length" => "max_tokens".to_string(),
        other => other.to_string(),
    }
}

/// 尝试解析为 JSON value，失败时返回原串作 JSON string。
fn try_parse_json(s: &str) -> Value {
    serde_json::from_str::<Value>(s).unwrap_or_else(|_| Value::String(s.to_string()))
}

/// 解析 OpenAI 格式响应并转换为 [`AnthropicResponse`]（供 chatMessagesOpenAI 使用）。
pub fn parse_openai_response_to_anthropic(raw: &[u8]) -> Result<AnthropicResponse> {
    let data = unwrap_api_response(raw)?;
    let oai: OpenAIChatResponse = serde_json::from_str(&data)
        .map_err(|e| Error::other(format!("decode openai response: {e}")))?;

    let mut resp = AnthropicResponse {
        id: oai.id.clone(),
        r#type: "message".to_string(),
        role: "assistant".to_string(),
        content: Vec::new(),
        model: oai.model.clone(),
        stop_reason: String::new(),
        stop_sequence: None,
        usage: AnthropicUsage {
            input_tokens: oai.usage.prompt_tokens,
            output_tokens: oai.usage.completion_tokens,
            ..Default::default()
        },
    };

    if let Some(choice) = oai.choices.first() {
        resp.stop_reason = map_finish_reason(&choice.finish_reason);

        if let Some(rc) = &choice.message.reasoning_content {
            if !rc.is_empty() {
                resp.content.push(AnthropicContentBlock {
                    r#type: "thinking".to_string(),
                    thinking: Some(rc.clone()),
                    ..Default::default()
                });
            }
        }

        if !choice.message.content.is_empty() {
            resp.content.push(AnthropicContentBlock {
                r#type: "text".to_string(),
                text: Some(choice.message.content.clone()),
                ..Default::default()
            });
        }

        if let Some(tcs) = &choice.message.tool_calls {
            for tc in tcs {
                resp.content.push(AnthropicContentBlock {
                    r#type: "tool_use".to_string(),
                    id: Some(tc.id.clone()),
                    name: Some(tc.function.name.clone()),
                    input: Some(try_parse_json(&tc.function.arguments)),
                    ..Default::default()
                });
            }
        }
    }

    Ok(resp)
}

// ============================================================================
// OpenAI SSE → Anthropic 事件转换器（供 chatMessagesStreamInternal 使用）
// ============================================================================

/// 将 OpenAI SSE chunks 转换为 Anthropic 兼容的 [`StreamEvent`]。
/// 有状态：跨 chunk 追踪 block 索引。
#[derive(Debug, Default)]
pub struct OpenAIStreamConverter {
    message_started: bool,
    thinking_started: bool,
    thinking_stopped: bool,
    /// thinking block 打开时占用的 Anthropic block index —— 关闭时必须用它，
    /// 不能用可能已被 text/tool 推进的 `block_index`（否则 content_block_stop 索引错配）。
    thinking_block_index: i64,
    text_started: bool,
    /// OpenAI tool_call index → Anthropic block index。
    /// 用插入有序的 `Vec<(key, value)>` 复刻 JS `Map` 的迭代顺序（finish 时按插入序关闭 tool block）。
    tool_block_index: Vec<(i64, i64)>,
    block_index: i64,
}

impl OpenAIStreamConverter {
    /// 新建转换器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 将一行 OpenAI SSE data 转换为零或多个 Anthropic 格式 StreamEvent。返回 `(events, done)`。
    pub fn convert(&mut self, data: &str) -> Result<(Vec<StreamEvent>, bool)> {
        if data == "[DONE]" {
            return Ok((Vec::new(), true));
        }

        let chunk: OpenAIStreamChunk = serde_json::from_str(data)
            .map_err(|e| Error::other(format!("parse openai stream chunk: {e}")))?;

        let mut events: Vec<StreamEvent> = Vec::new();
        let choice = match chunk.choices.first() {
            Some(c) => c,
            None => return Ok((events, false)),
        };

        // 首个 chunk：发送 message_start。
        if !self.message_started {
            self.message_started = true;
            let msg_json = json!({
                "type": "message_start",
                "message": {
                    "id": chunk.id,
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": "",
                },
            })
            .to_string();
            events.push(ev("message_start", msg_json));
        }

        // thinking delta（reasoning_content）。
        if let Some(rc) = choice.delta.reasoning_content.as_deref() {
            if !rc.is_empty() {
                if !self.thinking_started {
                    self.thinking_started = true;
                    self.thinking_block_index = self.block_index; // 记下 thinking 占用的 index
                    let block_json = json!({
                        "type": "content_block_start",
                        "index": self.block_index,
                        "content_block": { "type": "thinking", "thinking": "" },
                    })
                    .to_string();
                    events.push(ev("content_block_start", block_json));
                }
                let delta_json = json!({
                    "type": "content_block_delta",
                    "index": self.thinking_block_index,
                    "delta": { "type": "thinking_delta", "thinking": rc },
                })
                .to_string();
                events.push(ev("content_block_delta", delta_json));
            }
        }

        // text delta（content）。
        if let Some(content) = choice.delta.content.as_deref() {
            if !content.is_empty() {
                // 关闭 thinking block（如果有）—— 用 thinking_block_index 关，不用可能已推进的 block_index。
                if self.thinking_started && !self.thinking_stopped {
                    self.thinking_stopped = true;
                    let stop_json = json!({
                        "type": "content_block_stop",
                        "index": self.thinking_block_index,
                    })
                    .to_string();
                    events.push(ev("content_block_stop", stop_json));
                    self.block_index += 1;
                }
                if !self.text_started {
                    self.text_started = true;
                    let block_json = json!({
                        "type": "content_block_start",
                        "index": self.block_index,
                        "content_block": { "type": "text", "text": "" },
                    })
                    .to_string();
                    events.push(ev("content_block_start", block_json));
                }
                let delta_json = json!({
                    "type": "content_block_delta",
                    "index": self.block_index,
                    "delta": { "type": "text_delta", "text": content },
                })
                .to_string();
                events.push(ev("content_block_delta", delta_json));
            }
        }

        // tool_calls delta。
        if let Some(tcs) = &choice.delta.tool_calls {
            for tc in tcs {
                if !self.tool_block_index.iter().any(|(k, _)| *k == tc.index) {
                    // 关闭仍打开的 thinking block（镜像 text 分支）。用 thinking_block_index 关。
                    if self.thinking_started && !self.thinking_stopped {
                        self.thinking_stopped = true;
                        let stop_json = json!({
                            "type": "content_block_stop",
                            "index": self.thinking_block_index,
                        })
                        .to_string();
                        events.push(ev("content_block_stop", stop_json));
                        self.block_index += 1;
                    }
                    // 关闭 text block（如果有）。
                    if self.text_started {
                        let stop_json = json!({
                            "type": "content_block_stop",
                            "index": self.block_index,
                        })
                        .to_string();
                        events.push(ev("content_block_stop", stop_json));
                        self.block_index += 1;
                        self.text_started = false;
                    }
                    self.tool_block_index.push((tc.index, self.block_index));
                    let block_json = json!({
                        "type": "content_block_start",
                        "index": self.block_index,
                        "content_block": {
                            "type": "tool_use",
                            "id": tc.id.clone().unwrap_or_default(),
                            "name": tc.function.name,
                            "input": {},
                        },
                    })
                    .to_string();
                    events.push(ev("content_block_start", block_json));
                    self.block_index += 1; // 递增，为下一个 tool_call block 预留索引
                }
                if !tc.function.arguments.is_empty() {
                    let idx = self
                        .tool_block_index
                        .iter()
                        .find(|(k, _)| *k == tc.index)
                        .map(|(_, v)| *v)
                        .unwrap();
                    let delta_json = json!({
                        "type": "content_block_delta",
                        "index": idx,
                        "delta": {
                            "type": "input_json_delta",
                            "partial_json": tc.function.arguments,
                        },
                    })
                    .to_string();
                    events.push(ev("content_block_delta", delta_json));
                }
            }
        }

        // finish_reason：关闭所有 block + message_delta + message_stop。
        let finish = choice.finish_reason.as_deref().unwrap_or("");
        if !finish.is_empty() {
            // 关闭可能仍打开的 block。
            if self.text_started {
                let stop_json = json!({
                    "type": "content_block_stop",
                    "index": self.block_index,
                })
                .to_string();
                events.push(ev("content_block_stop", stop_json));
            } else if self.thinking_started && !self.thinking_stopped {
                // 用 thinking_block_index 关 —— thinking-only 流末尾若有 tool block 推进过 block_index，
                // 这里仍要用 thinking 自己打开时记下的 index，否则错配。
                self.thinking_stopped = true;
                let stop_json = json!({
                    "type": "content_block_stop",
                    "index": self.thinking_block_index,
                })
                .to_string();
                events.push(ev("content_block_stop", stop_json));
            }
            // 关闭 tool blocks（按插入序，复刻 JS Map 迭代序）。
            for (_, idx) in &self.tool_block_index {
                let stop_json = json!({
                    "type": "content_block_stop",
                    "index": idx,
                })
                .to_string();
                events.push(ev("content_block_stop", stop_json));
            }

            // stop_reason 映射。
            let stop_reason = match finish {
                "tool_calls" => "tool_use",
                "length" => "max_tokens",
                _ => "end_turn",
            };

            let delta_json = json!({
                "type": "message_delta",
                "delta": { "stop_reason": stop_reason },
            })
            .to_string();
            events.push(ev("message_delta", delta_json));

            let stop_json = json!({ "type": "message_stop" }).to_string();
            events.push(ev("message_stop", stop_json));
        }

        Ok((events, false))
    }
}

/// 工厂函数（对齐 TS `newOpenAIStreamConverter`）。
pub fn new_openai_stream_converter() -> OpenAIStreamConverter {
    OpenAIStreamConverter::new()
}

fn ev(event: &str, data: String) -> StreamEvent {
    StreamEvent {
        event: event.to_string(),
        data,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::types::{ChatMessage, EffortConfig};
    use serde_json::json;

    fn caps() -> ModelCapabilities {
        ModelCapabilities::default()
    }

    #[test]
    fn build_request_body_openai_wire_fields() {
        let req = ChatRequest {
            messages: Some(vec![ChatMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            }]),
            stream: Some(true),
            max_tokens: Some(64),
            parallel_tool_calls: Some(false),
            end_user_id: Some("u-9".to_string()),
            effort: Some(EffortConfig {
                level: "high".to_string(),
            }),
            ..Default::default()
        };
        let body = build_request_body(&caps(), &req);
        // parallelToolCalls → parallel_tool_calls (snake)
        assert_eq!(body["parallel_tool_calls"], json!(false));
        // endUserId → 顶层 user_id
        assert_eq!(body["user_id"], json!("u-9"));
        // effort → reasoning_effort
        assert_eq!(body["reasoning_effort"], json!("high"));
        // stream → stream_options.include_usage
        assert_eq!(body["stream_options"], json!({"include_usage": true}));
        // 不注入 anthropic betas
        assert!(body.get("betas").is_none());
    }

    #[test]
    fn end_user_id_wins_over_extra_body_user_id() {
        let mut extra = serde_json::Map::new();
        extra.insert("user_id".to_string(), json!("from-extra"));
        let req = ChatRequest {
            extra_body: Some(extra),
            end_user_id: Some("explicit".to_string()),
            ..Default::default()
        };
        let body = build_request_body(&caps(), &req);
        assert_eq!(body["user_id"], json!("explicit"));
    }

    #[test]
    fn parse_stream_line_done_and_invalid() {
        let (_, done) = parse_stream_line("", "[DONE]").unwrap();
        assert!(done);
        // 合法 JSON chunk → 不 done。
        let (ev, d) = parse_stream_line("message", "{}").unwrap();
        assert!(!d);
        assert_eq!(ev.event, "message");
        // 非法 JSON → Err（对齐 Go）。
        assert!(parse_stream_line("message", "not json").is_err());
    }

    #[test]
    fn convert_openai_to_chat_response_maps_finish_reason() {
        let body = br#"{"id":"c1","object":"chat.completion","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":"hello"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":5,"total_tokens":8}}"#;
        let resp = parse_response(body).unwrap();
        assert_eq!(resp.stop_reason, "end_turn");
        assert_eq!(resp.content.len(), 1);
        assert_eq!(resp.content[0].r#type, "text");
        assert_eq!(resp.usage.input_tokens, 3);
        assert_eq!(resp.token_remaining, -1);
    }

    #[test]
    fn stream_converter_emits_message_start_then_done() {
        let mut conv = new_openai_stream_converter();
        let chunk = r#"{"id":"c","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"hi"},"finish_reason":null}]}"#;
        let (events, done) = conv.convert(chunk).unwrap();
        assert!(!done);
        // 首 chunk 含 message_start + content_block_start + content_block_delta。
        assert_eq!(events[0].event, "message_start");
        assert!(events.iter().any(|e| e.event == "content_block_delta"));
        // [DONE]
        let (_, d) = conv.convert("[DONE]").unwrap();
        assert!(d);
    }
}
