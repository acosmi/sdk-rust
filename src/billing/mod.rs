//! 计费域：entitlements / metering / wallet / 商城（token packages）。
//!
//! 对齐 `billing/index.ts`。类型在 [`types`]；业务方法经 declaration-merging 模式
//! 分散在 [`entitlements`] / [`packages`] / [`wallet`] 的 `impl Client` 块（无 side-effect import）。

pub mod entitlements;
pub mod packages;
pub mod types;
pub mod wallet;

pub use types::{
    BalanceDetail, BalanceDetailEntitlement, BuyResponse, ConsumeRecord, ConsumeRecordPage,
    EntitlementBalance, EntitlementItem, ModelBucket, ModelByQuotaResponse, ModelCoefficient,
    Order, OrderListItem, OrderStatus, PayPayload, PaymentMethod, TokenPackage, Transaction,
    WalletStats,
};

use crate::core::client::Client;
use crate::core::http::DEFAULT_JSON_TIMEOUT_MS;
use crate::shared::{ApiResponse, Error, Result};
use serde::de::DeserializeOwned;
use tokio_util::sync::CancellationToken;

impl Client {
    /// 计费域内部：GET `{api}{path}` → `ApiResponse<T>` 解包 → `data`。
    /// 对应 TS `doJSON<APIResponse<T>>('GET',...)` 后 `.data`。
    pub(crate) async fn billing_get<T: DeserializeOwned>(
        &self,
        path: &str,
        signal: Option<CancellationToken>,
    ) -> Result<T> {
        let (env, _) = self
            .do_json_full::<ApiResponse<T>>(reqwest::Method::GET, path, None, signal)
            .await?;
        unwrap_billing(path, env)
    }

    /// 计费域内部：POST `{api}{path}`（无 body）→ `ApiResponse<T>` 解包 → `data`。
    pub(crate) async fn billing_post<T: DeserializeOwned>(
        &self,
        path: &str,
        signal: Option<CancellationToken>,
    ) -> Result<T> {
        self.billing_post_body(path, None, signal).await
    }

    /// 计费域内部：POST `{api}{path}`（可选 JSON body）→ `ApiResponse<T>` 解包 → `data`。
    pub(crate) async fn billing_post_body<T: DeserializeOwned>(
        &self,
        path: &str,
        body: Option<&str>,
        signal: Option<CancellationToken>,
    ) -> Result<T> {
        // 计费域默认 30s JSON 超时（与 do_json_full 同口径）；POST 默认不重试（计费安全）。
        let (bytes, _) = self
            .do_json_full_raw(
                reqwest::Method::POST,
                path,
                body,
                signal,
                DEFAULT_JSON_TIMEOUT_MS,
            )
            .await?;
        if bytes.is_empty() {
            return Err(Error::other(format!("{path}: empty response body")));
        }
        let env: ApiResponse<T> = serde_json::from_slice(&bytes)
            .map_err(|e| Error::other(format!("{path}: decode: {e}")))?;
        unwrap_billing(path, Some(env))
    }
}

/// `ApiResponse<T>` → `data`（`code != 0` 抛 BusinessError；空体抛强 Err）。
fn unwrap_billing<T>(path: &str, env: Option<ApiResponse<T>>) -> Result<T> {
    let env = env.ok_or_else(|| Error::other(format!("{path}: empty response body")))?;
    if let Some(err) = env.business_error() {
        return Err(err);
    }
    Ok(env.data)
}
