//! 模型网关域类型。端口自 `models/types.ts`（其端口自 `acosmi-sdk-go/types.go` v0.19.0）。
//!
//! 命名约定：字段名 = Go json tag 字面量（wire format），不做 camelCase 重映射。
//! mix 风格（`modelId` / `max_tokens` / `supports_thinking`）是历史 wire 设计的真实样貌；
//! camelCase wire 用 `#[serde(rename)]`，snake_case wire 直用。
//!
//! ## 联合类型扁平 struct（方案 §4.1 红线）
//! `ChatContentBlock` / `StreamEvent` 是**扁平 struct + 全 `Option`**，**不是** tagged enum：
//! `type:String` 判别符 + 动态字段 `serde_json::Value`，忠实镜像 Go json marshaling，
//! 保前向兼容（未知 type 不 panic）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

// =============================================================================
// Managed Models
// =============================================================================

/// 模型能力矩阵 —— 下游据此决定 UI 功能开关与 Beta Header 注入。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    // 思考能力
    pub supports_thinking: bool,
    pub supports_adaptive_thinking: bool,
    /// 交错思考（Interleaved Thinking）。
    pub supports_isp: bool,

    // 工具与搜索
    pub supports_web_search: bool,
    pub supports_tool_search: bool,
    pub supports_structured_output: bool,

    // 推理控制
    pub supports_effort: bool,
    /// 模型是否支持 thinking_level="max" 强度档（深度思考）。
    pub supports_max_effort: bool,
    /// Opus 4.6 独有（Speed="fast"）。
    pub supports_fast_mode: bool,
    /// Auto 模式（模型自主选择工具/搜索策略）。
    pub supports_auto_mode: bool,

    // 上下文与缓存
    pub supports_1m_context: bool,
    pub supports_prompt_cache: bool,
    /// 通过 context-management beta 控制。
    pub supports_cache_editing: bool,

    // 输出控制
    /// Claude 4 内置。
    pub supports_token_efficient: bool,
    pub supports_redact_thinking: bool,

    // Token 上限（冗余但便于查询）。
    pub max_input_tokens: i64,
    pub max_output_tokens: i64,

    /// 桌面视觉理解 sidecar 能力（v1.2+）。与 `inputModalities=['image']` 正交。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_desktop_visual_understanding: Option<bool>,

    /// 图片生成能力（v1.3+）。应通过 `generate_image()` 调用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_image_generation: Option<bool>,

    /// 视频生成能力（v1.3+）。应通过 `generate_video()` + `poll_video_task()` 调用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_video_generation: Option<bool>,

    /// 向量能力（v2.9+）。应通过 `embeddings()` 调用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_embedding: Option<bool>,

    /// 重排序能力（v2.9+）。应通过 `rerank()` 调用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_rerank: Option<bool>,
}

/// 图片生成请求（`generate_image`）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImageGenerationRequest {
    pub prompt: String,
    /// 缺省 1024。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    /// 缺省 1024。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
}

/// 图片生成响应。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImageGenerationResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b64_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revised_prompt: Option<String>,
    #[serde(rename = "requestId", default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// 视频生成请求（`generate_video`）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VideoGenerationRequest {
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    /// 时长（秒）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<i64>,
}

/// 视频任务响应（创建返回 `taskId`；轮询返回 `status`/`videoUrl`）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VideoTaskResponse {
    #[serde(rename = "taskId")]
    pub task_id: String,
    /// pending | running | completed | failed
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(rename = "videoUrl", default, skip_serializing_if = "Option::is_none")]
    pub video_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(rename = "requestId", default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

// ── 向量 (Embedding) / 重排序 (Rerank) (v2.9.0) ──────────────────────────────
// SDK 订阅会员经 POST /managed-models/:id/{embeddings,rerank} 调用; 仅
// capabilities.supports_embedding / supports_rerank 的托管模型可用。上游接 DashScope。

/// 向量输入：单条文本或文本数组（对外 = OpenAI `input` 字段）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EmbeddingInput {
    /// 单条文本。
    Single(String),
    /// 批量文本。
    Batch(Vec<String>),
}

/// 向量请求（`embeddings`）。对外 = OpenAI `/v1/embeddings` 标准。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingRequest {
    /// 待向量化的文本（单条）或文本数组（批量）。
    pub input: EmbeddingInput,
    /// 向量维度（可选；DashScope text-embedding-v4 支持 2048/1536/1024/768/512/256/128/64）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<i64>,
    /// 编码格式（可选，如 `"float"`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding_format: Option<String>,
}

