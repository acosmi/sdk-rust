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
use crate::models::wire_openai::{OpenAIChatResponse, OpenAIStreamChunk, OpenAIStreamToolCall};
use crate::shared::errors::{Error, Result};
use serde_json::{json, Map, Number, Value};

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
    let eff = resolve_openai_reasoning_effort_with_max(req, _caps.supports_max_effort);
    if !eff.is_empty() {
        body.insert("reasoning_effort".to_string(), Value::String(eff));
    }
    if _caps.supports_thinking
        && req.thinking.as_ref().and_then(|t| t.level.as_deref()) == Some(THINKING_OFF)
    {
        body.insert("thinking".to_string(), json!({"type":"disabled"}));
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
    resolve_openai_reasoning_effort_with_max(req, false)
}

fn resolve_openai_reasoning_effort_with_max(req: &ChatRequest, supports_max: bool) -> String {
    let max_effort = if supports_max { "max" } else { "high" };
    // effort 优先级最高（本身就是通用级别语义）。
    if let Some(effort) = &req.effort {
        if !effort.level.is_empty() {
            match effort.level.as_str() {
                "low" | "medium" | "high" | "xhigh" => return effort.level.clone(),
                // OpenAI 无 max 级别，等价最深 = high。
                "max" => return max_effort.to_string(),
                _ => {}
            }
        }
    }
    // thinking.level 次之。
    if let Some(thinking) = &req.thinking {
        match thinking.level.as_deref() {
            Some("low") => return "low".to_string(),
            Some("medium") => return "medium".to_string(),
            Some("xhigh") => return "xhigh".to_string(),
            Some(THINKING_HIGH) => return "high".to_string(),
            Some(THINKING_MAX) => return max_effort.to_string(),
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
                    // `name` 已是 `Option`：上游没给就不编一个空串出来（对齐 TS 的 `fn?.name`）。
                    name: tc.function.name.clone(),
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
                    name: tc.function.name.clone(),
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

/// 收尾 `message_delta` 上的 usage。只含与非流式路径相同的两个键；某个键为 `None` 表示上游没给
/// 该计数，不是 0。对应 TS `models/adapters/openai.ts` 的 `StreamMessageUsage`。
#[derive(Debug)]
struct StreamMessageUsage {
    input_tokens: Option<Number>,
    output_tokens: Option<Number>,
}

impl StreamMessageUsage {
    /// 序列化为 `message_delta.usage` 对象，只写在场的键。
    fn to_json(&self) -> Value {
        let mut usage = Map::new();
        if let Some(n) = &self.input_tokens {
            usage.insert("input_tokens".to_string(), Value::Number(n.clone()));
        }
        if let Some(n) = &self.output_tokens {
            usage.insert("output_tokens".to_string(), Value::Number(n.clone()));
        }
        Value::Object(usage)
    }
}

/// 读出一帧流式 chunk 上的 usage 对象，搬成收尾 `message_delta` 的 usage 形态。对应 TS
/// `models/adapters/openai.ts` 的 `readStreamUsage`。
///
/// 字段映射与同文件非流式路径（`convert_openai_to_chat_response` /
/// `parse_openai_response_to_anthropic`）逐字相同：只搬 `prompt_tokens → input_tokens`、
/// `completion_tokens → output_tokens`，数值原样。`cached_tokens` / `reasoning_tokens` 等明细刻意
/// 不映射，也不做 `prompt_tokens − cached` 之类的净额换算——usage 的语义归一只在网关，SDK 只做
/// 格式搬运。某个计数缺席或不是 JSON 数字时不写对应键。
///
/// 入参是帧上原始的 `usage` 值，而不是 `OpenAIStreamChunk::usage`：后者的 `OpenAIUsage` 要求
/// 三个计数都在且为整数，既表达不了「计数缺席」，遇到缺 `total_tokens` 之类的形态还会让整帧
/// 反序列化失败。
///
/// 帧上没有 usage 对象（缺失 / `null` / 非对象）返回 `None`：调用方据此区分「这帧没带 usage」与
/// 「带了 usage 但计数都缺席」（后者返回两个键都为 `None` 的值，仍算见过 usage）。
fn read_stream_usage(raw: Option<&Value>) -> Option<StreamMessageUsage> {
    let usage = raw?.as_object()?;
    let count = |key: &str| match usage.get(key) {
        Some(Value::Number(n)) => Some(n.clone()),
        _ => None,
    };
    Some(StreamMessageUsage {
        input_tokens: count("prompt_tokens"),
        output_tokens: count("completion_tokens"),
    })
}

/// 一条 tool_call delta 归属的块键（对应 TS `resolveToolKey` 的 `string | number` 键）。
///
/// 两个变体在语义上是两个不相交的命名空间：TS 用 `id:` 前缀让字符串键不可能撞上数字键，Rust 直接
/// 用枚举表达同一件事。**不导出** —— 它是转换器的内部归并键，不是 wire 契约。
#[derive(Debug, Clone, PartialEq, Eq)]
enum ToolKey {
    /// 上游给了 `index`：规范形态，同一次工具调用的所有分片都带同一个 index。
    Index(i64),
    /// 上游省了 `index` 但给了非空 `id`。
    Id(String),
}

/// 将 OpenAI SSE chunks 转换为 Anthropic 兼容的 [`StreamEvent`]。
/// 有状态：跨 chunk 追踪 block 索引，以及为等待 usage 尾帧而推迟的收尾。
///
/// 流在没有 `[DONE]` 的情况下正常结束（EOF）时，驱动方须在读循环结束后调用一次
/// [`OpenAIStreamConverter::flush`]，否则推迟中的 `message_delta` + `message_stop` 永远不会发出。
#[derive(Debug, Default)]
pub struct OpenAIStreamConverter {
    message_started: bool,
    thinking_started: bool,
    thinking_stopped: bool,
    /// thinking block 打开时占用的 Anthropic block index —— 关闭时必须用它，
    /// 不能用可能已被 text/tool 推进的 `block_index`（否则 content_block_stop 索引错配）。
    thinking_block_index: i64,
    text_started: bool,
    /// OpenAI tool_call 键 → Anthropic block index。
    /// 用插入有序的 `Vec<(key, value)>` 复刻 JS `Map` 的迭代顺序（finish 时按插入序关闭 tool block）。
    tool_block_index: Vec<(ToolKey, i64)>,
    /// 每个 tool block 已发出的 `partial_json` 累积，用于识别「每片重发全量参数」的上游
    /// （对应 TS `toolArgsAccum`）。键必须与 [`Self::tool_block_index`] 取自**同一次**
    /// [`Self::resolve_tool_key`] 调用，否则去重会挂在错误的 tool 上 —— 症状是「参数偶尔少一段」，
    /// 比不修更难查。
    tool_args_accum: Vec<(ToolKey, String)>,
    /// 上一次解析出的 tool 键，供既缺 `index` 又缺 `id` 的后续增量沿用（对应 TS `lastToolKey`）。
    last_tool_key: Option<ToolKey>,
    block_index: i64,
    /// 已发出 message_delta/message_stop（对应 TS `messageClosed`）：finish_reason、usage 尾帧、
    /// `[DONE]` 与 `flush` 共用它防重复收口——整条流恰好一个 message_stop。
    message_closed: bool,
    /// 已为仍打开的块发出 content_block_stop（对应 TS `blocksClosed`）。与 `message_closed` 分开记：
    /// 块在 finish_reason 帧上就关，message_delta/message_stop 却可能推迟到之后的帧。
    blocks_closed: bool,
    /// finish_reason 已到、但 message_delta/message_stop 因等待 usage 尾帧而推迟时记下的 stop_reason
    /// （对应 TS `pendingStopReason`）；没有推迟中的收尾时为 `None`。
    ///
    /// 类型是 `Option<String>` 而非 `Option<&'static str>`：未列出的 `finish_reason`
    /// （`content_filter` / `function_call` …）要原样透传，`&'static str` 结构上装不下来自帧的动态串，
    /// 于是只能压成 `end_turn` —— 把内容审查拦截伪装成正常结束。
    pending_stop_reason: Option<String>,
    /// 最近一次带 usage 对象的帧搬出的 usage，后到覆盖先到（对应 TS `usage`）；整条流从未出现
    /// usage 对象时为 `None`，收尾 message_delta 据此不写 usage 键。
    usage: Option<StreamMessageUsage>,
}

impl OpenAIStreamConverter {
    /// 新建转换器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 解析一条 tool_call delta 归属的块键（对应 TS `resolveToolKey`）。
    ///
    /// OpenAI 流式规范里 `index` 必填，但兼容实现常省略。此前键直接是 `tc.index: i64`：两个都省略
    /// index 的 tool_call 会共用键 `0`，于是只开一个块、两段参数拼进同一条 `partial_json` 流，产出
    /// `{…}{…}` 这种必然非法的 JSON，第二个调用的 id / name 彻底丢失。
    ///
    /// 三级降级让至少一种稳定标识生效：`index` → `id:<id>` → 沿用上一个键；三者皆无时落 `Index(0)`
    /// 并记为「上一个键」，于是同一串无标识的续片至少归到同一个块上。
    fn resolve_tool_key(&mut self, tc: &OpenAIStreamToolCall) -> ToolKey {
        if let Some(index) = tc.index {
            let key = ToolKey::Index(index);
            self.last_tool_key = Some(key.clone());
            return key;
        }
        if let Some(id) = tc.id.as_deref().filter(|id| !id.is_empty()) {
            let key = ToolKey::Id(id.to_string());
            self.last_tool_key = Some(key.clone());
            return key;
        }
        if let Some(key) = &self.last_tool_key {
            return key.clone();
        }
        let key = ToolKey::Index(0);
        self.last_tool_key = Some(key.clone());
        key
    }

    /// 取某个 tool 键上已累积的 `partial_json`；没发过则为空串（对应 TS `?? ''`）。
    fn tool_args_accum_of(&self, key: &ToolKey) -> String {
        self.tool_args_accum
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }

    /// 写回某个 tool 键上的累积值，保持插入序（对应 TS `Map::set`）。
    fn set_tool_args_accum(&mut self, key: &ToolKey, value: String) {
        match self.tool_args_accum.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => self.tool_args_accum.push((key.clone(), value)),
        }
    }

    /// 关闭仍打开的 text / thinking / tool 块，整条流只关一次（对应 TS `closeContentBlocks`）。
    ///
    /// 由 finish_reason 分支与 `[DONE]` 分支共用。此前关块与 message_delta/message_stop 是
    /// finish_reason 分支里同一时刻的事；usage 尾帧要求收尾推迟，于是拆成本方法与
    /// `emit_message_end` 两段，各自防重。
    fn close_content_blocks(&mut self, events: &mut Vec<StreamEvent>) {
        if self.blocks_closed {
            return;
        }
        self.blocks_closed = true;

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
    }

    /// 发出 message_delta + message_stop，整条流只发一次；同时清掉推迟中的收尾（对应 TS
    /// `emitMessageEnd`）。
    ///
    /// 见过 usage 对象就把它放进 message_delta：usage 必须出现在唯一的 message_stop 之前，
    /// message_stop 之后下游已无处安放用量。整条流从未出现 usage 对象时不写 usage 键——缺席
    /// 表示「上游没给」，不是 0。
    fn emit_message_end(&mut self, events: &mut Vec<StreamEvent>, stop_reason: &str) {
        if self.message_closed {
            return;
        }
        self.message_closed = true;
        self.pending_stop_reason = None;

        let mut delta_json = json!({
            "type": "message_delta",
            "delta": { "stop_reason": stop_reason },
        });
        if let Some(usage) = &self.usage {
            delta_json["usage"] = usage.to_json();
        }
        events.push(ev("message_delta", delta_json.to_string()));

        let stop_json = json!({ "type": "message_stop" }).to_string();
        events.push(ev("message_stop", stop_json));
    }

    /// 将一行 OpenAI SSE data 转换为零或多个 Anthropic 格式 StreamEvent。返回 `(events, done)`。
    pub fn convert(&mut self, data: &str) -> Result<(Vec<StreamEvent>, bool)> {
        if data == "[DONE]" {
            // 上游可能在 `[DONE]` 之前不发 finish_reason（部分兼容实现、被中断的流）。此前这里直接
            // 返回空事件，已打开的块永不闭合，下游也收不到 message_stop。
            let mut events: Vec<StreamEvent> = Vec::new();
            if let Some(stop_reason) = self.pending_stop_reason.clone() {
                // finish_reason 已到而 usage 尾帧始终没来：流已声明结束，推迟的收尾不能再等
                // （块已在 finish_reason 帧上关过）。
                self.emit_message_end(&mut events, &stop_reason);
            } else if self.message_started {
                self.close_content_blocks(&mut events);
                self.emit_message_end(&mut events, "end_turn");
            }
            return Ok((events, true));
        }

        let mut frame: Value = serde_json::from_str(data)
            .map_err(|e| Error::other(format!("parse openai stream chunk: {e}")))?;
        // usage 按原始 JSON 读（见 `read_stream_usage`），先从帧上取下再做类型化反序列化：
        // 不让 `OpenAIUsage` 的必填计数决定整帧能否解析。
        let raw_usage = frame.as_object_mut().and_then(|obj| obj.remove("usage"));
        let chunk: OpenAIStreamChunk = serde_json::from_value(frame)
            .map_err(|e| Error::other(format!("parse openai stream chunk: {e}")))?;

        // usage 必须在「没有 choices 就返回」之前读：`stream_options.include_usage` 的尾帧恰恰是
        // `{"choices":[],"usage":{...}}`。此前先判 choices 再返回、且从不读 usage，尾帧整帧丢弃，
        // 经本 SDK 走 OpenAI 线的流式调用 usage 恒缺。对应 TS `OpenAIStreamConverter.convert`。
        let frame_carries_usage = match read_stream_usage(raw_usage.as_ref()) {
            Some(usage) => {
                self.usage = Some(usage); // 后到覆盖先到
                true
            }
            None => false,
        };

        let mut events: Vec<StreamEvent> = Vec::new();
        let choice = match chunk.choices.first() {
            Some(c) => c,
            None => {
                // 没有 choices 的 data 帧（网关错误契约帧、usage 尾帧）不产出内容事件：带 usage 且有
                // 推迟中的收尾时在这里补发，不带 usage 的零事件。
                if frame_carries_usage {
                    if let Some(stop_reason) = self.pending_stop_reason.clone() {
                        self.emit_message_end(&mut events, &stop_reason);
                    }
                }
                return Ok((events, false));
            }
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
                    // 关闭仍打开的 text block（镜像 tool_calls 分支）：chunk 顺序 content →
                    // reasoning_content 时 text 仍开着且 block_index 未推进，若不在此关闭并递增，
                    // thinking block 会与 text 撞 index 0 —— 两个 content_block_start 落在同一个
                    // index 上，且收尾时 `close_content_blocks` 走 text 分支，thinking 块永不闭合。
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
                // 归并键只解析**一次**：建块、去重累积、查块三处必须是同一个值，
                // 否则去重会挂在与块不同的 tool 上。
                let tool_key = self.resolve_tool_key(tc);
                if !self.tool_block_index.iter().any(|(k, _)| *k == tool_key) {
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
                    self.tool_block_index
                        .push((tool_key.clone(), self.block_index));
                    // `content_block` 按键条件插入，不用 `json!` 字面量：`json!({"name": opt})` 在
                    // `opt == None` 时写出 `"name": null` 而**不是**省略该键，与 TS
                    // `JSON.stringify` 对 `undefined` 的行为不同。上游没给 id / name 时写空串或
                    // `null` 都是在替它发明一个值 —— 下游按「有这个键」判有效就会收到空 id。
                    let mut content_block = Map::new();
                    content_block.insert("type".to_string(), Value::String("tool_use".to_string()));
                    if let Some(id) = &tc.id {
                        content_block.insert("id".to_string(), Value::String(id.clone()));
                    }
                    if let Some(name) = &tc.function.name {
                        content_block.insert("name".to_string(), Value::String(name.clone()));
                    }
                    content_block.insert("input".to_string(), Value::Object(Map::new()));
                    let block_json = json!({
                        "type": "content_block_start",
                        "index": self.block_index,
                        "content_block": content_block,
                    })
                    .to_string();
                    events.push(ev("content_block_start", block_json));
                    self.block_index += 1; // 递增，为下一个 tool_call block 预留索引
                }
                let raw_args = tc.function.arguments.as_str();
                if !raw_args.is_empty() {
                    // 累计 vs 增量判别（对应 TS `toolArgsAccum`）：部分上游每个 chunk 重发**全量**
                    // 参数而非增量。此前无条件原样发，下游 `input += partial_json` 拼出
                    // `{"a":{"a":1{"a":1}` 这种必然非法的 JSON。判据取最保守的一种：新片严格以已
                    // 累积内容为前缀**且**更长时，才认为上游在重发全量，只发差值。真增量流里某一
                    // 片恰好等于「此前全部内容的延长」概率可忽略。
                    let accum = self.tool_args_accum_of(&tool_key);
                    let emit = if !accum.is_empty()
                        && raw_args.len() > accum.len()
                        && raw_args.starts_with(&accum)
                    {
                        // `starts_with` 成立 ⇒ `accum.len()` 落在字符边界上，切片安全。
                        let diff = raw_args[accum.len()..].to_string();
                        self.set_tool_args_accum(&tool_key, raw_args.to_string());
                        diff
                    } else {
                        self.set_tool_args_accum(&tool_key, format!("{accum}{raw_args}"));
                        raw_args.to_string()
                    };
                    if !emit.is_empty() {
                        // 键与上面建块用的是同一个 `tool_key`：块必已建好，查不到只可能是逻辑被改坏，
                        // 此时宁可不发 delta 也不 panic。
                        if let Some(idx) = self
                            .tool_block_index
                            .iter()
                            .find(|(k, _)| *k == tool_key)
                            .map(|(_, v)| *v)
                        {
                            let delta_json = json!({
                                "type": "content_block_delta",
                                "index": idx,
                                "delta": {
                                    "type": "input_json_delta",
                                    "partial_json": emit,
                                },
                            })
                            .to_string();
                            events.push(ev("content_block_delta", delta_json));
                        }
                    }
                }
            }
        }

        // finish_reason：关闭所有 block；message_delta + message_stop 视 usage 是否已到，立即发或推迟。
        let finish = choice.finish_reason.as_deref().unwrap_or("");
        if !finish.is_empty() {
            // stop_reason 映射。`content_filter` / `function_call` 等未列出的值刻意原样透传而不是
            // 压成 `end_turn` —— 把内容审查拦截伪装成正常结束会让下游无从分辨。同文件非流式路径的
            // `map_finish_reason` 一直是原样透传，这里跟它、也跟 TS 的 `switch` 默认臂对齐。
            let stop_reason = match finish {
                "tool_calls" => "tool_use",
                "length" => "max_tokens",
                "stop" => "end_turn",
                other => other,
            };
            // 只认第一个 finish_reason：收尾已发出或已推迟时，后到的 finish_reason 既不改写
            // stop_reason，也不再关块。
            if !self.message_closed && self.pending_stop_reason.is_none() {
                self.close_content_blocks(&mut events);
                if self.usage.is_some() {
                    // usage 已在本帧或更早的帧到达：一次发出带 usage 的收尾。
                    self.emit_message_end(&mut events, stop_reason);
                } else {
                    // `include_usage` 的标准帧序里 finish_reason 帧先于 usage 尾帧。此刻收尾，
                    // message_delta 只能不带 usage，之后到的 usage 已无处安放——于是推迟到 usage 帧 /
                    // `[DONE]` / EOF（`flush`）三者先到者。块照常在这一帧关闭。
                    self.pending_stop_reason = Some(stop_reason.to_string());
                }
            }
        }

        // 推迟收尾期间到达的带 choices 帧若携带 usage，同样立即补发。
        if frame_carries_usage {
            if let Some(stop_reason) = self.pending_stop_reason.clone() {
                self.emit_message_end(&mut events, &stop_reason);
            }
        }

        Ok((events, false))
    }

    /// 流在**没有** `[DONE]` 的情况下正常结束（EOF）时，由驱动方在读循环结束后调用一次。对应 TS
    /// `OpenAIStreamConverter.flush`。
    ///
    /// 只补发「finish_reason 已到、仅因等待 usage 尾帧而推迟」的 `message_delta` + `message_stop`。
    /// 从未收到 finish_reason 的流是被截断的流，这里刻意**不**替它伪造正常结束——一个 `end_turn`
    /// 的 `message_stop` 会把被截断的回答当成完整回答交给下游。已经收口（usage 帧 / `[DONE]` /
    /// 上一次 `flush`）后再调用返回空，不会发出第二个 `message_stop`。读取出错或被取消的流不应
    /// 调用本方法。
    pub fn flush(&mut self) -> Vec<StreamEvent> {
        let mut events: Vec<StreamEvent> = Vec::new();
        if let Some(stop_reason) = self.pending_stop_reason.clone() {
            self.emit_message_end(&mut events, &stop_reason);
        }
        events
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
    use std::collections::HashMap;

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

    // ---- 流式 usage 尾帧与收尾：端口自 TS `core/openai-line-stream-usage.test.ts` 的转换器用例 ----

    /// `stream_options.include_usage` 的 usage 尾帧：`[DONE]` 之前、没有 choices，带缓存 / 推理明细。
    const USAGE_TAIL_FRAME: &str = r#"{"choices":[],"usage":{"prompt_tokens":13171,"completion_tokens":16,"total_tokens":13187,"completion_tokens_details":{"reasoning_tokens":14},"prompt_tokens_details":{"cached_tokens":13056}}}"#;

    /// 网关以 `event: failed` 发出的错误契约帧（取自 TS `core/openai-line-stream-error.test.ts`）：
    /// 没有 `id` / `object` / `choices`。
    const GATEWAY_FAILED_FRAME: &str = r#"{"type":"managed_model_stream_failed","protocol":"managed-model.v2","stage":"provider","error":"gateway: tools[0].type:type cannot be empty. (kind=invalid_request, status=400)","errorCode":"invalid_request","errorContractVersion":1,"faultDomain":"provider","message":"","requestDisposition":"unknown","retryable":false,"requestId":"req-0001","consumeRequestId":"req-0001","providerRequestId":"prov-0001","transportRequestId":"trans-0001"}"#;

    type Parsed = Vec<(String, Value)>;

    fn content_chunk(text: &str) -> String {
        json!({
            "id": "c1",
            "object": "chat.completion.chunk",
            "choices": [{ "index": 0, "delta": { "content": text }, "finish_reason": null }],
        })
        .to_string()
    }

    fn finish_chunk(reason: &str, usage: Option<Value>) -> String {
        let mut chunk = json!({
            "id": "c1",
            "object": "chat.completion.chunk",
            "choices": [{ "index": 0, "delta": {}, "finish_reason": reason }],
        });
        if let Some(usage) = usage {
            chunk["usage"] = usage;
        }
        chunk.to_string()
    }

    fn parse_events(events: &[StreamEvent]) -> Parsed {
        events
            .iter()
            .map(|e| (e.event.clone(), serde_json::from_str(&e.data).unwrap()))
            .collect()
    }

    fn event_names(events: &[(String, Value)]) -> Vec<&str> {
        events.iter().map(|(name, _)| name.as_str()).collect()
    }

    fn count_events(events: &[(String, Value)], name: &str) -> usize {
        events.iter().filter(|(n, _)| n == name).count()
    }

    /// 逐帧喂入转换器，同时记下每一帧各自产出的事件名——断言「收尾在哪一帧发出」要用。
    fn feed(frames: &[&str]) -> (Vec<Vec<String>>, Parsed) {
        let mut conv = new_openai_stream_converter();
        let mut steps = Vec::new();
        let mut all = Vec::new();
        for frame in frames {
            let parsed = parse_events(&conv.convert(frame).unwrap().0);
            steps.push(parsed.iter().map(|(name, _)| name.clone()).collect());
            all.extend(parsed);
        }
        (steps, all)
    }

    /// 整条流恰好一个 message_delta 与一个 message_stop，message_stop 收尾且 message_delta 紧挨在
    /// 它之前。返回那个 message_delta 的 JSON。
    fn expect_single_close_at_end(all: &[(String, Value)]) -> &Value {
        let names = event_names(all);
        assert_eq!(count_events(all, "message_delta"), 1, "{names:?}");
        assert_eq!(count_events(all, "message_stop"), 1, "{names:?}");
        let stop_at = names.iter().position(|n| *n == "message_stop").unwrap();
        assert_eq!(stop_at, names.len() - 1, "{names:?}");
        assert_eq!(names[stop_at - 1], "message_delta", "{names:?}");
        &all[stop_at - 1].1
    }

    #[test]
    fn usage_tail_after_finish_reason_defers_close_to_tail_frame() {
        let (steps, all) = feed(&[
            &content_chunk("PONG"),
            &finish_chunk("stop", None),
            USAGE_TAIL_FRAME,
            "[DONE]",
        ]);
        assert_eq!(steps[1], ["content_block_stop"]); // finish_reason 帧只关块
        assert_eq!(steps[2], ["message_delta", "message_stop"]); // 尾帧到达即收尾
        assert!(steps[3].is_empty()); // [DONE] 不重复收口
        assert_eq!(count_events(&all, "content_block_stop"), 1); // 块只关一次
        let delta = expect_single_close_at_end(&all);
        assert_eq!(delta["delta"], json!({ "stop_reason": "end_turn" }));
        assert_eq!(
            delta["usage"],
            json!({ "input_tokens": 13171, "output_tokens": 16 })
        );
    }

    #[test]
    fn usage_in_finish_reason_frame_closes_in_that_frame() {
        let finish = finish_chunk(
            "length",
            Some(json!({ "prompt_tokens": 20, "completion_tokens": 3, "total_tokens": 23 })),
        );
        let (steps, all) = feed(&[&content_chunk("PONG"), &finish, "[DONE]"]);
        assert_eq!(
            steps[1],
            ["content_block_stop", "message_delta", "message_stop"]
        );
        assert!(steps[2].is_empty());
        let delta = expect_single_close_at_end(&all);
        assert_eq!(delta["delta"], json!({ "stop_reason": "max_tokens" }));
        assert_eq!(
            delta["usage"],
            json!({ "input_tokens": 20, "output_tokens": 3 })
        );
    }

    #[test]
    fn usage_on_choices_frame_during_deferral_closes_in_that_frame() {
        // 没有 `object` 键：同时覆盖 `OpenAIStreamChunk::object` 的缺省。
        let usage_frame = json!({
            "id": "c1",
            "choices": [{ "index": 0, "delta": {}, "finish_reason": null }],
            "usage": { "prompt_tokens": 30, "completion_tokens": 4, "total_tokens": 34 },
        })
        .to_string();
        let (steps, all) = feed(&[
            &content_chunk("PONG"),
            &finish_chunk("stop", None),
            &usage_frame,
            "[DONE]",
        ]);
        assert_eq!(steps[1], ["content_block_stop"]);
        assert_eq!(steps[2], ["message_delta", "message_stop"]);
        assert!(steps[3].is_empty());
        let delta = expect_single_close_at_end(&all);
        assert_eq!(
            delta["usage"],
            json!({ "input_tokens": 30, "output_tokens": 4 })
        );
    }

    #[test]
    fn usage_before_finish_reason_closes_at_finish_with_latest_usage() {
        // 两帧 usage 都没有 `total_tokens`：`OpenAIUsage` 的必填计数不得让整帧解析失败。
        let content_with_usage = |text: &str, completion_tokens: i64| {
            json!({
                "id": "c1",
                "choices": [{ "index": 0, "delta": { "content": text }, "finish_reason": null }],
                "usage": { "prompt_tokens": 7, "completion_tokens": completion_tokens },
            })
            .to_string()
        };
        let (steps, all) = feed(&[
            &content_with_usage("PO", 1),
            &content_with_usage("NG", 2),
            &finish_chunk("stop", None),
            "[DONE]",
        ]);
        assert_eq!(
            steps[2],
            ["content_block_stop", "message_delta", "message_stop"]
        );
        let delta = expect_single_close_at_end(&all);
        // 后到覆盖先到。
        assert_eq!(
            delta["usage"],
            json!({ "input_tokens": 7, "output_tokens": 2 })
        );
    }

    #[test]
    fn finish_reason_without_usage_closes_at_done_without_usage_key() {
        let (steps, all) = feed(&[
            &content_chunk("PONG"),
            &finish_chunk("stop", None),
            "[DONE]",
        ]);
        assert_eq!(steps[1], ["content_block_stop"]);
        assert_eq!(steps[2], ["message_delta", "message_stop"]);
        let delta = expect_single_close_at_end(&all);
        assert_eq!(delta["delta"], json!({ "stop_reason": "end_turn" }));
        assert!(delta.get("usage").is_none());
    }

    #[test]
    fn only_first_finish_reason_counts() {
        // 推迟期间再来 finish_reason：不再关块，也不改写 stop_reason。
        let (steps, all) = feed(&[
            &content_chunk("PONG"),
            &finish_chunk("length", None),
            &finish_chunk("stop", None),
            USAGE_TAIL_FRAME,
            "[DONE]",
        ]);
        assert!(steps[2].is_empty());
        assert_eq!(steps[3], ["message_delta", "message_stop"]);
        assert_eq!(count_events(&all, "content_block_stop"), 1);
        let delta = expect_single_close_at_end(&all);
        assert_eq!(delta["delta"], json!({ "stop_reason": "max_tokens" }));

        // 收尾已发出后再来 finish_reason：零事件。
        let finish_with_usage = finish_chunk(
            "stop",
            Some(json!({ "prompt_tokens": 1, "completion_tokens": 1 })),
        );
        let (steps, _) = feed(&[
            &content_chunk("PONG"),
            &finish_with_usage,
            &finish_chunk("length", None),
        ]);
        assert_eq!(
            steps[1],
            ["content_block_stop", "message_delta", "message_stop"]
        );
        assert!(steps[2].is_empty());
    }

    #[test]
    fn flush_emits_deferred_close_at_eof_exactly_once() {
        let mut conv = new_openai_stream_converter();
        let mut all = parse_events(&conv.convert(&content_chunk("PONG")).unwrap().0);
        all.extend(parse_events(
            &conv.convert(&finish_chunk("stop", None)).unwrap().0,
        ));
        let flushed = parse_events(&conv.flush());
        assert_eq!(event_names(&flushed), ["message_delta", "message_stop"]);
        all.extend(flushed);
        assert!(conv.flush().is_empty());
        let delta = expect_single_close_at_end(&all);
        assert!(delta.get("usage").is_none());
    }

    #[test]
    fn flush_after_usage_tail_close_is_empty() {
        let mut conv = new_openai_stream_converter();
        conv.convert(&content_chunk("PONG")).unwrap();
        conv.convert(&finish_chunk("stop", None)).unwrap();
        // 正向对照：尾帧确实触发了收尾，「flush 返回空」不是因为压根没收尾。
        let tail = parse_events(&conv.convert(USAGE_TAIL_FRAME).unwrap().0);
        assert_eq!(event_names(&tail), ["message_delta", "message_stop"]);
        assert!(conv.flush().is_empty());
    }

    #[test]
    fn flush_does_not_fake_normal_end_for_truncated_stream() {
        let mut conv = new_openai_stream_converter();
        // 正向对照：流确实开过块，「flush 返回空」不是因为转换器什么都没做。
        let opened = parse_events(&conv.convert(&content_chunk("PART")).unwrap().0);
        assert_eq!(
            event_names(&opened),
            [
                "message_start",
                "content_block_start",
                "content_block_delta"
            ]
        );
        assert!(conv.flush().is_empty());
    }

    #[test]
    fn frames_without_choices_or_usage_emit_nothing_and_do_not_close_early() {
        let mut conv = new_openai_stream_converter();
        let no_choices_no_usage = r#"{"type":"managed_model_stream_failed","message":""}"#;
        let empty_choices_null_usage = r#"{"id":"c1","choices":[],"usage":null}"#;

        // 流开头。
        assert!(conv.convert(no_choices_no_usage).unwrap().0.is_empty());
        conv.convert(&content_chunk("PONG")).unwrap();
        let at_finish = parse_events(&conv.convert(&finish_chunk("stop", None)).unwrap().0);
        assert_eq!(event_names(&at_finish), ["content_block_stop"]);
        // 推迟期间：不带 usage 的帧不提前触发收尾；`usage: null` 不是 usage 对象。
        assert!(conv.convert(no_choices_no_usage).unwrap().0.is_empty());
        assert!(conv.convert(empty_choices_null_usage).unwrap().0.is_empty());

        let (end, done) = conv.convert("[DONE]").unwrap();
        assert!(done);
        let end = parse_events(&end);
        assert_eq!(event_names(&end), ["message_delta", "message_stop"]);
        assert!(end[0].1.get("usage").is_none());
    }

    #[test]
    fn usage_frame_without_choices_key_closes_deferred_message() {
        // 既没有 choices 也没有 id / object、只带 usage 的帧，同样补发推迟中的收尾。
        let bare_usage = r#"{"usage":{"prompt_tokens":5,"completion_tokens":6}}"#;
        let (steps, all) = feed(&[
            &content_chunk("PONG"),
            &finish_chunk("stop", None),
            bare_usage,
            "[DONE]",
        ]);
        assert_eq!(steps[2], ["message_delta", "message_stop"]);
        assert!(steps[3].is_empty());
        let delta = expect_single_close_at_end(&all);
        assert_eq!(
            delta["usage"],
            json!({ "input_tokens": 5, "output_tokens": 6 })
        );
    }

    #[test]
    fn cached_and_reasoning_details_are_neither_mapped_nor_netted() {
        let (_, all) = feed(&[
            &content_chunk("PONG"),
            &finish_chunk("stop", None),
            USAGE_TAIL_FRAME,
            "[DONE]",
        ]);
        let usage = expect_single_close_at_end(&all)["usage"]
            .as_object()
            .unwrap();
        let mut keys: Vec<&str> = usage.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["input_tokens", "output_tokens"]);
        assert!(!usage.contains_key("cache_read_input_tokens"));
        // 原样搬运，不是 13171 − 13056。
        assert_eq!(usage["input_tokens"], json!(13171));
    }

    #[test]
    fn missing_or_non_numeric_counts_are_omitted_not_zero() {
        let usage_of = |usage: Value| {
            let tail = json!({ "choices": [], "usage": usage }).to_string();
            let (_, all) = feed(&[
                &content_chunk("PONG"),
                &finish_chunk("stop", None),
                &tail,
                "[DONE]",
            ]);
            expect_single_close_at_end(&all)["usage"].clone()
        };
        assert_eq!(
            usage_of(json!({ "prompt_tokens": null, "completion_tokens": 16 })),
            json!({ "output_tokens": 16 })
        );
        assert_eq!(
            usage_of(json!({ "prompt_tokens": 12, "completion_tokens": "16" })),
            json!({ "input_tokens": 12 })
        );
    }

    #[test]
    fn non_object_usage_is_not_usage() {
        // 只接受对象形态：数组 / 数字形态的 usage 与没有 usage 相同，不触发推迟中的收尾。
        for usage in [json!([13171, 16]), json!(29)] {
            let tail = json!({ "choices": [], "usage": usage }).to_string();
            let (steps, all) = feed(&[
                &content_chunk("PONG"),
                &finish_chunk("stop", None),
                &tail,
                "[DONE]",
            ]);
            assert!(steps[2].is_empty(), "{usage}");
            assert_eq!(steps[3], ["message_delta", "message_stop"], "{usage}");
            assert!(
                expect_single_close_at_end(&all).get("usage").is_none(),
                "{usage}"
            );
        }
    }

    #[test]
    fn gateway_error_frame_without_choices_parses_to_zero_events() {
        // 端口自 TS `core/openai-line-stream-error.test.ts` 的转换器用例。
        let mut conv = new_openai_stream_converter();
        // 形态一：完全没有 id / object / choices（网关错误契约帧）。
        assert!(conv.convert(GATEWAY_FAILED_FRAME).unwrap().0.is_empty());
        assert!(conv.convert(GATEWAY_FAILED_FRAME).unwrap().0.is_empty());
        // 形态二：choices 为空数组的 usage-only 帧。零事件是因为没有推迟中的收尾（从未收到
        // finish_reason）；usage 被记下而非丢弃，随之后的收尾带出。
        let usage_only = r#"{"id":"c1","object":"chat.completion.chunk","choices":[],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
        assert!(conv.convert(usage_only).unwrap().0.is_empty());
        conv.convert(&content_chunk("PONG")).unwrap();
        let end = parse_events(&conv.convert(&finish_chunk("stop", None)).unwrap().0);
        assert_eq!(
            event_names(&end),
            ["content_block_stop", "message_delta", "message_stop"]
        );
        assert_eq!(
            end[1].1["usage"],
            json!({ "input_tokens": 1, "output_tokens": 1 })
        );
    }

    // ---- block 配对：端口自 TS `test/openai-stream-converter.test.ts` ----
    // 收尾关块的逻辑已抽到 `close_content_blocks`，这组用例守住各种块顺序下 start/stop 的配对，以及
    // `[DONE]` 收口。Rust 的 `OpenAIStreamChoice` 要求 `index`，夹具比 TS 版多写这一个键。

    fn thinking_chunk(text: &str) -> String {
        json!({ "id": "c1", "choices": [{ "index": 0, "delta": { "reasoning_content": text } }] })
            .to_string()
    }

    fn tool_call_chunk(index: i64, id: &str, name: &str, args: &str) -> String {
        json!({
            "id": "c1",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [
                        { "index": index, "id": id, "function": { "name": name, "arguments": args } }
                    ],
                },
            }],
        })
        .to_string()
    }

    /// 喂完即流结束（EOF），与驱动方读循环结束处一致地调 `flush`。
    fn run_chunks(chunks: &[&str]) -> Parsed {
        let mut conv = new_openai_stream_converter();
        let mut all = Vec::new();
        for chunk in chunks {
            all.extend(parse_events(&conv.convert(chunk).unwrap().0));
        }
        all.extend(parse_events(&conv.flush()));
        all
    }

    /// 校验 content_block start/stop 严格配对、delta 只指向打开的块、同一 index 不被两种块复用，
    /// 且流末没有悬挂的块。返回每种块类型依次占用的 index。
    fn assert_blocks_well_formed(events: &[(String, Value)]) -> HashMap<String, Vec<i64>> {
        let mut open: HashMap<i64, String> = HashMap::new();
        let mut type_of_index: HashMap<i64, String> = HashMap::new();
        let mut start_index_by_type: HashMap<String, Vec<i64>> = HashMap::new();
        for (name, payload) in events {
            match name.as_str() {
                "content_block_start" => {
                    let index = payload["index"].as_i64().unwrap();
                    let block_type = payload["content_block"]["type"]
                        .as_str()
                        .unwrap()
                        .to_string();
                    if let Some(previous) = type_of_index.get(&index) {
                        assert_eq!(previous, &block_type, "index {index} 被不同类型的块复用");
                    }
                    assert!(!open.contains_key(&index), "index {index} 已打开却又 start");
                    open.insert(index, block_type.clone());
                    type_of_index.insert(index, block_type.clone());
                    start_index_by_type
                        .entry(block_type)
                        .or_default()
                        .push(index);
                }
                "content_block_stop" => {
                    let index = payload["index"].as_i64().unwrap();
                    assert!(
                        open.remove(&index).is_some(),
                        "index {index} stop 但未处于打开态"
                    );
                }
                "content_block_delta" => {
                    let index = payload["index"].as_i64().unwrap();
                    assert!(
                        open.contains_key(&index),
                        "delta index {index} 不指向打开的块"
                    );
                }
                _ => {}
            }
        }
        assert!(open.is_empty(), "仍有未关闭的块: {open:?}");
        start_index_by_type
    }

    #[test]
    fn thinking_then_tool_calls_do_not_share_an_index() {
        let events = run_chunks(&[
            &thinking_chunk("let me think"),
            &thinking_chunk(" more"),
            &tool_call_chunk(0, "call_1", "get_weather", r#"{"city":"#),
            &tool_call_chunk(0, "call_1", "get_weather", r#""sf"}"#),
            &finish_chunk("tool_calls", None),
        ]);
        let by_type = assert_blocks_well_formed(&events);
        assert_eq!(by_type["thinking"], [0]);
        assert_eq!(by_type["tool_use"], [1]);
        assert_eq!(
            events.last().map(|(name, _)| name.as_str()),
            Some("message_stop")
        );
    }

    #[test]
    fn thinking_text_tool_take_sequential_indexes() {
        let events = run_chunks(&[
            &thinking_chunk("reasoning"),
            &content_chunk("hello"),
            &tool_call_chunk(0, "call_1", "fn", "{}"),
            &finish_chunk("tool_calls", None),
        ]);
        let by_type = assert_blocks_well_formed(&events);
        assert_eq!(by_type["thinking"], [0]);
        assert_eq!(by_type["text"], [1]);
        assert_eq!(by_type["tool_use"], [2]);
    }

    #[test]
    fn text_only_block_pairs() {
        let events = run_chunks(&[
            &content_chunk("hi"),
            &content_chunk(" there"),
            &finish_chunk("stop", None),
        ]);
        let by_type = assert_blocks_well_formed(&events);
        assert_eq!(by_type["text"], [0]);
        assert!(!by_type.contains_key("thinking"));
    }

    #[test]
    fn thinking_only_block_closes_at_its_own_index() {
        let events = run_chunks(&[&thinking_chunk("think"), &finish_chunk("stop", None)]);
        let by_type = assert_blocks_well_formed(&events);
        assert_eq!(by_type["thinking"], [0]);
    }

    #[test]
    fn multiple_tool_calls_take_their_own_indexes() {
        let events = run_chunks(&[
            &thinking_chunk("plan"),
            &tool_call_chunk(0, "c0", "a", "{}"),
            &tool_call_chunk(1, "c1", "b", "{}"),
            &finish_chunk("tool_calls", None),
        ]);
        let by_type = assert_blocks_well_formed(&events);
        assert_eq!(by_type["thinking"], [0]);
        assert_eq!(by_type["tool_use"], [1, 2]);
    }

    #[test]
    fn done_without_finish_reason_closes_blocks_and_ends_turn() {
        let mut conv = new_openai_stream_converter();
        let mut all = parse_events(
            &conv
                .convert(&tool_call_chunk(0, "call_1", "f", r#"{"a":1}"#))
                .unwrap()
                .0,
        );
        let (end, done) = conv.convert("[DONE]").unwrap();
        assert!(done);
        let end = parse_events(&end);
        assert_eq!(
            event_names(&end),
            ["content_block_stop", "message_delta", "message_stop"]
        );
        all.extend(end);
        assert_blocks_well_formed(&all);
        let delta = expect_single_close_at_end(&all);
        assert_eq!(delta["delta"], json!({ "stop_reason": "end_turn" }));
    }

    #[test]
    fn done_after_finish_reason_does_not_close_twice() {
        let (steps, all) = feed(&[
            &tool_call_chunk(0, "call_1", "f", r#"{"a":1}"#),
            &finish_chunk("tool_calls", None),
            "[DONE]",
        ]);
        assert_eq!(steps[2], ["message_delta", "message_stop"]);
        assert_blocks_well_formed(&all);
        let delta = expect_single_close_at_end(&all);
        assert_eq!(delta["delta"], json!({ "stop_reason": "tool_use" }));
    }

    // ---- OpenAI 规范工具调用流：续片缺 name / 缺 function、arguments 非串、上游重发全量 ----
    // 参照 TS `models/adapters/openai.ts` 的 tool_calls 分支（`fn?.name` / `fn?.arguments` /
    // `JSON.stringify(fn.arguments)` / `toolArgsAccum`）。

    /// 工具调用**首片**：带 id + name，`arguments` 为空串 —— OpenAI 规范形态。
    fn tool_first_chunk(index: i64, id: &str, name: &str) -> String {
        json!({
            "id": "c1",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": { "tool_calls": [{
                    "index": index,
                    "id": id,
                    "type": "function",
                    "function": { "name": name, "arguments": "" },
                }] },
                "finish_reason": null,
            }],
        })
        .to_string()
    }

    /// 工具调用**续片**：只有 `index` 与 `arguments` 分片，**没有** `id` / `type` / `name`
    /// —— 这正是 OpenAI 规范里每一次工具调用从第二帧起的形态。
    fn tool_arg_chunk(index: i64, args: Value) -> String {
        json!({
            "id": "c1",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": { "tool_calls": [{ "index": index, "function": { "arguments": args } }] },
                "finish_reason": null,
            }],
        })
        .to_string()
    }

    /// 空心续片：tool_call 上只有 `index`，连 `function` 都没有。
    fn tool_hollow_chunk(index: i64) -> String {
        json!({
            "id": "c1",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": { "tool_calls": [{ "index": index }] },
                "finish_reason": null,
            }],
        })
        .to_string()
    }

    /// 另一种空心续片：有 `function` 但里面只有 `name`，没有 `arguments`。
    fn tool_name_only_chunk(index: i64, name: &str) -> String {
        json!({
            "id": "c1",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": { "tool_calls": [{ "index": index, "function": { "name": name } }] },
                "finish_reason": null,
            }],
        })
        .to_string()
    }

    /// 逐帧喂入，保留每帧各自的 `Result` —— 「零错误」这类断言要点名错误原文，不能靠 `unwrap` 的
    /// panic 代劳。返回 `(每帧结果, 全部成功帧的事件)`。
    #[allow(clippy::type_complexity)]
    fn feed_results(frames: &[&str]) -> (Vec<std::result::Result<Parsed, String>>, Parsed) {
        let mut conv = new_openai_stream_converter();
        let mut steps = Vec::new();
        let mut all = Vec::new();
        for frame in frames {
            match conv.convert(frame) {
                Ok((events, _)) => {
                    let parsed = parse_events(&events);
                    all.extend(parsed.clone());
                    steps.push(Ok(parsed));
                }
                Err(e) => steps.push(Err(e.to_string())),
            }
        }
        (steps, all)
    }

    fn step_errors(steps: &[std::result::Result<Parsed, String>]) -> Vec<&String> {
        steps.iter().filter_map(|s| s.as_ref().err()).collect()
    }

    /// 按发出顺序取出全部 `input_json_delta` 的 `partial_json`。
    fn partial_json_pieces(events: &Parsed) -> Vec<String> {
        events
            .iter()
            .filter(|(name, payload)| {
                name == "content_block_delta" && payload["delta"]["type"] == "input_json_delta"
            })
            .map(|(_, payload)| {
                payload["delta"]["partial_json"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn openai_spec_toolcall_continuation_frames_carry_no_name() {
        let (steps, all) = feed_results(&[
            &tool_first_chunk(0, "call_1", "get_weather"),
            &tool_arg_chunk(0, json!(r#"{"city":"#)),
            &tool_arg_chunk(0, json!(r#" "SF"}"#)),
            &finish_chunk("tool_calls", None),
            "[DONE]",
        ]);
        assert!(step_errors(&steps).is_empty(), "{:?}", step_errors(&steps));
        // 承重：参数一字节不落地拼回来。
        assert_eq!(partial_json_pieces(&all).concat(), r#"{"city": "SF"}"#);
        // 续片没有 name，也不能被当成第二个工具调用而另开一个块。
        let by_type = assert_blocks_well_formed(&all);
        assert_eq!(by_type["tool_use"], [0]);
        assert_eq!(count_events(&all, "message_stop"), 1);
        let delta = expect_single_close_at_end(&all);
        assert_eq!(delta["delta"], json!({ "stop_reason": "tool_use" }));
    }

    #[test]
    fn toolcall_delta_without_function_emits_no_delta() {
        let (steps, all) = feed_results(&[
            &tool_first_chunk(0, "call_1", "get_weather"),
            &tool_hollow_chunk(0),
            &tool_name_only_chunk(0, "get_weather"),
            &tool_arg_chunk(0, json!("{}")),
            &finish_chunk("tool_calls", None),
            "[DONE]",
        ]);
        assert!(step_errors(&steps).is_empty(), "{:?}", step_errors(&steps));
        // 两种空心帧自己都零事件：既不另开块，也不发 delta。
        assert_eq!(steps[1].as_ref().map(|p| event_names(p)), Ok(Vec::new()));
        assert_eq!(steps[2].as_ref().map(|p| event_names(p)), Ok(Vec::new()));
        // 正向对照：它们后面的真参数片照常送达，证明空心帧没把转换器弄坏。
        assert_eq!(partial_json_pieces(&all), ["{}"]);
        assert_eq!(assert_blocks_well_formed(&all)["tool_use"], [0]);
    }

    #[test]
    fn choice_without_index_is_not_a_stream_error() {
        // 兼容实现常省略 choice 上的 `index`；转换器从不读它（只取 `choices[0]`）。此前必填，
        // 这类帧以 `missing field \`index\`` 终止整条流。
        let no_index = r#"{"id":"c1","choices":[{"delta":{"content":"hi"}}]}"#;
        let (steps, all) = feed_results(&[no_index, &finish_chunk("stop", None), "[DONE]"]);
        assert!(step_errors(&steps).is_empty(), "{:?}", step_errors(&steps));
        // 正向对照：正文照常送达、块照常配对，证明「没报错」不是靠把整帧丢掉换来的。
        let texts: Vec<&str> = all
            .iter()
            .filter(|(name, p)| name == "content_block_delta" && p["delta"]["type"] == "text_delta")
            .map(|(_, p)| p["delta"]["text"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(texts, ["hi"]);
        assert_eq!(assert_blocks_well_formed(&all)["text"], [0]);
    }

    #[test]
    fn toolcall_arguments_object_is_stringified_not_rejected() {
        let (steps, all) = feed_results(&[
            &tool_first_chunk(0, "call_1", "get_weather"),
            &tool_arg_chunk(0, json!({ "city": "sf" })),
            &finish_chunk("tool_calls", None),
            "[DONE]",
        ]);
        assert!(step_errors(&steps).is_empty(), "{:?}", step_errors(&steps));
        let joined = partial_json_pieces(&all).concat();
        assert!(!joined.contains("[object Object]"), "{joined}");
        assert_eq!(
            serde_json::from_str::<Value>(&joined).unwrap(),
            json!({ "city": "sf" })
        );
    }

    #[test]
    fn full_args_resend_upstream_yields_one_valid_json() {
        // 每片重发全量而非增量的上游。
        let (steps, all) = feed_results(&[
            &tool_first_chunk(0, "call_1", "f"),
            &tool_arg_chunk(0, json!(r#"{"a":"#)),
            &tool_arg_chunk(0, json!(r#"{"a":1"#)),
            &tool_arg_chunk(0, json!(r#"{"a":1}"#)),
            &finish_chunk("tool_calls", None),
            "[DONE]",
        ]);
        assert!(step_errors(&steps).is_empty(), "{:?}", step_errors(&steps));
        let joined = partial_json_pieces(&all).concat();
        assert_eq!(joined, r#"{"a":1}"#);
        assert_eq!(
            serde_json::from_str::<Value>(&joined).unwrap(),
            json!({ "a": 1 })
        );
    }

    #[test]
    fn true_incremental_args_are_still_forwarded_piece_by_piece() {
        // 正向对照：真增量流。第二片**比已累积内容更长**却不是它的延长 —— 只有「严格前缀且更长」
        // 这一档判据能同时放过它和上一条用例的重发流；放宽成「只要更长就只发差值」会从这里吃掉
        // 开头的若干字节。
        let (steps, all) = feed_results(&[
            &tool_first_chunk(0, "call_1", "f"),
            &tool_arg_chunk(0, json!(r#"{"city":"#)),
            &tool_arg_chunk(0, json!(r#""san francisco"}"#)),
            &finish_chunk("tool_calls", None),
            "[DONE]",
        ]);
        assert!(step_errors(&steps).is_empty(), "{:?}", step_errors(&steps));
        let pieces = partial_json_pieces(&all);
        assert_eq!(pieces, [r#"{"city":"#, r#""san francisco"}"#]);
        let joined = pieces.concat();
        assert_eq!(joined, r#"{"city":"san francisco"}"#);
        assert_eq!(
            serde_json::from_str::<Value>(&joined).unwrap(),
            json!({ "city": "san francisco" })
        );
    }

    // ---- 归并键三级降级 / id·name 键省略 / text 在前时的 thinking 块 / 缺 delta 的 choice ----
    // 参照 TS `models/adapters/openai.ts` 的 `resolveToolKey` 与 `content_block: { id: tc.id,
    // name: fn?.name }`（`JSON.stringify` 对 `undefined` 省略该键）。

    /// 不带 `index` 的 tool_call：只有 `id` + `function`（兼容实现的形态）。
    fn tool_call_chunk_without_index(id: &str, name: &str, args: &str) -> String {
        json!({
            "id": "c1",
            "choices": [{
                "index": 0,
                "delta": { "tool_calls": [{
                    "id": id,
                    "type": "function",
                    "function": { "name": name, "arguments": args },
                }] },
            }],
        })
        .to_string()
    }

    /// 既无 `index` 也无 `id` 的续片：只能靠沿用上一个键归位。
    fn tool_arg_chunk_anonymous(args: &str) -> String {
        json!({
            "id": "c1",
            "choices": [{
                "index": 0,
                "delta": { "tool_calls": [{ "function": { "arguments": args } }] },
            }],
        })
        .to_string()
    }

    /// 带 `index` 但既无 `id` 也无 `name` 的 tool_call。
    fn tool_call_chunk_index_only(index: i64, args: &str) -> String {
        json!({
            "id": "c1",
            "choices": [{
                "index": 0,
                "delta": { "tool_calls": [{ "index": index, "function": { "arguments": args } }] },
            }],
        })
        .to_string()
    }

    /// 按发出顺序取出每个 `tool_use` 块的 `content_block` 对象。
    fn tool_use_content_blocks(events: &Parsed) -> Vec<Value> {
        events
            .iter()
            .filter(|(name, p)| {
                name == "content_block_start" && p["content_block"]["type"] == "tool_use"
            })
            .map(|(_, p)| p["content_block"].clone())
            .collect()
    }

    /// 按块 index 归并 `input_json_delta`，返回 (index, 拼接后的参数)，按块首次出现的顺序。
    fn partial_json_by_block(events: &Parsed) -> Vec<(i64, String)> {
        let mut out: Vec<(i64, String)> = Vec::new();
        for (name, payload) in events {
            if name != "content_block_delta" || payload["delta"]["type"] != "input_json_delta" {
                continue;
            }
            let index = payload["index"].as_i64().unwrap_or(-1);
            let piece = payload["delta"]["partial_json"]
                .as_str()
                .unwrap_or_default();
            match out.iter_mut().find(|(i, _)| *i == index) {
                Some(slot) => slot.1.push_str(piece),
                None => out.push((index, piece.to_string())),
            }
        }
        out
    }

    #[test]
    fn two_index_less_toolcalls_do_not_share_a_block() {
        // 承重：两个都省略 `index` 的 tool_call。此前键是 `i64`，缺席落 0 ⇒ 共用一个块、两段参数
        // 拼成 `{"x":1}{"y":2}` 这种必然非法的 JSON，第二个调用的 id / name 彻底丢失。
        let (steps, all) = feed_results(&[
            &tool_call_chunk_without_index("call_1", "a", r#"{"x":1}"#),
            &tool_call_chunk_without_index("call_2", "b", r#"{"y":2}"#),
            &finish_chunk("tool_calls", None),
            "[DONE]",
        ]);
        assert!(step_errors(&steps).is_empty(), "{:?}", step_errors(&steps));
        let by_type = assert_blocks_well_formed(&all);
        assert_eq!(by_type["tool_use"], [0, 1]);
        // 每块的参数各自是一段合法 JSON，没有被拼进同一条流。
        let args = partial_json_by_block(&all);
        assert_eq!(
            args,
            [(0, r#"{"x":1}"#.to_string()), (1, r#"{"y":2}"#.to_string())]
        );
        for (_, piece) in &args {
            serde_json::from_str::<Value>(piece).expect("每块参数必须各自合法");
        }
        // 两个调用的身份都还在。
        let blocks = tool_use_content_blocks(&all);
        assert_eq!(blocks[0]["id"], json!("call_1"));
        assert_eq!(blocks[0]["name"], json!("a"));
        assert_eq!(blocks[1]["id"], json!("call_2"));
        assert_eq!(blocks[1]["name"], json!("b"));
    }

    /// 正向对照，**独立用例**：带 `index` 时同样是两个块 —— 证明上一条钉的是「缺 index 也能分开」，
    /// 而不是「随便什么都分开」。把 `resolve_tool_key` 的 `id:` 降级删掉时这条必须仍绿。
    #[test]
    fn two_indexed_toolcalls_still_take_their_own_blocks() {
        let (steps, all) = feed_results(&[
            &tool_call_chunk(0, "call_1", "a", r#"{"x":1}"#),
            &tool_call_chunk(1, "call_2", "b", r#"{"y":2}"#),
            &finish_chunk("tool_calls", None),
            "[DONE]",
        ]);
        assert!(step_errors(&steps).is_empty(), "{:?}", step_errors(&steps));
        assert_eq!(assert_blocks_well_formed(&all)["tool_use"], [0, 1]);
        assert_eq!(
            partial_json_by_block(&all),
            [(0, r#"{"x":1}"#.to_string()), (1, r#"{"y":2}"#.to_string())]
        );
    }

    #[test]
    fn anonymous_continuation_fragments_stay_on_the_last_tool_key() {
        // 三级降级的最后一级：续片既无 `index` 也无 `id`，只能沿用上一个键。另开一个块会让参数
        // 劈成两半，两半各自都不是合法 JSON。
        let (steps, all) = feed_results(&[
            &tool_call_chunk_without_index("call_1", "f", r#"{"a":"#),
            &tool_arg_chunk_anonymous("1}"),
            &finish_chunk("tool_calls", None),
            "[DONE]",
        ]);
        assert!(step_errors(&steps).is_empty(), "{:?}", step_errors(&steps));
        assert_eq!(assert_blocks_well_formed(&all)["tool_use"], [0]);
        assert_eq!(partial_json_by_block(&all), [(0, r#"{"a":1}"#.to_string())]);
    }

    #[test]
    fn missing_id_and_name_keys_are_omitted_not_empty_strings() {
        // 承重：上游没给 id / name 时不能替它发明一个空串 —— 下游按「有 id 键」判有效会收到 `""`。
        let (steps, all) = feed_results(&[
            &tool_call_chunk_index_only(0, r#"{"a":1}"#),
            &finish_chunk("tool_calls", None),
            "[DONE]",
        ]);
        assert!(step_errors(&steps).is_empty(), "{:?}", step_errors(&steps));
        let blocks = tool_use_content_blocks(&all);
        assert_eq!(blocks.len(), 1);
        let block = blocks[0].as_object().expect("content_block 是对象");
        assert!(!block.contains_key("id"), "{block:?}");
        assert!(!block.contains_key("name"), "{block:?}");
        // 其余键照常在，`null` 也不许冒充「省略」。
        assert_eq!(block["type"], json!("tool_use"));
        assert_eq!(block["input"], json!({}));
        assert!(!block.values().any(Value::is_null), "{block:?}");
    }

    /// 正向对照，**独立用例**：给了 id / name 时两个键都必须在、值正确 —— 证明上一条钉的是
    /// 「缺席才省略」，不是「这两个键从来就没写过」。
    #[test]
    fn present_id_and_name_keys_are_written_through() {
        let (steps, all) = feed_results(&[
            &tool_first_chunk(0, "call_1", "get_weather"),
            &finish_chunk("tool_calls", None),
            "[DONE]",
        ]);
        assert!(step_errors(&steps).is_empty(), "{:?}", step_errors(&steps));
        let blocks = tool_use_content_blocks(&all);
        assert_eq!(blocks[0]["id"], json!("call_1"));
        assert_eq!(blocks[0]["name"], json!("get_weather"));
    }

    #[test]
    fn text_then_thinking_do_not_share_an_index() {
        // 承重（三份 SDK 共有缺陷）：上游先发正文再发 reasoning_content 时，thinking 分支不关
        // 已打开的 text 块、也不推进 block_index ⇒ 两个 content_block_start 都落在 index 0，
        // 且收尾 `close_content_blocks` 走 text 分支，thinking 块永不闭合。
        let events = run_chunks(&[
            &content_chunk("hello"),
            &thinking_chunk("thinking"),
            &finish_chunk("stop", None),
        ]);
        let by_type = assert_blocks_well_formed(&events);
        assert_eq!(by_type["text"], [0]);
        assert_eq!(by_type["thinking"], [1]);
        // thinking 的 delta 必须指向它自己那个块。
        let thinking_delta_index = events
            .iter()
            .find(|(name, p)| {
                name == "content_block_delta" && p["delta"]["type"] == "thinking_delta"
            })
            .map(|(_, p)| p["index"].as_i64().unwrap_or(-1));
        assert_eq!(thinking_delta_index, Some(1));
    }

    /// 正向对照，**独立用例**：thinking 在前、正文在后的既有顺序一字不变 —— 证明上一条钉的是
    /// 「反序也要各占一个 index」，而不是把块顺序整个改了。撤回 C-3 修复时这条必须仍绿。
    #[test]
    fn thinking_then_text_order_is_unchanged() {
        let events = run_chunks(&[
            &thinking_chunk("thinking"),
            &content_chunk("hello"),
            &finish_chunk("stop", None),
        ]);
        let by_type = assert_blocks_well_formed(&events);
        assert_eq!(by_type["thinking"], [0]);
        assert_eq!(by_type["text"], [1]);
    }

    #[test]
    fn choice_without_delta_is_not_a_stream_error() {
        // 承重（三份 SDK 共有缺陷）：`delta` 此前必填，缺它的帧以 `missing field \`delta\`` 终止
        // 整条流。TS 在 `choice.delta.reasoning_content` 处同样炸，Go 反而给零值 —— 三份一并对齐。
        let bare_choice = r#"{"id":"c1","choices":[{"index":0}]}"#;
        let finish_without_delta = r#"{"id":"c1","choices":[{"index":0,"finish_reason":"stop"}]}"#;
        let (steps, all) = feed_results(&[
            &content_chunk("hi"),
            bare_choice,
            finish_without_delta,
            "[DONE]",
        ]);
        assert!(step_errors(&steps).is_empty(), "{:?}", step_errors(&steps));
        // 缺 delta 的空 choice 零事件；带 finish_reason 的那帧照常关块并收口。
        assert_eq!(steps[1].as_ref().map(|p| event_names(p)), Ok(Vec::new()));
        assert_blocks_well_formed(&all);
        let delta = expect_single_close_at_end(&all);
        assert_eq!(delta["delta"], json!({ "stop_reason": "end_turn" }));
    }

    /// 正向对照，**独立用例**：delta 在场时正文照常送达 —— 证明上一条钉的是「缺 delta 不报错」，
    /// 不是「delta 整个不看了」。
    #[test]
    fn choice_with_delta_still_emits_its_content() {
        let (steps, all) = feed_results(&[
            &content_chunk("hi"),
            &content_chunk(" there"),
            &finish_chunk("stop", None),
            "[DONE]",
        ]);
        assert!(step_errors(&steps).is_empty(), "{:?}", step_errors(&steps));
        let texts: Vec<&str> = all
            .iter()
            .filter(|(name, p)| name == "content_block_delta" && p["delta"]["type"] == "text_delta")
            .map(|(_, p)| p["delta"]["text"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(texts, ["hi", " there"]);
    }

    /// 喂「正文 + 该 finish_reason + `[DONE]`」，取收尾 message_delta 上的 stop_reason。
    fn stop_reason_for(reason: &str) -> String {
        let (steps, all) =
            feed_results(&[&content_chunk("hi"), &finish_chunk(reason, None), "[DONE]"]);
        assert!(step_errors(&steps).is_empty(), "{:?}", step_errors(&steps));
        expect_single_close_at_end(&all)["delta"]["stop_reason"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn unlisted_finish_reason_is_passed_through() {
        // 承重：未列出的值原样透传，绝不压成 end_turn —— 把内容审查拦截伪装成正常结束会让下游
        // 无从分辨。
        assert_eq!(stop_reason_for("content_filter"), "content_filter");
        assert_eq!(stop_reason_for("function_call"), "function_call");
    }

    /// 负向对照，与上一条**分开一个用例**：默认臂被改回 `_ => "end_turn"` 时这条必须仍然绿 ——
    /// 它证明上一条钉的是「透传」，不是「随便什么都过」。
    #[test]
    fn listed_finish_reasons_still_map_to_anthropic_stop_reasons() {
        assert_eq!(stop_reason_for("stop"), "end_turn");
        assert_eq!(stop_reason_for("tool_calls"), "tool_use");
        assert_eq!(stop_reason_for("length"), "max_tokens");
    }

    #[test]
    fn non_array_choices_yields_zero_events_not_a_stream_error() {
        // 病态上游 / 网关异形帧：`choices` 是对象而不是数组。
        let bogus = r#"{"id":"c1","choices":{"0":{"index":0,"delta":{"content":"x"}}}}"#;
        let (steps, all) = feed_results(&[
            &content_chunk("hi"),
            bogus,
            &finish_chunk("stop", None),
            "[DONE]",
        ]);
        assert!(step_errors(&steps).is_empty(), "{:?}", step_errors(&steps));
        assert_eq!(steps[1].as_ref().map(|p| event_names(p)), Ok(Vec::new()));
        // 正向对照：这一帧之后的正常帧照常处理，流照常收口 —— 转换器没被它弄坏。
        assert_blocks_well_formed(&all);
        let delta = expect_single_close_at_end(&all);
        assert_eq!(delta["delta"], json!({ "stop_reason": "end_turn" }));
    }

    // ---- 非流式：`content` 为 null / 缺席、`finish_reason` 为 null ----
    // 参照 TS 同文件的 `if (choice.message.content && choice.message.content !== '')` 与
    // `switch (choice.finish_reason) { … default: resp.stop_reason = choice.finish_reason }`。

    /// 只有工具调用的非流式回包，`content` 按 OpenAI 规范为 `null`。
    const TOOLCALL_RESPONSE_CONTENT_NULL: &[u8] = br#"{"id":"c1","object":"chat.completion","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"SF\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}}"#;

    /// 同上，但 `content` 键整个缺席。
    const TOOLCALL_RESPONSE_CONTENT_ABSENT: &[u8] = br#"{"id":"c1","object":"chat.completion","model":"m","choices":[{"index":0,"message":{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"SF\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}}"#;

    /// 正向对照：同一形态但带非空正文。（字面量是 raw **byte** string，只能写 ASCII。）
    const TOOLCALL_RESPONSE_WITH_TEXT: &[u8] = br#"{"id":"c1","object":"chat.completion","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":"let me check","tool_calls":[{"id":"call_1","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"SF\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}}"#;

    fn block_types(resp: &ChatResponse) -> Vec<&str> {
        resp.content.iter().map(|b| b.r#type.as_str()).collect()
    }

    #[test]
    fn null_content_with_tool_calls_yields_tool_use_and_no_empty_text_block() {
        let resp = parse_response(TOOLCALL_RESPONSE_CONTENT_NULL).unwrap();
        // 承重：`"content": null` 是 OpenAI 规范对「只有工具调用」的标准形态，不是解码失败。
        assert_eq!(block_types(&resp), ["tool_use"]);
        assert_eq!(resp.content[0].id.as_deref(), Some("call_1"));
        assert_eq!(resp.content[0].name.as_deref(), Some("get_weather"));
        assert_eq!(resp.content[0].input, Some(json!({ "city": "SF" })));
        assert_eq!(resp.stop_reason, "tool_use");

        // 同一形态在 chatMessagesOpenAI 那条路径上也不能失败。
        let anthropic = parse_openai_response_to_anthropic(TOOLCALL_RESPONSE_CONTENT_NULL).unwrap();
        assert_eq!(
            anthropic
                .content
                .iter()
                .map(|b| b.r#type.as_str())
                .collect::<Vec<_>>(),
            ["tool_use"]
        );

        // 正向对照：正文非空时 text 块必须在，证明断言钉的是「空内容不产块」而不是「从不产块」。
        let with_text = parse_response(TOOLCALL_RESPONSE_WITH_TEXT).unwrap();
        assert_eq!(block_types(&with_text), ["text", "tool_use"]);
        assert_eq!(with_text.content[0].text.as_deref(), Some("let me check"));
    }

    #[test]
    fn absent_content_key_is_decoded_like_an_empty_one() {
        let resp = parse_response(TOOLCALL_RESPONSE_CONTENT_ABSENT).unwrap();
        assert_eq!(block_types(&resp), ["tool_use"]);
        assert_eq!(resp.content[0].name.as_deref(), Some("get_weather"));
        let anthropic =
            parse_openai_response_to_anthropic(TOOLCALL_RESPONSE_CONTENT_ABSENT).unwrap();
        assert_eq!(anthropic.content.len(), 1);
    }

    /// C-4：缺 `choices` 的非流式回包，两个公开入口都必须解得动并给零内容块。实测此前两者与直接
    /// `from_str` 一起报 `missing field \`choices\`` —— 其余字段再完整，调用方一个字节也拿不到。
    #[test]
    fn missing_choices_response_yields_zero_blocks_not_a_decode_error() {
        let no_choices = br#"{"id":"c1","object":"chat.completion","model":"m","usage":{"prompt_tokens":3,"completion_tokens":5,"total_tokens":8}}"#;
        let resp = parse_response(no_choices).expect("missing choices must not fail the response");
        assert!(block_types(&resp).is_empty());
        // 其余字段照常转换：id / model / usage 不因为没有 choices 就一起丢掉。
        assert_eq!(resp.id, "c1");
        assert_eq!(resp.model, "m");
        assert_eq!(resp.usage.input_tokens, 3);
        assert_eq!(resp.usage.output_tokens, 5);
        // 没有 choice 就没有 finish_reason，空串即「没有」，绝不伪造一个 `end_turn`。
        assert_eq!(resp.stop_reason, "");

        // 同一形态在 chatMessagesOpenAI 那条路径上也不能失败。
        let anthropic = parse_openai_response_to_anthropic(no_choices)
            .expect("missing choices must not fail the anthropic projection");
        assert!(anthropic.content.is_empty());
        assert_eq!(anthropic.usage.input_tokens, 3);
    }

    /// C-4 的另一半：`"choices": null` 在两个公开入口上同样只该给零内容块。实测此前两者与直接
    /// `from_str` 一起报 `invalid type: null, expected a sequence`。
    #[test]
    fn null_choices_response_yields_zero_blocks_not_a_decode_error() {
        let null_choices = br#"{"id":"c1","object":"chat.completion","model":"m","choices":null,"usage":{"prompt_tokens":3,"completion_tokens":5,"total_tokens":8}}"#;
        let resp = parse_response(null_choices).expect("null choices must not fail the response");
        assert!(block_types(&resp).is_empty());
        assert_eq!(resp.id, "c1");
        assert_eq!(resp.usage.input_tokens, 3);
        assert_eq!(resp.stop_reason, "");

        let anthropic = parse_openai_response_to_anthropic(null_choices)
            .expect("null choices must not fail the anthropic projection");
        assert!(anthropic.content.is_empty());
        assert_eq!(anthropic.usage.input_tokens, 3);
    }

    /// 上两条的正向对照，**独立用例**：`choices` 在场时内容块照常产出 —— 证明它们钉的是
    /// 「缺 choices / null 也能解」，不是「这两条路径从来就不产内容块」。
    #[test]
    fn present_choices_still_yield_content_blocks() {
        let resp =
            parse_response(TOOLCALL_RESPONSE_WITH_TEXT).expect("choices present must decode");
        assert_eq!(block_types(&resp), ["text", "tool_use"]);
        assert_eq!(resp.stop_reason, "tool_use");
        let anthropic = parse_openai_response_to_anthropic(TOOLCALL_RESPONSE_WITH_TEXT)
            .expect("choices present must decode");
        assert_eq!(anthropic.content.len(), 2);
    }

    #[test]
    fn null_finish_reason_is_not_a_decode_failure() {
        let body = br#"{"id":"c1","object":"chat.completion","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":null}],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}}"#;
        let resp = parse_response(body).unwrap();
        // `null` = 上游没给停止原因，落成空串（`ChatResponse` 的 `stop_reason` 是 `String`，
        // 空串就是它表达「没有」的形态），绝不伪造一个 `end_turn`。
        assert_eq!(resp.stop_reason, "");
        assert_eq!(block_types(&resp), ["text"]);
        // 负向对照：给了停止原因时照常映射。
        let stopped = br#"{"id":"c1","object":"chat.completion","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}}"#;
        assert_eq!(parse_response(stopped).unwrap().stop_reason, "end_turn");
    }
}
