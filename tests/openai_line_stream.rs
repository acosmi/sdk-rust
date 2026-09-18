//! OpenAI 线 `chat_messages_stream`：usage 尾帧随收尾送达、EOF 收场、网关失败帧分流。
//!
//! 端口自 TS `core/openai-line-stream-usage.test.ts` 的 client 级用例与
//! `core/openai-line-stream-error.test.ts`。EOF 收场只能在驱动层验证：流在没有 `[DONE]` 时结束，
//! 补发推迟收尾的调用点在 `chat_messages_stream` 的读循环结束处，转换器自己看不到 EOF。
use acosmi::*;
use async_trait::async_trait;
use bytes::Bytes;
use futures::{stream, StreamExt};
use http::{header::HeaderMap, StatusCode};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;

const BASE: &str = "https://gateway.invalid";
const MODEL: &str = "openai-line-model";

/// `stream_options.include_usage` 的 usage 尾帧：`[DONE]` 之前、没有 choices，带缓存 / 推理明细。
const USAGE_TAIL_FRAME: &str = r#"{"choices":[],"usage":{"prompt_tokens":13171,"completion_tokens":16,"total_tokens":13187,"completion_tokens_details":{"reasoning_tokens":14},"prompt_tokens_details":{"cached_tokens":13056}}}"#;

/// 网关以 `event: failed` 发出的错误契约帧（取自 TS `core/openai-line-stream-error.test.ts`）。
const GATEWAY_FAILED_FRAME: &str = r#"{"type":"managed_model_stream_failed","protocol":"managed-model.v2","stage":"provider","error":"gateway: tools[0].type:type cannot be empty. (kind=invalid_request, status=400)","errorCode":"invalid_request","errorContractVersion":1,"faultDomain":"provider","message":"","requestDisposition":"unknown","retryable":false,"requestId":"req-0001","consumeRequestId":"req-0001","providerRequestId":"prov-0001","transportRequestId":"trans-0001"}"#;

/// 按序返回预置响应：第一个是模型目录（把模型路由到 OpenAI 线），第二个是流式响应体。
struct Scripted(Mutex<VecDeque<HttpResponse>>);

#[async_trait]
impl HttpTransport for Scripted {
    async fn execute(
        &self,
        _request: HttpRequest,
        _cancel: CancellationToken,
    ) -> std::result::Result<HttpResponse, TransportError> {
        self.0
            .lock()
            .unwrap()
            .pop_front()
            .ok_or(TransportError::Rejected)
    }
}

fn ok(body: String) -> HttpResponse {
    HttpResponse {
        status: StatusCode::OK,
        headers: HeaderMap::new(),
        body: Box::pin(stream::iter([Ok(Bytes::from(body))])),
    }
}

/// 已登录、目录里唯一的模型走 OpenAI 线的 client；流式请求拿到 `stream_body`。
async fn openai_line_client(stream_body: String) -> Client {
    let model = ManagedModel {
        id: MODEL.into(),
        name: "OpenAI line".into(),
        provider: "openai".into(),
        supported_formats: Some(vec!["openai".into()]),
        preferred_format: Some("openai".into()),
        ..Default::default()
    };
    let catalog = json!({ "code": 0, "data": [model] }).to_string();
    let store = Arc::new(InMemoryTokenStore::new());
    store
        .save(&TokenSet {
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            expires_at: "2099-01-01T00:00:00Z".into(),
            scope: "ai".into(),
            client_id: "client".into(),
            server_url: BASE.into(),
        })
        .await
        .unwrap();
    Client::create_with_transport(
        Config {
            server_url: Some(BASE.into()),
            store: Some(store),
            ..Default::default()
        },
        Arc::new(Scripted(Mutex::new(VecDeque::from([
            ok(catalog),
            ok(stream_body),
        ])))),
    )
    .await
    .unwrap()
}

async fn run(stream_body: String) -> Vec<acosmi::Result<StreamEvent>> {
    let client = openai_line_client(stream_body).await;
    client
        .chat_messages_stream(MODEL, &ChatRequest::default(), None)
        .collect()
        .await
}

fn sse(frames: &[&str]) -> String {
    frames
        .iter()
        .map(|frame| format!("data: {frame}\n\n"))
        .collect()
}

fn content_chunk(text: &str) -> String {
    json!({
        "id": "c1",
        "object": "chat.completion.chunk",
        "choices": [{ "index": 0, "delta": { "content": text }, "finish_reason": null }],
    })
    .to_string()
}

