//! Rust integration: exclusive transport, cancellation and observers.
use acosmi::*;
use async_trait::async_trait;
use bytes::Bytes;
use futures::{stream, StreamExt};
use reqwest::{header::HeaderMap, StatusCode};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const BASE: &str = "https://gateway.invalid";
const CANARY: &str = "SECRET_CANARY_DO_NOT_LOG";

struct Mock {
    responses: Mutex<VecDeque<HttpResponse>>,
    requests: Mutex<Vec<HttpRequest>>,
    cancels: Mutex<Vec<CancellationToken>>,
}
#[async_trait]
impl HttpTransport for Mock {
    async fn execute(
        &self,
        request: HttpRequest,
        cancel: CancellationToken,
    ) -> std::result::Result<HttpResponse, TransportError> {
        assert!(!cancel.is_cancelled());
        self.requests.lock().unwrap().push(request);
        self.cancels.lock().unwrap().push(cancel);
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or(TransportError::Rejected)
    }
}
fn response(status: u16, body: impl Into<String>, id: Option<&str>) -> HttpResponse {
    let mut headers = HeaderMap::new();
    if let Some(id) = id {
        headers.insert(GATEWAY_REQUEST_ID_HEADER, id.parse().unwrap());
    }
    headers.insert("X-Request-ID", "transport-only".parse().unwrap());
    HttpResponse {
        status: StatusCode::from_u16(status).unwrap(),
        headers,
        body: Box::pin(stream::iter([Ok(Bytes::from(body.into()))])),
    }
}
fn model(format: &str) -> ManagedModel {
    ManagedModel {
        id: "model /1".into(),
        name: "Test".into(),
        provider: format.into(),
        supported_formats: Some(vec![format.into()]),
        preferred_format: Some(format.into()),
        ..Default::default()
    }
}
fn catalog(models: Vec<ManagedModel>) -> HttpResponse {
    response(
        200,
        serde_json::json!({"code":0,"data":models}).to_string(),
        None,
    )
}
fn tokens(expired: bool) -> TokenSet {
    TokenSet {
        access_token: CANARY.into(),
        refresh_token: format!("refresh-{CANARY}"),
        expires_at: if expired {
            "2000-01-01T00:00:00Z"
        } else {
            "2099-01-01T00:00:00Z"
        }
        .into(),
        scope: "ai".into(),
        client_id: "client".into(),
        server_url: BASE.into(),
    }
}
fn metadata() -> ServerMetadata {
    ServerMetadata {
        issuer: BASE.into(),
        authorization_endpoint: format!("{BASE}/authorize"),
        token_endpoint: format!("{BASE}/token"),
        registration_endpoint: format!("{BASE}/register"),
        revocation_endpoint: format!("{BASE}/revoke"),
        scopes_supported: vec![],
    }
}
fn discovery() -> HttpResponse {
    response(200, serde_json::to_string(&metadata()).unwrap(), None)
}
fn refreshed() -> HttpResponse {
    response(
        200,
        r#"{"access_token":"rotated","refresh_token":"rotated-refresh","token_type":"Bearer","expires_in":3600}"#,
        None,
    )
}
async fn client(responses: Vec<HttpResponse>, expired: bool) -> (Client, Arc<Mock>) {
    let mock = Arc::new(Mock {
        responses: Mutex::new(responses.into()),
        requests: Mutex::new(vec![]),
        cancels: Mutex::new(vec![]),
    });
    let store = Arc::new(InMemoryTokenStore::new());
    store.save(&tokens(expired)).await.unwrap();
    let client = Client::create_with_transport(
        Config {
            server_url: Some(BASE.into()),
            store: Some(store),
            ..Default::default()
        },
        mock.clone(),
    )
    .await
    .unwrap();
    (client, mock)
}
fn options(log: &Arc<Mutex<Vec<String>>>) -> ChatOptions {
    let activity = log.clone();
    let ids = log.clone();
    ChatOptions {
        on_upstream_activity: Some(Arc::new(move || {
            activity.lock().unwrap().push("activity".into())
        })),
        on_gateway_request_id: Some(Arc::new(move |id| ids.lock().unwrap().push(id.into()))),
    }
}

