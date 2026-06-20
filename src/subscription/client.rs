//! 订阅域业务方法。端口自 `subscription/client.ts`（declaration-merging → `impl Client` 块）。
//!
//! 端点对应 tk-dist `/api/distribution/public/pricing/plans` 等，经 Go 网关 `/api/v4/...` 反代。
//! 当前订阅规范入口是网关 GET /entitlements/membership（get_membership）。

use super::types::{
    Membership, SubscriptionAudience, SubscriptionPlan, SubscriptionPrecheckResult,
    SubscriptionTier,
};
use crate::billing::entitlements::urlencoding;
use crate::core::client::Client;
use crate::shared::Result;
use tokio_util::sync::CancellationToken;

impl Client {
    /// 列出当前可售订阅计划；`audience` 可选过滤。对应 TS `listPlans`。
    ///
    /// 空 data → 空数组（对应 TS `Array.isArray(resp.data) ? resp.data : []`）。
    pub async fn list_plans(
        &self,
        audience: Option<SubscriptionAudience>,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<SubscriptionPlan>> {
        let path = match audience {
            Some(a) => format!(
                "/distribution/public/pricing/plans?audience={}",
                urlencoding(a.as_str())
            ),
            None => "/distribution/public/pricing/plans".to_string(),
        };
        self.commerce_get_list(&path, signal).await
    }

    /// 查询当前用户会员/订阅概览（C 端会员中心）。对应 TS `getMembership`。
    ///
    /// 无活跃订阅时 has_active=false / is_free=true。
    pub async fn get_membership(&self, signal: Option<CancellationToken>) -> Result<Membership> {
        self.commerce_get("/entitlements/membership", signal).await
    }

    /// 由活跃权益推导订阅层级（free/pro）。对应 TS `getSubscriptionTier`。
    pub async fn get_subscription_tier(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<SubscriptionTier> {
        self.commerce_get("/entitlements/subscription", signal)
            .await
    }

    /// 订阅支付前绑定硬闸。对应 TS `subscriptionPrecheck`。
    ///
    /// 返回 `{ok:true}` 表示已绑定手机/邮箱可放行支付；未绑定时网关返回 HTTP 403 + 业务码
    /// 41001（[`crate::Error::Http`] / [`crate::Error::Business`]），调用方据此引导用户先绑定联系方式。
    pub async fn subscription_precheck(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<SubscriptionPrecheckResult> {
        self.commerce_get("/consumer/subscriptions/precheck", signal)
            .await
    }

    /// **@deprecated** 网关无订阅列表端点；现委托 `get_membership()`：有活跃订阅返回单元素数组，
    /// 否则空数组。对应 TS `listUserSubscriptions`。
    pub async fn list_user_subscriptions(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<Membership>> {
        let m = self.get_membership(signal).await?;
        if m.has_active {
            Ok(vec![m])
        } else {
            Ok(Vec::new())
        }
    }

    /// 按 planCode 精确取单个可售订阅计划（复用 list_plans 客户端过滤）。对应 TS `getPlanByCode`。
    ///
    /// 未命中返回 `None`。空 plan_code 直接返回 `None`。
    pub async fn get_plan_by_code(
        &self,
        plan_code: &str,
        signal: Option<CancellationToken>,
    ) -> Result<Option<SubscriptionPlan>> {
        if plan_code.is_empty() {
            return Ok(None);
        }
        let plans = self.list_plans(None, signal).await?;
        Ok(plans.into_iter().find(|p| p.plan_code == plan_code))
    }
}
