//! OpenAI 兼容 wire-format 响应 DTO（非 Anthropic 厂商）。端口自 `models/wire-openai.ts`
//! （其端口自 `acosmi-sdk-go/types.go` v0.19.0 的 OpenAI 兼容响应类型段）。
//!
//! 命名约定：字段名 = Go json tag 字面量（wire format），不做 camelCase 重映射。

use serde::{Deserialize, Serialize};

/// OpenAI 兼容同步响应。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIChatResponse {
    pub id: String,
    /// "chat.completion"
    pub object: String,
    pub model: String,
    pub choices: Vec<OpenAIChatChoice>,
    pub usage: OpenAIUsage,
}

/// OpenAI choices 元素。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIChatChoice {
    pub index: i64,
    pub message: OpenAIChatMessage,
    /// "stop", "tool_calls", "length"
    pub finish_reason: String,
}

/// OpenAI message。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIToolCall>>,
    /// GLM/DeepSeek thinking。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

/// OpenAI tool_call。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIToolCall {
    pub id: String,
    /// "function"
    pub r#type: String,
    pub function: OpenAIFunctionCall,
}

/// OpenAI function call。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIFunctionCall {
    pub name: String,
    pub arguments: String,
}

/// OpenAI token 用量。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIUsage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

/// OpenAI SSE delta 格式。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIStreamChunk {
    pub id: String,
    /// "chat.completion.chunk"
    pub object: String,
    pub choices: Vec<OpenAIStreamChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<OpenAIUsage>,
}

/// OpenAI SSE choice。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIStreamChoice {
    pub index: i64,
    pub delta: OpenAIStreamDelta,
    /// nullable（`string | null`）。
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// OpenAI SSE delta。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIStreamDelta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIStreamToolCall>>,
}

/// OpenAI SSE tool_call delta。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIStreamToolCall {
    pub index: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    pub function: OpenAIFunctionCall,
}