#[tokio::test]
async fn thinking_levels_preserve_missing_empty_unknown_and_cache() {
    let mut models = vec![model("anthropic"); 3];
    models[0].id = "missing".into();
    models[1].id = "empty".into();
    models[2].id = "unknown".into();
    models[1].thinking_levels = Some(vec![]);
    models[2].thinking_levels = Some(vec!["off".into(), "future-level".into()]);
    models[2].input_modalities = Some(vec![InputModality::Image]);
    let (c, m) = client(vec![catalog(models.clone()), catalog(models)], false).await;
    let listed = c.list_models(None, false).await.unwrap();
    assert!(listed[0].thinking_levels.is_none());
    assert_eq!(listed[1].thinking_levels, Some(vec![]));
    assert_eq!(
        listed[2].thinking_levels.as_ref().unwrap(),
        &["off", "future-level"]
    );
    assert_eq!(listed[2].input_modalities, Some(vec![InputModality::Image]));
    let (again, _) = c.list_models_with_status(None, false).await.unwrap();
    assert_eq!(again[2].thinking_levels, listed[2].thinking_levels);
    c.get_model_capabilities("unknown", None).await.unwrap();
    assert_eq!(m.requests.lock().unwrap().len(), 2);
    for request in m.requests.lock().unwrap().iter() {
        assert_eq!(request.method, "GET");
        assert_eq!(request.url.path(), "/api/v4/managed-models");
        assert_eq!(request.context.response_mode, HttpResponseMode::Buffered);
        assert_eq!(request.context.timeout, Duration::from_secs(30));
    }
}

#[tokio::test]
async fn both_stream_methods_both_wires_observe_keepalive_before_first_event() {
    for format in ["anthropic", "openai"] {
        for messages in [false, true] {
            let log = Arc::new(Mutex::new(vec![]));
            let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(4);
            let body = stream::unfold(rx, |mut rx| async { rx.recv().await.map(|b| (Ok(b), rx)) });
            let mut resp = response(200, "", Some("billing-123"));
            resp.body = Box::pin(body);
            let (c, m) = client(vec![catalog(vec![model(format)]), resp], false).await;
            let req = ChatRequest::default();
            let mut stream: std::pin::Pin<
                Box<dyn futures::Stream<Item = acosmi::Result<StreamEvent>>>,
            > = if messages {
                Box::pin(c.chat_messages_stream_with_options("model /1", &req, None, options(&log)))
            } else {
                Box::pin(c.chat_stream_with_options("model /1", &req, None, options(&log)))
            };
            tx.send(Bytes::from_static(b": first\n: second\n: third\n"))
                .await
                .unwrap();
            assert!(
                tokio::time::timeout(Duration::from_millis(20), stream.next())
                    .await
                    .is_err()
            );
            assert_eq!(
                *log.lock().unwrap(),
                ["billing-123", "activity", "activity", "activity"]
            );
            if messages && format == "openai" {
                tx.send(Bytes::from_static(b"data: {\"id\":\"content-only\",\"object\":\"chat.completion.chunk\",\"choices\":[]}\n\n")).await.unwrap();
                assert!(
                    tokio::time::timeout(Duration::from_millis(20), stream.next())
                        .await
                        .is_err()
                );
                assert_eq!(log.lock().unwrap().len(), 6);
            }
            let event = if format == "anthropic" {
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n"
            } else {
                "data: {\"id\":\"content-only\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n\n"
            };
            tx.send(Bytes::from(event)).await.unwrap();
            drop(tx);
            assert!(stream.next().await.unwrap().is_ok());
            drop(stream);
            let requests = m.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            let req = &requests[1];
            assert_eq!(req.method, "POST");
            assert_eq!(
                req.url.path(),
                if format == "anthropic" {
                    "/api/v4/managed-models/model%20%2F1/anthropic"
                } else {
                    "/api/v4/managed-models/model%20%2F1/chat"
                }
            );
            assert_eq!(req.context.purpose, HttpPurpose::Model);
            assert_eq!(req.context.response_mode, HttpResponseMode::Streaming);
            assert_eq!(req.context.timeout, Duration::from_secs(660));
            assert!(m.cancels.lock().unwrap()[1].is_cancelled());
        }
    }
}

#[tokio::test]
async fn refresh_is_once_and_id_belongs_only_to_final_response_even_on_error() {
    for format in ["anthropic", "openai"] {
        for messages in [false, true] {
            let (c, m) = client(
                vec![
                    catalog(vec![model(format)]),
                    response(401, "{}", Some("discarded-401")),
                    discovery(),
                    refreshed(),
                    response(401, "{}", Some("final-billing-id")),
                ],
                false,
            )
            .await;
            let log = Arc::new(Mutex::new(vec![]));
            let req = ChatRequest::default();
            let events: Vec<_> = if messages {
                c.chat_messages_stream_with_options("model /1", &req, None, options(&log))
                    .collect()
                    .await
            } else {
                c.chat_stream_with_options("model /1", &req, None, options(&log))
                    .collect()
                    .await
            };
            assert_eq!(events.len(), 1);
            assert!(events[0].is_err());
            assert_eq!(*log.lock().unwrap(), ["final-billing-id"]);
            let requests = m.requests.lock().unwrap();
            assert_eq!(requests.len(), 5);
            assert_eq!(requests[2].context.purpose, HttpPurpose::OAuthDiscovery);
            assert_eq!(requests[3].context.purpose, HttpPurpose::OAuthToken);
            assert_eq!(requests[3].context.timeout, Duration::from_secs(30));
            assert_eq!(requests[4].headers["authorization"], "Bearer rotated");
        }
    }
}

