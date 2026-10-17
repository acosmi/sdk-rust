//! 订阅双池展示契约（方案 D-14 / §4.8 / §5 P5-SDK）—— 三方共用契约 fixture 的 Rust 侧钉子。
//!
//! `tests/fixtures/` 下四份 fixture 与 `acosmi-sdk-ts/test/fixtures`、`acosmi-sdk-go/testdata` 是同字节副本
//! （各 SDK 子目录会被镜像成独立仓，所以每边各持一份）。Go 网关（P5-Go）与 CrabCode（P5-CrabCode）以它为准。
//!
//! 契约核心是"缺席 ≠ 0"：老合同没有 F 池，老网关不认识 F 池，两种情况 `freePool` 整个键都不在响应里。
//! 纯 serde 测试，不碰网络，不依赖 feature。

use acosmi::{
    ManagedModel, QuotaSummary, CREDIT_UNIT_COST_CNY, CREDIT_UNIT_OFFICIAL_CNY,
    QUOTA_UNIT_MICRO_CREDIT,
};
use serde::Deserialize;
use serde_json::{json, Value};

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

/// 信封 `{code, message, data}` 里的 `data`。
fn data(name: &str) -> Value {
    fixture(name)["data"].clone()
}

fn quota(name: &str) -> QuotaSummary {
    serde_json::from_value(data(name)).unwrap_or_else(|e| panic!("decode {name}: {e}"))
}

/// 契约的值域写成可执行断言；fixture 被篡改成契约外的值时必须返回 Err。
fn check_two_pool_contract(s: &QuotaSummary) -> Result<(), String> {
    if let Some(fp) = &s.free_pool {
        if fp.unit != QUOTA_UNIT_MICRO_CREDIT {
            return Err(format!(
                "freePool.unit = {:?}, want {QUOTA_UNIT_MICRO_CREDIT:?}",
                fp.unit
            ));
        }
        if fp.quota <= 0 {
            return Err(format!("freePool.quota = {}，额度 0 的池不下发", fp.quota));
        }
        if fp.remaining != fp.quota - fp.used {
            return Err(format!(
                "freePool.remaining = {}, want quota-used = {}",
                fp.remaining,
                fp.quota - fp.used
            ));
        }
    }
    match s.credit_unit.as_deref() {
        None | Some(CREDIT_UNIT_OFFICIAL_CNY) | Some(CREDIT_UNIT_COST_CNY) => Ok(()),
        Some(other) => Err(format!(
            "creditUnit = {other:?}, 契约值域是 {CREDIT_UNIT_OFFICIAL_CNY:?} | {CREDIT_UNIT_COST_CNY:?}"
        )),
    }
}

