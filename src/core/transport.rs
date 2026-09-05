//! HTTP injection without SDK-owned connections or redirect handling.
use crate::shared::errors::{Error, NetworkError, Result};
use async_trait::async_trait;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use http::{header::HeaderMap, Method, StatusCode};
use serde::{de::DeserializeOwned, Serialize};
use std::{fmt, pin::Pin, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use url::Url;

/// Diagnostic metadata, never an authorization grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpPurpose {
    Api,
    Model,
    OAuthDiscovery,
    OAuthRegistration,
    OAuthToken,
    OAuthRevocation,
    Transfer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpResponseMode {
    Buffered,
    Streaming,
}

/// Buffered timeout covers headers and body; streaming timeout covers headers
/// only. Streaming body lifetime is controlled by cancellation, never this timer.
#[derive(Debug, Clone, Copy)]
pub struct HttpContext {
    pub purpose: HttpPurpose,
    pub response_mode: HttpResponseMode,
    pub timeout: Duration,
}
impl HttpContext {
    pub fn buffered(purpose: HttpPurpose, timeout_ms: u64) -> Self {
        Self {
            purpose,
            response_mode: HttpResponseMode::Buffered,
            timeout: Duration::from_millis(timeout_ms),
        }
    }
    pub fn streaming(purpose: HttpPurpose, headers_timeout_ms: u64) -> Self {
        Self {
            purpose,
            response_mode: HttpResponseMode::Streaming,
            timeout: Duration::from_millis(headers_timeout_ms),
        }
    }
}
impl Default for HttpContext {
    fn default() -> Self {
        Self::buffered(HttpPurpose::Api, super::http::DEFAULT_JSON_TIMEOUT_MS)
    }
}

/// Owned request, including potentially sensitive headers and JSON/form body.
/// The transport owns redirect authorization at every hop, credential stripping,
/// DNS/TLS/connection policy and any stricter budgets. It MUST NOT silently retry
/// non-idempotent requests. A returned redirect is not followed by the SDK.
/// Body bytes owned by this value are zeroized on drop. Taking ownership of the
/// bytes transfers that responsibility to the transport. HeaderMap allocations
/// and copies made by HTTP libraries cannot be guaranteed zeroized.
pub struct HttpRequest {
    pub method: Method,
    pub url: Url,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub context: HttpContext,
}
impl Drop for HttpRequest {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.body);
    }
}
impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpRequest")
            .field("context", &self.context)
            .finish_non_exhaustive()
    }
}

/// Closed, payload-free transport failures. Do not put backend error strings,
/// request URLs, response bytes, credentials or panic payloads into errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    #[error("http_transport_cancelled")]
    Cancelled,
    #[error("http_transport_timeout")]
    Timeout,
    #[error("http_transport_connection_failed")]
    Connection,
    #[error("http_transport_rejected")]
    Rejected,
    #[error("http_transport_invalid_request")]
    InvalidRequest,
    #[error("http_transport_body_failed")]
    Body,
    #[error("http_transport_unsupported_body")]
    UnsupportedBody,
    #[error("http_transport_unsupported_websocket")]
    UnsupportedWebSocket,
}
impl From<TransportError> for Error {
    fn from(value: TransportError) -> Self {
        let mut error = NetworkError::new("HTTP transport", "", value.to_string());
        error.timeout = value == TransportError::Timeout;
        error.eof = value == TransportError::Connection;
        Error::Network(error)
    }
}
pub type HttpBody = Pin<Box<dyn Stream<Item = std::result::Result<Bytes, TransportError>> + Send>>;

/// Headers arrive before any body polling. Dropping the body must release the
/// transport's connection/driver; implementations must also honor cancellation.
pub struct HttpResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: HttpBody,
}
impl fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpResponse")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

#[async_trait]
pub trait HttpTransport: Send + Sync {
    async fn execute(
        &self,
        request: HttpRequest,
        cancel: CancellationToken,
    ) -> std::result::Result<HttpResponse, TransportError>;
}

/// HTTP handle for the `*_with_transport` OAuth free functions. Clones share the
/// transport. This handle never falls back to direct networking in custom mode.
#[derive(Clone)]
enum Backend {
    Custom(Arc<dyn HttpTransport>),
    #[cfg(feature = "native-http")]
    Native(reqwest::Client),
}

