//! 法律案件咨询 namespace 类型。端口自 `casehall/types.ts`（商品化总规划 P5 方案 B）。
//!
//! 与 tk-dist `yudao-module-casehall` 对齐。SDK 仅暴露公开端点视图 + 登录态 me/* 端点。

use crate::macros::open_string_union;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 律师档案公开视图（PII L3 字段 licenseNo 等已脱敏剥离）。TS `LawyerSummary`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LawyerSummary {
    pub id: i64,
    #[serde(rename = "realName")]
    pub real_name: String,
    #[serde(rename = "lawFirm", default, skip_serializing_if = "Option::is_none")]
    pub law_firm: Option<String>,
    /// 执业领域（后端 practice_area_json 解析为数组）。
    #[serde(
        rename = "practiceAreas",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub practice_areas: Option<Vec<String>>,
    #[serde(
        rename = "yearsOfPractice",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub years_of_practice: Option<i64>,
    #[serde(rename = "avatarUrl", default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub languages: Option<String>,
    /// 评分（f64，非金额）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
    #[serde(
        rename = "caseHandledCount",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub case_handled_count: Option<i64>,
    /// PENDING / VERIFIED / REJECTED / EXPIRED。
    #[serde(
        rename = "verificationStatus",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub verification_status: Option<String>,
    /// ACTIVE / DISABLED / SUSPENDED。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// 案件线索创建请求。TS `SubmitCaseLeadRequest`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitCaseLeadRequest {
    /// CONSULTATION / DOC_REVIEW / CASE_REPRESENT。
    #[serde(rename = "caseType", default, skip_serializing_if = "Option::is_none")]
    pub case_type: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// LOW / NORMAL / URGENT。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urgency: Option<String>,
    /// 预算（整数分，§3）。
    #[serde(rename = "budgetFen", default, skip_serializing_if = "Option::is_none")]
    pub budget_fen: Option<i64>,
    /// ISO 日期 yyyy-MM-dd。
    #[serde(
        rename = "expectedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub expected_at: Option<String>,
}

/// 案件线索 ID 返回体（`{id}`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseLeadIdResult {
    pub id: i64,
}

/// 案件线索（用户视角）。TS `CaseLead`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseLead {
    pub id: i64,
    #[serde(rename = "caseType", default, skip_serializing_if = "Option::is_none")]
    pub case_type: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urgency: Option<String>,
    /// 预算（整数分，§3）。
    #[serde(rename = "budgetFen", default, skip_serializing_if = "Option::is_none")]
    pub budget_fen: Option<i64>,
    #[serde(
        rename = "expectedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub expected_at: Option<String>,
    /// OPEN / MATCHED / CLAIMED / CLOSED / EXPIRED。
    pub status: String,
    #[serde(
        rename = "claimCount",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub claim_count: Option<i64>,
    #[serde(
        rename = "matchedLawyerId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub matched_lawyer_id: Option<i64>,
    #[serde(rename = "expiresAt", default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(
        rename = "createTime",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub create_time: Option<String>,
}

/// 案件（委托人视角）。TS `CaseMatter`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseMatter {
    pub id: i64,
    #[serde(rename = "leadId", default, skip_serializing_if = "Option::is_none")]
    pub lead_id: Option<i64>,
    #[serde(rename = "lawyerId")]
    pub lawyer_id: i64,
    #[serde(
        rename = "matterType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub matter_type: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// ACTIVE / ON_HOLD / CLOSED / ARCHIVED。
    pub status: String,
    /// WON / LOST / SETTLED / WITHDRAWN。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(rename = "startedAt", default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(rename = "closedAt", default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<String>,
}

/// 法律咨询单（双方视图）。TS `LegalConsultation`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegalConsultation {
    pub id: i64,
    #[serde(rename = "lawyerId", default, skip_serializing_if = "Option::is_none")]
    pub lawyer_id: Option<i64>,
    /// 命中 dist_compliance_sku.sku_code，benefit_type='LEGAL_SERVICE'。
    #[serde(rename = "skuCode")]
    pub sku_code: String,
    #[serde(rename = "matterId", default, skip_serializing_if = "Option::is_none")]
    pub matter_id: Option<i64>,
    #[serde(
        rename = "durationMin",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub duration_min: Option<i64>,
    #[serde(
        rename = "scheduledAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub scheduled_at: Option<String>,
    #[serde(rename = "startedAt", default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(rename = "endedAt", default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    /// PENDING / SCHEDULED / ONGOING / DONE / CANCELED。
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
}

