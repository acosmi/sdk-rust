//! 企业席位（P6a）类型。端口自 `enterprise/types.ts`（商品化总规划 2026-05-25）。
//!
//! 与 tk-dist `dist_enterprise_*` / `dist_org_subscription` / `dist_org_seat` 五表族对齐。

use crate::macros::open_string_union;
use serde::{Deserialize, Serialize};

/// 企业组织公开视图（PII L3 字段 contactPhone/Email 仅 OWNER/ADMIN 可见）。TS `EnterpriseSummary`。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnterpriseSummary {
    pub id: i64,
    #[serde(rename = "orgName")]
    pub org_name: String,
    /// 工商注册号。
    #[serde(rename = "orgCode", default, skip_serializing_if = "Option::is_none")]
    pub org_code: Option<String>,
    #[serde(
        rename = "legalRepresentative",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub legal_representative: Option<String>,
    /// 主联系人 user_id（UUID，仅 OWNER 可见）。
    #[serde(
        rename = "contactUserId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contact_user_id: Option<String>,
    #[serde(
        rename = "contactPhone",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contact_phone: Option<String>,
    #[serde(
        rename = "contactEmail",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contact_email: Option<String>,
    /// 销售对接人 user_id（UUID）。
    #[serde(
        rename = "salesRepUserId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sales_rep_user_id: Option<String>,
    /// CN / OVERSEAS。
    #[serde(
        rename = "regionScope",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub region_scope: Option<String>,
    /// ACTIVE / SUSPENDED / TERMINATED。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// 企业成员视图。TS `EnterpriseMember`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnterpriseMember {
    pub id: i64,
    #[serde(rename = "enterpriseId")]
    pub enterprise_id: i64,
    /// UUID。
    #[serde(rename = "userId")]
    pub user_id: String,
    /// OWNER / ADMIN / MEMBER。
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub department: Option<String>,
    #[serde(rename = "joinedAt", default, skip_serializing_if = "Option::is_none")]
    pub joined_at: Option<String>,
    #[serde(rename = "leftAt", default, skip_serializing_if = "Option::is_none")]
    pub left_at: Option<String>,
    /// ACTIVE / INACTIVE。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// 企业订阅视图。TS `OrgSubscription`。
///
/// **注意 *Tk 字段语义**：`per_seat_monthly_cap_tk` / `pool_total_tk` / `pool_used_tk` 是
/// **token 用量**（i64，非金额分），切勿与 `total_price_fen`（整数分金额）混用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgSubscription {
    pub id: i64,
    #[serde(rename = "enterpriseId")]
    pub enterprise_id: i64,
    #[serde(rename = "planId")]
    pub plan_id: i64,
    /// ENT_PRO / ENT_ULTRA。
    #[serde(rename = "planCode")]
    pub plan_code: String,
    #[serde(rename = "seatCountPurchased")]
    pub seat_count_purchased: i64,
    #[serde(rename = "seatCountAssigned")]
    pub seat_count_assigned: i64,
    /// 总价（整数分，§3）。
    #[serde(rename = "totalPriceFen")]
    pub total_price_fen: i64,
    /// 阶梯折扣命中值（0.85 = 15% off；f64，比率非金额）。
    #[serde(
        rename = "discountRate",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub discount_rate: Option<f64>,
    #[serde(
        rename = "billingCycle",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub billing_cycle: Option<String>,
    #[serde(
        rename = "nextDeductDate",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub next_deduct_date: Option<String>,
    #[serde(rename = "seatChangeMaxPerMonth")]
    pub seat_change_max_per_month: i64,
    #[serde(rename = "seatChangeUsedThisMonth")]
    pub seat_change_used_this_month: i64,
    #[serde(rename = "salesTakeoverThreshold")]
    pub sales_takeover_threshold: i64,
    /// 派生：pool/seats × 1.5。**token 用量**（i64，非金额）。
    #[serde(
        rename = "perSeatMonthlyCapTk",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub per_seat_monthly_cap_tk: Option<i64>,
    /// 派生：seats × Pro Max × 0.8。**token 用量**（i64，非金额）。
    #[serde(
        rename = "poolTotalTk",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pool_total_tk: Option<i64>,
    /// **token 用量**（i64，非金额）。
    #[serde(
        rename = "poolUsedTk",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pool_used_tk: Option<i64>,
    /// SHARED / PER_SEAT / HYBRID（P6a 阶段仅 SHARED）。
    #[serde(rename = "poolMode", default, skip_serializing_if = "Option::is_none")]
    pub pool_mode: Option<String>,
    /// ACTIVE / PAUSED / EXPIRED / CANCELED。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(rename = "startedAt", default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(rename = "expiresAt", default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// 企业席位视图。TS `OrgSeat`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgSeat {
    pub id: i64,
    #[serde(rename = "orgSubscriptionId")]
    pub org_subscription_id: i64,
    /// 1..N。
    #[serde(rename = "seatNo")]
    pub seat_no: i64,
    /// AVAILABLE / ASSIGNED / REVOKED。
    pub status: String,
    #[serde(
        rename = "assignedMemberId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub assigned_member_id: Option<i64>,
    /// UUID。
    #[serde(
        rename = "assignedUserId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub assigned_user_id: Option<String>,
    /// 单席位月度配额 override（NULL = 走订阅默认）。**token 用量**（i64，非金额）。
    #[serde(
        rename = "perSeatMonthlyCapTk",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub per_seat_monthly_cap_tk: Option<i64>,
    /// **token 用量**（i64，非金额）。
    #[serde(
        rename = "usedTkThisMonth",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub used_tk_this_month: Option<i64>,
}