#[derive(Clone)]
pub struct HttpClient {
    backend: Backend,
    signal: Option<CancellationToken>,
}
impl HttpClient {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            backend: Backend::Custom(transport),
            signal: None,
        }
    }
    pub fn with_cancellation(&self, signal: Option<CancellationToken>) -> Self {
        Self {
            signal,
            ..self.clone()
        }
    }
    #[cfg(feature = "native-http")]
    pub(crate) fn legacy(builder: reqwest::Client) -> Self {
        Self {
            backend: Backend::Native(builder),
            signal: None,
        }
    }
    #[cfg(feature = "notifications-ws")]
    pub(crate) fn is_custom(&self) -> bool {
        matches!(self.backend, Backend::Custom(_))
    }
    pub(crate) fn request(&self, method: Method, url: &str) -> HttpRequestBuilder {
        let inner = match &self.backend {
            Backend::Custom(_) => RequestKind::Custom(
                Url::parse(url)
                    .map(|url| HttpRequest {
                        method,
                        url,
                        headers: HeaderMap::new(),
                        body: Vec::new(),
                        context: HttpContext::default(),
                    })
                    .map_err(|_| TransportError::InvalidRequest),
            ),
            #[cfg(feature = "native-http")]
            Backend::Native(client) => RequestKind::Native(client.request(method, url)),
        };
        HttpRequestBuilder {
            inner,
            client: self.clone(),
            context: HttpContext::default(),
        }
    }
    pub(crate) fn get(&self, url: &str) -> HttpRequestBuilder {
        self.request(Method::GET, url)
    }
    pub(crate) fn post(&self, url: &str) -> HttpRequestBuilder {
        self.request(Method::POST, url)
    }
}
enum RequestKind {
    Custom(std::result::Result<HttpRequest, TransportError>),
    #[cfg(feature = "native-http")]
    Native(reqwest::RequestBuilder),
}
enum PreparedRequest {
    Custom(HttpRequest),
    #[cfg(feature = "native-http")]
    Native(reqwest::Request),
}
pub(crate) struct HttpRequestBuilder {
    inner: RequestKind,
    client: HttpClient,
    context: HttpContext,
}
impl HttpRequestBuilder {
    pub(crate) fn context(mut self, context: HttpContext) -> Self {
        self.context = context;
        self
    }
    pub(crate) fn timeout(mut self, timeout: Duration) -> Self {
        self.context.timeout = timeout;
        self
    }
    pub(crate) fn purpose(mut self, purpose: HttpPurpose) -> Self {
        self.context.purpose = purpose;
        self
    }
    pub(crate) fn cancel(mut self, signal: Option<CancellationToken>) -> Self {
        self.client.signal = signal;
        self
    }
    pub(crate) fn header<K, V>(mut self, key: K, value: V) -> Self
    where
        http::header::HeaderName: TryFrom<K>,
        <http::header::HeaderName as TryFrom<K>>::Error: Into<http::Error>,
        http::header::HeaderValue: TryFrom<V>,
        <http::header::HeaderValue as TryFrom<V>>::Error: Into<http::Error>,
    {
        self.inner = match self.inner {
            RequestKind::Custom(request) => RequestKind::Custom(request.and_then(|mut r| {
                let key = http::header::HeaderName::try_from(key)
                    .map_err(|_| TransportError::InvalidRequest)?;
                let mut value = http::header::HeaderValue::try_from(value)
                    .map_err(|_| TransportError::InvalidRequest)?;
                value.set_sensitive(true);
                r.headers.append(key, value);
                Ok(r)
            })),
            #[cfg(feature = "native-http")]
            RequestKind::Native(r) => RequestKind::Native(r.header(key, value)),
        };
        self
    }
    pub(crate) fn body(mut self, body: String) -> Self {
        self.inner = match self.inner {
            RequestKind::Custom(r) => RequestKind::Custom(r.map(|mut r| {
                r.body = body.into_bytes();
                r
            })),
            #[cfg(feature = "native-http")]
            RequestKind::Native(r) => RequestKind::Native(r.body(body)),
        };
        self
    }
    pub(crate) fn json<T: Serialize + ?Sized>(mut self, body: &T) -> Self {
        self.inner = match self.inner {
            RequestKind::Custom(r) => RequestKind::Custom(r.and_then(|mut r| {
                r.body = serde_json::to_vec(body).map_err(|_| TransportError::InvalidRequest)?;
                r.headers
                    .entry(http::header::CONTENT_TYPE)
                    .or_insert(http::header::HeaderValue::from_static("application/json"));
                Ok(r)
            })),
            #[cfg(feature = "native-http")]
            RequestKind::Native(r) => RequestKind::Native(r.json(body)),
        };
        self
    }
    pub(crate) fn form<T: Serialize + ?Sized>(mut self, body: &T) -> Self {
        self.inner = match self.inner {
            RequestKind::Custom(r) => RequestKind::Custom(r.and_then(|mut r| {
                r.body = serde_urlencoded::to_string(body)
                    .map_err(|_| TransportError::InvalidRequest)?
                    .into_bytes();
                r.headers.entry(http::header::CONTENT_TYPE).or_insert(
                    http::header::HeaderValue::from_static("application/x-www-form-urlencoded"),
                );
                Ok(r)
            })),
            #[cfg(feature = "native-http")]
            RequestKind::Native(r) => RequestKind::Native(r.form(body)),
        };
        self
    }
    #[cfg(feature = "native-http")]
    pub(crate) fn multipart(mut self, body: reqwest::multipart::Form) -> Self {
        self.inner = match self.inner {
            RequestKind::Custom(_) => RequestKind::Custom(Err(TransportError::UnsupportedBody)),
            RequestKind::Native(r) => RequestKind::Native(r.multipart(body)),
        };
        self
    }

