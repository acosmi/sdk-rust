//! 计费域类型（entitlements / metering / wallet / 商城）。
//!
//! 端口自 `billing/types.ts`（其端口自 `acosmi-sdk-go/types.go` v0.19.0）。
//!
//! 命名约定：字段名 = Go json tag 字面量（wire format），不做 camelCase 重映射 → serde 直用
//! 同名 snake/camel；非同名经 `#[serde(rename)]`。
//!
//! **金额三阵营（方案 §3，逐字段查）**：
//! - 钱包域 [`WalletStats`] / [`Transaction::amount`] = `f64`（Go `wallet.go` 用 float64，wire 是 JSON 数字）。
//! - 商城/finance `*Cent`/`*Fen` = `i64`（整数分）。
//! - json.Number 透传金额 = `String`（精度安全；本域内 [`OrderListItem::discount_rate`] 为此类）。
//! - 同名分歧：`OrderListItem.discount_rate`=`Option<String>` ⊥ `TokenPackage.discount_rate`=`f64`，不可统一。

use serde::{Deserialize, Serialize};

// =============================================================================
// Entitlements
// =============================================================================
//
// ⚠️ 额度单位双体系：本域所有 token* 字段（token_quota/token_remaining/token_used/
// token_total）的单位取决于权益是否付费 ——
//   - 免费档（TK 体系）：type 非 TOKEN_PACKAGE/SUBSCRIPTION 的权益，数值 = 原始 Token。
//   - 付费会员（Credits 代币体系）：type ∈ {TOKEN_PACKAGE, SUBSCRIPTION}，数值 = 微 Credits（÷1000 = Credits）。
// 绝不能把免费区与付费区的 token* 直接求和（单位不同）。判据 = EntitlementItem.r#type。

/// 权益余额（聚合）。token* 单位双体系：免费=Token / 付费(TOKEN_PACKAGE,SUBSCRIPTION)=微Credits(÷1000=Credits)。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EntitlementBalance {
    #[serde(rename = "totalTokenQuota")]
    pub total_token_quota: i64,
    #[serde(rename = "totalTokenUsed")]
    pub total_token_used: i64,
    #[serde(rename = "totalTokenRemaining")]
    pub total_token_remaining: i64,
    #[serde(rename = "totalCallQuota")]
    pub total_call_quota: i64,
    #[serde(rename = "totalCallUsed")]
    pub total_call_used: i64,
    #[serde(rename = "totalCallRemaining")]
    pub total_call_remaining: i64,
    #[serde(rename = "activeEntitlements")]
    pub active_entitlements: i64,
}

/// 单条权益明细。token* 单位由 type 决定：TOKEN_PACKAGE/SUBSCRIPTION=微Credits(÷1000=Credits)，其余=原始Token。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EntitlementItem {
    pub id: String,
    /// 权益类型，也是额度单位判据：TOKEN_PACKAGE/SUBSCRIPTION → 付费(Credits 体系)，其余 → 免费(Token 体系)。
    #[serde(rename = "type")]
    pub r#type: String,
    pub status: String,
    #[serde(rename = "tokenQuota")]
    pub token_quota: i64,
    #[serde(rename = "tokenUsed")]
    pub token_used: i64,
    #[serde(rename = "tokenRemaining")]
    pub token_remaining: i64,
    #[serde(rename = "callQuota")]
    pub call_quota: i64,
    #[serde(rename = "callUsed")]
    pub call_used: i64,
    #[serde(rename = "callRemaining")]
    pub call_remaining: i64,
    #[serde(rename = "expiresAt", default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(rename = "sourceId", default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(
        rename = "sourceType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub source_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remark: Option<String>,
    /// 仅出现在 /grants map（toEntitlementMap 之外的 grants 端点）。
    #[serde(rename = "createdAt", default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// list（toEntitlementMap）与 grants 均返回 activatedAt。
    #[serde(
        rename = "activatedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub activated_at: Option<String>,
}

/// 形状严格对齐网关 `model.InternalBalanceResponse`（entitlement.go GetBalanceDetail 直透）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BalanceDetailEntitlement {
    pub id: String,
    #[serde(
        rename = "sourceOrderId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub source_order_id: Option<String>,
    pub status: String,
    #[serde(rename = "tokenQuota")]
    pub token_quota: i64,
    #[serde(rename = "tokenUsed")]
    pub token_used: i64,
    #[serde(rename = "expiresAt", default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// 详细余额（含每条权益明细）。token* 单位双体系：免费=Token / 付费=微Credits(÷1000=Credits)，
