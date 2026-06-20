//! Entitlements + V29 Per-Model Bucket 业务方法。端口自 `billing/entitlements.ts`
//! （declaration-merging → 此处 `impl Client` 块）。

use super::types::{
    BalanceDetail, ConsumeRecordPage, EntitlementBalance, EntitlementItem, ModelBucket,
    ModelByQuotaResponse, ModelCoefficient,
};
use crate::core::client::Client;
use crate::core::http::COEF_CACHE_TTL_MS;
use crate::shared::{Error, Result};
use tokio_util::sync::CancellationToken;

impl Client {
    /// 查询当前用户的权益余额（聚合）。对应 TS `getBalance`。
    pub async fn get_balance(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<EntitlementBalance> {
        self.billing_get("/entitlements/balance", signal).await
    }

    /// 查询详细余额（含每条权益明细）。对应 TS `getBalanceDetail`。
    pub async fn get_balance_detail(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<BalanceDetail> {
        self.billing_get("/entitlements/balance-detail", signal)
            .await
    }

    /// 查询当前用户权益列表；status："ACTIVE" / "EXPIRED" / ""（全部）。对应 TS `listEntitlements`。
    pub async fn list_entitlements(
        &self,
        status: &str,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<EntitlementItem>> {
        let path = if status.is_empty() {
            "/entitlements".to_string()
        } else {
            format!("/entitlements?status={}", urlencoding(status))
        };
        self.billing_get(&path, signal).await
    }

    /// 查询核销记录（分页）。对应 TS `listConsumeRecords`。
    pub async fn list_consume_records(
        &self,
        page: i64,
        page_size: i64,
        signal: Option<CancellationToken>,
    ) -> Result<ConsumeRecordPage> {
        let path = format!("/entitlements/consume-records?page={page}&pageSize={page_size}");
        self.billing_get(&path, signal).await
    }

    /// 领取当月免费额度 —— 幂等：已领取时返回已有权益。对应 TS `claimMonthlyFree`。
    pub async fn claim_monthly_free(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<EntitlementItem> {
        self.billing_post("/entitlements/claim-monthly", signal)
            .await
    }

    /// 查询当前用户在指定模型下的剩余 token（raw + ETU）。对应 TS `getByModel`。
    pub async fn get_by_model(
        &self,
        model_id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<ModelByQuotaResponse> {
        if model_id.is_empty() {
            return Err(Error::other("modelID required"));
        }
        let path = format!("/entitlements/by-model?modelId={}", urlencoding(model_id));
        self.billing_get(&path, signal).await
    }

    /// 列出当前用户的全部桶。对应 TS `listBuckets`。
    pub async fn list_buckets(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<ModelBucket>> {
        self.billing_get("/entitlements/buckets", signal).await
    }

    /// 拉取模型系数表；SDK 自带 8s TTL 内存缓存以减小调用风暴。对应 TS `listCoefficients`。
    ///
    /// **@deprecated** 系数已退役，网关恒返回 `[]`。
    pub async fn list_coefficients(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<ModelCoefficient>> {
        // 简单内存缓存 + TTL（8s）。
        {
            let cache = self.coef_cache().read().unwrap();
            if let Some((data, t)) = cache.as_ref() {
                if (t.elapsed().as_millis() as u64) < COEF_CACHE_TTL_MS {
                    return Ok(data.clone()); // shallow copy 防外部篡改。
                }
            }
        }
        let data: Vec<ModelCoefficient> = self
            .billing_get("/entitlements/coefficients", signal)
            .await?;
        *self.coef_cache().write().unwrap() = Some((data.clone(), std::time::Instant::now()));
        Ok(data)
    }

    /// 手动失效系数缓存（admin 调价后建议立即调一次）。对应 TS `invalidateCoefficientCache`。
    ///
    /// **@deprecated** 系数已退役。
    pub fn invalidate_coefficient_cache(&self) {
        *self.coef_cache().write().unwrap() = None;
    }
}

/// `encodeURIComponent` 等价：保留与 JS 一致的未编码集（字母数字 + `-_.!~*'()`）。
pub(crate) fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
