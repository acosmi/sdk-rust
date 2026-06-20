//! sanitize/types — 端口自 `acosmi-sdk-ts/src/sanitize/types.ts`
//! （其本身端口自 `acosmi-sdk-go/sanitize/types.go`）。
//!
//! Provider 无关的 content-block 防御性处理类型。职责边界：
//!   - SDK 层：只做所有下游 provider 都不可能接受的底线剥除 + 早失败；
//!   - Gateway 层：按 provider preset 精细剥除（不在本包内）。
//!
//! TS 侧 `BlockType` 是 16 个字符串字面量联合 + 16 个同名 `const`；按 §4.1 收进
//! `enum` 变体（不逐个建 const），保留 `as_str()` 作为 wire 字符串对照锚点。`DeltaType`
//! 同理（5 成员，与 `BlockType` 正交）。

/// Anthropic content block 类型（请求 + 响应 + ephemeral）。
///
/// 16 个成员对齐 TS `BlockText`/`BlockImage`/… const 集（§4.1：收进 enum 不逐个建 const）。
/// `as_str()` 返回 wire 字符串字面量，与 Go/TS 跨语言一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockType {
    Text,
    Image,
    Video,
    Document,
    SearchResult,
    Thinking,
    RedactedThinking,
    ToolUse,
    ToolResult,
    ToolReference,
    ServerToolUse,
    WebSearchToolResult,
    CodeExecutionToolResult,
    McpToolUse,
    McpToolResult,
    ContainerUpload,
}

impl BlockType {
    /// 借出 wire 字符串字面量。
    pub fn as_str(&self) -> &'static str {
        match self {
            BlockType::Text => "text",
            BlockType::Image => "image",
            BlockType::Video => "video",
            BlockType::Document => "document",
            BlockType::SearchResult => "search_result",
            BlockType::Thinking => "thinking",
            BlockType::RedactedThinking => "redacted_thinking",
            BlockType::ToolUse => "tool_use",
            BlockType::ToolResult => "tool_result",
            BlockType::ToolReference => "tool_reference",
            BlockType::ServerToolUse => "server_tool_use",
            BlockType::WebSearchToolResult => "web_search_tool_result",
            BlockType::CodeExecutionToolResult => "code_execution_tool_result",
            BlockType::McpToolUse => "mcp_tool_use",
            BlockType::McpToolResult => "mcp_tool_result",
            BlockType::ContainerUpload => "container_upload",
        }
    }
}

impl std::fmt::Display for BlockType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 流式响应 delta 类型（与 [`BlockType`] 正交）。对齐 TS `DeltaType` 5 成员。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeltaType {
    TextDelta,
    InputJsonDelta,
    ThinkingDelta,
    SignatureDelta,
    CitationsDelta,
}

impl DeltaType {
    /// 借出 wire 字符串字面量。
    pub fn as_str(&self) -> &'static str {
        match self {
            DeltaType::TextDelta => "text_delta",
            DeltaType::InputJsonDelta => "input_json_delta",
            DeltaType::ThinkingDelta => "thinking_delta",
            DeltaType::SignatureDelta => "signature_delta",
            DeltaType::CitationsDelta => "citations_delta",
        }
    }
}

impl std::fmt::Display for DeltaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 网关在 block JSON 中注入的 in-band 标记字段名。对齐 TS `EphemeralMarkerField`。
///
/// 存在且值为 `true` 时代表该 block 不应回传下一轮。选择 in-band 标记而非独立 SSE 事件的
/// 理由：零缓冲 / 零顺序依赖 / 零延迟；history 剥离天然可做。
pub const EPHEMERAL_MARKER_FIELD: &str = "acosmi_ephemeral";
