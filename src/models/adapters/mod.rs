//! 双格式 adapter 路由。端口自 `models/adapters/index.ts`
//! （其端口自 `acosmi-sdk-go/adapter.go` v0.19.0）。
//!
//! SDK 层按上游托管模型元数据选路：
//!   preferred_format == "anthropic" → AnthropicAdapter → POST /managed-models/:id/anthropic
//!   preferred_format == "openai"    → OpenAIAdapter    → POST /managed-models/:id/chat
//!
//! SDK 只负责：格式路由 + 请求结构转换 + 响应结构转换。
//! 厂商特定协议差异（endpoint/auth/region/字段裁剪）由 Nexus Gateway Profile 处理。
//!
//! ## 红线（双产品消费）
//! AnthropicAdapter + OpenAIAdapter 等地位，不可合并/降级。
//!
//! ## Rust 设计
//! TS `ProviderAdapter` interface 有恰两个**无状态**实现，Rust 用 `enum Adapter`
//! （`Anthropic` / `OpenAI`）+ `impl` 方法，比 `Box<dyn>` 干净且零分配。
//! 方法签名 1:1 对齐 TS：`format` / `endpoint_suffix` / `build_request_body`（返回
//! `serde_json::Map<String,Value>` = TS `Record`）/ `parse_response`（→ `ChatResponse`）/
//! `parse_stream_line`（→ `(StreamEvent, bool /*done*/)`）。

pub mod anthropic;
pub mod openai;

use super::types::{ChatRequest, ChatResponse, ManagedModel, ModelCapabilities, StreamEvent};
use crate::shared::errors::Result;
use serde_json::{Map, Value};

/// 标识请求格式。对齐 TS `enum ProviderFormat { Anthropic=0, OpenAI=1 }`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ProviderFormat {
    /// Anthropic 原生格式。
    Anthropic = 0,
    /// OpenAI 兼容格式。
    OpenAI = 1,
}

/// 将 ChatRequest 转换为特定格式的 adapter。两个无状态实现的 enum 等价 TS
/// `ProviderAdapter` interface 的两个 class。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adapter {
    /// Anthropic 原生格式 adapter。
    Anthropic,
    /// OpenAI 兼容格式 adapter（所有非 Anthropic 厂商）。
    OpenAI,
}

impl Adapter {
    /// 此 adapter 使用的请求格式。
    pub fn format(&self) -> ProviderFormat {
        match self {
            Adapter::Anthropic => ProviderFormat::Anthropic,
            Adapter::OpenAI => ProviderFormat::OpenAI,
        }
    }

    /// API 路径后缀。Anthropic: `/anthropic`，OpenAI: `/chat`。
    pub fn endpoint_suffix(&self) -> &'static str {
        match self {
            Adapter::Anthropic => "/anthropic",
            Adapter::OpenAI => "/chat",
        }
    }

    /// 将 ChatRequest 转换为 HTTP body（`serde_json::Map` → JSON）。`caps` 用于条件化字段注入。
    pub fn build_request_body(
        &self,
        caps: &ModelCapabilities,
        req: &ChatRequest,
    ) -> Map<String, Value> {
        match self {
            Adapter::Anthropic => anthropic::build_request_body(caps, req),
            Adapter::OpenAI => openai::build_request_body(caps, req),
        }
    }

    /// 解析同步响应 body 为 [`ChatResponse`]。
    pub fn parse_response(&self, body: &[u8]) -> Result<ChatResponse> {
        match self {
            Adapter::Anthropic => anthropic::parse_response(body),
            Adapter::OpenAI => openai::parse_response(body),
        }
    }

    /// 解析一行 SSE data 为 [`StreamEvent`]。返回 `(event, done)`；
    /// `done=true` 表示流结束（`[DONE]` 或 message_stop）。
    pub fn parse_stream_line(&self, event_type: &str, data: &str) -> Result<(StreamEvent, bool)> {
        match self {
            Adapter::Anthropic => Ok(anthropic::parse_stream_line(event_type, data)),
            Adapter::OpenAI => openai::parse_stream_line(event_type, data),
        }
    }
}

/// 根据 provider 返回对应的 adapter（v0.5.0 遗留 API，向后兼容）。
/// 新代码应使用 [`get_adapter_for_model`]。
///
/// `anthropic` / `acosmi` → AnthropicAdapter；其他 → OpenAIAdapter。
pub fn get_adapter(provider: &str) -> Adapter {
    match provider.to_lowercase().as_str() {
        "anthropic" | "acosmi" => Adapter::Anthropic,
        _ => Adapter::OpenAI,
    }
}