/// 单条向量结果（OpenAI 标准）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmbeddingData {
    #[serde(default)]
    pub object: String,
    #[serde(default)]
    pub index: i64,
    #[serde(default)]
    pub embedding: Vec<f64>,
}

/// 向量 / 重排序用量。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmbeddingUsage {
    #[serde(default)]
    pub prompt_tokens: i64,
    #[serde(default)]
    pub total_tokens: i64,
}

/// 向量响应（OpenAI `/v1/embeddings` 标准）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    #[serde(default)]
    pub object: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub data: Vec<EmbeddingData>,
    #[serde(default)]
    pub usage: EmbeddingUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// 重排序请求（`rerank`）。统一扁平契约，网关内部按模型线路转换。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankRequest {
    /// 查询文本。
    pub query: String,
    /// 候选文档列表。
    pub documents: Vec<String>,
    /// 返回前 N 条（可选；缺省返回全部）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_n: Option<i64>,
    /// 是否在结果中回传文档原文（可选，缺省 false）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_documents: Option<bool>,
    /// 排序指令（可选；仅 OpenAI 兼容线路支持，原生线路忽略）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruct: Option<String>,
}

/// 单条重排序结果。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RerankResult {
    /// 文档在原 `documents` 数组中的下标。
    #[serde(default)]
    pub index: i64,
    /// 相关性得分（越高越相关）。
    #[serde(default)]
    pub relevance_score: f64,
    /// 文档原文（仅 `return_documents=true` 时存在）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
}

/// 重排序响应。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RerankResponse {
    #[serde(default)]
    pub results: Vec<RerankResult>,
    #[serde(default)]
    pub usage: EmbeddingUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// 模型可接收的用户输入模态（v1.2+）。闭 union。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputModality {
    /// 文本输入。
    Text,
    /// 截图 / 图片输入（多模态）。
    Image,
}

impl InputModality {
    /// 借出 wire 字符串。
    pub fn as_str(&self) -> &'static str {
        match self {
            InputModality::Text => "text",
            InputModality::Image => "image",
        }
    }
}

/// BucketClass 字面量常量。
pub const BUCKET_CLASS_COMMERCIAL: &str = "COMMERCIAL";
/// BucketClass 字面量常量。
pub const BUCKET_CLASS_GENERIC: &str = "GENERIC";

/// 用户在某 modelId 上的桶余额聚合视图（V30 entitlement-listing）。单位 ETU。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BucketInfo {
    #[serde(rename = "quotaEtu")]
    pub quota_etu: i64,
    #[serde(rename = "usedEtu")]
    pub used_etu: i64,
    #[serde(rename = "remainingEtu")]
    pub remaining_etu: i64,
    #[serde(
        rename = "sharedPoolEtu",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shared_pool_etu: Option<i64>,
    #[serde(rename = "expiresAt", default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(rename = "bucketClass")]
    pub bucket_class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expired: Option<bool>,
    /// v0.19+ —— GENERIC alive 桶 tokenRemaining 求和（"免费余额"）。
    #[serde(rename = "freeRemainingEtu")]
    pub free_remaining_etu: i64,
    /// v0.19+ —— COMMERCIAL alive 桶 tokenRemaining 求和（"付费余额"）。
    #[serde(rename = "paidRemainingEtu")]
    pub paid_remaining_etu: i64,
}

/// 大小写不敏感判定 —— 与 `buildBucketView` EqualFold 语义对齐。
pub fn bucket_info_is_commercial(b: Option<&BucketInfo>) -> bool {
    match b {
        None => false,
        Some(b) => b.bucket_class.to_lowercase() == BUCKET_CLASS_COMMERCIAL.to_lowercase(),
    }
}

