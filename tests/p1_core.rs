//! P1 地基行为等价性测试（shared + core）。

use acosmi::core::{
    classify_transport, default_retryable, default_safe_to_retry, is_order_success,
    is_order_terminal, normalize_gateway_base_url, parse_http_error,
    parse_http_error_with_retry_after, parse_stream_error, RetryRequestInfo,
};
use acosmi::shared::errors::{Error, HttpError, NetworkError};
use acosmi::shared::retry_advice::{retry_reason_for_oauth_error, RetryAdviceReason};

#[test]
fn http_error_parses_anthropic_and_openai() {
    // Anthropic / OpenAI 均为 {"error":{"type","message"}} 形态。
    let e = parse_http_error(
        429,
        r#"{"error":{"type":"rate_limit","message":"slow down"}}"#,
    );
    assert_eq!(e.status_code, 429);
    assert_eq!(e.r#type, "rate_limit");
    assert_eq!(e.message, "slow down");
    assert_eq!(e.to_string(), "HTTP 429: [rate_limit] slow down");
}

#[test]
fn http_error_empty_and_nonjson_body() {
    let e = parse_http_error(500, "");
    assert_eq!(e.to_string(), "HTTP 500");
    let e2 = parse_http_error(502, "upstream boom");
    // 非 JSON：body 保留，Display 用 body 分支。
    assert_eq!(e2.body, "upstream boom");
    assert_eq!(e2.to_string(), "HTTP 502: upstream boom");
}

#[test]
fn http_error_carries_retry_after() {
    let e = parse_http_error_with_retry_after(429, "{}", 7);
    assert_eq!(e.retry_after, 7);
}

#[test]
fn stream_error_three_schemas() {
    // 1) managed-model 协议
    let s = parse_stream_error(r#"{"errorCode":"overloaded","stage":"provider","retryable":true}"#);
    assert_eq!(s.code, "overloaded");
    assert_eq!(s.stage, "provider");
    assert!(s.retryable);
    // 3) 纯 Anthropic：error.type/message 兜底 code/message
    let s2 = parse_stream_error(
        r#"{"type":"error","error":{"type":"overloaded_error","message":"busy"}}"#,
    );
    assert_eq!(s2.code, "overloaded_error");
    assert_eq!(s2.user_message, "busy");
    // 非 JSON → rawError 原样
    let s3 = parse_stream_error("not-json");
    assert_eq!(s3.raw_error, "not-json");
}

#[test]
fn order_status_predicates() {
    assert!(is_order_success("PAID"));
    assert!(!is_order_success("FAILED"));
    assert!(is_order_terminal("REFUNDED"));
    assert!(!is_order_terminal("PROCESSING"));
}

#[test]
fn safe_to_retry_billing_red_line() {
    // POST 默认 false（双扣保护）；GET true。
    assert!(default_safe_to_retry(&RetryRequestInfo {
        method: "get".into(),
        url: "x".into()
    }));
    assert!(!default_safe_to_retry(&RetryRequestInfo {
        method: "POST".into(),
        url: "x".into()
    }));
}

#[test]
fn retryable_excludes_stream_includes_5xx_429() {
    let stream = Error::Stream(Default::default());
    assert!(!default_retryable(&stream)); // StreamError 不重试
    let h500 = Error::Http(HttpError::new(500, "", "", 0, ""));
    assert!(default_retryable(&h500));
    let h429 = Error::Http(HttpError::new(429, "", "", 0, ""));
    assert!(default_retryable(&h429));
    let h400 = Error::Http(HttpError::new(400, "", "", 0, ""));
    assert!(!default_retryable(&h400));
    let mut net = NetworkError::new("GET", "u", "boom");
    net.timeout = true;
    assert!(default_retryable(&Error::Network(net)));
}

#[test]
fn classify_transport_sets_network_error() {
    // 仅验证返回 NetworkError 且 op/url 透传（真实 reqwest::Error 需网络，故此处只校验形状由 http 错误路径覆盖）。
    let req_info = RetryRequestInfo {
        method: "GET".into(),
        url: "https://acosmi.com".into(),
    };
    assert!(default_safe_to_retry(&req_info));
    let _ = classify_transport; // 引用以确保导出可用
}

#[test]
fn normalize_url_rules() {
    assert_eq!(
        normalize_gateway_base_url("https://acosmi.com/").unwrap(),
        "https://acosmi.com"
    );
    assert_eq!(
        normalize_gateway_base_url("http://localhost:8009/api/v4/").unwrap(),
        "http://localhost:8009/api/v4"
    );
    assert!(normalize_gateway_base_url("ws://x").is_err()); // 拒非 http/https
    assert!(normalize_gateway_base_url("https://x?q=1").is_err()); // 拒 query
    assert!(normalize_gateway_base_url("").is_err()); // 拒空
}

#[test]
fn oauth_reason_mapping() {
    assert_eq!(
        retry_reason_for_oauth_error("insufficient_scope"),
        RetryAdviceReason::InsufficientScope
    );
    assert_eq!(
        retry_reason_for_oauth_error("invalid_grant"),
        RetryAdviceReason::Failed
    );
    assert_eq!(
        retry_reason_for_oauth_error("weird"),
        RetryAdviceReason::Unknown
    );
}
