//! 技能商店 / 技能生产器 / 统一工具域类型。
//!
//! 端口自 `skills/types.ts`（其端口自 `acosmi-sdk-go/types.go` v0.19.0）。
//!
//! 命名约定：字段名 = Go json tag 字面量（wire format），不做 camelCase 重映射。

use serde::{Deserialize, Serialize};

// =============================================================================
// Skill Store
// =============================================================================

/// 技能商店完整条目（Detail / resolve / browse 全量返回）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillStoreItem {
    pub id: String,
    #[serde(rename = "pluginId")]
    pub plugin_id: String,
    pub key: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub category: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: String,
    #[serde(rename = "outputSchema")]
    pub output_schema: String,
    pub timeout: i64,
    #[serde(rename = "retryCount")]
    pub retry_count: i64,
    #[serde(rename = "retryDelay")]
    pub retry_delay: i64,
    pub version: String,
    #[serde(rename = "totalCalls")]
    pub total_calls: i64,
    #[serde(rename = "avgDurationMs")]
    pub avg_duration_ms: i64,
    #[serde(rename = "successRate")]
    pub success_rate: f64,
    #[serde(rename = "isEnabled")]
    pub is_enabled: bool,
    #[serde(rename = "securityLevel")]
    pub security_level: String,
    #[serde(rename = "securityScore")]
    pub security_score: i64,
    pub scope: String,
    pub status: String,
    #[serde(rename = "downloadCount")]
    pub download_count: i64,
    /// 网关 `json:"readme,omitempty"`，空时缺字段，故可选。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readme: Option<String>,
    /// SKILL.md（Anthropic 标准格式）；网关 `json:"skillMd,omitempty"`，仅 Detail/resolve/browse 全量返回。
    #[serde(rename = "skillMd", default, skip_serializing_if = "Option::is_none")]
    pub skill_md: Option<String>,
    pub tags: Vec<String>,
    pub author: String,
    #[serde(rename = "publisherId")]
    pub publisher_id: String,
    #[serde(rename = "isPublished")]
    pub is_published: bool,
    #[serde(rename = "pluginName")]
    pub plugin_name: String,
    #[serde(rename = "pluginIcon")]
    pub plugin_icon: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    #[serde(
        rename = "certificationStatus",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub certification_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// 技能商店搜索参数（非 wire 类型，用于 client 方法参数）。
#[derive(Debug, Clone, Default)]
pub struct SkillStoreQuery {
    pub category: Option<String>,
    pub keyword: Option<String>,
    pub tag: Option<String>,
}

/// 技能统计概览。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillSummary {
    pub installed: i64,
    pub created: i64,
    pub total: i64,
    #[serde(rename = "storeAvailable")]
    pub store_available: i64,
}

/// 技能商店分页浏览响应。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillBrowseResponse {
    pub items: Vec<SkillStoreItem>,
    pub total: i64,
    pub page: i64,
    #[serde(rename = "pageSize")]
    pub page_size: i64,
}

/// 技能商店列表项（轻量，仅含浏览所需字段）。
/// 配合服务端 `fields=minimal` 参数使用，响应体积缩减 90%+。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillStoreListItem {
    pub id: String,
    pub key: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub category: String,
    pub version: String,
    pub author: String,
    #[serde(rename = "downloadCount")]
    pub download_count: i64,
    pub tags: Vec<String>,
    #[serde(
        rename = "certificationStatus",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub certification_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

/// 技能商店轻量浏览响应。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillBrowseListResponse {
    pub items: Vec<SkillStoreListItem>,
    pub total: i64,
    pub page: i64,
    #[serde(rename = "pageSize")]
    pub page_size: i64,
}

/// 技能认证状态响应。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CertificationStatus {
    #[serde(rename = "skillId")]
    pub skill_id: String,
    #[serde(rename = "certificationStatus")]
    pub certification_status: String,
    #[serde(
        rename = "certifiedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub certified_at: Option<i64>,
    #[serde(
        rename = "securityLevel",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub security_level: Option<String>,
    #[serde(rename = "securityScore")]
    pub security_score: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<serde_json::Value>,
}

// =============================================================================
// Skill Generator
// =============================================================================

/// 技能生成请求。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenerateSkillRequest {
    pub purpose: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub examples: Option<Vec<String>>,
    #[serde(
        rename = "inputHints",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub input_hints: Option<String>,
    #[serde(
        rename = "outputHints",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub output_hints: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

/// 技能生成结果。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenerateSkillResult {
    #[serde(rename = "skillName")]
    pub skill_name: String,
    #[serde(rename = "skillKey")]
    pub skill_key: String,
    pub description: String,
    #[serde(rename = "skillMd")]
    pub skill_md: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: String,
    #[serde(rename = "outputSchema")]
    pub output_schema: String,
    #[serde(rename = "testCases")]
    pub test_cases: Vec<String>,
    pub readme: String,
    pub category: String,
    pub tags: Vec<String>,
    pub timeout: i64,
}

/// 技能优化请求。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OptimizeSkillRequest {
    #[serde(rename = "skillName")]
    pub skill_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(
        rename = "inputSchema",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub input_schema: Option<String>,
    #[serde(
        rename = "outputSchema",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub output_schema: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspects: Option<Vec<String>>,
}

/// 技能优化结果。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OptimizeSkillResult {
    #[serde(rename = "optimizedSkill")]
    pub optimized_skill: GenerateSkillResult,
    pub changes: Vec<String>,
    pub score: f64,
}

// =============================================================================
// Unified Tools
// =============================================================================

/// 统一工具视图。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolView {
    pub id: String,
    pub key: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub category: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: String,
    #[serde(rename = "outputSchema")]
    pub output_schema: String,
    pub timeout: i64,
    #[serde(rename = "isEnabled")]
    pub is_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ToolProvider>,
}

/// 工具提供方。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolProvider {
    pub id: String,
    pub name: String,
    pub icon: String,
    #[serde(rename = "sourceType")]
    pub source_type: String,
    #[serde(
        rename = "mcpEndpoint",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub mcp_endpoint: Option<String>,
    #[serde(rename = "isEnabled")]
    pub is_enabled: bool,
}

/// 工具列表响应。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolListResponse {
    pub skills: Vec<ToolView>,
    pub total: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_store_item_round_trip() {
        let json = r#"{"id":"s1","pluginId":"p1","key":"k","name":"N","description":"D","icon":"i","category":"CODING","inputSchema":"{}","outputSchema":"{}","timeout":30,"retryCount":1,"retryDelay":100,"version":"1.0.0","totalCalls":10,"avgDurationMs":200,"successRate":0.99,"isEnabled":true,"securityLevel":"LOW","securityScore":80,"scope":"TENANT","status":"ACTIVE","downloadCount":5,"tags":["a"],"author":"me","publisherId":"pub","isPublished":true,"pluginName":"pn","pluginIcon":"pi","updatedAt":"2026-06-19T00:00:00Z"}"#;
        let item: SkillStoreItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.id, "s1");
        assert_eq!(item.success_rate, 0.99);
        assert!(item.is_enabled);
        assert_eq!(item.readme, None);
        // round-trip 不丢字段（缺省可选不输出）。
        let back: SkillStoreItem =
            serde_json::from_str(&serde_json::to_string(&item).unwrap()).unwrap();
        assert_eq!(back.download_count, 5);
    }
}