/// 按 entitlements[].id 对应权益 type 区分，勿跨单位求和。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BalanceDetail {
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "tokenRemaining")]
    pub token_remaining: i64,
    #[serde(rename = "tokenTotal")]
    pub token_total: i64,
    #[serde(rename = "callRemaining")]
    pub call_remaining: i64,
    #[serde(rename = "callTotal")]
    pub call_total: i64,
    pub entitlements: Vec<BalanceDetailEntitlement>,
}

/// 核销记录。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConsumeRecord {
    pub id: String,
    #[serde(rename = "entitlementId")]
    pub entitlement_id: String,
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "modelId", default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(rename = "tokensConsumed")]
    pub tokens_consumed: i64,
    pub status: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    // V32 缓存/预留/调用计数列（ConsumeRecordDO），旧表未迁移时为 0/缺省。
    #[serde(
        rename = "reservedTokens",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub reserved_tokens: Option<i64>,
    #[serde(
        rename = "callsConsumed",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub calls_consumed: Option<i64>,
    #[serde(
        rename = "inputTokens",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub input_tokens: Option<i64>,
    #[serde(
        rename = "outputTokens",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub output_tokens: Option<i64>,
    #[serde(
        rename = "cacheReadTokens",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cache_read_tokens: Option<i64>,
    #[serde(
        rename = "cacheCreateTokens",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cache_create_tokens: Option<i64>,
}

/// 核销记录分页响应。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConsumeRecordPage {
    pub records: Vec<ConsumeRecord>,
    pub total: i64,
    pub page: i64,
    #[serde(rename = "pageSize")]
    pub page_size: i64,
}

// =============================================================================
// V29 Per-Model Bucket
// =============================================================================

/// 单桶视图（用户多桶 hero / 模型切换提示用）。
///
/// 字段名仍叫 ETU 但 T3 死代码清除后 = raw token（V29 系数管理已退役）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelBucket {
    #[serde(rename = "bucketId")]
    pub bucket_id: String,
    #[serde(rename = "entitlementId")]
    pub entitlement_id: String,
    /// `"*"` = 通配。
    #[serde(rename = "modelId")]
    pub model_id: String,
    /// COMMERCIAL / GENERIC。
    #[serde(rename = "bucketClass")]
    pub bucket_class: String,
    #[serde(rename = "tokenQuota")]
    pub token_quota: i64,
    #[serde(rename = "tokenUsed")]
    pub token_used: i64,
    #[serde(rename = "tokenRemaining")]
    pub token_remaining: i64,
    #[serde(rename = "callQuota")]
    pub call_quota: i64,
    #[serde(rename = "callUsed")]
    pub call_used: i64,
    #[serde(rename = "callRemaining")]
    pub call_remaining: i64,
    #[serde(
        rename = "allowedModelsJson",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub allowed_models_json: Option<String>,
}

/// GetByModel 响应；primaryBucket 在 bucketId 为空时表示无可用桶。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelByQuotaResponse {
    #[serde(rename = "modelId")]
    pub model_id: String,
    /// 折算后剩余（调度判定用）。
    #[serde(rename = "etuRemaining")]
    pub etu_remaining: i64,
    /// 反系数估算的原始 token（UI 展示用）。
    #[serde(rename = "rawTokenRemaining")]
    pub raw_token_remaining: i64,
    #[serde(rename = "hasQuota")]
    pub has_quota: bool,
    #[serde(
        rename = "primaryBucket",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub primary_bucket: Option<ModelBucket>,
}

/// 单条模型系数（SDK TTL 8s 缓存源）。
///
/// **@deprecated** 系数管理已退役（raw 1:1 计费）。网关 `/entitlements/coefficients` 永久返回 `[]`。
/// 本类型仅为向后兼容保留。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelCoefficient {
    #[serde(rename = "modelId")]
    pub model_id: String,
    #[serde(rename = "tenantId")]
    pub tenant_id: String,
    #[serde(rename = "inputCoef")]
    pub input_coef: f64,
    #[serde(rename = "outputCoef")]
    pub output_coef: f64,
    #[serde(rename = "cacheReadCoef")]
    pub cache_read_coef: f64,
    #[serde(rename = "cacheCreationCoef")]
    pub cache_creation_coef: f64,
    pub version: i64,
    #[serde(rename = "effectiveAt")]
    pub effective_at: String,
}

