//! Optional model request observers. Neither callback controls the request.
use std::sync::Arc;

/// Billing request identifier header; distinct from transport `X-Request-ID`
/// and identifiers inside model content.
pub const GATEWAY_REQUEST_ID_HEADER: &str = "X-Acosmi-Request-Id";
pub type UpstreamActivityCallback = Arc<dyn Fn() + Send + Sync>;
pub type GatewayRequestIDCallback = Arc<dyn Fn(&str) + Send + Sync>;

/// Optional observers for model requests. Existing methods use empty options.
///
/// Activity runs once per decoded SSE line, before filtering/conversion, including
/// comments and empty lines. The ID callback runs before reading the final
/// response body (including HTTP errors), after the one allowed 401 refresh.
/// Callbacks must be short, synchronous and nonblocking. Unwinding panics are
/// caught and that callback is disabled for the operation; the process panic
/// hook still runs. `panic=abort` cannot be caught. Returned streams retain their
/// normal cancellation and backpressure behavior.
#[derive(Clone, Default)]
pub struct ChatOptions {
    pub on_upstream_activity: Option<UpstreamActivityCallback>,
    pub on_gateway_request_id: Option<GatewayRequestIDCallback>,
}

/// Read a single bounded ASCII identifier. No fallback or synthesized ID.
/// Surrounding HTTP whitespace is trimmed; duplicates, control/non-ASCII bytes,
/// embedded whitespace, commas and values over 256 bytes are rejected.
pub fn read_gateway_request_id(headers: &reqwest::header::HeaderMap) -> Option<&str> {
    let mut values = headers.get_all(GATEWAY_REQUEST_ID_HEADER).iter();
    let value = values.next()?.to_str().ok()?.trim();
    if values.next().is_some()
        || value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
    {
        return None;
    }
    Some(value)
}

impl ChatOptions {
    pub(crate) fn activity(&mut self) {
        if let Some(callback) = self.on_upstream_activity.take() {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback())).is_ok() {
                self.on_upstream_activity = Some(callback);
            }
        }
    }

    pub(crate) fn response(&mut self, headers: &reqwest::header::HeaderMap) {
        if let (Some(callback), Some(id)) = (
            self.on_gateway_request_id.take(),
            read_gateway_request_id(headers),
        ) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(id)));
        }
    }
}