/// 托管模型。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManagedModel {
    pub id: String,
    pub name: String,
    pub provider: String,
    #[serde(rename = "modelId")]
    pub model_id: String,
    #[serde(rename = "maxTokens")]
    pub max_tokens: i64,
    #[serde(rename = "isEnabled")]
    pub is_enabled: bool,
    /// @deprecated 公开 `/managed-models` 端点不返回此字段。
    #[serde(
        rename = "pricePerMTok",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub price_per_m_tok: Option<f64>,
    /// @deprecated 公开 `/managed-models` 端点不返回此字段。
    #[serde(rename = "isDefault", default, skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
    #[serde(
        rename = "contextWindow",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub context_window: Option<i64>,
    pub capabilities: ModelCapabilities,

    /// P1 商品化档位门控：0=FREE..4=ULTRA。
    #[serde(
        rename = "minPlanTier",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub min_plan_tier: Option<i64>,
    /// 该模型能否在 C 端对话（ADK 托管运行时）使用；false 时选择器应过滤。
    #[serde(
        rename = "chatRuntimeSupported",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub chat_runtime_supported: Option<bool>,
    /// 模型默认绑定的工具 ID 列表。
    #[serde(
        rename = "defaultToolIds",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub default_tool_ids: Option<Vec<String>>,
    /// 该模型对当前用户档位是否被限制策略锁定。true=展示但置灰+升级引导。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    /// 是否属于"免费会员可用"分区（仅分类，与当前用户档位无关）。
    #[serde(rename = "freeTier", default, skip_serializing_if = "Option::is_none")]
    pub free_tier: Option<bool>,

    /// 上游 gateway 为此模型启用的请求格式列表："anthropic" | "openai"。
    /// 空值表示上游未声明，SDK 回落 provider 硬编码分支（向后兼容）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_formats: Option<Vec<String>>,

    /// 上游建议客户端优先使用的格式："anthropic" | "openai"；空值等价 `supported_formats[0]`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_format: Option<String>,

    /// 当前用户在此模型上的桶余额聚合（V0.18 V30）。仅非 admin 用户上游才返回。
    #[serde(
        rename = "bucketInfo",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub bucket_info: Option<BucketInfo>,

    /// 模型可接收的用户输入模态（v1.2+）。
    /// 兼容上游字段名 `input_modalities`（snake_case）—— `list_models` 写缓存前会做归一化。
    /// **缺失语义**：上游未下发时为 `None`，调用方必须保守按 text-only / unknown 处理。
    #[serde(
        rename = "inputModalities",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub input_modalities: Option<Vec<InputModality>>,

    /// 上游 snake_case 别名 `input_modalities`。归一化前的原始字段，归一化后 SDK 用户只读
    /// `input_modalities`（camel 字段 `inputModalities`）。对齐 TS `normalizeInputModalities`。
    #[serde(
        rename = "input_modalities",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub input_modalities_snake: Option<Vec<Value>>,
}

// =============================================================================
// QuotaSummary —— v0.19+ 账户级权益总览（免费/付费切分）
// =============================================================================

/// 单桶视图 —— `QuotaSummary.freeBuckets/paidBuckets` 元素。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BucketRow {
    #[serde(rename = "bucketId")]
    pub bucket_id: String,
    /// 精确桶为具体 modelId，通配桶为 `"*"`。
    #[serde(rename = "modelId")]
    pub model_id: String,
    /// COMMERCIAL | GENERIC
    #[serde(rename = "bucketClass")]
    pub bucket_class: String,
    #[serde(rename = "tokenQuota")]
    pub token_quota: i64,
    #[serde(rename = "tokenUsed")]
    pub token_used: i64,
    #[serde(rename = "tokenRemaining")]
    pub token_remaining: i64,
    /// 永久桶为 `None`。
    #[serde(rename = "expiresAt", default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expired: Option<bool>,
}

/// 大小写不敏感判定。
pub fn bucket_row_is_commercial(r: Option<&BucketRow>) -> bool {
    match r {
        None => false,
        Some(r) => r.bucket_class.to_lowercase() == BUCKET_CLASS_COMMERCIAL.to_lowercase(),
    }
}

/// `GET /api/v4/entitlements/quota-summary` 返回体。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuotaSummary {
    /// GENERIC（免费/赠送）alive 桶 tokenRemaining 求和。
    #[serde(rename = "freeTotalEtu")]
    pub free_total_etu: i64,
    /// COMMERCIAL（付费购买）alive 桶 tokenRemaining 求和。
    #[serde(rename = "paidTotalEtu")]
    pub paid_total_etu: i64,
    /// GENERIC 桶详情（含过期）；空时为空数组。
    #[serde(rename = "freeBuckets", default)]
    pub free_buckets: Vec<BucketRow>,
    /// COMMERCIAL 桶详情（含过期）；空时为空数组。
    #[serde(rename = "paidBuckets", default)]
    pub paid_buckets: Vec<BucketRow>,
    /// GENERIC alive 桶中最早到期；永久桶/无 alive 时缺失。
    #[serde(
        rename = "nextFreeExpiresAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub next_free_expires_at: Option<String>,
    /// COMMERCIAL alive 桶中最早到期；同上。
    #[serde(
        rename = "nextPaidExpiresAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub next_paid_expires_at: Option<String>,
}

