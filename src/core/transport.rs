//! HTTP injection without SDK-owned connections or redirect handling.
use crate::shared::errors::{Error, NetworkError, Result};
use async_trait::async_trait;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use reqwest::{header::HeaderMap, Method, StatusCode, Url};
use serde::{de::DeserializeOwned, Serialize};
use std::{fmt, pin::Pin, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

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
pub struct HttpClient {
    builder: reqwest::Client,
    custom: Option<Arc<dyn HttpTransport>>,
    signal: Option<CancellationToken>,
}
impl HttpClient {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            builder: reqwest::Client::new(),
            custom: Some(transport),
            signal: None,
        }
    }
    pub fn with_cancellation(&self, signal: Option<CancellationToken>) -> Self {
        Self {
            signal,
            ..self.clone()
        }
    }
    pub(crate) fn legacy(builder: reqwest::Client) -> Self {
        Self {
            builder,
            custom: None,
            signal: None,
        }
    }
    pub(crate) fn is_custom(&self) -> bool {
        self.custom.is_some()
    }
    pub(crate) fn request(&self, method: Method, url: &str) -> HttpRequestBuilder {
        HttpRequestBuilder {
            inner: self.builder.request(method, url),
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

pub(crate) struct HttpRequestBuilder {
    inner: reqwest::RequestBuilder,
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
        reqwest::header::HeaderName: TryFrom<K>,
        <reqwest::header::HeaderName as TryFrom<K>>::Error: Into<http::Error>,
        reqwest::header::HeaderValue: TryFrom<V>,
        <reqwest::header::HeaderValue as TryFrom<V>>::Error: Into<http::Error>,
    {
        self.inner = self.inner.header(key, value);
        self
    }
    pub(crate) fn body(mut self, body: String) -> Self {
        self.inner = self.inner.body(body);
        self
    }
    pub(crate) fn json<T: Serialize + ?Sized>(mut self, body: &T) -> Self {
        self.inner = self.inner.json(body);
        self
    }
    pub(crate) fn form<T: Serialize + ?Sized>(mut self, body: &T) -> Self {
        self.inner = self.inner.form(body);
        self
    }
    pub(crate) fn multipart(mut self, body: reqwest::multipart::Form) -> Self {
        self.inner = self.inner.multipart(body);
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
        let mut request = self
            .inner
            .build()
            .map_err(|_| TransportError::InvalidRequest)?;
        // Mark all outgoing header values sensitive before handing them to HTTP libraries.
        for value in request.headers_mut().values_mut() {
            value.set_sensitive(true);
        }
        let send = async {
            if let Some(custom) = &self.client.custom {
                let body = match request.body() {
                    None => Vec::new(),
                    Some(body) => body
                        .as_bytes()
                        .ok_or(TransportError::UnsupportedBody)?
                        .to_vec(),
                };
                let owned = HttpRequest {
                    method: request.method().clone(),
                    url: request.url().clone(),
                    headers: request.headers().clone(),
                    body,
                    context: self.context,
                };
                drop(request);
                let response = custom
                    .execute(owned, cancel.clone())
                    .await
                    .map_err(Error::from)?;
                Ok::<_, Error>((
                    response.status,
                    response.headers,
                    Box::pin(response.body.map(|v| v.map_err(Error::from))) as ResponseBody,
                ))
            } else {
                let response = self.client.builder.execute(request).await.map_err(|e| {
                    if e.is_timeout() {
                        Error::from(TransportError::Timeout)
                    } else if e.is_connect() {
                        Error::from(TransportError::Connection)
                    } else {
                        Error::from(TransportError::Rejected)
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
