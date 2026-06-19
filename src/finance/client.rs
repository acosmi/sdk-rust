//! 财务（P7）业务方法。端口自 `finance/client.ts`（declaration-merging → `impl Client` 块）。
//!
//! 决策 14 对公转账：弹窗 + 企微对接，零银行 API。决策 15 退款规则按 Policy 表配置。
//! 端点路径形如 `/api/distribution/finance/*`。admin 板块由 admin UI 直连，不在 SDK 边界。

use super::types::{
    CorporateTransfer, InitiateCorporateTransferInput, InitiateCorporateTransferResult, Invoice,
    RefundRecord, RequestInvoiceInput, RequestRefundInput,
};
use crate::billing::entitlements::urlencoding;
use crate::core::client::Client;
use crate::shared::{Error, Result};
use tokio_util::sync::CancellationToken;

impl Client {
    // ========================================================================
    // 退款（决策 15）
    // ========================================================================

    /// 申请退款（决策 15：按 policyCode 自动判定；NO_REFUND 即时拒）。对应 TS `requestRefund`。
    ///
    /// 空 data → Err（`refund/request: empty response`）。
    pub async fn request_refund(
        &self,
        req: &RequestRefundInput,
        signal: Option<CancellationToken>,
    ) -> Result<RefundRecord> {
        let body = serde_json::to_string(req)
            .map_err(|e| Error::other(format!("requestRefund: marshal: {e}")))?;
        self.commerce_post_opt::<RefundRecord>(
            "/api/distribution/finance/refund/request",
            Some(&body),
            signal,
        )
        .await?
        .ok_or_else(|| Error::other("refund/request: empty response"))
    }

    /// 我的退款记录。对应 TS `listMyRefunds`。空 data → []。
    pub async fn list_my_refunds(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<RefundRecord>> {
        self.commerce_get_list("/api/distribution/finance/refund/my", signal)
            .await
    }

    // ========================================================================
    // 发票
    // ========================================================================

    /// 申请开票。对应 TS `requestInvoice`。空 data → Err（`invoice/request: empty response`）。
    pub async fn request_invoice(
        &self,
        req: &RequestInvoiceInput,
        signal: Option<CancellationToken>,
    ) -> Result<Invoice> {
        let body = serde_json::to_string(req)
            .map_err(|e| Error::other(format!("requestInvoice: marshal: {e}")))?;
        self.commerce_post_opt::<Invoice>(
            "/api/distribution/finance/invoice/request",
            Some(&body),
            signal,
        )
        .await?
        .ok_or_else(|| Error::other("invoice/request: empty response"))
    }

    /// 我的发票列表。对应 TS `listMyInvoices`。空 data → []。
    pub async fn list_my_invoices(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<Invoice>> {
        self.commerce_get_list("/api/distribution/finance/invoice/my", signal)
            .await
    }

    // ========================================================================
    // 对公转账（决策 14：弹窗 + 企微对接）
    // ========================================================================

    /// 发起对公转账（决策 14）。对应 TS `initiateCorporateTransfer`。
    ///
    /// 服务端创建 INITIATED 记录，返回销售名片二维码 + 销售企微 ID + 财务邮箱。
    /// 空 data → Err（`corporate-transfer/initiate: empty response`）。
    pub async fn initiate_corporate_transfer(
        &self,
        req: &InitiateCorporateTransferInput,
        signal: Option<CancellationToken>,
    ) -> Result<InitiateCorporateTransferResult> {
        let body = serde_json::to_string(req)
            .map_err(|e| Error::other(format!("initiateCorporateTransfer: marshal: {e}")))?;
        self.commerce_post_opt::<InitiateCorporateTransferResult>(
            "/api/distribution/finance/corporate-transfer/initiate",
            Some(&body),
            signal,
        )
        .await?
        .ok_or_else(|| Error::other("corporate-transfer/initiate: empty response"))
    }

    /// 上传对公转账凭证 URL。对应 TS `uploadCorporateTransferProof`。
    ///
    /// **bug-for-bug**：后端契约是 `@RequestParam("proofUrl")`，走 **query 参数**（非 body）；
    /// `encodeURIComponent` 直拼 path。空 data → false（TS `?? false`）。
    pub async fn upload_corporate_transfer_proof(
        &self,
        id: i64,
        proof_url: &str,
        signal: Option<CancellationToken>,
    ) -> Result<bool> {
        let path = format!(
            "/api/distribution/finance/corporate-transfer/{}/upload-proof?proofUrl={}",
            urlencoding(&id.to_string()),
            urlencoding(proof_url)
        );
        self.commerce_post_bool(&path, None, signal).await
    }

    /// 我的对公转账记录。对应 TS `listMyCorporateTransfers`。空 data → []。
    pub async fn list_my_corporate_transfers(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<CorporateTransfer>> {
        self.commerce_get_list("/api/distribution/finance/corporate-transfer/my", signal)
            .await
    }
}