// =============================================================================
// Chat
// =============================================================================

/// 聊天消息（简单文本格式，CrabClaw 使用）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Anthropic 响应内容块。**扁平 struct**（方案 §4.1 红线）：`type:String` 判别符 + 全
/// `Option` + 动态字段 `serde_json::Value`，**禁 tagged enum**。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatContentBlock {
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citations: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// json.RawMessage in Go。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// Anthropic 格式 token 用量。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<i64>,
}

// ---------- Thinking ----------

/// 三档思考级别（v0.9.0）。
pub const THINKING_OFF: &str = "off";
/// 三档思考级别（v0.9.0）。
pub const THINKING_HIGH: &str = "high";
/// 三档思考级别（v0.9.0）。
pub const THINKING_MAX: &str = "max";

/// 标准思考最低 maxTokens —— CrabCode 默认 MAX_OUTPUT_TOKENS_DEFAULT = 32_000。
pub const THINKING_HIGH_MIN_MAX_TOKENS: i64 = 32_000;
/// 深度思考回退 maxTokens —— Opus 4.6 上限 128K。
pub const THINKING_MAX_FALLBACK_MAX_TOKENS: i64 = 128_000;

/// 控制模型思考行为。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThinkingConfig {
    /// "enabled" | "disabled" | "adaptive"
    pub r#type: String,
    /// 仅 type="enabled" 时（旧模型回退）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<i64>,
    /// 思考级别（v0.9.0）："off" | "high" | "max"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    /// "none" | "summary" | ""（默认空=完整）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

/// 根据三档 level 创建配置。
pub fn new_thinking_config(level: &str) -> ThinkingConfig {
    if level.is_empty() || level == THINKING_OFF {
        return ThinkingConfig {
            r#type: "disabled".to_string(),
            ..Default::default()
        };
    }
    ThinkingConfig {
        r#type: "adaptive".to_string(),
        level: Some(level.to_string()),
        ..Default::default()
    }
}

/// 服务端工具定义 —— SDK 将工具 schema 合入 API 请求的 tools 数组。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerTool {
    pub r#type: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Map<String, Value>>,
}

/// Server Tool 类型常量。
pub const SERVER_TOOL_TYPE_WEB_SEARCH: &str = "web_search_20250305";

/// 地理位置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeoLoc {
    /// ISO 3166-1 alpha-2。
    pub country: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
}

/// `ServerTool.config` 结构（type=web_search_20250305）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WebSearchConfig {
    /// 每请求最大搜索次数，默认 8。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<i64>,
    /// 域名白名单（与 `blocked_domains` 互斥）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_domains: Option<Vec<String>>,
    /// 域名黑名单（与 `allowed_domains` 互斥）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_domains: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_location: Option<GeoLoc>,
}

/// 控制推理努力级别。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EffortConfig {
    /// "low" | "medium" | "high" | "max"
    pub level: String,
}

/// 控制输出格式（结构化输出）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutputConfig {
    /// "json_schema" | ""
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
}