#[tokio::test]
async fn buffered_model_observer_gets_error_headers_and_explicit_budget_without_retry() {
    for messages in [false, true] {
        for format in ["anthropic", "openai"] {
            let (c, m) = client(
                vec![
                    catalog(vec![model(format)]),
                    response(500, "{}", Some("billing-error")),
                ],
                false,
            )
            .await;
            let log = Arc::new(Mutex::new(vec![]));
            if messages {
                assert!(c
                    .chat_messages_with_options(
                        "model /1",
                        &ChatRequest::default(),
                        None,
                        options(&log)
                    )
                    .await
                    .is_err());
            } else {
                assert!(c
                    .chat_with_options("model /1", &ChatRequest::default(), None, options(&log))
                    .await
                    .is_err());
            }
            assert_eq!(*log.lock().unwrap(), ["billing-error"]);
            let requests = m.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(
                requests[1].context.response_mode,
                HttpResponseMode::Buffered
            );
            assert_eq!(requests[1].context.timeout, Duration::from_secs(660));
        }
    }
}

#[tokio::test]
async fn callback_panic_is_contained_and_disabled() {
    let n = Arc::new(AtomicUsize::new(0));
    let called = n.clone();
    let (c, _) = client(
        vec![
            catalog(vec![model("anthropic")]),
            response(200, ":a\n:b\ndata: {}\n", Some("id")),
        ],
        false,
    )
    .await;
    let options = ChatOptions {
        on_upstream_activity: Some(Arc::new(move || {
            called.fetch_add(1, Ordering::SeqCst);
            panic!("observer failure")
        })),
        on_gateway_request_id: Some(Arc::new(|_| panic!("observer failure"))),
    };
    let events: Vec<_> = c
        .chat_stream_with_options("model /1", &ChatRequest::default(), None, options)
        .collect()
        .await;
    assert_eq!(events.len(), 1);
    assert!(events[0].is_ok());
    assert_eq!(n.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancel_interrupts_body_and_refresh_mutex_wait_and_drops_transport() {
    let mut hanging = refreshed();
    hanging.body = Box::pin(stream::pending());
    let (c, m) = client(vec![discovery(), hanging], true).await;
    let cancel = CancellationToken::new();
    let first = c.ensure_token(Some(cancel.clone()));
    tokio::pin!(first);
    assert!(tokio::time::timeout(Duration::from_millis(20), &mut first)
        .await
        .is_err());
    let waiting_cancel = CancellationToken::new();
    let second = c.force_refresh(Some(waiting_cancel.clone()));
    tokio::pin!(second);
    assert!(tokio::time::timeout(Duration::from_millis(20), &mut second)
        .await
        .is_err());
    waiting_cancel.cancel();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut second)
            .await
            .unwrap()
            .is_err()
    );
    cancel.cancel();
    assert!(tokio::time::timeout(Duration::from_millis(100), &mut first)
        .await
        .unwrap()
        .is_err());
    assert_eq!(m.requests.lock().unwrap().len(), 2);
    assert!(m
        .cancels
        .lock()
        .unwrap()
        .iter()
        .all(CancellationToken::is_cancelled));
}

