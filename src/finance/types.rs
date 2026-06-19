//! 财务（P7）类型。端口自 `finance/types.ts`（商品化总规划 2026-05-25）。
//!
//! 与 tk-dist `dist_invoice` / `dist_refund_*` / `dist_corporate_transfer` 七表族对齐。
//! **金额阵营（§3）**：所有 `*Fen` 字段 = i64 整数分；`taxRate` = f64 比率。

use crate::macros::open_string_union;
use serde::{Deserialize, Serialize};

/// 发票视图（P7）。TS `Invoice`。
///
/// PII 分级（与 tk-dist `DistInvoiceDO @Sensitive` 对齐）：L3 字段（taxId/bankAccount/
/// contactPhone/bankName）由服务端按上下文脱敏/加密，SDK 类型不强制 PII 级。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invoice {
    pub id: i64,
    /// 发票号（开票后回写）。L0。
    #[serde(rename = "invoiceNo", default, skip_serializing_if = "Option::is_none")]
    pub invoice_no: Option<String>,
    #[serde(rename = "orderId", default, skip_serializing_if = "Option::is_none")]
    pub order_id: Option<i64>,
    /// UUID。L0。
    #[serde(rename = "userId", default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(
        rename = "enterpriseId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub enterprise_id: Option<i64>,
    /// NORMAL / VAT_GENERAL / VAT_SPECIAL。L0。
    #[serde(
        rename = "invoiceType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub invoice_type: Option<String>,
    /// 抬头。L2。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// 税号。L3 —— 服务端按上下文脱敏/加密。
    #[serde(rename = "taxId", default, skip_serializing_if = "Option::is_none")]
    pub tax_id: Option<String>,
    /// 开户行。L3。
    #[serde(rename = "bankName", default, skip_serializing_if = "Option::is_none")]
    pub bank_name: Option<String>,
    /// 银行账号。L3 —— `@FieldEncrypt` 存储加密。
    #[serde(
        rename = "bankAccount",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub bank_account: Option<String>,
    /// 收件地址。L2。
    #[serde(
        rename = "contactAddress",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contact_address: Option<String>,
    /// 联系手机。L3 —— `@FieldEncrypt` 存储加密。
    #[serde(
        rename = "contactPhone",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contact_phone: Option<String>,
    /// 金额（整数分，§3）。
    #[serde(rename = "amountFen", default, skip_serializing_if = "Option::is_none")]
    pub amount_fen: Option<i64>,
    /// 6% 默认（f64，比率非金额）。
    #[serde(rename = "taxRate", default, skip_serializing_if = "Option::is_none")]
    pub tax_rate: Option<f64>,
    /// 税额（整数分，§3）。
    #[serde(
        rename = "taxAmountFen",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub tax_amount_fen: Option<i64>,
    /// PENDING / ISSUED / VOIDED / REISSUED。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(rename = "issuedAt", default, skip_serializing_if = "Option::is_none")]
    pub issued_at: Option<String>,
    #[serde(rename = "pdfUrl", default, skip_serializing_if = "Option::is_none")]
    pub pdf_url: Option<String>,
}

open_string_union! {
    /// 发票类型（开票申请用）。TS `'NORMAL'|'VAT_GENERAL'|'VAT_SPECIAL'`。
    InvoiceType {
        NORMAL => "NORMAL",
        VAT_GENERAL => "VAT_GENERAL",
        VAT_SPECIAL => "VAT_SPECIAL",
    }
}

/// 开票申请 request。TS `RequestInvoiceInput`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestInvoiceInput {
    #[serde(rename = "orderId", default, skip_serializing_if = "Option::is_none")]
    pub order_id: Option<i64>,
    #[serde(
        rename = "enterpriseId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub enterprise_id: Option<i64>,
    #[serde(
        rename = "invoiceType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub invoice_type: Option<InvoiceType>,
    pub title: String,
    #[serde(rename = "taxId", default, skip_serializing_if = "Option::is_none")]
    pub tax_id: Option<String>,
    #[serde(rename = "bankName", default, skip_serializing_if = "Option::is_none")]
    pub bank_name: Option<String>,
    #[serde(
        rename = "bankAccount",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub bank_account: Option<String>,
    #[serde(
        rename = "contactAddress",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contact_address: Option<String>,
    #[serde(
        rename = "contactPhone",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contact_phone: Option<String>,
    /// 金额（整数分，§3）。
    #[serde(rename = "amountFen")]
    pub amount_fen: i64,
    /// 税率（f64，比率非金额）。
    #[serde(rename = "taxRate", default, skip_serializing_if = "Option::is_none")]
    pub tax_rate: Option<f64>,
}

