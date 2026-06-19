//! 企业席位（P6a）业务方法。端口自 `enterprise/client.ts`（declaration-merging → `impl Client` 块）。
//!
//! 端点路径形如 `/api/admin/enterprises/...`（后台管理）/ `/me/enterprises/...`（登录态）。

use super::types::{
    AssignSeatRequest, EnterpriseKycMyStatusView, EnterpriseMember, EnterpriseSummary,
    InviteMemberRequest, OrgConsumeReport, OrgSeat, OrgSubscription,
};
use crate::billing::entitlements::urlencoding;
use crate::core::client::Client;
use crate::shared::{Error, Result};
use tokio_util::sync::CancellationToken;

impl Client {
    // =====================================================================
    // 企业组织（登录态：我所在的企业）
    // =====================================================================

    /// 列出我所在的企业。对应 TS `listMyEnterprises`。空 data → []。
    pub async fn list_my_enterprises(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<EnterpriseSummary>> {
        self.commerce_get_list("/me/enterprises", signal).await
    }

    /// 获取企业详情。对应 TS `getEnterprise`。空 data → Err（`enterprise {id} not found`）。
    pub async fn get_enterprise(
        &self,
        id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<EnterpriseSummary> {
        self.commerce_get_opt::<EnterpriseSummary>(&format!("/api/admin/enterprises/{id}"), signal)
            .await?
            .ok_or_else(|| Error::other(format!("enterprise {id} not found")))
    }

    // =====================================================================
    // 成员管理
    // =====================================================================

    /// 邀请成员加入（P6a 简化：直接 add，OWNER/ADMIN 可调用）。对应 TS `inviteMember`。
    ///
    /// 空 data → Err（`invite member failed`）。
    pub async fn invite_member(
        &self,
        req: &InviteMemberRequest,
        signal: Option<CancellationToken>,
    ) -> Result<EnterpriseMember> {
        let body = serde_json::to_string(req)
            .map_err(|e| Error::other(format!("inviteMember: marshal: {e}")))?;
        self.commerce_post_opt::<EnterpriseMember>(
            "/api/admin/enterprise-members",
            Some(&body),
            signal,
        )
        .await?
        .ok_or_else(|| Error::other("invite member failed"))
    }

    /// 列出企业成员。对应 TS `listEnterpriseMembers`。空 data → []。
    pub async fn list_enterprise_members(
        &self,
        enterprise_id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<EnterpriseMember>> {
        self.commerce_get_list(
            &format!("/api/admin/enterprise-members/by-enterprise/{enterprise_id}"),
            signal,
        )
        .await
    }

    // =====================================================================
    // 订阅 + 席位
    // =====================================================================

    /// 列出企业订阅。对应 TS `listOrgSubscriptions`。空 data → []。
    pub async fn list_org_subscriptions(
        &self,
        enterprise_id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<OrgSubscription>> {
        self.commerce_get_list(
            &format!("/api/admin/org-subscriptions/by-enterprise/{enterprise_id}"),
            signal,
        )
        .await
    }

    /// 列出订阅下的席位（1 订阅 N 席，seat_no 1..N）。对应 TS `listSeats`。空 data → []。
    pub async fn list_seats(
        &self,
        org_subscription_id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<OrgSeat>> {
        self.commerce_get_list(
            &format!("/api/admin/org-seats/by-subscription/{org_subscription_id}"),
            signal,
        )
        .await
    }

    /// 分配席位（月度变更次数 +1，超 3 次返 41xxx 业务码）。对应 TS `assignSeat`。
    ///
    /// 空 data → Err（`assign seat failed`）。
    pub async fn assign_seat(
        &self,
        req: &AssignSeatRequest,
        signal: Option<CancellationToken>,
    ) -> Result<OrgSeat> {
        let body = serde_json::to_string(req)
            .map_err(|e| Error::other(format!("assignSeat: marshal: {e}")))?;
        self.commerce_post_opt::<OrgSeat>("/api/admin/org-seats/assign", Some(&body), signal)
            .await?
            .ok_or_else(|| Error::other("assign seat failed"))
    }

    /// 收回席位（席位状态 → AVAILABLE，累计用量保留）。对应 TS `revokeSeat`（返回 void）。
    pub async fn revoke_seat(
        &self,
        seat_id: i64,
        note: Option<&str>,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        let path = match note {
            Some(n) => format!(
                "/api/admin/org-seats/{seat_id}/revoke?note={}",
                urlencoding(n)
            ),
            None => format!("/api/admin/org-seats/{seat_id}/revoke"),
        };
        self.commerce_post_discard(&path, None, signal).await
    }

    // =====================================================================
    // 用量报表
    // =====================================================================

    /// 企业消耗汇总（订阅维度池子合计）。对应 TS `getOrgConsumeReport`。
    ///
    /// 空 data → 回退全零 report（TS 默认对象）。
    pub async fn get_org_consume_report(
        &self,
        enterprise_id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<OrgConsumeReport> {
        match self
            .commerce_get_opt::<OrgConsumeReport>(
                &format!("/api/admin/enterprise-settlements/overview/{enterprise_id}"),
                signal,
            )
            .await?
        {
            Some(r) => Ok(r),
            None => Ok(OrgConsumeReport {
                enterprise_id,
                subscription_count: 0,
                total_pool_tk: 0,
                total_used_tk: 0,
                total_price_fen: 0,
                note: None,
            }),
        }
    }

    // =====================================================================
    // KYC 自查（v2.0.0+）
    // =====================================================================

    /// 企业 OWNER 自查 KYC 状态。对应 TS `getMyEnterpriseKycStatus`。
    ///
    /// 用户不是任何企业 OWNER 时返空 view（`enterpriseId` 缺省）。空 data → 默认（`{}`）。
    pub async fn get_my_enterprise_kyc_status(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<EnterpriseKycMyStatusView> {
        Ok(self
            .commerce_get_opt::<EnterpriseKycMyStatusView>(
                "/api/distribution/enterprise/kyc/my",
                signal,
            )
            .await?
            .unwrap_or_default())
    }
}