#[tokio::test]
async fn custom_redirect_is_not_followed_and_unsupported_paths_do_not_connect() {
    let mut redirect = response(302, "", None);
    redirect
        .headers
        .insert("location", "https://other.invalid/secret".parse().unwrap());
    let (c, m) = client(vec![redirect], false).await;
    assert!(c.list_models(None, false).await.is_err());
    assert_eq!(m.requests.lock().unwrap().len(), 1);
    let e = c.connect(Default::default(), None).await.unwrap_err();
    assert!(e.to_string().contains("unsupported_websocket"));
    assert!(c
        .upload_skill(vec![], "private", "install", None)
        .await
        .is_err());
    assert_eq!(m.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn oauth_free_functions_use_same_transport_and_preserve_form_contract() {
    let (_, m) = client(
        vec![
            discovery(),
            response(201, r#"{"client_id":"registered"}"#, None),
            refreshed(),
            refreshed(),
            response(200, "", None),
        ],
        false,
    )
    .await;
    let http = HttpClient::new(m.clone());
    let meta = discover_with_transport(&http, BASE).await.unwrap();
    register_with_transport(&http, &meta, "test").await.unwrap();
    exchange_code_with_transport(
        &http,
        &meta,
        "client",
        "code",
        "http://127.0.0.1/callback",
        "verifier",
    )
    .await
    .unwrap();
    refresh_token_with_transport(&http, &meta, "client", CANARY)
        .await
        .unwrap();
    revoke_token_with_transport(&http, &meta, CANARY)
        .await
        .unwrap();
    let r = m.requests.lock().unwrap();
    assert_eq!(r.len(), 5);
    assert_eq!(
        r.iter().map(|r| r.context.purpose).collect::<Vec<_>>(),
        [
            HttpPurpose::OAuthDiscovery,
            HttpPurpose::OAuthRegistration,
            HttpPurpose::OAuthToken,
            HttpPurpose::OAuthToken,
            HttpPurpose::OAuthRevocation
        ]
    );
    assert_eq!(r[1].url.path(), "/register");
    assert_eq!(r[2].url.path(), "/token");
    assert_eq!(r[4].url.path(), "/revoke");
    assert!(String::from_utf8_lossy(&r[2].body).contains("grant_type=authorization_code"));
    assert!(String::from_utf8_lossy(&r[3].body).contains("grant_type=refresh_token"));
    for req in r.iter() {
        assert_eq!(req.context.timeout, Duration::from_secs(30));
        assert!(!format!("{req:?}").contains(CANARY));
    }
}

#[test]
fn billing_id_rejects_absence_duplicates_bad_values_and_other_id_sources() {
    let mut headers = HeaderMap::new();
    headers.insert("X-Request-ID", "transport-only".parse().unwrap());
    assert_eq!(read_gateway_request_id(&headers), None);
    for raw in ["", " ", "id id", "a,b", "a/b", &"x".repeat(257)] {
        headers.insert(GATEWAY_REQUEST_ID_HEADER, raw.parse().unwrap());
        assert_eq!(read_gateway_request_id(&headers), None);
    }
    headers.insert(
        GATEWAY_REQUEST_ID_HEADER,
        " bill_123-abc.4:5 ".parse().unwrap(),
    );
    assert_eq!(read_gateway_request_id(&headers), Some("bill_123-abc.4:5"));
    headers.append(GATEWAY_REQUEST_ID_HEADER, "second".parse().unwrap());
    assert_eq!(read_gateway_request_id(&headers), None);
}

#[test]
fn secret_debug_and_oauth_error_display_are_redacted() {
    let tr = TokenResponse {
        access_token: CANARY.into(),
        refresh_token: Some(CANARY.into()),
        token_type: "Bearer".into(),
        expires_in: 1,
        scope: None,
    };
    let reg = ClientRegistration {
        client_id: "client".into(),
        client_secret: Some(CANARY.into()),
    };
    let err = OAuthTokenEndpointError {
        status: 400,
        oauth_error: CANARY.into(),
        error_description: CANARY.into(),
    };
    for output in [
        format!("{:?}", tokens(false)),
        format!("{tr:?}"),
        format!("{reg:?}"),
        format!("{err:?}"),
        err.to_string(),
    ] {
        assert!(!output.contains(CANARY));
    }
}

#[tokio::test]
async fn refreshed_success_keeps_observers_and_legacy_events_equal() {
    for messages in [false, true] {
        for format in ["anthropic", "openai"] {
            let body = if format == "anthropic" {
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n"
            } else {
                "data: {\"id\":\"body-id\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"}}]}\ndata: [DONE]\n"
            };
            let (c, m) = client(
                vec![
                    catalog(vec![model(format)]),
                    response(401, "{}", Some("ignored")),
                    discovery(),
                    refreshed(),
                    response(200, body, Some("success-id")),
                    response(200, body, None),
                ],
                false,
            )
            .await;
            let log = Arc::new(Mutex::new(vec![]));
            let req = ChatRequest::default();
            let observed: Vec<_> = if messages {
                c.chat_messages_stream_with_options("model /1", &req, None, options(&log))
                    .collect()
                    .await
            } else {
                c.chat_stream_with_options("model /1", &req, None, options(&log))
                    .collect()
                    .await
            };
            let legacy: Vec<_> = if messages {
                c.chat_messages_stream("model /1", &req, None)
                    .collect()
                    .await
            } else {
                c.chat_stream("model /1", &req, None).collect().await
            };
            let project = |v: Vec<acosmi::Result<StreamEvent>>| {
                v.into_iter()
                    .map(|e| {
                        let e = e.unwrap();
                        (e.event, e.data)
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(project(observed), project(legacy));
            assert_eq!(log.lock().unwrap()[0], "success-id");
            assert!(log.lock().unwrap().len() > 1);
            assert_eq!(m.requests.lock().unwrap().len(), 6);
        }
    }
}

#[tokio::test]
async fn absent_and_invalid_billing_header_never_calls_observer() {
    for id in [None, Some(""), Some(" "), Some("invalid id")] {
        let (c, _) = client(
            vec![catalog(vec![model("anthropic")]), response(500, "{}", id)],
            false,
        )
        .await;
        let log = Arc::new(Mutex::new(vec![]));
        let _: Vec<_> = c
            .chat_stream_with_options("model /1", &ChatRequest::default(), None, options(&log))
            .collect()
            .await;
        assert!(log.lock().unwrap().is_empty());
    }
}

struct LockedStore;
#[async_trait]
impl TokenStore for LockedStore {
    async fn save(&self, _: &TokenSet) -> acosmi::Result<()> {
        Ok(())
    }
    async fn load(&self) -> acosmi::Result<Option<TokenSet>> {
        Ok(Some(tokens(true)))
    }
    async fn clear(&self) -> acosmi::Result<()> {
        Ok(())
    }
    async fn lock(&self) -> acosmi::Result<Box<dyn Send>> {
        futures::future::pending().await
    }
}
#[tokio::test]
async fn cancellation_interrupts_token_store_lock_without_network_request() {
    let (_, m) = client(vec![], true).await;
    let c = Client::create_with_transport(
        Config {
            server_url: Some(BASE.into()),
            store: Some(Arc::new(LockedStore)),
            ..Default::default()
        },
        m.clone(),
    )
    .await
    .unwrap();
    let cancel = CancellationToken::new();
    let operation = c.ensure_token(Some(cancel.clone()));
    tokio::pin!(operation);
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut operation)
            .await
            .is_err()
    );
    cancel.cancel();
    assert!(tokio::time::timeout(Duration::from_millis(100), operation)
        .await
        .unwrap()
        .is_err());
    assert!(m.requests.lock().unwrap().is_empty());
}

