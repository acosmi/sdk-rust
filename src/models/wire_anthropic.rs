//! Anthropic wire-format 响应 DTO。端口自 `models/wire-anthropic.ts`
//! （其端口自 `acosmi-sdk-go/types.go` v0.19.0 的 AnthropicResponse 段）。
//!
//! 命名约定：字段名 = Go json tag 字面量（wire format），不做 camelCase 重映射。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Anthropic 内容块。覆盖 text / thinking / redacted_thinking / tool_use / tool_result /
/// server_tool_use / mcp_tool_use / mcp_tool_result。**扁平 struct**（方案 §4.1 红线）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicContentBlock {
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// tool_use / server_tool_use / mcp_tool_use block ID。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// tool_use function name。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// tool_use arguments（json.RawMessage）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    /// thinking block content。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,

    /// text —— web_search 搜索引用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citations: Option<Value>,
    /// thinking —— Anthropic 签名（后续请求必须回传）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// redacted_thinking —— base64 编码的被审查思考内容。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    /// server_tool_use / mcp_tool_use / mcp_tool_result —— 服务端工具来源。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    /// mcp_tool_use —— MCP 调用者上下文。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<Value>,
    /// tool_result / mcp_tool_result —— 工具执行结果。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// Anthropic token 用量。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<i64>,
}

/// Anthropic 原生格式同步响应。`POST /managed-models/:id/anthropic` 返回此格式（无包装）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicResponse {
    pub id: String,
    /// "message"
    pub r#type: String,
    /// "assistant"
    pub role: String,
    pub content: Vec<AnthropicContentBlock>,
    pub model: String,
    pub stop_reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    pub usage: AnthropicUsage,
}

/// 提取所有 text 类型内容块的文本，拼接返回。
pub fn anthropic_response_text_content(r: &AnthropicResponse) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for b in &r.content {
        if b.r#type == "text" {
            if let Some(t) = &b.text {
                if !t.is_empty() {
                    parts.push(t);
                }
            }
        }
    }
    parts.concat()
}

/// 提取所有 thinking 类型内容块的文本，拼接返回。
pub fn anthropic_response_thinking_content(r: &AnthropicResponse) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for b in &r.content {
        if b.r#type == "thinking" {
            if let Some(t) = &b.thinking {
                if !t.is_empty() {
                    parts.push(t);
                }
            }
        }
    }
    parts.concat()
}

/// 返回所有 tool_use 类型的内容块。
pub fn anthropic_response_tool_use_blocks(r: &AnthropicResponse) -> Vec<AnthropicContentBlock> {
    r.content
        .iter()
        .filter(|b| b.r#type == "tool_use")
        .cloned()
        .collect()
}
