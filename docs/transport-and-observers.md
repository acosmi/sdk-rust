# Rust 3.0: transport and model gateway integration

The gateway contract covered here was verified against the official TypeScript 2.19.0 package and its public source commit d245a7b003f286ee5fe5d513d3b6746ecd63664c. Other SDK domains and later TypeScript converter changes are outside this parity claim.

## Migration from Rust 2.17

Existing Client, Config, TokenStore and free OAuth function signatures remain available. The public ManagedModel struct has a new field, thinking_levels: Option<Vec<String>>. Exhaustive struct literals must add thinking_levels: None; literals using ..Default::default() already work. This source incompatibility is why the Rust release is 3.0.

Missing thinking_levels means unknown; Some(vec![]) means no selectable levels. Unknown strings are retained. No level is inferred from the model name. The snake_case JSON field survives model listing and cache normalization.

Malformed UTF-8 SSE is now an error, not replacement characters. The 1 MiB line cap applies before copying complete or fragmented lines. A trailing CR and LF are framing, not content bytes. Long streams still have no SDK global deadline.

## Exclusive HTTP transport

~~~rust,no_run
use std::sync::Arc;
use acosmi::{Client, Config, HttpTransport, InMemoryTokenStore};

fn connect(transport: Arc<dyn HttpTransport>) -> acosmi::Result<Client> {
    Client::new_with_transport(Config {
        server_url: Some("https://gateway.example".into()),
        store: Some(Arc::new(InMemoryTokenStore::new())),
        ..Default::default()
    }, transport)
}
~~~

The custom transport replaces every SDK HTTP connection: model catalog, normal APIs, both model wire formats, agent-run streams, downloads, OAuth discovery/registration/code exchange/refresh/revocation, and refresh proxy. Config.http remains supported for legacy constructors and is ignored by custom constructors. Unsupported multipart uploads and WebSockets return explicit errors in custom mode; there is no direct-network fallback.

The async HttpTransport::execute method receives an owned HttpRequest plus CancellationToken and returns HttpResponse or the payload-free TransportError enum. Requests contain standard Method, Url, HeaderMap, buffered Vec<u8> body, and HttpContext. Responses contain status, headers, and an unpolled Send byte stream. HTTP types are the same http 1.x types re-exported by reqwest 0.12.

HttpContext is descriptive metadata, never an authorization credential:

| Purpose | Response mode | SDK timeout meaning |
| --- | --- | --- |
| OAuthDiscovery / OAuthRegistration / OAuthToken / OAuthRevocation | Buffered | 30 seconds through response body |
| Api | Buffered | Existing normal JSON budget, usually 30 seconds |
| Model | Buffered | 660 seconds through response body |
| Model | Streaming | 660 seconds to response headers; no body deadline |
| Transfer | Buffered | 300 seconds for skill transfer |

Agent-run SSE uses Api + Streaming with a 30-second header budget. Callers may impose stricter destination, header, size, header-deadline or read-gap limits. Application cancellation remains the mechanism for a streaming run's total lifetime.

The transport owns DNS, connection binding, TLS, proxies, and authorization at every redirect hop. It must strip credentials appropriately across origins, reject unsafe redirects of secret-bearing bodies, and must not implicitly retry non-idempotent requests. The SDK never follows a custom transport's returned redirect. A policy transport should reject unsupported methods, purposes, headers or URLs itself.

Cancellation is checked before header polling and before each body poll. Dropping a request future or response cancels its child token and drops the underlying future/body. A transport with a background driver must honor the token and release the driver on body drop; the SDK cannot close resources retained independently by a transport implementation. Token refresh cancellation also covers waits for the mutex and TokenStore lock/load.

Free OAuth functions keep their reqwest signatures. Their corresponding *_with_transport functions accept a shared HttpClient:

~~~rust,no_run
use std::sync::Arc;
use acosmi::{HttpClient, HttpTransport, discover_with_transport};

async fn discover(transport: Arc<dyn HttpTransport>) -> acosmi::Result<()> {
    let http = HttpClient::new(transport)
        .with_cancellation(Some(tokio_util::sync::CancellationToken::new()));
    let _metadata = discover_with_transport(&http, "https://gateway.example").await?;
    Ok(())
}
~~~

## Observers

Use chat_with_options, chat_messages_with_options, chat_stream_with_options, chat_messages_stream_with_options or chat_stream_with_usage_with_options with ChatOptions. The original methods delegate with empty options.

- on_upstream_activity runs for each successfully decoded SSE line before filtering or conversion, including comments, empty lines and OpenAI chunks that yield zero content events.
- on_gateway_request_id runs at most once, before reading the final response body and before HTTP-error parsing. An initial 401 discarded for the single refresh does not notify. It reports X-Acosmi-Request-Id only, never X-Request-ID or a content/message ID.
- No header means no notification. The accepted value is trimmed, single-valued, 1–256 bytes, with ASCII letters, digits, dash, underscore, period or colon only. Empty, duplicate, comma-joined, whitespace-containing and malformed values are omitted. This is deliberately stricter than the TypeScript 2.19 whitespace-only check.
- Callbacks are synchronous and should not block. An unwinding panic is caught and disables that callback for the operation. The process panic hook still runs; panic=abort cannot be caught. SDK observers never retry a stream.

## Sources

classify_sources_event returns NotSources, EmptySources, Sources or MalformedSources with a stable error code. It accepts the SSE event name or JSON type discriminator and ignores unknown extra fields. Validation is structural; it does not authorize URLs. The older parse_sources_event retains its existing empty/malformed behavior.

## Secret ownership and errors

TokenSet, TokenResponse and ClientRegistration Debug output is redacted. OAuthTokenEndpointError Display/Debug reports status without remote descriptions; its existing public fields remain available for deliberate inspection. Injected TokenStore failures are not interpolated into log messages. TransportError variants carry no arbitrary strings, URLs, headers or response body text.

Client caches, InMemoryTokenStore entries and serialized FileTokenStore buffers use zeroizing owners. HttpRequest body bytes are zeroized on drop; moving those bytes out transfers responsibility to the transport. These measures do not guarantee erasure of every copy: public token snapshots remain ordinary owned Strings, serialization and reqwest may allocate copies, HeaderMap does not guarantee zeroization, and allocators/OS/network stacks can retain buffers. Callers own copies returned by token_set/ensure_token and may use the DTOs' Zeroize implementations explicitly.

No new persistence path is introduced. Legacy default FileTokenStore is a JSON file intended for development; production applications should inject their existing protected TokenStore. Cancellation can interrupt persistence after token rotation, so applications must handle a later refresh or reauthorization according to their storage policy.

## Validation and MSRV

CI tests Linux and macOS, checks all features and no-default-features, and verifies Rust 1.82 against a dependency graph resolved for that MSRV. With modern Cargo, set CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback when generating that lockfile, then select idna_adapter 1.2.0 (cargo update -p idna_adapter --precise 1.2.0) before building it with Cargo 1.82. The latest ICU 2 transitive constraints exceed Rust 1.82 even when fallback resolution is enabled. This library does not pin consumer lockfiles; choosing newer transitive dependencies can require a newer compiler.

Publishing requires successful CI on the exact public source commit and a matching version tag. GitHub Release includes the official downloaded crate, SHA256SUMS and source.json; the workflow compares downloaded bytes against the archive sent to crates.io.
