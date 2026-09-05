# Rust 4.0: custom-only builds and durable token authority

Rust 4.0 keeps the default Client/Config/OAuth/TokenStore APIs and the legacy tolerant TokenStore behavior. It adds an opt-in strict authority and makes the native HTTP/WebSocket dependencies optional. Version 3.0.0 remains published and unchanged.

## Feature migration

Default features are sanitize, native-http and notifications-ws. Applications that disabled defaults previously still received the native networking stack in 3.0. In 4.0 they must explicitly enable native-http (and notifications-ws if needed) to retain the corresponding native APIs. This public feature change is why the release is major.

For a caller-owned transport with no SDK reqwest, Hyper, TLS or WebSocket dependency, use:

~~~toml
acosmi-sdk = { version = "=4.0.0", default-features = false, features = ["custom-transport", "sanitize", "desktop-loopback"] }
~~~

The desktop-loopback feature is optional: it enables the SDK's local OAuth callback listener, not an outbound TLS/HTTP client. The custom-transport feature is a marker; HTTP transport interfaces and both model wire adapters are available even without it. Cargo features are additive: another dependency enabling native-http or notifications-ws on the same SDK will include those dependencies in the resolved graph. Inspect the application's final graph.

Config.http and the legacy free functions accepting reqwest clients/errors require native-http. The transport-neutral iter_sse_lines_result / iter_sse_lines_result_with_cap / read_limited_result / read_limited_text_result helpers are available under every feature combination and accept SDK Result byte streams; their signatures do not change when features are unified. The Error::Http2 variant also requires native-http. WSConfig and Client::connect/disconnect require notifications-ws. Client::new/create without native-http return an explicit transport-required error; use a transport constructor. Multipart upload remains explicitly unsupported in custom mode.

HttpRequest and HttpResponse now use http 1.x and url 2 types directly; these are identical to the types previously re-exported by reqwest. Custom JSON/form requests are encoded directly without constructing a reqwest Client, even if native features are also compiled.

The zeroize requirement is now the compatible 1.8.2 range, not an exact pin. An isolated consumer using exact zeroize 1.9.0 must resolve a single zeroize 1.9.0 and run successfully. Rust 1.82 is tested with a locked compatible graph selecting zeroize 1.8.2 and idna_adapter 1.2.0; newer dependency selections can need a newer compiler.

## Strict authority contract

~~~rust,no_run
use std::sync::Arc;
use acosmi::{Client, Config, HttpTransport, StrictTokenAuthority};

async fn connect(
    transport: Arc<dyn HttpTransport>,
    authority: Arc<dyn StrictTokenAuthority>,
) -> acosmi::Result<Client> {
    Client::create_with_authority(
        Config::default(),
        transport,
        authority,
        Some(tokio_util::sync::CancellationToken::new()),
    ).await
}
~~~

Config.store is ignored by this constructor. There is no fallback to TokenStore, FileTokenStore or a cached token. The constructor checks authority: a load error or RotationPending fails; Missing creates an unauthenticated client that may perform an explicit login.

StrictTokenAuthority has no default lock implementation:

| Method | Required host behavior |
| --- | --- |
| lock | Return a guard serializing every client/process sharing the authority. |
| load | Return Ready(TokenSet), Missing or RotationPending; errors remain errors. Never substitute stale Ready state for deletion, an error or unresolved pending. |
| begin_rotation | Durably set RotationPending before returning success. Supports a refresh from Ready or a new login from Missing. On failure/cancellation, the transaction may already have committed. |
| commit_rotation | Atomically persist new tokens and change Pending to Ready. Return success only after confirmed durable commit. |
| clear | Durably delete the authority; success must be followed by Missing. |

The SDK holds its mutex and the required authority guard for each strict operation. It reloads on every ensure_token, including unexpired tokens, and rechecks protected retry dispatch. Missing/errors clear its cache and stop the operation. Public is_authorized/token_set remain snapshots, not durable authorization proofs; application code must not treat a snapshot or a previously returned String as an ongoing grant.

Before a token exchange/refresh network effect, the SDK clears its cache, blocks its own token use, and awaits begin_rotation. Only then does it call the OAuth token endpoint/proxy. It calls commit_rotation and reads back all token fields; only an exact confirmed match can publish a token. A begin, transport, commit, readback or cancellation failure leaves that Client blocked. The SDK does not automatically retry, reuse the old refresh token, or clear durable Pending.

A second/recreated Client rejects a Pending authority. This guarantee depends on the authority implementation actually providing the documented durable and serialized semantics. The SDK cannot verify arbitrary storage implementations, invent a distributed transaction, or prove that a returned success was fsynced. The host owns database transactions, CAS, actor/tenant/revision fences, revocation and uncertain-transaction recovery. Its transport still authorizes each actual outbound operation.

Strict interactive login uses the same pending/commit/readback protocol around its token exchange. Strict logout rejects Pending, clears the authority before best-effort remote revocation, and verifies Missing; a failed or cancelled clear blocks the Client. Neither logout nor recovery silently clears Pending.

## Explicit recovery

After the host has resolved uncertain transactions and established its current actor/revision authority, call reconcile_authority(signal). It takes the required locks and only reads state:

- Ready replaces the local snapshot and unblocks the Client.
- Missing clears the snapshot and returns false.
- RotationPending or read/lock error remains a failure.

It never sends an OAuth request, calls commit_rotation, or clears Pending. Calling it is the host's explicit assertion that reconciliation has completed; do not invoke it as a generic retry loop or assume that loading an old token is itself proof of reconciliation.

The lower-level free OAuth functions do not belong to a Client and do not acquire its authority. Hosts using these functions directly own the equivalent durable state protocol. Use the strict Client login/refresh path when relying on SDK-enforced ordering.

## Errors and compatibility

Authority methods return the payload-free TokenAuthorityError enum. SDK methods map these stable codes into the existing Error::Other without carrying storage errors, SQL, URLs or credentials. Secret Debug output and token cache zeroization from 3.0 are retained.

The old TokenStore contract remains deliberately tolerant: cached unexpired tokens can be returned; load errors/Missing can preserve cache during legacy refresh; save_refreshed_token failures do not fail legacy refresh. Choose StrictTokenAuthority when those behaviors are unsuitable. Injecting a TokenStore alone is not a durable authority guarantee.