/// 退款规则字典（决策 15）。TS `RefundPolicy`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefundPolicy {
    pub id: i64,
    /// SUBSCRIPTION_NO_REFUND / TOKEN_PACK_7DAY_UNUSED / ...
    #[serde(rename = "policyCode")]
    pub policy_code: String,
    /// MODEL_MEMBERSHIP / TOKEN_PACK / COMPLIANCE / LEGAL_SERVICE。
    #[serde(
        rename = "productFamily",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub product_family: Option<String>,
    /// 退款窗口期（NULL = 不退）。
    #[serde(
        rename = "refundWindowDays",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub refund_window_days: Option<i64>,
    /// NO_REFUND / FULL_IF_UNUSED / PRORATA_UNUSED。
    #[serde(
        rename = "refundRule",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub refund_rule: Option<String>,
    #[serde(
        rename = "requireProof",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub require_proof: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// 退款记录。TS `RefundRecord`。所有 `*Fen` = i64 整数分（§3）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefundRecord {
    pub id: i64,
    #[serde(rename = "orderId")]
    pub order_id: i64,
    /// UUID。
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(
        rename = "policyCode",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub policy_code: Option<String>,
    /// 申请金额（整数分，§3）。
    #[serde(
        rename = "requestedAmountFen",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub requested_amount_fen: Option<i64>,
    /// 核准金额（整数分，§3）。
    #[serde(
        rename = "approvedAmountFen",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub approved_amount_fen: Option<i64>,
    /// 实退（扣已用）（整数分，§3）。
    #[serde(
        rename = "actualAmountFen",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub actual_amount_fen: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// PENDING / APPROVED / REFUNDED / REJECTED。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// UUID。
    #[serde(
        rename = "operatorUserId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub operator_user_id: Option<String>,
    #[serde(
        rename = "refundedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub refunded_at: Option<String>,
    #[serde(
        rename = "rejectedReason",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rejected_reason: Option<String>,
}

open_string_union! {
    /// 商品族（退款申请用）。TS `'MODEL_MEMBERSHIP'|'TOKEN_PACK'|'COMPLIANCE'|'LEGAL_SERVICE'`。
    RefundProductFamily {
        MODEL_MEMBERSHIP => "MODEL_MEMBERSHIP",
        TOKEN_PACK => "TOKEN_PACK",
        COMPLIANCE => "COMPLIANCE",
        LEGAL_SERVICE => "LEGAL_SERVICE",
    }
}

