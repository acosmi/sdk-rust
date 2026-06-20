//! SDK-safe 合同模板公共领域类型。端口自 `compliance/template/types.ts`。
//!
//! 设计原则见 `compliance/evidence/mod.rs` 顶部说明。模板生命周期：DRAFT → 上传 PDF → 编辑
//! 字段 → publish 进入 PUBLISHED；可 archive 进入 ARCHIVED；删除只允许在 DRAFT 状态。

use crate::macros::open_string_union;
use crate::shared::pagination::PageRequest;
use serde::{Deserialize, Serialize};

// =============================================================================
// Field overlay
// =============================================================================

open_string_union! {
    /// 模板上的可填充字段类型：签名 / 印章 / 文本 / 日期 / 勾选。开放联合。
    ContractTemplateFieldType {
        SIGNATURE => "signature",
        SEAL => "seal",
        TEXT => "text",
        DATE => "date",
        CHECK => "check",
    }
}

#[allow(clippy::derivable_impls)]
impl Default for ContractTemplateFieldType {
    fn default() -> Self {
        Self(String::new())
    }
}

/// 模板字段叠加项。位置以 `page` + `x`/`y`/`width`/`height` 描述。SDK 不在客户端做几何 /
/// 坐标系校验，原样透传给后端。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContractTemplateField {
    /// 字段稳定 key，调用方业务侧自定。
    pub key: String,
    /// 字段类型。
    pub r#type: ContractTemplateFieldType,
    /// 字段在 UI 上展示的标签。
    pub label: String,
    /// PDF 页码（1-based 或调用方约定，SDK 原样透传）。
    pub page: i64,
    /// PDF 坐标系横坐标。
    pub x: f64,
    /// PDF 坐标系纵坐标。
    pub y: f64,
    /// 字段宽度。
    pub width: f64,
    /// 字段高度。
    pub height: f64,
    /// 字段绑定的角色（签署人角色 key，可选）。
    #[serde(
        rename = "assignedRole",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub assigned_role: Option<String>,
    /// 字段在模板内的排序键。
    pub order: i64,
    /// 是否为必填字段。
    pub required: bool,
}

// =============================================================================
// Template
// =============================================================================

open_string_union! {
    /// 模板状态。DRAFT 可编辑 / 删除 / 上传 PDF / publish；PUBLISHED / ARCHIVED 只读。开放联合。
    ContractTemplateStatus {
        DRAFT => "DRAFT",
        PUBLISHED => "PUBLISHED",
        ARCHIVED => "ARCHIVED",
    }
}

/// 合同模板详情。对应后端 G5 `ContractTemplateResp`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractTemplateResp {
    pub id: i64,
    /// 模板编号（业务编号，区别于数值主键 `id`）。
    #[serde(rename = "templateNo")]
    pub template_no: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: ContractTemplateStatus,
    /// 已上传 PDF 的哈希；未上传时缺省。
    #[serde(rename = "pdfHash", default, skip_serializing_if = "Option::is_none")]
    pub pdf_hash: Option<String>,
    /// 已上传 PDF 的页数；未上传时缺省。
    #[serde(
        rename = "pdfPageCount",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pdf_page_count: Option<i64>,
    pub fields: Vec<ContractTemplateField>,
    /// 已发布版本号；DRAFT 阶段为 0。
    #[serde(rename = "currentVersion")]
    pub current_version: i64,
    /// 创建时间 ISO-8601。
    #[serde(rename = "createTime")]
    pub create_time: String,
}

/// 合同模板分页【列表项】视图。对应后端 G5 `ContractTemplatePageItem`。**不含 `fields`**。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractTemplatePageItem {
    pub id: i64,
    #[serde(rename = "templateNo")]
    pub template_no: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: ContractTemplateStatus,
    #[serde(rename = "pdfHash", default, skip_serializing_if = "Option::is_none")]
    pub pdf_hash: Option<String>,
    #[serde(
        rename = "pdfPageCount",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pdf_page_count: Option<i64>,
    #[serde(rename = "currentVersion")]
    pub current_version: i64,
    /// 创建时间 ISO-8601。
    #[serde(rename = "createTime")]
    pub create_time: String,
}

/// 合同模板版本快照。对应后端 G5 `ContractTemplateVersion`。版本是【不可变】的离线复核依据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractTemplateVersion {
    pub id: i64,
    #[serde(rename = "templateId")]
    pub template_id: i64,
    pub version: i64,
    pub name: String,
    #[serde(rename = "pdfHash", default, skip_serializing_if = "Option::is_none")]
    pub pdf_hash: Option<String>,
    pub fields: Vec<ContractTemplateField>,
    /// publish 时模板状态的字面量快照。
    #[serde(rename = "statusAtSnapshot")]
    pub status_at_snapshot: String,
    /// 创建时间 ISO-8601。
    #[serde(rename = "createTime")]
    pub create_time: String,
}

// =============================================================================
// Requests
// =============================================================================

/// 创建合同模板请求。`fields` 可选。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreateContractTemplateRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<ContractTemplateField>>,
}

/// 更新合同模板请求。仅 DRAFT 状态下允许调用。所有字段可选，缺省的字段视为【不修改】。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateContractTemplateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<ContractTemplateField>>,
}

/// 上传模板 PDF 请求。`pdf_base64` 为 base64 编码的 PDF 原文。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadContractTemplatePdfRequest {
    /// base64 编码的 PDF 原文。
    #[serde(rename = "pdfBase64")]
    pub pdf_base64: String,
}

/// `list_contract_templates` 请求参数。
#[derive(Debug, Clone, Default)]
pub struct ListContractTemplatesRequest {
    pub page: PageRequest,
    /// 模板状态过滤。
    pub status: Option<String>,
    pub create_time_start: Option<String>,
    pub create_time_end: Option<String>,
}
