//! SDK-safe 证据资产 / 证据包公共领域类型。端口自 `compliance/evidence/types.ts`。
//!
//! 设计原则（严格）：
//!   - 仅声明 Acosmi 领域抽象；不暴露受控证书/密钥材料、provider endpoint、下游内部路由码、
//!     provider raw payload、callback signature material、billing commit 内部字段、storage
//!     bucket/key、tenant id、subject snapshot id。
//!   - 字段名严格 camelCase（serde rename），对应 Java 服务端 jackson 默认序列化。
//!   - status 开放字符串字面量与 Java 后端枚举 name() 保持一致；前端按字面量分支判断，不依赖
//!     后端文案。
//!   - 时间字段统一 ISO-8601 字符串（Java LocalDateTime.toString()）。
//!   - L0/L2/L3 PII 分级字段端口为普通字段（加密落盘是后端职责）。

use crate::macros::open_string_union;
use crate::shared::pagination::PageRequest;
use serde::{Deserialize, Serialize};

// =============================================================================
// Evidence Asset
// =============================================================================

open_string_union! {
    /// 证据资产类型。开放联合，后端保留新增空间。
    ComplianceAssetType {
        CONTRACT => "CONTRACT",
        CODE => "CODE",
        IMAGE => "IMAGE",
        DOCUMENT => "DOCUMENT",
        ARCHIVE => "ARCHIVE",
        HASH_ONLY => "HASH_ONLY",
        URL_SNAPSHOT => "URL_SNAPSHOT",
        RELEASE => "RELEASE",
        LOG => "LOG",
        OTHER => "OTHER",
    }
}

open_string_union! {
    /// 哈希算法。开放联合。
    ComplianceHashAlgorithm {
        SHA256 => "sha256",
        SHA512 => "sha512",
        SM3 => "sm3",
    }
}

open_string_union! {
    /// digest 来源。开放联合。
    ComplianceDigestSource {
        CLIENT => "CLIENT",
        COMPLIANCE_SERVICE => "COMPLIANCE_SERVICE",
        PROVIDER => "PROVIDER",
    }
}

open_string_union! {
    /// 隐私级别。开放联合。
    CompliancePrivacyLevel {
        PUBLIC => "public",
        PRIVATE => "private",
    }
}

/// 证据资产对外稳定视图。对应后端 `EvidenceAssetRespVO`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceAsset {
    pub id: i64,
    /// 业务编号；SDK / 前端引用时优先使用 evidenceNo。
    #[serde(rename = "evidenceNo")]
    pub evidence_no: String,
    /// publish 后的公开 verify code；DRAFT / 非 public privacy 时为 null。
    #[serde(
        rename = "publicVerifyCode",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub public_verify_code: Option<String>,
    #[serde(rename = "assetType")]
    pub asset_type: ComplianceAssetType,
    pub name: String,
    #[serde(rename = "mimeType", default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    #[serde(rename = "hashAlgorithm")]
    pub hash_algorithm: ComplianceHashAlgorithm,
    /// 资产 hash（hex 字符串）。
    #[serde(rename = "contentHash")]
    pub content_hash: String,
    #[serde(
        rename = "canonicalizationProfile",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub canonicalization_profile: Option<String>,
    #[serde(rename = "digestSource")]
    pub digest_source: ComplianceDigestSource,
    #[serde(rename = "privacyLevel")]
    pub privacy_level: CompliancePrivacyLevel,
    pub status: String,
}

/// 创建证据资产请求；与 Java `EvidenceAssetCreateReq` 对齐。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreateEvidenceAssetRequest {
    /// AssetTypeEnum.name()。
    #[serde(rename = "assetType")]
    pub asset_type: ComplianceAssetType,
    pub name: String,
    #[serde(rename = "mimeType", default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// 'sha256' / 'sha512' / 'sm3'。
    #[serde(rename = "hashAlgorithm")]
    pub hash_algorithm: ComplianceHashAlgorithm,
    /// hex 字符串；hash-only 资产必填。
    #[serde(
        rename = "declaredHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub declared_hash: Option<String>,
    /// CLIENT / COMPLIANCE_SERVICE / PROVIDER。
    #[serde(
        rename = "digestSource",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub digest_source: Option<ComplianceDigestSource>,
    /// 原文 base64；hash-only 时缺省即可。
    #[serde(
        rename = "contentBase64",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub content_base64: Option<String>,
    /// public / private（默认 private）。
    #[serde(
        rename = "privacyLevel",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub privacy_level: Option<CompliancePrivacyLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

// `open_string_union!` 生成的类型无 Default；为请求体 derive(Default) 补上（空串 = 未设置）。
#[allow(clippy::derivable_impls)]
impl Default for ComplianceAssetType {
    fn default() -> Self {
        Self(String::new())
    }
}
#[allow(clippy::derivable_impls)]
impl Default for ComplianceHashAlgorithm {
    fn default() -> Self {
        Self(String::new())
    }
}