/// 创建搜索 Server Tool 的便捷方法。
/// `allowed_domains` 与 `blocked_domains` 互斥，同时传入返回 `Err`（对齐 TS throw）。
pub fn new_web_search_tool(
    cfg: Option<&WebSearchConfig>,
) -> crate::shared::errors::Result<ServerTool> {
    let mut st = ServerTool {
        r#type: SERVER_TOOL_TYPE_WEB_SEARCH.to_string(),
        name: "web_search".to_string(),
        config: None,
    };
    if let Some(cfg) = cfg {
        let allowed_len = cfg.allowed_domains.as_ref().map(|v| v.len()).unwrap_or(0);
        let blocked_len = cfg.blocked_domains.as_ref().map(|v| v.len()).unwrap_or(0);
        if allowed_len > 0 && blocked_len > 0 {
            return Err(crate::shared::errors::Error::other(
                "web search: allowed_domains and blocked_domains are mutually exclusive",
            ));
        }
        let mut m = serde_json::Map::new();
        if let Some(mu) = cfg.max_uses {
            if mu > 0 {
                m.insert("max_uses".to_string(), Value::from(mu));
            }
        }
        if allowed_len > 0 {
            m.insert(
                "allowed_domains".to_string(),
                serde_json::to_value(cfg.allowed_domains.as_ref().unwrap())?,
            );
        }
        if blocked_len > 0 {
            m.insert(
                "blocked_domains".to_string(),
                serde_json::to_value(cfg.blocked_domains.as_ref().unwrap())?,
            );
        }
        if let Some(loc) = &cfg.user_location {
            m.insert("user_location".to_string(), serde_json::to_value(loc)?);
        }
        if !m.is_empty() {
            st.config = Some(m);
        }
    }
    Ok(st)
}

/// 聊天请求。基础字段供 CrabClaw，扩展字段供 CrabCode。所有新增字段零值不改变行为。
///
/// 扩展字段（`raw_messages`/`system`/`tools`/...）不会被原样序列化，而由 adapter
/// `build_request_body` 选择性序列化到请求体。
#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    // ── 基础字段（CrabClaw 兼容）──
    pub messages: Option<Vec<ChatMessage>>,
    pub stream: Option<bool>,
    pub max_tokens: Option<i64>,

    // ── 完整请求控制（CrabCode 扩展）── 由 build_request_body 处理
    /// 复杂消息体（含 content blocks / 多模态），非 nil 时优先于 `messages`。
    pub raw_messages: Option<Value>,
    /// string 或 ContentBlock[]。
    pub system: Option<Value>,
    /// 标准工具定义（Tool[]）。
    pub tools: Option<Value>,
    pub temperature: Option<f64>,
    pub thinking: Option<ThinkingConfig>,
    pub metadata: Option<std::collections::BTreeMap<String, String>>,
    /// 显式 beta（SDK 自动合并）。
    pub betas: Option<Vec<String>>,
    /// 服务端工具（build_request_body 合入 tools 数组）。
    pub server_tools: Option<Vec<ServerTool>>,
    /// "" | "fast"（Fast Mode）。
    pub speed: Option<String>,
    pub effort: Option<EffortConfig>,
    pub output_config: Option<OutputConfig>,
    /// 任意扩展字段（build_request_body 合入请求体）。
    pub extra_body: Option<serde_json::Map<String, Value>>,

    /// v0.13.0: OpenAI wire format 原生字段。AnthropicAdapter 忽略，OpenAIAdapter 按规范序列化。
    /// 对应 OpenAI `parallel_tool_calls` 顶层字段，`None` = 不设置（沿用上游默认 true）。
    pub parallel_tool_calls: Option<bool>,

    /// v1.6.0: 业务侧终端用户 id，跨 provider 通用语义。
    /// OpenAI wire → 顶层 `body["user_id"]`；Anthropic wire → 合并到 `body["metadata"]["user_id"]`。
    pub end_user_id: Option<String>,
}

// ---------- Web Search Sources ----------

/// 联网搜索结果来源（与后端 `adk/stream_helpers.go SourceItem` 对齐）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WebSearchSource {
    pub title: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// 搜索来源事件（从 SSE "sources" 事件解析）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourcesEvent {
    pub sources: Vec<WebSearchSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// 从 [`StreamEvent`] 中解析搜索来源。返回 `None` 表示该事件不是 sources 类型。
pub fn parse_sources_event(ev: &StreamEvent) -> Option<SourcesEvent> {
    #[derive(Deserialize)]
    struct Wrapper {
        #[serde(default)]
        r#type: Option<String>,
        #[serde(default)]
        sources: Option<Vec<WebSearchSource>>,
        #[serde(default)]
        session_id: Option<String>,
    }
    let wrapper: Wrapper = serde_json::from_str(&ev.data).ok()?;
    if wrapper.r#type.as_deref() != Some("sources") && ev.event != "sources" {
        return None;
    }
    let sources = wrapper.sources.filter(|s| !s.is_empty())?;
    Some(SourcesEvent {
        sources,
        session_id: wrapper.session_id,
    })
}

// =============================================================================
// ChatResponse
// =============================================================================