open_string_union! {
    /// 成员角色（邀请请求用）。TS `'OWNER'|'ADMIN'|'MEMBER'`。
    MemberRole {
        OWNER => "OWNER",
        ADMIN => "ADMIN",
        MEMBER => "MEMBER",
    }
}

/// 邀请成员请求（P6a 简化：直接 add，不走邀请确认流程）。TS `InviteMemberRequest`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteMemberRequest {
    #[serde(rename = "enterpriseId")]
    pub enterprise_id: i64,
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<MemberRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub department: Option<String>,
}

/// 分配席位请求。TS `AssignSeatRequest`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignSeatRequest {
    #[serde(rename = "subscriptionId")]
    pub subscription_id: i64,
    #[serde(rename = "memberId")]
    pub member_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// 企业消耗汇总视图（P6a 简化：订阅维度池子汇总）。TS `OrgConsumeReport`。
///
/// **注意 *Tk 字段语义**：`total_pool_tk` / `total_used_tk` 是 **token 用量**（i64，非金额分）；
/// `total_price_fen` 是整数分金额。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgConsumeReport {
    #[serde(rename = "enterpriseId")]
    pub enterprise_id: i64,
    #[serde(rename = "subscriptionCount")]
    pub subscription_count: i64,
    /// **token 用量**（i64，非金额）。
    #[serde(rename = "totalPoolTk")]
    pub total_pool_tk: i64,
    /// **token 用量**（i64，非金额）。
    #[serde(rename = "totalUsedTk")]
    pub total_used_tk: i64,
    /// 金额（整数分，§3）。
    #[serde(rename = "totalPriceFen")]
    pub total_price_fen: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// 企业 OWNER 自查 KYC 状态视图（v2.0.0+）。TS `EnterpriseKycMyStatusView`。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnterpriseKycMyStatusView {
    /// null 表示用户不是任何企业 OWNER。
    #[serde(
        rename = "enterpriseId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub enterprise_id: Option<i64>,
    /// PENDING / COMPLETED / FAILED。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// LOW / MEDIUM / HIGH / UNKNOWN。
    #[serde(rename = "riskLevel", default, skip_serializing_if = "Option::is_none")]
    pub risk_level: Option<String>,
    /// 反洗钱命中标志，default false。
    #[serde(
        rename = "sanctionsHit",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sanctions_hit: Option<bool>,
    /// admin override 后填充：APPROVED / REJECTED。
    #[serde(
        rename = "overrideDecision",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub override_decision: Option<String>,
    /// 仅 admin override 时填（= overrideReason）。
    #[serde(
        rename = "reviewerNotes",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub reviewer_notes: Option<String>,
    /// ISO-8601，admin override 时间。
    #[serde(
        rename = "reviewedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub reviewed_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn org_subscription_tk_is_token_not_money() {
        // *Tk 字段 = token 用量（i64），与 totalPriceFen（整数分金额）语义分离。
        let s: OrgSubscription = serde_json::from_str(
            r#"{"id":1,"enterpriseId":10,"planId":2,"planCode":"ENT_PRO",
                "seatCountPurchased":50,"seatCountAssigned":30,"totalPriceFen":399000,
                "seatChangeMaxPerMonth":3,"seatChangeUsedThisMonth":1,"salesTakeoverThreshold":200,
                "perSeatMonthlyCapTk":900000000,"poolTotalTk":24000000000,"poolUsedTk":1200000000}"#,
        )
        .unwrap();
        // token 用量字段是 i64，可超 2^32（24亿 pool）。
        assert_eq!(s.pool_total_tk, Some(24_000_000_000_i64));
        assert_eq!(s.per_seat_monthly_cap_tk, Some(900_000_000_i64));
        assert_eq!(s.pool_used_tk, Some(1_200_000_000_i64));
        // 金额字段独立（整数分）。
        assert_eq!(s.total_price_fen, 399_000_i64);
        // 比率是 f64（缺省 None）。
        assert!(s.discount_rate.is_none());
    }

    #[test]
    fn org_consume_report_tk_vs_fen_separation() {
        let r: OrgConsumeReport = serde_json::from_str(
            r#"{"enterpriseId":10,"subscriptionCount":2,"totalPoolTk":48000000000,
                "totalUsedTk":36000000000,"totalPriceFen":798000}"#,
        )
        .unwrap();
        // totalPoolTk/totalUsedTk = token 用量（i64）。
        assert_eq!(r.total_pool_tk, 48_000_000_000_i64);
        assert_eq!(r.total_used_tk, 36_000_000_000_i64);
        // totalPriceFen = 金额整数分。
        assert_eq!(r.total_price_fen, 798_000_i64);
    }
}
