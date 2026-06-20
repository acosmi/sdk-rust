//! Beta Header 常量。端口自 `models/betas.ts`（其端口自 `acosmi-sdk-go/betas.go`）。
//!
//! 经联网验证的真实 Anthropic API beta 值。虚构/错误日期的 header 已剔除。

use super::types::{ChatRequest, ModelCapabilities, THINKING_OFF};

/// ISP：交错思考。
const BETA_INTERLEAVED_THINKING: &str = "interleaved-thinking-2025-05-14";
/// 1M 上下文（retiring 2026-04-30）。
const BETA_CONTEXT_1M: &str = "context-1m-2025-08-07";
/// 上下文编辑。
const BETA_CONTEXT_MANAGEMENT: &str = "context-management-2025-06-27";
/// 结构化输出。
const BETA_STRUCTURED_OUTPUTS: &str = "structured-outputs-2025-11-13";
/// Tool Search。
const BETA_ADVANCED_TOOL_USE: &str = "advanced-tool-use-2025-11-20";
/// Effort 控制（Opus 4.5 需要，4.6 stable）。
const BETA_EFFORT: &str = "effort-2025-11-24";
/// 缓存作用域隔离。
const BETA_PROMPT_CACHING_SCOPE: &str = "prompt-caching-scope-2026-01-05";
/// 快速推理（Opus 4.6）。
const BETA_FAST_MODE: &str = "fast-mode-2026-02-01";
/// 思考脱敏。
const BETA_REDACT_THINKING: &str = "redact-thinking-2026-02-12";
/// 高效工具（Claude 3.7，Claude 4 内置）。
const BETA_TOKEN_EFFICIENT_TOOLS: &str = "token-efficient-tools-2025-02-19";

/// 根据模型能力和请求参数自动组装 beta header 列表。
pub fn build_betas(caps: &ModelCapabilities, req: &ChatRequest) -> Vec<String> {
    let mut betas: Vec<String> = Vec::new();

    // ── 思考相关 ──
    if caps.supports_isp {
        betas.push(BETA_INTERLEAVED_THINKING.to_string());
        betas.push(BETA_CONTEXT_MANAGEMENT.to_string());
    }
    if caps.supports_redact_thinking
        && req.thinking.as_ref().and_then(|t| t.display.as_deref()) == Some("summary")
    {
        betas.push(BETA_REDACT_THINKING.to_string());
    }

    // ── 上下文 ──
    if caps.supports_1m_context {
        betas.push(BETA_CONTEXT_1M.to_string());
    }

    // ── 输出控制（互斥：structured-outputs ⊕ token-efficient-tools）──
    let has_structured_output = caps.supports_structured_output && req.output_config.is_some();
    if has_structured_output {
        betas.push(BETA_STRUCTURED_OUTPUTS.to_string());
    } else if caps.supports_token_efficient {
        betas.push(BETA_TOKEN_EFFICIENT_TOOLS.to_string());
    }

    // ── Tool Search ──
    if caps.supports_tool_search {
        betas.push(BETA_ADVANCED_TOOL_USE.to_string());
    }

    // ── 推理控制 ──
    // Level 模式下 resolve_thinking_level 直接写 body["effort"]，req.effort 仍为 None，
    // 所以需额外判断 Level 是否激活了 effort。
    let thinking_level_active = req
        .thinking
        .as_ref()
        .and_then(|t| t.level.as_deref())
        .map(|lvl| !lvl.is_empty() && lvl != THINKING_OFF)
        .unwrap_or(false);
    let needs_effort = req.effort.is_some() || thinking_level_active;
    if caps.supports_effort && needs_effort {
        betas.push(BETA_EFFORT.to_string());
    }
    if caps.supports_fast_mode && req.speed.as_deref() == Some("fast") {
        betas.push(BETA_FAST_MODE.to_string());
    }

    // ── 缓存 ──
    if caps.supports_prompt_cache {
        betas.push(BETA_PROMPT_CACHING_SCOPE.to_string());
    }

    // ── 合并客户端显式传入（去重）──
    unique_merge(betas, req.betas.clone().unwrap_or_default())
}

/// 合并两个字符串数组并去重，保留顺序。`extra` 空时直接返回 `base`。
pub fn unique_merge(mut base: Vec<String>, extra: Vec<String>) -> Vec<String> {
    if extra.is_empty() {
        return base;
    }
    let mut seen: std::collections::HashSet<String> = base.iter().cloned().collect();
    for s in extra {
        if !seen.contains(&s) {
            seen.insert(s.clone());
            base.push(s);
        }
    }
    base
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::types::{OutputConfig, ThinkingConfig};

    fn caps_all_off() -> ModelCapabilities {
        ModelCapabilities::default()
    }

    #[test]
    fn structured_outputs_and_token_efficient_are_mutually_exclusive() {
        // 两能力都开 + 有 output_config → 只发 structured-outputs。
        let mut caps = caps_all_off();
        caps.supports_structured_output = true;
        caps.supports_token_efficient = true;
        let req = ChatRequest {
            output_config: Some(OutputConfig {
                format: Some("json_schema".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let betas = build_betas(&caps, &req);
        assert!(betas.iter().any(|b| b == "structured-outputs-2025-11-13"));
        assert!(!betas
            .iter()
            .any(|b| b == "token-efficient-tools-2025-02-19"));
    }

    #[test]
    fn token_efficient_when_no_output_config() {
        let mut caps = caps_all_off();
        caps.supports_structured_output = true;
        caps.supports_token_efficient = true;
        let req = ChatRequest::default(); // 无 output_config
        let betas = build_betas(&caps, &req);
        assert!(!betas.iter().any(|b| b == "structured-outputs-2025-11-13"));
        assert!(betas
            .iter()
            .any(|b| b == "token-efficient-tools-2025-02-19"));
    }

    #[test]
    fn effort_beta_triggered_by_thinking_level() {
        let mut caps = caps_all_off();
        caps.supports_effort = true;
        let req = ChatRequest {
            thinking: Some(ThinkingConfig {
                level: Some("high".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let betas = build_betas(&caps, &req);
        assert!(betas.iter().any(|b| b == "effort-2025-11-24"));
    }

    #[test]
    fn unique_merge_dedups_preserving_order() {
        let base = vec!["a".to_string(), "b".to_string()];
        let extra = vec!["b".to_string(), "c".to_string()];
        assert_eq!(unique_merge(base, extra), vec!["a", "b", "c"]);
    }
}