#[test]
fn two_pool_fixture_reads_values_and_round_trips() {
    let raw = data("quota-summary.two-pool.json");
    let s = quota("quota-summary.two-pool.json");

    let fp = s
        .free_pool
        .as_ref()
        .expect("free_pool must be Some for the two-pool fixture");
    assert_eq!(fp.quota, 19_950_000_000);
    assert_eq!(fp.used, 500_000_000);
    assert_eq!(fp.remaining, 19_450_000_000);
    assert_eq!(fp.expires_at.as_deref(), Some("2026-11-17T00:00:00Z"));
    assert_eq!(fp.unit, QUOTA_UNIT_MICRO_CREDIT);
    assert_eq!(
        fp.eligible_models,
        vec!["deepseek-flash", "qwen3.7-flash", "ZHIPU/GLM-5.3-Flash"]
    );
    assert_eq!(s.credit_unit.as_deref(), Some(CREDIT_UNIT_OFFICIAL_CNY));

    // subscriptionPool 语义不变 = 付费池 P，与 freePool 是两个独立对象；paidTotalEtu 仍是 P.remaining。
    let p = s.subscription_pool.as_ref().expect("subscription_pool");
    assert_eq!(p.quota, 44_900_000_000);
    assert_eq!(p.remaining, 40_700_000_000);
    assert_eq!(p.unit, QUOTA_UNIT_MICRO_CREDIT);
    assert_ne!(p.quota, fp.quota);
    assert_eq!(s.paid_total_etu, p.remaining);
    assert_ne!(
        s.free_total_etu, fp.remaining,
        "GENERIC 赠送桶不是 freePool"
    );
    assert_eq!(p.booster_remaining, Some(5_000_000_000));
    assert_eq!(p.booster_count, Some(1));

    let wl = s.window_limits.as_ref().expect("window_limits");
    assert_eq!(wl.len(), 2);
    assert_eq!(s.window_five_hour_continue_enabled, Some(true));
    assert_eq!(wl[0].inflight_held_credits, Some(30_000_000));
    assert!(wl[1].inflight_held_credits.is_none() && wl[1].reset_at.is_none());
    check_two_pool_contract(&s).unwrap();

    // 方向 1：fixture → struct → JSON，与 fixture 的 data 逐键相等（struct 没有吞键，也没有凭空加键）。
    let out = serde_json::to_value(&s).unwrap();
    assert_eq!(out, raw, "fixture → struct → JSON drifted from fixture");
    // 方向 2：JSON → struct 再回到 JSON，稳定。
    let again: QuotaSummary = serde_json::from_value(out.clone()).unwrap();
    assert_eq!(serde_json::to_value(&again).unwrap(), out);
}

#[test]
fn legacy_fixture_has_no_free_pool_and_stays_absent() {
    let raw = data("quota-summary.legacy.json");
    let s = quota("quota-summary.legacy.json");

    assert!(
        s.free_pool.is_none(),
        "legacy free_pool must be None, not a zero pool"
    );
    assert!(s.credit_unit.is_none());
    // 正向对照：legacy 响应本身完整可读。
    assert_eq!(
        s.subscription_pool.as_ref().map(|p| p.quota),
        Some(44_900_000_000)
    );
    assert_eq!(s.window_limits.as_ref().map(Vec::len), Some(2));
    check_two_pool_contract(&s).unwrap();

    let out = serde_json::to_value(&s).unwrap();
    let obj = out.as_object().unwrap();
    assert!(
        !obj.contains_key("freePool"),
        "re-marshalled legacy must not contain freePool"
    );
    assert!(
        !obj.contains_key("creditUnit"),
        "re-marshalled legacy must not contain creditUnit"
    );
    assert_eq!(out, raw, "legacy fixture → struct → JSON drifted");
}

#[test]
fn legacy_and_two_pool_differ_by_exactly_two_keys() {
    let two = data("quota-summary.two-pool.json");
    let legacy = data("quota-summary.legacy.json");
    let (two, legacy) = (two.as_object().unwrap(), legacy.as_object().unwrap());

    let mut extra: Vec<&String> = two.keys().filter(|k| !legacy.contains_key(*k)).collect();
    extra.sort();
    assert_eq!(extra, ["creditUnit", "freePool"]);
    for (k, v) in legacy {
        assert_eq!(
            two.get(k),
            Some(v),
            "key {k} changed between legacy and two-pool"
        );
    }
}

/// 订阅双池之前的客户端形状（只认老字段）：新响应喂给它必须原样解出，不报错（旧客户端零破坏）。
#[derive(Deserialize, PartialEq, Debug)]
struct OldClientQuotaSummary {
    #[serde(rename = "freeTotalEtu")]
    free_total_etu: i64,
    #[serde(rename = "paidTotalEtu")]
    paid_total_etu: i64,
    #[serde(rename = "freeBuckets")]
    free_buckets: Vec<Value>,
    #[serde(rename = "paidBuckets")]
    paid_buckets: Vec<Value>,
    #[serde(rename = "nextFreeExpiresAt")]
    next_free_expires_at: Option<String>,
    #[serde(rename = "nextPaidExpiresAt")]
    next_paid_expires_at: Option<String>,
}

