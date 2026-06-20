//! 订阅域类型。端口自 `subscription/types.ts`（商品化总规划 P1 订阅档位）。
//!
//! 与 tk-dist `DistSubscriptionPlanDO` / 网关 `membershipResponse` / `GetSubscription` 对齐。

use crate::macros::open_string_union;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

open_string_union! {
    /// 订阅档位受众。TS `SubscriptionAudience`（闭集，但保留前向兼容 → 开放 union）。
    SubscriptionAudience {
        PERSONAL => "PERSONAL",
        ENTERPRISE => "ENTERPRISE",
    }
}

open_string_union! {
    /// 滚存策略。TS `RolloverPolicy`。
    RolloverPolicy {
        NONE => "NONE",
        PARTIAL => "PARTIAL",
        FULL => "FULL",
    }
}

/// 公开订阅计划（字段白名单，不含 feature_gate / seat_rule_json 等内部字段）。TS `SubscriptionPlan`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionPlan {
    pub id: i64,
    #[serde(rename = "planCode")]
    pub plan_code: String,
    pub audience: SubscriptionAudience,
    #[serde(rename = "tierLevel")]
    pub tier_level: i64,
    #[serde(rename = "planName")]
    pub plan_name: String,
    #[serde(rename = "planDesc")]
    pub plan_desc: String,
    #[serde(rename = "billingCycle")]
    pub billing_cycle: String,
    /// 基准价（整数分，§3 finance/商品化阵营）。
    #[serde(rename = "basePriceFen")]
    pub base_price_fen: i64,
    /// 档位标准额度。付费档单位 = 微 Credits（÷1000 = Credits）；免费档 = 原始 Token。
    #[serde(rename = "tokenQuota")]
    pub token_quota: i64,
    #[serde(rename = "seatMin", default, skip_serializing_if = "Option::is_none")]
    pub seat_min: Option<i64>,
    #[serde(rename = "seatMax", default, skip_serializing_if = "Option::is_none")]
    pub seat_max: Option<i64>,
    #[serde(
        rename = "rolloverPolicy",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rollover_policy: Option<RolloverPolicy>,
    #[serde(
        rename = "basePlanCode",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub base_plan_code: Option<String>,
    /// 与 base_plan_code 一组使用；十进制系数透传字符串（§3 json.Number 阵营）。
    #[serde(
        rename = "tkMultiplier",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub tk_multiplier: Option<String>,
    #[serde(
        rename = "priceMultiplier",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub price_multiplier: Option<String>,
    /// grant_policy 摘要（P3 商品中心补；动态 JSON）。
    #[serde(
        rename = "grantPolicyDigest",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub grant_policy_digest: Option<HashMap<String, serde_json::Value>>,
}

/// C 端会员中心订阅概览。严格对齐网关 GET /entitlements/membership。TS `Membership`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Membership {
    #[serde(rename = "hasActive")]
    pub has_active: bool,
    #[serde(rename = "planCode")]
    pub plan_code: String,
    #[serde(rename = "planName")]
    pub plan_name: String,
    pub tier: String,
    #[serde(rename = "billingCycle")]
    pub billing_cycle: String,
    pub status: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: String,
    /// 价格（整数分，§3）。
    #[serde(rename = "priceFen")]
    pub price_fen: i64,
    /// 周期总额度。有活跃付费订阅时单位 = 微 Credits；免费档 = 原始 Token。
    #[serde(rename = "tokenQuota")]
    pub token_quota: i64,
    /// 当前周期已用（后端 float64）。单位同 token_quota：付费=微 Credits / 免费=Token。
    #[serde(rename = "tokenUsed")]
    pub token_used: f64,
    #[serde(rename = "periodStart")]
    pub period_start: String,
    /// is_free = !has_active（无活跃付费订阅即免费档）。
    #[serde(rename = "isFree")]
    pub is_free: bool,
}

/// 由活跃权益推导的订阅层级。对齐网关 GET /entitlements/subscription。TS `SubscriptionTier`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionTier {
    /// "free" | "pro"（后端按权益类型推导）。
    #[serde(rename = "subscriptionType")]
    pub subscription_type: String,
    #[serde(rename = "activeEntitlementTypes")]
    pub active_entitlement_types: Vec<String>,
}

/// 订阅支付前绑定硬闸返回体（`{ok:bool}`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionPrecheckResult {
    pub ok: bool,
}

/// **@deprecated** 网关未暴露订阅列表端点；该形状不对应任何真实响应。
/// 请改用 [`Membership`] + `get_membership()`。保留仅为向后兼容。TS `UserSubscription`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSubscription {
    pub id: i64,
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "planId")]
    pub plan_id: i64,
    #[serde(rename = "planCode", default, skip_serializing_if = "Option::is_none")]
    pub plan_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience: Option<SubscriptionAudience>,
    /// active / paused / cancelled。
    pub status: String,
    /// ISO date — yyyy-MM-dd 字符串（Java LocalDate.toString）。
    #[serde(
        rename = "nextDeductDate",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub next_deduct_date: Option<String>,
    #[serde(
        rename = "agreementNo",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub agreement_no: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_plan_base_price_fen_i64_token_used_f64() {
        let p: SubscriptionPlan = serde_json::from_str(
            r#"{"id":1,"planCode":"PRO","audience":"PERSONAL","tierLevel":2,"planName":"Pro",
                "planDesc":"d","billingCycle":"MONTHLY","basePriceFen":39900,"tokenQuota":300000000,
                "tkMultiplier":"5","priceMultiplier":"3.3"}"#,
        )
        .unwrap();
        // basePriceFen = i64 整数分（§3）。
        assert_eq!(p.base_price_fen, 39900_i64);
        // tokenQuota = i64（额度，>2^28）。
        assert_eq!(p.token_quota, 300_000_000_i64);
        // 十进制系数透传字符串（§3 json.Number 阵营）。
        assert_eq!(p.tk_multiplier.as_deref(), Some("5"));
        assert_eq!(p.price_multiplier.as_deref(), Some("3.3"));
        assert_eq!(p.audience.as_str(), "PERSONAL");
    }

    #[test]
    fn membership_token_used_is_f64() {
        let m: Membership = serde_json::from_str(
            r#"{"hasActive":true,"planCode":"PRO","planName":"Pro","tier":"pro",
                "billingCycle":"MONTHLY","status":"ACTIVE","expiresAt":"2026-12-31",
                "priceFen":39900,"tokenQuota":300000000,"tokenUsed":12345.5,
                "periodStart":"2026-06-01","isFree":false}"#,
        )
        .unwrap();
        // priceFen = i64；tokenUsed = f64（后端 float64）。
        assert_eq!(m.price_fen, 39900_i64);
        assert_eq!(m.token_used, 12345.5_f64);
        assert!(m.has_active);
    }
}
