//! 法律案件咨询 namespace 业务方法。端口自 `casehall/client.ts`（declaration-merging → `impl Client` 块）。
//!
//! 端点路径形如 `/casehall/app/...`（C 端公开）/ `/casehall/me/...`（登录态）。
//!
//! bug-for-bug：多数方法 `@experimental`（后端 consumer 端点尚未实现，casehall 模块当前仅
//! `getMyLawyerCredentialStatus` 在产；调用将返回 404）。SDK 仍按 TS 现状封装。

use super::types::{
    BookConsultationRequest, BookConsultationResult, CaseLead, CaseLeadIdResult, CaseMatter,
    LawyerCredentialMyView, LawyerSummary, LegalConsultation, LegalServiceOrder, LegalServiceSku,
    ListLawyersParams, SubmitCaseLeadRequest,
};
use crate::billing::entitlements::urlencoding;
use crate::core::client::Client;
use crate::shared::{Error, Result};
use tokio_util::sync::CancellationToken;

impl Client {
    // ===========================================================================
    // 律师库（C 端公开）
    // ===========================================================================

    /// 列出已认证律师（公开端点，仅 VERIFIED + ACTIVE；PII L3 字段已脱敏）。对应 TS `listLawyers`。
    ///
    /// **@experimental** 对应后端 consumer 端点尚未实现；调用将返回 404。空 data → []。
    pub async fn list_lawyers(
        &self,
        params: Option<&ListLawyersParams>,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<LawyerSummary>> {
        let mut qs: Vec<String> = Vec::new();
        if let Some(p) = params {
            if let Some(pa) = &p.practice_area {
                qs.push(format!("practiceArea={}", urlencoding(pa)));
            }
            if let Some(loc) = &p.location {
                qs.push(format!("location={}", urlencoding(loc)));
            }
            if let Some(n) = p.page_no {
                qs.push(format!("pageNo={n}"));
            }
            if let Some(sz) = p.page_size {
                qs.push(format!("pageSize={sz}"));
            }
        }
        let path = if qs.is_empty() {
            "/casehall/app/lawyers".to_string()
        } else {
            format!("/casehall/app/lawyers?{}", qs.join("&"))
        };
        self.commerce_get_list(&path, signal).await
    }

    /// 获取律师公开详情（脱敏）。对应 TS `getLawyer`。
    ///
    /// **@experimental** 404 未实现。空 data → Err（`lawyer {id} not found`）。
    pub async fn get_lawyer(
        &self,
        id: i64,
        signal: Option<CancellationToken>,
    ) -> Result<LawyerSummary> {
        self.commerce_get_opt::<LawyerSummary>(&format!("/casehall/app/lawyers/{id}"), signal)
            .await?
            .ok_or_else(|| Error::other(format!("lawyer {id} not found")))
    }

    // ===========================================================================
    // 案件线索（C 端登录态）
    // ===========================================================================

    /// 提交案件线索（登录态）。对应 TS `submitCaseLead`。
    ///
    /// **@experimental** 404 未实现。空 data → 回退 `{id:0}`（TS `?? {id:0}`）。
    pub async fn submit_case_lead(
        &self,
        req: &SubmitCaseLeadRequest,
        signal: Option<CancellationToken>,
    ) -> Result<CaseLeadIdResult> {
        let body = serde_json::to_string(req)
            .map_err(|e| Error::other(format!("submitCaseLead: marshal: {e}")))?;
        Ok(self
            .commerce_post_opt::<CaseLeadIdResult>("/casehall/me/case-leads", Some(&body), signal)
            .await?
            .unwrap_or(CaseLeadIdResult { id: 0 }))
    }

    /// 我的案件线索列表。对应 TS `listMyCaseLeads`。**@experimental** 404 未实现。空 data → []。
    pub async fn list_my_case_leads(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<CaseLead>> {
        self.commerce_get_list("/casehall/me/case-leads", signal)
            .await
    }

    /// 我的案件列表（委托后）。对应 TS `getMyCases`。**@experimental** 404 未实现。空 data → []。
    pub async fn get_my_cases(&self, signal: Option<CancellationToken>) -> Result<Vec<CaseMatter>> {
        self.commerce_get_list("/casehall/me/cases", signal).await
    }

    // ===========================================================================
    // 法律咨询
    // ===========================================================================

    /// 预约法律咨询（sku_code 必填；lawyer_id 留空走 AI 推荐池）。对应 TS `bookConsultation`。
    ///
    /// **@experimental** 404 未实现。空 data → 回退 `{consultationId:0}`。
    pub async fn book_consultation(
        &self,
        req: &BookConsultationRequest,
        signal: Option<CancellationToken>,
    ) -> Result<BookConsultationResult> {
        let body = serde_json::to_string(req)
            .map_err(|e| Error::other(format!("bookConsultation: marshal: {e}")))?;
        Ok(self
            .commerce_post_opt::<BookConsultationResult>(
                "/casehall/me/consultations",
                Some(&body),
                signal,
            )
            .await?
            .unwrap_or(BookConsultationResult { consultation_id: 0 }))
    }

    /// 我的咨询单列表。对应 TS `listMyConsultations`。**@experimental** 404 未实现。空 data → []。
    pub async fn list_my_consultations(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<LegalConsultation>> {
        self.commerce_get_list("/casehall/me/consultations", signal)
            .await
    }

    // ===========================================================================
    // 法律服务订单 & SKU
    // ===========================================================================

    /// 我的法律服务订单列表。对应 TS `listMyLegalOrders`。**@experimental** 404 未实现。空 data → []。
    pub async fn list_my_legal_orders(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<LegalServiceOrder>> {
        self.commerce_get_list("/casehall/me/orders", signal).await
    }

    /// 列出公开的 LEGAL_SERVICE SKU（匿名可调用，复用 dist_compliance_sku benefit_type='LEGAL_SERVICE'）。
    /// 对应 TS `listLegalSKUs`。
    ///
    /// **@experimental** 404 未实现。**强制拼 `benefitType=LEGAL_SERVICE`**（bug-for-bug）。空 data → []。
    pub async fn list_legal_skus(
        &self,
        region: Option<&str>,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<LegalServiceSku>> {
        let path = match region {
            Some(r) => format!(
                "/casehall/app/legal-skus?region={}&benefitType=LEGAL_SERVICE",
                urlencoding(r)
            ),
            None => "/casehall/app/legal-skus?benefitType=LEGAL_SERVICE".to_string(),
        };
        self.commerce_get_list(&path, signal).await
    }

    // ===========================================================================
    // 律师自查执业证审核状态（v2.0.0+，唯一在产端点）
    // ===========================================================================

    /// 律师自查执业证审核状态 —— 返回登录用户作为律师身份提交的所有 credential 列表。
    /// 普通用户（无 lawyer_profile）调返 [] 不抛。对应 TS `getMyLawyerCredentialStatus`。
    ///
    /// 端点 `GET /api/casehall/lawyer-credentials/my`（注意：TS 原样裸路径，经 doJSON 前缀 base）。
    pub async fn get_my_lawyer_credential_status(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<LawyerCredentialMyView>> {
        // bug-for-bug：TS 此处 path 以 `/api/casehall/...` 开头（doJSON 仍前缀 base/api/v4），保持不动。
        self.commerce_get_list("/api/casehall/lawyer-credentials/my", signal)
            .await
    }
}
