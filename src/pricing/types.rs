//! 定价域类型。端口自 `pricing/types.ts`（商品化总规划 P1 公开业务参数 + P4 csign SKU）。

use crate::macros::open_string_union;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 公开业务参数（key → 字符串原值，caller 自解析数值/布尔）。TS `PricingConfig`。
pub type PricingConfig = HashMap<String, String>;

/// 公开模型摘要（P3 商品中心会扩展）。TS `PublicModelSummary`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicModelSummary {
    pub id: String,
    pub name: String,
    #[serde(rename = "modelId")]
    pub model_id: String,
    pub provider: String,
    /// 商品化档位门控：0=FREE 1=BASIC 2=PRO 3=PRO_MAX 4=ULTRA。
    #[serde(
        rename = "minPlanTier",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub min_plan_tier: Option<i64>,
    #[serde(rename = "isEnabled")]
    pub is_enabled: bool,
}

open_string_union! {
    /// csign SKU benefit_type。TS `ComplianceBenefitType`。
    ComplianceBenefitType {
        CONTRACT => "CONTRACT",
        IDENTITY => "IDENTITY",
        EVIDENCE => "EVIDENCE",
        SEAL => "SEAL",
    }
}

/// csign SKU 公开视图（不含 upstreamCostFen / status 等内部字段）。TS `ComplianceSku`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceSku {
    #[serde(rename = "skuCode")]
    pub sku_code: String,
    /// 开放 union（已知 benefit_type + 任意 string 兜底）。
    #[serde(rename = "benefitType")]
    pub benefit_type: ComplianceBenefitType,
    #[serde(
        rename = "providerProduct",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub provider_product: Option<String>,
    /// 单价（整数分，§3）。
    #[serde(
        rename = "unitPriceFen",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub unit_price_fen: Option<i64>,
    /// 超量价（整数分，§3）。
    #[serde(
        rename = "overagePriceFen",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub overage_price_fen: Option<i64>,
    #[serde(
        rename = "regionScope",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub region_scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// 套餐覆盖次数：`{"PRO":10,"PRO_MAX":50,...}`。
    #[serde(
        rename = "includedInPlans",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub included_in_plans: Option<HashMap<String, i64>>,
}

/// 匿名估价响应（无用户态，不查覆盖余额）。TS `ComplianceQuoteResponse`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceQuoteResponse {
    #[serde(rename = "skuCode")]
    pub sku_code: String,
    #[serde(
        rename = "regionScope",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub region_scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantity: Option<i64>,
    #[serde(
        rename = "unitPriceFen",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub unit_price_fen: Option<i64>,
    #[serde(
        rename = "overagePriceFen",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub overage_price_fen: Option<i64>,
    #[serde(
        rename = "subtotalFen",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub subtotal_fen: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<bool>,
    #[serde(
        rename = "benefitType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub benefit_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
