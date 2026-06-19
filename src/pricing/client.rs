//! 定价域业务方法。端口自 `pricing/client.ts`（declaration-merging → `impl Client` 块）。
//!
//! 端点：tk-dist `/api/distribution/public/pricing/{config,models}` + `/public/compliance/{skus,quote}`，
//! 经 Go 反代。

use super::types::{ComplianceQuoteResponse, ComplianceSku, PricingConfig, PublicModelSummary};
use crate::billing::entitlements::urlencoding;
use crate::core::client::Client;
use crate::shared::{Error, Result};
use tokio_util::sync::CancellationToken;

impl Client {
    /// 查询公开业务参数。对应 TS `getPricingConfig`。
    ///
    /// 不传 key → 默认返三件（tk_to_fen_ratio / usd_cny_rate / freezone_reset_timezone）；
    /// 传 key（白名单内）→ 仅返该 key。空 data → 空 map（TS `?? {}`）。
    pub async fn get_pricing_config(
        &self,
        key: Option<&str>,
        signal: Option<CancellationToken>,
    ) -> Result<PricingConfig> {
        let path = match key {
            Some(k) => format!("/distribution/public/pricing/config?key={}", urlencoding(k)),
            None => "/distribution/public/pricing/config".to_string(),
        };
        Ok(self
            .commerce_get_opt::<PricingConfig>(&path, signal)
            .await?
            .unwrap_or_default())
    }

    /// 公开模型列表（P3 商品中心会重做；P1 后端 stub 返空数组）。对应 TS `listPublicModels`。
    pub async fn list_public_models(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<PublicModelSummary>> {
        self.commerce_get_list("/distribution/public/pricing/models", signal)
            .await
    }

    /// 列出 csign 公开 SKU（匿名可调用）。对应 TS `listComplianceSkus`。
    ///
    /// `region`：CN / OS / GLOBAL（空 fallback CN，同时返回 GLOBAL）。
    pub async fn list_compliance_skus(
        &self,
        region: Option<&str>,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<ComplianceSku>> {
        let path = match region {
            Some(r) => format!(
                "/distribution/public/compliance/skus?region={}",
                urlencoding(r)
            ),
            None => "/distribution/public/compliance/skus".to_string(),
        };
        self.commerce_get_list(&path, signal).await
    }

    /// 匿名估价（无用户态，不查覆盖余额，仅返回 overage 价 × 数量）。对应 TS `quoteCompliance`。
    ///
    /// `sku_code` 必填（空 → Err）。空 data → 回退 `{sku_code, available:false}`（TS `?? {...}`）。
    pub async fn quote_compliance(
        &self,
        sku_code: &str,
        quantity: Option<i64>,
        region: Option<&str>,
        signal: Option<CancellationToken>,
    ) -> Result<ComplianceQuoteResponse> {
        if sku_code.is_empty() {
            return Err(Error::other("quoteCompliance: skuCode is required"));
        }
        let mut params: Vec<String> = vec![format!("skuCode={}", urlencoding(sku_code))];
        if let Some(q) = quantity {
            params.push(format!("quantity={q}"));
        }
        if let Some(r) = region {
            params.push(format!("region={}", urlencoding(r)));
        }
        let path = format!("/distribution/public/compliance/quote?{}", params.join("&"));
        match self
            .commerce_get_opt::<ComplianceQuoteResponse>(&path, signal)
            .await?
        {
            Some(r) => Ok(r),
            None => Ok(ComplianceQuoteResponse {
                sku_code: sku_code.to_string(),
                region_scope: None,
                quantity: None,
                unit_price_fen: None,
                overage_price_fen: None,
                subtotal_fen: None,
                available: Some(false),
                benefit_type: None,
                description: None,
            }),
        }
    }
}