/// 同步聊天响应（Anthropic format，v0.4.1）。
///
/// `token_remaining` / `call_remaining` / `model_token_remaining*` 不在 wire JSON 中，
/// 由 client 从 Header 填充。哨兵 `-1` 表示服务端未返回。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub r#type: String,
    pub model: String,
    pub role: String,
    pub content: Vec<ChatContentBlock>,
    pub stop_reason: String,
    pub usage: ChatUsage,
    /// -1 表示服务端未返回。
    #[serde(rename = "tokenRemaining", default)]
    pub token_remaining: i64,
    #[serde(rename = "callRemaining", default)]
    pub call_remaining: i64,
    #[serde(rename = "modelTokenRemaining", default)]
    pub model_token_remaining: i64,
    #[serde(rename = "modelTokenRemainingETU", default)]
    pub model_token_remaining_etu: i64,
}

// =============================================================================
// SSE Stream
// =============================================================================

/// SSE 流式事件。**扁平 struct**（方案 §4.1 红线）。`data` 是裸 JSON 串，由 helper
/// （[`parse_sources_event`]/[`parse_settlement`]）二次解析。
///
/// v0.11.0 新增 `blockIndex`/`blockType`/`ephemeral` 三字段（in-band content block 元数据）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamEvent {
    pub event: String,
    pub data: String,
    /// 对齐 Anthropic content_block_start/delta/stop 的 index 字段。
    #[serde(
        rename = "blockIndex",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub block_index: Option<i64>,
    /// 由 content_block_start 解析得到，delta/stop 从 index→type 映射查出。
    #[serde(rename = "blockType", default, skip_serializing_if = "Option::is_none")]
    pub block_type: Option<String>,
    /// 网关标记此 block 下一轮不应回传。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ephemeral: Option<bool>,
}

/// 流式结算事件（从 settled SSE 事件解析）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamSettlement {
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "consumeStatus")]
    pub consume_status: String,
    #[serde(rename = "inputTokens")]
    pub input_tokens: i64,
    #[serde(rename = "outputTokens")]
    pub output_tokens: i64,
    #[serde(rename = "totalTokens")]
    pub total_tokens: i64,
    /// 结算后剩余 token（-1 表示服务端未返回）。
    #[serde(rename = "tokenRemaining")]
    pub token_remaining: i64,
    /// 结算后剩余调用次数（-1 表示服务端未返回）。
    #[serde(rename = "callRemaining")]
    pub call_remaining: i64,
}

/// 从 settled 类型的 [`StreamEvent`] 中解析结算信息。不是 settled 类型则返回 `None`。
pub fn parse_settlement(ev: &StreamEvent) -> Option<StreamSettlement> {
    if ev.event != "settled" && ev.event != "pending_settle" {
        return None;
    }
    // s 的字段全可选 → 解析失败返回 None，缺字段用默认/哨兵（对齐 TS `?? 默认`）。
    #[derive(Deserialize)]
    struct Partial {
        #[serde(rename = "requestId", default)]
        request_id: Option<String>,
        #[serde(rename = "consumeStatus", default)]
        consume_status: Option<String>,
        #[serde(rename = "inputTokens", default)]
        input_tokens: Option<i64>,
        #[serde(rename = "outputTokens", default)]
        output_tokens: Option<i64>,
        #[serde(rename = "totalTokens", default)]
        total_tokens: Option<i64>,
        #[serde(rename = "tokenRemaining", default)]
        token_remaining: Option<i64>,
        #[serde(rename = "callRemaining", default)]
        call_remaining: Option<i64>,
    }
    let s: Partial = serde_json::from_str(&ev.data).ok()?;
    Some(StreamSettlement {
        request_id: s.request_id.unwrap_or_default(),
        consume_status: s.consume_status.unwrap_or_default(),
        input_tokens: s.input_tokens.unwrap_or(0),
        output_tokens: s.output_tokens.unwrap_or(0),
        total_tokens: s.total_tokens.unwrap_or(0),
        token_remaining: s.token_remaining.unwrap_or(-1),
        call_remaining: s.call_remaining.unwrap_or(-1),
    })
}

/// 零值 [`ModelCapabilities`]（对齐 TS `zeroModelCapabilities`）。模型不在列表时返回。
pub fn zero_model_capabilities() -> ModelCapabilities {
    ModelCapabilities {
        supports_desktop_visual_understanding: Some(false),
        ..Default::default()
    }
}