fn finish_chunk(reason: &str) -> String {
    json!({
        "id": "c1",
        "object": "chat.completion.chunk",
        "choices": [{ "index": 0, "delta": {}, "finish_reason": reason }],
    })
    .to_string()
}

/// 工具调用首片：带 id + name，`arguments` 为空串（OpenAI 规范形态）。
fn tool_first_chunk(id: &str, name: &str) -> String {
    json!({
        "id": "c1",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": { "tool_calls": [{
                "index": 0,
                "id": id,
                "type": "function",
                "function": { "name": name, "arguments": "" },
            }] },
            "finish_reason": null,
        }],
    })
    .to_string()
}

/// 工具调用续片：**只有** index 与 arguments 分片，没有 id / type / name —— OpenAI 规范里每一次
/// 工具调用从第二帧起都是这个形态。
fn tool_arg_chunk(args: &str) -> String {
    json!({
        "id": "c1",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": { "tool_calls": [{ "index": 0, "function": { "arguments": args } }] },
            "finish_reason": null,
        }],
    })
    .to_string()
}

/// 把全部事件解析为 `(事件名, data JSON)`；任何一项是错误即失败。
fn parse_ok(items: &[acosmi::Result<StreamEvent>]) -> Vec<(String, Value)> {
    items
        .iter()
        .map(|item| {
            let ev = item.as_ref().expect("unexpected stream error");
            (ev.event.clone(), serde_json::from_str(&ev.data).unwrap())
        })
        .collect()
}

fn event_names(events: &[(String, Value)]) -> Vec<&str> {
    events.iter().map(|(name, _)| name.as_str()).collect()
}

/// 整条流恰好一个 message_delta 与一个 message_stop，message_stop 收尾且 message_delta 紧挨在
/// 它之前。返回那个 message_delta 的 JSON。
fn expect_single_close_at_end(all: &[(String, Value)]) -> &Value {
    let names = event_names(all);
    assert_eq!(
        names.iter().filter(|n| **n == "message_delta").count(),
        1,
        "{names:?}"
    );
    assert_eq!(
        names.iter().filter(|n| **n == "message_stop").count(),
        1,
        "{names:?}"
    );
    let stop_at = names.iter().position(|n| *n == "message_stop").unwrap();
    assert_eq!(stop_at, names.len() - 1, "{names:?}");
    assert_eq!(names[stop_at - 1], "message_delta", "{names:?}");
    &all[stop_at - 1].1
}