/// 退款申请 request。TS `RequestRefundInput`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestRefundInput {
    #[serde(rename = "orderId")]
    pub order_id: i64,
    /// 直传 policyCode；留空可用 productFamily + anyUsage 派生。
    #[serde(
        rename = "policyCode",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub policy_code: Option<String>,
    #[serde(
        rename = "productFamily",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub product_family: Option<RefundProductFamily>,
    #[serde(rename = "anyUsage", default, skip_serializing_if = "Option::is_none")]
    pub any_usage: Option<bool>,
    /// 申请金额（整数分，§3）。
    #[serde(rename = "requestedAmountFen")]
    pub requested_amount_fen: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// 对公转账记录（决策 14）。TS `CorporateTransfer`。`amountFen` = i64 整数分（§3）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorporateTransfer {
    pub id: i64,
    #[serde(rename = "orderId", default, skip_serializing_if = "Option::is_none")]
    pub order_id: Option<i64>,
    #[serde(
        rename = "contractId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contract_id: Option<i64>,
    /// UUID。
    #[serde(rename = "userId", default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(
        rename = "enterpriseId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub enterprise_id: Option<i64>,
    /// 金额（整数分，§3）。
    #[serde(rename = "amountFen", default, skip_serializing_if = "Option::is_none")]
    pub amount_fen: Option<i64>,
    #[serde(rename = "payerBank", default, skip_serializing_if = "Option::is_none")]
    pub payer_bank: Option<String>,
    #[serde(
        rename = "payerAccount",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub payer_account: Option<String>,
    #[serde(
        rename = "receiverBank",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub receiver_bank: Option<String>,
    #[serde(
        rename = "receiverAccount",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub receiver_account: Option<String>,
    /// UUID。
    #[serde(
        rename = "salesRepUserId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sales_rep_user_id: Option<String>,
    /// UUID。
    #[serde(
        rename = "financeUserId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub finance_user_id: Option<String>,
    #[serde(
        rename = "wechatGroupUrl",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub wechat_group_url: Option<String>,
    /// INITIATED / WAITING_PROOF / PROOF_RECEIVED / CONFIRMED / FAILED。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(rename = "proofUrl", default, skip_serializing_if = "Option::is_none")]
    pub proof_url: Option<String>,
    #[serde(
        rename = "confirmedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub confirmed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// 对公转账发起 request。TS `InitiateCorporateTransferInput`。`amountFen` = i64 整数分（§3）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitiateCorporateTransferInput {
    #[serde(rename = "orderId", default, skip_serializing_if = "Option::is_none")]
    pub order_id: Option<i64>,
    #[serde(
        rename = "enterpriseId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub enterprise_id: Option<i64>,
    #[serde(
        rename = "contractId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contract_id: Option<i64>,
    /// 金额（整数分，§3）。
    #[serde(rename = "amountFen")]
    pub amount_fen: i64,
    #[serde(rename = "payerBank", default, skip_serializing_if = "Option::is_none")]
    pub payer_bank: Option<String>,
    #[serde(
        rename = "payerAccount",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub payer_account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// 对公转账发起 response（决策 14）。TS `InitiateCorporateTransferResult`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitiateCorporateTransferResult {
    pub id: i64,
    /// 销售名片二维码 URL（ops 配置）。
    #[serde(rename = "qrUrl", default, skip_serializing_if = "Option::is_none")]
    pub qr_url: Option<String>,
    /// 销售企微 ID。
    #[serde(
        rename = "salesWechatId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sales_wechat_id: Option<String>,
    /// 财务对接邮箱。
    #[serde(
        rename = "financeEmail",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub finance_email: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invoice_amount_fen_is_i64_integer() {
        // §3：所有 *Fen 字段是 i64 整数分（非 f64 元、非 string）。
        let v: Invoice = serde_json::from_str(
            r#"{"id":1,"amountFen":12345,"taxRate":0.06,"taxAmountFen":740,"orderId":99}"#,
        )
        .unwrap();
        assert_eq!(v.amount_fen, Some(12345_i64));
        assert_eq!(v.tax_amount_fen, Some(740_i64));
        // taxRate 是 f64 比率（非金额）。
        assert_eq!(v.tax_rate, Some(0.06_f64));
        // wire 仍是 camelCase。
        let back = serde_json::to_value(&v).unwrap();
        assert_eq!(back["amountFen"], 12345);
        assert_eq!(back["taxAmountFen"], 740);
    }

    #[test]
    fn refund_record_all_fen_i64() {
        let r: RefundRecord = serde_json::from_str(
            r#"{"id":2,"orderId":3,"userId":"u-uuid","requestedAmountFen":50000,"approvedAmountFen":40000,"actualAmountFen":30000}"#,
        )
        .unwrap();
        assert_eq!(r.requested_amount_fen, Some(50000_i64));
        assert_eq!(r.approved_amount_fen, Some(40000_i64));
        assert_eq!(r.actual_amount_fen, Some(30000_i64));
    }

    #[test]
    fn initiate_input_amount_fen_i64_serialize() {
        let inp = InitiateCorporateTransferInput {
            order_id: Some(7),
            enterprise_id: None,
            contract_id: None,
            amount_fen: 99_000_000,
            payer_bank: Some("ICBC".into()),
            payer_account: None,
            note: None,
        };
        let body = serde_json::to_value(&inp).unwrap();
        assert_eq!(body["amountFen"], 99_000_000_i64);
        // 大额（>2^32）仍是整数，不丢精度。
        assert!(body["amountFen"].is_i64());
    }

    #[test]
    fn invoice_type_open_union_roundtrip_unknown() {
        // 开放 union 容忍未知值（前向兼容）。
        let it: InvoiceType = serde_json::from_value(json_str("VAT_DIGITAL")).unwrap();
        assert_eq!(it.as_str(), "VAT_DIGITAL");
        assert_eq!(InvoiceType::VAT_SPECIAL, "VAT_SPECIAL");
    }

    fn json_str(s: &str) -> serde_json::Value {
        serde_json::Value::String(s.to_string())
    }
}