// =============================================================================
// Token Packages（商城）
// =============================================================================

/// 流量包商品。形状对齐 `ConsumerPublicController.toProductView`（/api/products 端点，/token-packages 代理转发）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenPackage {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "originalPriceCent")]
    pub original_price_cent: i64,
    #[serde(rename = "campaignPriceCent")]
    pub campaign_price_cent: i64,
    #[serde(rename = "renewalPriceCent")]
    pub renewal_price_cent: i64,
    /// 折扣率 0~1（BigDecimal，JSON 数字）；0 表示无折扣。
    /// **金额三阵营**：此处 `discountRate` 是 JSON 数字 → `f64`（与 [`OrderListItem::discount_rate`] 的 String 不同，不可统一）。
    #[serde(rename = "discountRate")]
    pub discount_rate: f64,
    #[serde(
        rename = "modelsJson",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub models_json: Option<String>,
    #[serde(
        rename = "productImage",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub product_image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<String>>,
    #[serde(rename = "billingCycle")]
    pub billing_cycle: String,
    pub featured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eyebrow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<String>,
    #[serde(rename = "sortOrder")]
    pub sort_order: i64,
}

/// 支付方式枚举字面量（来自 /payment-options）。闭 union → enum + serde rename。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaymentMethod {
    #[serde(rename = "WECHAT_NATIVE")]
    WechatNative,
    #[serde(rename = "ALIPAY_PRECREATE")]
    AlipayPrecreate,
    #[serde(rename = "BANK_TRANSFER")]
    BankTransfer,
}

/// 下单请求。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PayPayload {
    /// 支付方式枚举字面量（来自 /payment-options）。字段名必须是 `paymentMethod`（后端 BuyRequest.paymentMethod）。
    #[serde(
        rename = "paymentMethod",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub payment_method: Option<PaymentMethod>,
    #[serde(rename = "deviceId", default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// 幂等请求 ID。
    #[serde(
        rename = "clientRequestId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub client_request_id: Option<String>,
}

/// 下单 / 订单状态响应（`OrderPaymentService.BuyResponse`，buy 与状态查询共用）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuyResponse {
    #[serde(rename = "orderId")]
    pub order_id: i64,
    #[serde(rename = "orderNo")]
    pub order_no: String,
    #[serde(rename = "productId", default, skip_serializing_if = "Option::is_none")]
    pub product_id: Option<String>,
    #[serde(
        rename = "productName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub product_name: Option<String>,
    /// **金额三阵营**：`amountFen` = 整数分 → `i64`。
    #[serde(rename = "amountFen")]
    pub amount_fen: i64,
    #[serde(rename = "orderStatus")]
    pub order_status: String,
    #[serde(rename = "paymentMethod")]
    pub payment_method: String,
    #[serde(rename = "paymentStatus")]
    pub payment_status: String,
    #[serde(
        rename = "qrCodeContent",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub qr_code_content: Option<String>,
    #[serde(rename = "payUrl", default, skip_serializing_if = "Option::is_none")]
    pub pay_url: Option<String>,
    #[serde(
        rename = "paymentExpiresAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub payment_expires_at: Option<String>,
    /// 对公转账信息（BANK_TRANSFER 时），形状为后端嵌套对象。
    #[serde(
        rename = "bankTransferInfo",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub bank_transfer_info: Option<serde_json::Value>,
}

/// 我的订单列表行（toOrderMap）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrderListItem {
    pub id: String,
    #[serde(
        rename = "bizOrderId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub biz_order_id: Option<String>,
    #[serde(
        rename = "productName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub product_name: Option<String>,
    /// **金额三阵营**：`amountCent` = 整数分 → `i64`。
    #[serde(rename = "amountCent")]
    pub amount_cent: i64,
    #[serde(
        rename = "originalPriceCent",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub original_price_cent: Option<i64>,
    /// **金额三阵营**：此处 `discountRate` 是 String（json.Number 透传）→ `Option<String>`
    /// （与 [`TokenPackage::discount_rate`] 的 `f64` 不同，bug-for-bug 不可统一）。
    #[serde(
        rename = "discountRate",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub discount_rate: Option<String>,
    #[serde(
        rename = "paymentMethod",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub payment_method: Option<String>,
    #[serde(rename = "payStatus")]
    pub pay_status: String,
    #[serde(
        rename = "commissionStatus",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub commission_status: Option<String>,
    #[serde(
        rename = "issueStatus",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub issue_status: Option<String>,
    #[serde(
        rename = "channelCode",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub channel_code: Option<String>,
    #[serde(rename = "createdAt", default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(rename = "payTime", default, skip_serializing_if = "Option::is_none")]
    pub pay_time: Option<String>,
}