#[test]
fn old_client_shape_decodes_the_new_body_with_same_old_fields() {
    let new_body: OldClientQuotaSummary =
        serde_json::from_value(data("quota-summary.two-pool.json")).unwrap();
    let legacy_body: OldClientQuotaSummary =
        serde_json::from_value(data("quota-summary.legacy.json")).unwrap();
    assert_eq!(
        new_body, legacy_body,
        "老字段在新响应里的含义必须一个都没挪"
    );
}

#[test]
fn contract_tamper_is_rejected() {
    let tamper = |mutate: &dyn Fn(&mut QuotaSummary)| {
        let mut s = quota("quota-summary.two-pool.json");
        mutate(&mut s);
        assert!(
            check_two_pool_contract(&s).is_err(),
            "tampered fixture passed the contract check"
        );
    };
    tamper(&|s| s.free_pool.as_mut().unwrap().unit = "CREDIT".into());
    tamper(&|s| s.credit_unit = Some("USD".into()));
    tamper(&|s| s.free_pool.as_mut().unwrap().remaining = 1);

    // [] 合法，且与 free_pool 整体缺席不同。
    let mut s = quota("quota-summary.two-pool.json");
    s.free_pool.as_mut().unwrap().eligible_models = vec![];
    check_two_pool_contract(&s).unwrap();
    assert!(s.free_pool.is_some());
    let out = serde_json::to_value(&s).unwrap();
    assert_eq!(out["freePool"]["eligibleModels"], json!([]));

    // 把 legacy 当成"有 freePool"来读必红。
    let legacy = quota("quota-summary.legacy.json");
    let read_as_two_pool = legacy.free_pool.as_ref().ok_or("freePool absent");
    assert!(
        read_as_two_pool.is_err(),
        "reading legacy as two-pool must be an error"
    );
}

#[test]
fn catalog_free_pool_eligible_is_tri_state() {
    let two: Vec<ManagedModel> =
        serde_json::from_value(data("managed-models.two-pool.json")).unwrap();
    let want = [
        ("deepseek-flash", true),
        ("ZHIPU/GLM-5.3-Flash", true),
        ("qwen3.8-max", false),
        ("kimi-k3", false),
    ];
    assert_eq!(two.len(), want.len());
    for m in &two {
        let (_, w) = want
            .iter()
            .find(|(id, _)| *id == m.model_id)
            .expect("unexpected model");
        assert_eq!(m.free_pool_eligible, Some(*w), "{}", m.model_id);
    }
    // freeTier 与 freePoolEligible 是两套集合，并存且互不派生；locked 语义不动。
    let by_id = |id: &str| two.iter().find(|m| m.model_id == id).unwrap();
    assert_eq!(by_id("deepseek-flash").free_tier, Some(true));
    assert_eq!(by_id("ZHIPU/GLM-5.3-Flash").free_tier, Some(false));
    assert_eq!(by_id("kimi-k3").locked, Some(true));

    let legacy: Vec<ManagedModel> =
        serde_json::from_value(data("managed-models.legacy.json")).unwrap();
    assert_eq!(legacy.len(), want.len());
    for m in &legacy {
        assert!(
            m.free_pool_eligible.is_none(),
            "{}: legacy free_pool_eligible must be None (not Some(false))",
            m.model_id
        );
        let out = serde_json::to_value(m).unwrap();
        assert!(
            !out.as_object().unwrap().contains_key("freePoolEligible"),
            "{}: None must not be serialized (absent stays absent)",
            m.model_id
        );
    }

    // Some(false) 必须被序列化出来（与缺席区分）。
    let explicit_false = ManagedModel {
        free_pool_eligible: Some(false),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(&explicit_false).unwrap()["freePoolEligible"],
        json!(false)
    );
}