/// 按 [`ManagedModel`] 的 `preferred_format` / `supported_formats` 选择 adapter。
///
/// 决策顺序（逐字对齐 `adapters/index.ts`）：
///  1. `preferred_format` 非空 **且** 该格式在 `supported_formats` 内（或 `supported_formats`
///     未声明）→ 按其值返回（anthropic | openai）。
///  2. `supported_formats` 含 "anthropic" → AnthropicAdapter。
///  3. `supported_formats` 含 "openai" → OpenAIAdapter。
///  4. 两字段均空（旧上游）→ 回落 provider 名硬编码（原 `get_adapter` 行为）。
///
/// 格式一致性护栏：`preferred_format` 仅在确被 `supported_formats` 收录时才采信，
/// 防止上游元数据漂移把 SDK 路由到模型并不支持的格式端点。
///
/// # Examples
///
/// ```
/// use acosmi::{get_adapter_for_model, Adapter, ManagedModel};
///
/// let m = ManagedModel { preferred_format: Some("openai".into()), ..Default::default() };
/// assert_eq!(get_adapter_for_model(&m), Adapter::OpenAI);
/// ```
pub fn get_adapter_for_model(m: &ManagedModel) -> Adapter {
    let mut has_anthropic = false;
    let mut has_openai = false;
    if let Some(formats) = &m.supported_formats {
        for f in formats {
            match f.trim().to_lowercase().as_str() {
                "anthropic" => has_anthropic = true,
                "openai" => has_openai = true,
                _ => {}
            }
        }
    }
    let declared = has_anthropic || has_openai;

    let pref = m
        .preferred_format
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    // preferred_format 仅在确被 supported_formats 收录（或未声明）时采信。
    match pref.as_str() {
        "anthropic" if !declared || has_anthropic => return Adapter::Anthropic,
        "openai" if !declared || has_openai => return Adapter::OpenAI,
        _ => {}
    }

    if has_anthropic {
        return Adapter::Anthropic;
    }
    if has_openai {
        return Adapter::OpenAI;
    }

    // 旧上游未填字段：回落到 provider 名硬编码（向后兼容）。
    get_adapter(&m.provider.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(
        provider: &str,
        supported: Option<Vec<&str>>,
        preferred: Option<&str>,
    ) -> ManagedModel {
        ManagedModel {
            provider: provider.to_string(),
            supported_formats: supported.map(|v| v.iter().map(|s| s.to_string()).collect()),
            preferred_format: preferred.map(|s| s.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn route_preferred_anthropic_supported() {
        let m = model(
            "deepseek",
            Some(vec!["anthropic", "openai"]),
            Some("anthropic"),
        );
        assert_eq!(get_adapter_for_model(&m), Adapter::Anthropic);
    }

    #[test]
    fn route_preferred_conflicts_supported_falls_through() {
        // 护栏：preferred=anthropic 但 supported=[openai] → 不采信 preferred，回落 hasOpenAI。
        let m = model("acosmi", Some(vec!["openai"]), Some("anthropic"));
        assert_eq!(get_adapter_for_model(&m), Adapter::OpenAI);
    }

    #[test]
    fn route_preferred_openai_conflicts_falls_through() {
        let m = model("anthropic", Some(vec!["anthropic"]), Some("openai"));
        assert_eq!(get_adapter_for_model(&m), Adapter::Anthropic);
    }

    #[test]
    fn route_preferred_when_supported_undeclared_is_trusted() {
        // supported_formats 未声明 → preferred 被采信。
        let m = model("deepseek", None, Some("anthropic"));
        assert_eq!(get_adapter_for_model(&m), Adapter::Anthropic);
    }

    #[test]
    fn route_supported_only_anthropic() {
        let m = model("deepseek", Some(vec!["anthropic"]), None);
        assert_eq!(get_adapter_for_model(&m), Adapter::Anthropic);
    }

    #[test]
    fn route_fallback_to_provider_name() {
        // 两字段均空 → provider 名硬编码。
        assert_eq!(
            get_adapter_for_model(&model("anthropic", None, None)),
            Adapter::Anthropic
        );
        assert_eq!(
            get_adapter_for_model(&model("acosmi", None, None)),
            Adapter::Anthropic
        );
        assert_eq!(
            get_adapter_for_model(&model("deepseek", None, None)),
            Adapter::OpenAI
        );
    }

    #[test]
    fn endpoint_suffix_and_format() {
        assert_eq!(Adapter::Anthropic.endpoint_suffix(), "/anthropic");
        assert_eq!(Adapter::OpenAI.endpoint_suffix(), "/chat");
        assert_eq!(Adapter::Anthropic.format(), ProviderFormat::Anthropic);
        assert_eq!(Adapter::OpenAI.format(), ProviderFormat::OpenAI);
    }
}
