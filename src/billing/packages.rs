//! Token Packages / 商城业务方法。端口自 `billing/packages.ts`
//! （declaration-merging → 此处 `impl Client` 块）。

use super::entitlements::urlencoding;
use super::types::{BuyResponse, OrderListItem, PayPayload, TokenPackage};
use crate::core::client::Client;
use crate::core::http::{is_order_success, is_order_terminal};
use crate::shared::{Error, Result, YudaoPageResult};
use tokio_util::sync::CancellationToken;

impl Client {
    /// 获取商城流量包列表（兼容 yudao 分页和直接数组两种格式）。对应 TS `listTokenPackages`。
    pub async fn list_token_packages(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<TokenPackage>> {
        let raw: serde_json::Value = self.billing_get("/token-packages", signal).await?;
        decode_list_or_page("token packages", raw)
    }

    /// 获取流量包详情。对应 TS `getTokenPackageDetail`。
    pub async fn get_token_package_detail(
        &self,
        package_id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<TokenPackage> {
        let path = format!("/token-packages/{}", urlencoding(package_id));
        self.billing_get(&path, signal).await
    }

    /// 购买流量包（创建订单）。对应 TS `buyTokenPackage`。
    pub async fn buy_token_package(
        &self,
        package_id: &str,
        payload: Option<&PayPayload>,
        signal: Option<CancellationToken>,
    ) -> Result<BuyResponse> {
        let path = format!("/token-packages/{}/buy", urlencoding(package_id));
        let body = match payload {
            Some(p) => Some(
                serde_json::to_string(p)
                    .map_err(|e| Error::other(format!("serialize pay payload: {e}")))?,
            ),
            None => None,
        };
        self.billing_post_body(&path, body.as_deref(), signal).await
    }

    /// 查询订单支付状态。对应 TS `getOrderStatus`。
    pub async fn get_order_status(
        &self,
        order_id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<BuyResponse> {
        let path = format!("/token-packages/orders/{}/status", urlencoding(order_id));
        self.billing_get(&path, signal).await
    }

    /// 查询我的订单列表。对应 TS `listMyOrders`。
    pub async fn list_my_orders(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<OrderListItem>> {
        let raw: serde_json::Value = self.billing_get("/token-packages/my", signal).await?;
        decode_list_or_page("orders", raw)
    }

    /// 轮询订单支付状态直到终态。对应 TS `waitForPayment`。
    ///
    /// 成功支付返回 [`BuyResponse`]；终态失败抛 [`Error::OrderTerminal`]。
    /// 终态判定基于 `paymentStatus`（回退 `orderStatus`）。`poll_interval_ms <= 0` 时默认 2 秒。
    pub async fn wait_for_payment(
        &self,
        order_id: &str,
        mut poll_interval_ms: i64,
        signal: Option<CancellationToken>,
    ) -> Result<BuyResponse> {
        if poll_interval_ms <= 0 {
            poll_interval_ms = 2000;
        }
        loop {
            let status = self.get_order_status(order_id, signal.clone()).await?;
            let st = if status.payment_status.is_empty() {
                status.order_status.clone()
            } else {
                status.payment_status.clone()
            };
            if is_order_terminal(&st) {
                if is_order_success(&st) {
                    return Ok(status);
                }
                return Err(Error::OrderTerminal {
                    order_id: order_id.to_string(),
                    status: st,
                });
            }
            sleep_with_cancel(poll_interval_ms as u64, signal.as_ref()).await?;
        }
    }
}

/// yudao 分页 `{list:[...]}` 或裸数组两种形态解码（对齐 TS `listTokenPackages`/`listMyOrders`）。
fn decode_list_or_page<T: serde::de::DeserializeOwned>(
    what: &str,
    raw: serde_json::Value,
) -> Result<Vec<T>> {
    // 尝试 yudao 分页格式。
    if raw.is_object() && raw.get("list").is_some() {
        let page: YudaoPageResult<T> =
            serde_json::from_value(raw).map_err(|e| Error::other(format!("decode {what}: {e}")))?;
        return Ok(page.list);
    }
    // 降级：直接数组。
    if raw.is_array() {
        return serde_json::from_value(raw)
            .map_err(|e| Error::other(format!("decode {what}: {e}")));
    }
    Err(Error::other(format!("decode {what}: unexpected shape")))
}

/// 可取消 sleep（对应 TS `sleepWithSignal`）；取消触发抛 aborted。
async fn sleep_with_cancel(ms: u64, signal: Option<&CancellationToken>) -> Result<()> {
    if ms == 0 {
        return Ok(());
    }
    match signal {
        Some(c) if c.is_cancelled() => Err(Error::other("aborted")),
        Some(c) => {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(ms)) => Ok(()),
                _ = c.cancelled() => Err(Error::other("aborted")),
            }
        }
        None => {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            Ok(())
        }
    }
}