#[tokio::test]
async fn spec_toolcall_stream_reaches_caller_intact() {
    // 端到端：标准 OpenAI 工具调用流（续片不带 name），整条流必须**零 Err** 地走到 message_stop。
    // 此前 `OpenAIFunctionCall.name` 必填，第二帧就 `missing field \`name\``，流当场终止、参数一
    // 字节不落。
    let items = run(sse(&[
        &tool_first_chunk("call_1", "get_weather"),
        &tool_arg_chunk(r#"{"city":"#),
        &tool_arg_chunk(r#" "SF"}"#),
        &finish_chunk("tool_calls"),
        "[DONE]",
    ]))
    .await;
    let errors: Vec<&acosmi::Error> = items.iter().filter_map(|i| i.as_ref().err()).collect();
    assert!(errors.is_empty(), "{errors:?}");

    let all = parse_ok(&items);
    assert_eq!(
        event_names(&all),
        [
            "message_start",
            "content_block_start",
            "content_block_delta",
            "content_block_delta",
            "content_block_stop",
            "message_delta",
            "message_stop"
        ]
    );
    let args: String = all
        .iter()
        .filter(|(name, _)| name == "content_block_delta")
        .map(|(_, p)| p["delta"]["partial_json"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(args, r#"{"city": "SF"}"#);
    let delta = expect_single_close_at_end(&all);
    assert_eq!(delta["delta"], json!({ "stop_reason": "tool_use" }));
}

#[tokio::test]
async fn usage_tail_frame_reaches_caller_in_closing_message_delta() {
    let items = run(sse(&[
        &content_chunk("PONG"),
        &finish_chunk("stop"),
        USAGE_TAIL_FRAME,
        "[DONE]",
    ]))
    .await;
    let all = parse_ok(&items);
    let delta = expect_single_close_at_end(&all);
    assert_eq!(delta["delta"], json!({ "stop_reason": "end_turn" }));
    assert_eq!(
        delta["usage"],
        json!({ "input_tokens": 13171, "output_tokens": 16 })
    );
}

#[tokio::test]
async fn eof_without_done_emits_deferred_close_exactly_once() {
    let items = run(sse(&[&content_chunk("PONG"), &finish_chunk("stop")])).await;
    let all = parse_ok(&items);
    let delta = expect_single_close_at_end(&all);
    assert_eq!(delta["delta"], json!({ "stop_reason": "end_turn" }));
    assert!(delta.get("usage").is_none());
}

#[tokio::test]
async fn eof_after_usage_tail_close_does_not_emit_second_stop() {
    let items = run(sse(&[
        &content_chunk("PONG"),
        &finish_chunk("stop"),
        USAGE_TAIL_FRAME,
    ]))
    .await;
    let all = parse_ok(&items);
    let delta = expect_single_close_at_end(&all);
    assert_eq!(
        delta["usage"],
        json!({ "input_tokens": 13171, "output_tokens": 16 })
    );
}

#[tokio::test]
async fn truncated_stream_without_finish_reason_is_not_faked_into_normal_end() {
    let items = run(sse(&[&content_chunk("PART")])).await;
    // 正向对照与断言合一：事件确实流过，且其中没有 message_delta / message_stop。
    assert_eq!(
        event_names(&parse_ok(&items)),
        [
            "message_start",
            "content_block_start",
            "content_block_delta"
        ]
    );
}

#[tokio::test]
async fn gateway_envelope_events_do_not_break_the_openai_line() {
    // 网关成功流的外形：`started` 事件在首个上游帧之前，`settled` 事件在 `[DONE]` 之前，两者的
    // data 都没有 choices。
    let started = json!({
        "type": "managed_model_stream_started",
        "protocol": "managed-model.v2",
        "requestId": "req-0001",
    });
    let settled = json!({
        "type": "managed_model_stream_settled",
        "protocol": "managed-model.v2",
        "requestId": "req-0001",
        "inputTokens": 13171,
        "outputTokens": 16,
        "totalTokens": 13187,
    });
    let body = format!(
        "event: started\ndata: {started}\n\n{}event: settled\ndata: {settled}\n\ndata: [DONE]\n\n",
        sse(&[
            &content_chunk("PONG"),
            &finish_chunk("stop"),
            USAGE_TAIL_FRAME
        ]),
    );
    let items = run(body).await;
    let all = parse_ok(&items);
    let delta = expect_single_close_at_end(&all);
    assert_eq!(
        delta["usage"],
        json!({ "input_tokens": 13171, "output_tokens": 16 })
    );
}

#[tokio::test]
async fn gateway_failed_event_surfaces_as_stream_error() {
    let items = run(format!("event: failed\ndata: {GATEWAY_FAILED_FRAME}\n\n")).await;
    assert_eq!(items.len(), 1, "{items:?}");
    match &items[0] {
        Err(Error::Stream(err)) => {
            assert_eq!(err.code, "invalid_request");
            // 承重：真实的上游错误必须抵达调用方。
            assert!(err.raw_error.contains("tools[0].type"), "{err:?}");
        }
        other => panic!("expected Error::Stream, got {other:?}"),
    }
}

#[tokio::test]
async fn gateway_failed_after_finish_reason_is_not_turned_into_normal_end() {
    // finish_reason 之后网关结算失败：失败必须以错误抵达调用方，而不是由 EOF 处的 flush 补发一个
    // 看似正常的 message_stop。
    let failed = json!({
        "type": "managed_model_stream_failed",
        "protocol": "managed-model.v2",
        "requestId": "req-0001",
        "stage": "settlement",
        "error": "结算失败且未能建立补偿追踪",
    });
    let body = format!(
        "{}event: failed\ndata: {failed}\n\n",
        sse(&[&content_chunk("PONG"), &finish_chunk("stop")])
    );
    let items = run(body).await;
    let (last, delivered) = items.split_last().unwrap();
    match last {
        Err(Error::Stream(err)) => assert_eq!(err.stage, "settlement"),
        other => panic!("expected Error::Stream, got {other:?}"),
    }
    // 正向对照：失败之前的内容事件照常送达（finish_reason 帧上已关块），其中没有收尾事件。
    assert_eq!(
        event_names(&parse_ok(delivered)),
        [
            "message_start",
            "content_block_start",
            "content_block_delta",
            "content_block_stop"
        ]
    );
}