/// 公开 verify 结果。隐私边界：不暴露 PII / 合同原文 / storage / provider raw。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicEvidenceVerifyResult {
    #[serde(rename = "evidenceNo")]
    pub evidence_no: String,
    #[serde(rename = "assetType")]
    pub asset_type: String,
    #[serde(rename = "hashAlgorithm")]
    pub hash_algorithm: String,
    #[serde(rename = "contentHash")]
    pub content_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    #[serde(
        rename = "canonicalizationProfile",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub canonicalization_profile: Option<String>,
    #[serde(rename = "verifiedAt")]
    pub verified_at: String,
    #[serde(rename = "packageId", default, skip_serializing_if = "Option::is_none")]
    pub package_id: Option<i64>,
    #[serde(
        rename = "manifestHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub manifest_hash: Option<String>,
    #[serde(
        rename = "packageHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub package_hash: Option<String>,
    #[serde(rename = "manifestOfflineVerify")]
    pub manifest_offline_verify: bool,
}

// =============================================================================
// Evidence Package
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidencePackage {
    pub id: i64,
    #[serde(rename = "assetId")]
    pub asset_id: i64,
    #[serde(
        rename = "timestampTokenId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub timestamp_token_id: Option<i64>,
    #[serde(rename = "chainId")]
    pub chain_id: String,
    #[serde(rename = "packageVersion")]
    pub package_version: String,
    #[serde(rename = "hashAlgorithm")]
    pub hash_algorithm: String,
    #[serde(rename = "manifestHash")]
    pub manifest_hash: String,
    #[serde(rename = "packageHash")]
    pub package_hash: String,
    pub status: String,
}

// =============================================================================
// List / Page (compliance gateway S1 — gap-register U-1)
// =============================================================================

/// 证据资产分页【列表项】视图。对应后端 G1 `EvidenceAssetPageItem`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceAssetPageItem {
    pub id: i64,
    #[serde(rename = "evidenceNo")]
    pub evidence_no: String,
    #[serde(
        rename = "publicVerifyCode",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub public_verify_code: Option<String>,
    #[serde(rename = "assetType")]
    pub asset_type: ComplianceAssetType,
    pub name: String,
    #[serde(rename = "mimeType", default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    #[serde(rename = "hashAlgorithm")]
    pub hash_algorithm: ComplianceHashAlgorithm,
    #[serde(rename = "contentHash")]
    pub content_hash: String,
    #[serde(
        rename = "canonicalizationProfile",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub canonicalization_profile: Option<String>,
    #[serde(rename = "digestSource")]
    pub digest_source: ComplianceDigestSource,
    #[serde(rename = "privacyLevel")]
    pub privacy_level: CompliancePrivacyLevel,
    pub status: String,
    /// 创建时间 ISO-8601。
    #[serde(rename = "createTime")]
    pub create_time: String,
}

/// `list_evidence_assets` 请求参数。`create_time_start` / `create_time_end` 为调用方提供的
/// 【原样字符串】（`yyyy-MM-dd HH:mm:ss`），SDK 不做格式校验或时区转换。
#[derive(Debug, Clone, Default)]
pub struct ListEvidenceAssetsRequest {
    pub page: PageRequest,
    /// 资产类型过滤（`AssetTypeEnum.name()`）。
    pub asset_type: Option<String>,
    /// 资产状态过滤。
    pub status: Option<String>,
    /// 创建时间下界，`yyyy-MM-dd HH:mm:ss`。
    pub create_time_start: Option<String>,
    /// 创建时间上界，`yyyy-MM-dd HH:mm:ss`。
    pub create_time_end: Option<String>,
}

/// 证据包分页【列表项】视图。对应后端 G1 `EvidencePackagePageItem`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidencePackagePageItem {
    pub id: i64,
    #[serde(rename = "assetId")]
    pub asset_id: i64,
    #[serde(
        rename = "timestampTokenId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub timestamp_token_id: Option<i64>,
    #[serde(rename = "chainId")]
    pub chain_id: String,
    #[serde(rename = "packageVersion")]
    pub package_version: String,
    #[serde(rename = "hashAlgorithm")]
    pub hash_algorithm: String,
    #[serde(rename = "manifestHash")]
    pub manifest_hash: String,
    #[serde(rename = "packageHash")]
    pub package_hash: String,
    pub status: String,
    /// 创建时间 ISO-8601。
    #[serde(rename = "createTime")]
    pub create_time: String,
}

/// `list_evidence_packages` 请求参数。
#[derive(Debug, Clone, Default)]
pub struct ListEvidencePackagesRequest {
    pub page: PageRequest,
    /// 证据包状态过滤。
    pub status: Option<String>,
    pub create_time_start: Option<String>,
    pub create_time_end: Option<String>,
}