/// **@deprecated** 旧形状与任何真实端点都不符；buy/状态查询改用 [`BuyResponse`]，列表用 [`OrderListItem`]。
pub type Order = BuyResponse;

/// **@deprecated** getOrderStatus 现返回 [`BuyResponse`]（无 status 字段）。
pub type OrderStatus = BuyResponse;

// =============================================================================
// Wallet（钱包）
// =============================================================================

/// 钱包统计。
///
/// **金额三阵营**：Go `wallet.go` 用 float64（非 json.Number）→ wire 是 JSON 数字，
/// 故 `balance`/`monthly_consumption`/`monthly_recharge` = `f64`（唯一浮点金额端点，不可映成 String）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WalletStats {
    pub balance: f64,
    #[serde(rename = "monthlyConsumption")]
    pub monthly_consumption: f64,
    #[serde(rename = "monthlyRecharge")]
    pub monthly_recharge: f64,
    #[serde(rename = "transactionCount")]
    pub transaction_count: i64,
}

/// 交易记录。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: String,
    /// **金额三阵营**：钱包域 float64 → `f64`。
    pub amount: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remark: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallet_stats_amounts_are_f64() {
        // 钱包域金额 = JSON 数字（含小数）→ f64；i64 会反序列化失败。
        let json = r#"{"balance":123.45,"monthlyConsumption":67.8,"monthlyRecharge":9.0,"transactionCount":12}"#;
        let s: WalletStats = serde_json::from_str(json).unwrap();
        assert_eq!(s.balance, 123.45);
        assert_eq!(s.monthly_consumption, 67.8);
        assert_eq!(s.monthly_recharge, 9.0);
        assert_eq!(s.transaction_count, 12);
        // round-trip
        let back: WalletStats = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.balance, 123.45);
    }

    #[test]
    fn transaction_amount_is_f64() {
        let json =
            r#"{"id":"t1","type":"CONSUME","amount":-3.25,"createdAt":"2026-06-19T00:00:00Z"}"#;
        let t: Transaction = serde_json::from_str(json).unwrap();
        assert_eq!(t.amount, -3.25);
        assert_eq!(t.r#type, "CONSUME");
    }

    #[test]
    fn buy_response_amount_fen_is_i64() {
        // 商城金额 = 整数分 → i64。
        let json = r#"{"orderId":1001,"orderNo":"NO1","amountFen":1990,"orderStatus":"PENDING","paymentMethod":"WECHAT_NATIVE","paymentStatus":"PENDING"}"#;
        let r: BuyResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.amount_fen, 1990i64);
        assert_eq!(r.order_id, 1001i64);
    }

    #[test]
    fn discount_rate_divergence_string_vs_f64() {
        // OrderListItem.discount_rate = String（json.Number 透传）。
        let oj = r#"{"id":"o1","amountCent":1990,"discountRate":"0.85","payStatus":"PAID"}"#;
        let o: OrderListItem = serde_json::from_str(oj).unwrap();
        assert_eq!(o.discount_rate.as_deref(), Some("0.85"));
        assert_eq!(o.amount_cent, 1990i64);

        // TokenPackage.discount_rate = f64。
        let pj = r#"{"id":"p1","name":"Pack","originalPriceCent":1000,"campaignPriceCent":800,"renewalPriceCent":900,"discountRate":0.8,"billingCycle":"MONTHLY","featured":false,"sortOrder":1}"#;
        let p: TokenPackage = serde_json::from_str(pj).unwrap();
        assert_eq!(p.discount_rate, 0.8f64);
        assert_eq!(p.original_price_cent, 1000i64);
    }

    #[test]
    fn payment_method_round_trip() {
        let m = PaymentMethod::AlipayPrecreate;
        let s = serde_json::to_string(&m).unwrap();
        assert_eq!(s, "\"ALIPAY_PRECREATE\"");
        let back: PaymentMethod = serde_json::from_str(&s).unwrap();
        assert_eq!(back, PaymentMethod::AlipayPrecreate);
    }
}
