//! 模型网关域：ManagedModel / Chat / wire-format DTO / 双格式 adapter / 选模型 helper。
//!
//! 对齐 `models/index.ts`。**双 adapter（AnthropicAdapter + OpenAIAdapter）P0 红线**：
//! 等地位、恒编译、不可降级。

pub mod adapters;
pub mod betas;
pub mod enduser;
pub mod model_helpers;
pub mod stream_meta;
pub mod types;
pub mod wire_anthropic;
pub mod wire_openai;

// === 类型（对齐 export * from './types'）===
pub use types::{
    bucket_info_is_commercial, bucket_row_is_commercial, new_thinking_config, new_web_search_tool,
    parse_settlement, parse_sources_event, zero_model_capabilities, BucketInfo, BucketRow,
    ChatContentBlock, ChatMessage, ChatRequest, ChatResponse, ChatUsage, EffortConfig,
    EmbeddingData, EmbeddingInput, EmbeddingRequest, EmbeddingResponse, EmbeddingUsage, GeoLoc,
    ImageGenerationRequest, ImageGenerationResponse, InputModality, ManagedModel,
    ModelCapabilities, OutputConfig, QuotaSummary, RerankRequest, RerankResponse, RerankResult,
    ServerTool, SourcesEvent, StreamEvent,
    StreamSettlement, ThinkingConfig, VideoGenerationRequest, VideoTaskResponse, WebSearchConfig,
    WebSearchSource, BUCKET_CLASS_COMMERCIAL, BUCKET_CLASS_GENERIC, SERVER_TOOL_TYPE_WEB_SEARCH,
    THINKING_HIGH, THINKING_HIGH_MIN_MAX_TOKENS, THINKING_MAX, THINKING_MAX_FALLBACK_MAX_TOKENS,
    THINKING_OFF,
};

// === Anthropic wire DTO（对齐 export * from './wire-anthropic'）===
pub use wire_anthropic::{
    anthropic_response_text_content, anthropic_response_thinking_content,
    anthropic_response_tool_use_blocks, AnthropicContentBlock, AnthropicResponse, AnthropicUsage,
};

// === OpenAI wire DTO（对齐 export * from './wire-openai'）===
pub use wire_openai::{
    OpenAIChatChoice, OpenAIChatMessage, OpenAIChatResponse, OpenAIFunctionCall,
    OpenAIStreamChoice, OpenAIStreamChunk, OpenAIStreamDelta, OpenAIStreamToolCall, OpenAIToolCall,
    OpenAIUsage,
};

// === Model catalog helpers（v1.2+）===
pub use model_helpers::{
    find_desktop_visual_understanding_model, find_first_model_by_input_modality,
    model_supports_image_input, model_supports_input_modality,
};

// === Adapters（双格式 P0 红线）===
pub use adapters::openai::{new_openai_stream_converter, OpenAIStreamConverter};
pub use adapters::{get_adapter, get_adapter_for_model, Adapter, ProviderFormat};

// === Stream meta ===
pub use stream_meta::{extract_anthropic_block_meta, BlockMeta};

// === Betas ===
pub use betas::{build_betas, unique_merge};

// === v1.6.0 业务侧终端用户 id helper ===
pub use enduser::{is_sse_comment_line, validate_end_user_id, MAX_END_USER_ID_LENGTH};