    pub(crate) async fn send(self) -> Result<Response> {
        let cancel = self
            .client
            .signal
            .as_ref()
            .map(CancellationToken::child_token)
            .unwrap_or_default();
        let guard = cancel.clone().drop_guard();
        let deadline = tokio::time::Instant::now() + self.context.timeout;
        let request = match self.inner {
            RequestKind::Custom(request) => {
                let mut request = request?;
                request.context = self.context;
                PreparedRequest::Custom(request)
            }
            #[cfg(feature = "native-http")]
            RequestKind::Native(request) => {
                let mut request = request
                    .build()
                    .map_err(|_| TransportError::InvalidRequest)?;
                for value in request.headers_mut().values_mut() {
                    value.set_sensitive(true);
                }
                PreparedRequest::Native(request)
            }
        };
        let send = async {
            match (&self.client.backend, request) {
                (Backend::Custom(custom), PreparedRequest::Custom(request)) => {
                    let response = custom
                        .execute(request, cancel.clone())
                        .await
                        .map_err(Error::from)?;
                    Ok::<_, Error>((
                        response.status,
                        response.headers,
                        Box::pin(response.body.map(|v| v.map_err(Error::from))) as ResponseBody,
                    ))
                }
                #[cfg(feature = "native-http")]
                (Backend::Native(client), PreparedRequest::Native(request)) => {
                    let response = client.execute(request).await.map_err(|e| {
                        if e.is_timeout() {
                            TransportError::Timeout
                        } else if e.is_connect() {
                            TransportError::Connection
                        } else {
                            TransportError::Rejected
                        }
                    })?;
                    let status = response.status();
                    let headers = response.headers().clone();
                    let body = response.bytes_stream().map(|v| {
                        v.map_err(|e| {
                            if e.is_timeout() {
                                TransportError::Timeout.into()
                            } else {
                                TransportError::Body.into()
                            }
                        })
                    });
                    Ok((status, headers, Box::pin(body) as ResponseBody))
                }
                #[cfg(feature = "native-http")]
                _ => Err(TransportError::InvalidRequest.into()),
            }
        };
        let (status, headers, mut body) = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(TransportError::Cancelled.into()),
            result = tokio::time::timeout_at(deadline, send) => result.map_err(|_| TransportError::Timeout)??,
        };
        let mode = self.context.response_mode;
        let body = Box::pin(async_stream::try_stream! {
            let _guard = guard;
            loop {
                let next = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => Err(Error::from(TransportError::Cancelled)),
                    _ = tokio::time::sleep_until(deadline), if mode == HttpResponseMode::Buffered => Err(Error::from(TransportError::Timeout)),
                    item = body.next() => Ok(item),
                }?;
                match next { Some(item) => yield item?, None => break }
            }
        });
        Ok(Response {
            status,
            headers,
            body,
        })
    }
}

