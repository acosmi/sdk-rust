//! Wallet 业务方法。端口自 `billing/wallet.ts`（declaration-merging → 此处 `impl Client` 块）。

use super::types::{Transaction, WalletStats};
use crate::core::client::Client;
use crate::shared::Result;
use tokio_util::sync::CancellationToken;

impl Client {
    /// 获取钱包统计（余额/月消费/月充值）。对应 TS `getWalletStats`。
    pub async fn get_wallet_stats(&self, signal: Option<CancellationToken>) -> Result<WalletStats> {
        self.billing_get("/wallet/stats", signal).await
    }

    /// 获取最近交易记录。对应 TS `getWalletTransactions`。
    pub async fn get_wallet_transactions(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<Transaction>> {
        self.billing_get("/wallet/transactions", signal).await
    }
}