struct SocketTransport(std::net::SocketAddr);
#[async_trait]
impl HttpTransport for SocketTransport {
    async fn execute(
        &self,
        request: HttpRequest,
        _: CancellationToken,
    ) -> std::result::Result<HttpResponse, TransportError> {
        if request.context.purpose == HttpPurpose::Api {
            return Ok(catalog(vec![model("anthropic")]));
        }
        let socket = tokio::net::TcpStream::connect(self.0)
            .await
            .map_err(|_| TransportError::Connection)?;
        let body = stream::unfold(socket, |mut socket| async {
            use tokio::io::AsyncReadExt;
            let mut bytes = vec![0; 128];
            match socket.read(&mut bytes).await {
                Ok(0) => None,
                Ok(n) => {
                    bytes.truncate(n);
                    Some((Ok(Bytes::from(bytes)), socket))
                }
                Err(_) => Some((Err(TransportError::Body), socket)),
            }
        });
        Ok(HttpResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: Box::pin(body),
        })
    }
}
#[tokio::test]
async fn dropping_sdk_stream_closes_custom_transport_socket() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        socket.write_all(b": keepalive\n").await.unwrap();
        let mut byte = [0];
        // A dropped transport body must close the real peer, not just cancel an SDK flag.
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), socket.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    });
    let store = Arc::new(InMemoryTokenStore::new());
    store.save(&tokens(false)).await.unwrap();
    let c = Client::create_with_transport(
        Config {
            server_url: Some(BASE.into()),
            store: Some(store),
            ..Default::default()
        },
        Arc::new(SocketTransport(address)),
    )
    .await
    .unwrap();
    let req = ChatRequest::default();
    let observed = Arc::new(tokio::sync::Notify::new());
    let signal = observed.clone();
    let mut stream = Box::pin(c.chat_stream_with_options(
        "model /1",
        &req,
        None,
        ChatOptions {
            on_upstream_activity: Some(Arc::new(move || signal.notify_one())),
            ..Default::default()
        },
    ));
    tokio::select! {
        _ = observed.notified() => {},
        event = stream.next() => panic!("unexpected content: {event:?}"),
        _ = tokio::time::sleep(Duration::from_secs(2)) => panic!("custom socket did not deliver keepalive"),
    }
    drop(stream);
    peer.await.unwrap();
}