/// 法律咨询预约请求。TS `BookConsultationRequest`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookConsultationRequest {
    /// 必填：LEGAL_CONSULTATION_ONCE / LEGAL_CONSULTATION_60MIN。
    #[serde(rename = "skuCode")]
    pub sku_code: String,
    /// 可选：指定律师，留空走 AI 推荐池。
    #[serde(rename = "lawyerId", default, skip_serializing_if = "Option::is_none")]
    pub lawyer_id: Option<i64>,
    /// ISO 时刻。
    #[serde(
        rename = "scheduledAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub scheduled_at: Option<String>,
    /// 关联案件。
    #[serde(rename = "matterId", default, skip_serializing_if = "Option::is_none")]
    pub matter_id: Option<i64>,
}

/// 预约咨询 ID 返回体（`{consultationId}`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookConsultationResult {
    #[serde(rename = "consultationId")]
    pub consultation_id: i64,
}

/// 法律服务订单（用户视角）。TS `LegalServiceOrder`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegalServiceOrder {
    pub id: i64,
    #[serde(rename = "lawyerId", default, skip_serializing_if = "Option::is_none")]
    pub lawyer_id: Option<i64>,
    #[serde(rename = "skuCode")]
    pub sku_code: String,
    /// 跨模块松耦合关联 distribution 主订单。
    #[serde(
        rename = "distributionOrderId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub distribution_order_id: Option<i64>,
    /// 金额（整数分，§3）。
    #[serde(rename = "amountFen")]
    pub amount_fen: i64,
    /// PENDING / PAID / FULFILLING / DONE / REFUNDED / CANCELED。
    pub status: String,
    #[serde(rename = "paidAt", default, skip_serializing_if = "Option::is_none")]
    pub paid_at: Option<String>,
    #[serde(rename = "doneAt", default, skip_serializing_if = "Option::is_none")]
    pub done_at: Option<String>,
}

/// 律师自查执业证审核状态视图（v2.0.0+）。TS `LawyerCredentialMyView`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LawyerCredentialMyView {
    /// Credential 主键。
    pub id: i64,
    /// LICENSE / CERTIFICATE / FIRM_LETTER / DIPLOMA / OTHER。
    #[serde(
        rename = "credentialType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub credential_type: Option<String>,
    /// PENDING / OCR_PARSED / MANUAL_REVIEW / APPROVED / REJECTED。
    #[serde(
        rename = "verificationStatus",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub verification_status: Option<String>,
    /// OCR 置信度 0~1（f64，非金额）；null 表示尚未 OCR。
    #[serde(
        rename = "ocrConfidence",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ocr_confidence: Option<f64>,
    /// 低置信度需 admin 人工复核。
    #[serde(
        rename = "manualReviewRequired",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub manual_review_required: Option<bool>,
    /// 仅 REJECTED 时非空。
    #[serde(
        rename = "rejectionReason",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rejection_reason: Option<String>,
    /// ISO-8601 = createTime。
    #[serde(
        rename = "submittedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub submitted_at: Option<String>,
    /// ISO-8601，admin 审核完成时间；null 表示尚未审核。
    #[serde(
        rename = "reviewedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub reviewed_at: Option<String>,
}

/// 5 Legal SKU code（与 dist_compliance_sku benefit_type='LEGAL_SERVICE' 同源）。
/// TS `LegalSkuCode`（闭 union → enum）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LegalSkuCode {
    #[serde(rename = "LEGAL_CONSULTATION_ONCE")]
    ConsultationOnce,
    #[serde(rename = "LEGAL_CONSULTATION_60MIN")]
    Consultation60Min,
    #[serde(rename = "LEGAL_DOC_REVIEW_HUMAN")]
    DocReviewHuman,
    #[serde(rename = "LEGAL_CASE_LEAD_CLAIM")]
    CaseLeadClaim,
    #[serde(rename = "LEGAL_LAWYER_SERVICE_PKG")]
    LawyerServicePkg,
}

