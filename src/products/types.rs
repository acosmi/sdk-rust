//! 商品中心类型。端口自 `products/types.ts`（商品化总规划 P3）。
//!
//! 公开端点字段白名单严格 —— feature_gate_json / retired_at / sales_channel_json /
//! price_snapshot_policy 不暴露。

use serde::{Deserialize, Serialize};

/// 商品族（与 V47 dist_product_mapping.product_family 对齐）。TS `ProductFamily`（闭 union → enum）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductFamily {
    #[serde(rename = "MODEL_MEMBERSHIP")]
    ModelMembership,
    #[serde(rename = "TOKEN_PACK")]
    TokenPack,
    #[serde(rename = "COMPLIANCE")]
    Compliance,
    #[serde(rename = "LEGAL")]
    Legal,
    #[serde(rename = "DESIGN_AGENT")]
    DesignAgent,
    #[serde(rename = "ENTERPRISE")]
    Enterprise,
}

impl ProductFamily {
    /// wire 字符串值（对齐 TS `ProductFamilyEnum`）。
    pub fn as_str(self) -> &'static str {
        match self {
            ProductFamily::ModelMembership => "MODEL_MEMBERSHIP",
            ProductFamily::TokenPack => "TOKEN_PACK",
            ProductFamily::Compliance => "COMPLIANCE",
            ProductFamily::Legal => "LEGAL",
            ProductFamily::DesignAgent => "DESIGN_AGENT",
            ProductFamily::Enterprise => "ENTERPRISE",
        }
    }
}

/// 受众。TS `Audience`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Audience {
    #[serde(rename = "PERSONAL")]
    Personal,
    #[serde(rename = "ENTERPRISE")]
    Enterprise,
    #[serde(rename = "DEVELOPER")]
    Developer,
}

impl Audience {
    pub fn as_str(self) -> &'static str {
        match self {
            Audience::Personal => "PERSONAL",
            Audience::Enterprise => "ENTERPRISE",
            Audience::Developer => "DEVELOPER",
        }
    }
}

/// 计费模式。TS `BillingMode`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BillingMode {
    #[serde(rename = "ONE_TIME")]
    OneTime,
    #[serde(rename = "SUBSCRIPTION")]
    Subscription,
    #[serde(rename = "METERED")]
    Metered,
    #[serde(rename = "HYBRID")]
    Hybrid,
}

impl BillingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            BillingMode::OneTime => "ONE_TIME",
            BillingMode::Subscription => "SUBSCRIPTION",
            BillingMode::Metered => "METERED",
            BillingMode::Hybrid => "HYBRID",
        }
    }
}

/// 地区范围。TS `RegionScope`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegionScope {
    #[serde(rename = "CN")]
    Cn,
    #[serde(rename = "OS")]
    Os,
    #[serde(rename = "GLOBAL")]
    Global,
}

impl RegionScope {
    pub fn as_str(self) -> &'static str {
        match self {
            RegionScope::Cn => "CN",
            RegionScope::Os => "OS",
            RegionScope::Global => "GLOBAL",
        }
    }
}

/// 公开商品响应（字段白名单严格）。TS `Product`。
///
/// 后端 toPublicResponse 仅输出以下字段；不会出现 featureGateJson / retiredAt / salesChannelJson /
/// priceSnapshotPolicy。`display_metadata_json` 是后端原文字符串（caller 自 JSON.parse，缺省可能 null）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    pub id: i64,
    /// 公开 slug = biz_product_id（V1 建表 UNIQUE）。
    #[serde(rename = "publicSlug")]
    pub public_slug: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "productFamily")]
    pub product_family: Option<ProductFamily>,
    pub audience: Option<Audience>,
    #[serde(rename = "billingMode")]
    pub billing_mode: Option<BillingMode>,
    #[serde(rename = "regionScope")]
    pub region_scope: Option<RegionScope>,
    /// 基准价（整数分，§3）。
    #[serde(rename = "basePriceFen")]
    pub base_price_fen: Option<i64>,
    #[serde(rename = "tokenQuota")]
    pub token_quota: Option<i64>,
    /// 后端原文 JSON 字符串（含 title/subtitle/badge/highlights/icon 等），由 caller 自解析。
    #[serde(rename = "displayMetadataJson")]
    pub display_metadata_json: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_closed_enums_and_nullable_fields() {
        let p: Product = serde_json::from_str(
            r#"{"id":1,"publicSlug":"pro","displayName":"Pro","productFamily":"MODEL_MEMBERSHIP",
                "audience":"PERSONAL","billingMode":"SUBSCRIPTION","regionScope":"CN",
                "basePriceFen":39900,"tokenQuota":300000000,"displayMetadataJson":null}"#,
        )
        .unwrap();
        assert_eq!(p.product_family, Some(ProductFamily::ModelMembership));
        assert_eq!(p.audience, Some(Audience::Personal));
        assert_eq!(p.billing_mode, Some(BillingMode::Subscription));
        assert_eq!(p.region_scope, Some(RegionScope::Cn));
        // basePriceFen = i64 整数分。
        assert_eq!(p.base_price_fen, Some(39900_i64));
        assert!(p.display_metadata_json.is_none());
        // 枚举 wire 值对齐后端。
        assert_eq!(ProductFamily::TokenPack.as_str(), "TOKEN_PACK");
    }

    #[test]
    fn product_null_enum_fields_ok() {
        // productFamily 等可 null（TS `ProductFamily | null`）。
        let p: Product = serde_json::from_str(
            r#"{"id":2,"publicSlug":"x","displayName":"X","productFamily":null,
                "audience":null,"billingMode":null,"regionScope":null,
                "basePriceFen":null,"tokenQuota":null,"displayMetadataJson":null}"#,
        )
        .unwrap();
        assert!(p.product_family.is_none());
        assert!(p.base_price_fen.is_none());
    }
}