type ResponseBody = Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>>;
pub(crate) struct Response {
    status: StatusCode,
    headers: HeaderMap,
    body: ResponseBody,
}
impl Response {
    pub(crate) fn status(&self) -> StatusCode {
        self.status
    }
    pub(crate) fn headers(&self) -> &HeaderMap {
        &self.headers
    }
    pub(crate) fn bytes_stream(self) -> ResponseBody {
        self.body
    }
    pub(crate) async fn bytes(self) -> Result<Bytes> {
        let mut body = self.body;
        let mut output = Vec::new();
        while let Some(chunk) = body.next().await {
            output.extend_from_slice(&chunk?);
        }
        Ok(Bytes::from(output))
    }
    pub(crate) async fn text(self) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.bytes().await?).into_owned())
    }
    pub(crate) async fn json<T: DeserializeOwned>(self) -> Result<T> {
        Ok(serde_json::from_slice(&self.bytes().await?)?)
    }
}

/// Poll cancellation before the operation, including synchronous ready results.
pub(crate) async fn with_cancel<T>(
    signal: Option<&CancellationToken>,
    future: impl std::future::Future<Output = T>,
) -> Result<T> {
    match signal {
        Some(cancel) => tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(TransportError::Cancelled.into()),
            result = future => Ok(result),
        },
        None => Ok(future.await),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    struct PendingTransport {
        headers_pending: bool,
        cancel: Mutex<Option<CancellationToken>>,
    }
    #[async_trait]
    impl HttpTransport for PendingTransport {
        async fn execute(
            &self,
            _request: HttpRequest,
            cancel: CancellationToken,
        ) -> std::result::Result<HttpResponse, TransportError> {
            *self.cancel.lock().unwrap() = Some(cancel);
            if self.headers_pending {
                return futures::future::pending().await;
            }
            Ok(HttpResponse {
                status: StatusCode::OK,
                headers: HeaderMap::new(),
                body: Box::pin(futures::stream::pending()),
            })
        }
    }
    fn pending(headers_pending: bool) -> Arc<PendingTransport> {
        Arc::new(PendingTransport {
            headers_pending,
            cancel: Mutex::new(None),
        })
    }
    #[tokio::test]
    async fn cancellation_and_timeout_cover_header_wait_without_fallback() {
        for timeout in [true, false] {
            let transport = pending(true);
            let cancel = CancellationToken::new();
            let http = HttpClient::new(transport.clone()).with_cancellation(Some(cancel.clone()));
            let request = http
                .get("https://invalid.invalid/")
                .timeout(Duration::from_millis(20));
            let send = request.send();
            tokio::pin!(send);
            assert!(tokio::time::timeout(Duration::from_millis(5), &mut send)
                .await
                .is_err());
            if !timeout {
                cancel.cancel();
            }
            assert!(tokio::time::timeout(Duration::from_millis(100), send)
                .await
                .unwrap()
                .is_err());
            assert!(transport
                .cancel
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .is_cancelled());
        }
    }
    #[tokio::test]
    async fn buffered_budget_covers_body_streaming_budget_only_headers() {
        for mode in [HttpResponseMode::Buffered, HttpResponseMode::Streaming] {
            let transport = pending(false);
            let cancel = CancellationToken::new();
            let http = HttpClient::new(transport.clone()).with_cancellation(Some(cancel.clone()));
            let response = http
                .get("https://invalid.invalid/")
                .context(HttpContext {
                    purpose: HttpPurpose::Model,
                    response_mode: mode,
                    timeout: Duration::from_millis(10),
                })
                .send()
                .await
                .unwrap();
            let bytes = response.bytes();
            tokio::pin!(bytes);
            if mode == HttpResponseMode::Buffered {
                assert!(tokio::time::timeout(Duration::from_millis(100), bytes)
                    .await
                    .unwrap()
                    .is_err());
            } else {
                assert!(tokio::time::timeout(Duration::from_millis(30), &mut bytes)
                    .await
                    .is_err());
                cancel.cancel();
                assert!(bytes.await.is_err());
            }
            assert!(transport
                .cancel
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .is_cancelled());
        }
    }
    #[tokio::test]
    async fn dropping_unpolled_response_cancels_transport() {
        let transport = pending(false);
        let response = HttpClient::new(transport.clone())
            .get("https://invalid.invalid/")
            .send()
            .await
            .unwrap();
        drop(response);
        assert!(transport
            .cancel
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .is_cancelled());
    }
}