impl LegalSkuCode {
    pub fn as_str(self) -> &'static str {
        match self {
            LegalSkuCode::ConsultationOnce => "LEGAL_CONSULTATION_ONCE",
            LegalSkuCode::Consultation60Min => "LEGAL_CONSULTATION_60MIN",
            LegalSkuCode::DocReviewHuman => "LEGAL_DOC_REVIEW_HUMAN",
            LegalSkuCode::CaseLeadClaim => "LEGAL_CASE_LEAD_CLAIM",
            LegalSkuCode::LawyerServicePkg => "LEGAL_LAWYER_SERVICE_PKG",
        }
    }
}

open_string_union! {
    /// LEGAL_SERVICE benefit type（固定 'LEGAL_SERVICE'，开放 union 保前向兼容）。
    LegalBenefitType {
        LEGAL_SERVICE => "LEGAL_SERVICE",
    }
}

/// 法律服务 SKU 公开视图（与 ComplianceSku 同 schema，benefit_type 收敛为 LEGAL_SERVICE）。
/// TS `LegalServiceSku`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegalServiceSku {
    #[serde(rename = "skuCode")]
    pub sku_code: String,
    /// 固定 'LEGAL_SERVICE'（开放 union）。
    #[serde(rename = "benefitType")]
    pub benefit_type: LegalBenefitType,
    #[serde(
        rename = "providerProduct",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub provider_product: Option<String>,
    /// 单价（整数分，§3）。
    #[serde(
        rename = "unitPriceFen",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub unit_price_fen: Option<i64>,
    /// 超量价（整数分，§3）。
    #[serde(
        rename = "overagePriceFen",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub overage_price_fen: Option<i64>,
    #[serde(
        rename = "regionScope",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub region_scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(
        rename = "includedInPlans",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub included_in_plans: Option<HashMap<String, i64>>,
}

/// `list_lawyers` 查询参数。对应 TS `listLawyers(params?)`。
#[derive(Debug, Clone, Default)]
pub struct ListLawyersParams {
    pub practice_area: Option<String>,
    pub location: Option<String>,
    pub page_no: Option<i64>,
    pub page_size: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_sku_code_closed_enum_wire() {
        let c: LegalSkuCode = serde_json::from_str("\"LEGAL_CONSULTATION_60MIN\"").unwrap();
        assert_eq!(c, LegalSkuCode::Consultation60Min);
        assert_eq!(c.as_str(), "LEGAL_CONSULTATION_60MIN");
    }

    #[test]
    fn legal_service_sku_benefit_open_union_and_fen_i64() {
        let s: LegalServiceSku = serde_json::from_str(
            r#"{"skuCode":"LEGAL_DOC_REVIEW_HUMAN","benefitType":"LEGAL_SERVICE",
                "unitPriceFen":19900,"overagePriceFen":9900,"includedInPlans":{"PRO":3}}"#,
        )
        .unwrap();
        // benefitType 开放 union 容忍未来值。
        assert_eq!(s.benefit_type.as_str(), "LEGAL_SERVICE");
        // *Fen = i64 整数分（§3）。
        assert_eq!(s.unit_price_fen, Some(19900_i64));
        assert_eq!(s.overage_price_fen, Some(9900_i64));
        assert_eq!(s.included_in_plans.unwrap().get("PRO"), Some(&3_i64));
    }

    #[test]
    fn case_lead_budget_fen_i64() {
        let req = SubmitCaseLeadRequest {
            case_type: Some("CONSULTATION".into()),
            title: "t".into(),
            summary: None,
            location: None,
            urgency: None,
            budget_fen: Some(500_000),
            expected_at: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["budgetFen"], 500_000_i64);
        // 缺省 Option 字段不出现在 wire（skip None）。
        assert!(v.get("summary").is_none());
    }
}
