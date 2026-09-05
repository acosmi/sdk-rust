use acosmi::*;
use async_trait::async_trait;
use bytes::Bytes;
use futures::stream;
use http::{HeaderMap, StatusCode};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const BASE: &str = "https://authority.invalid";
const SECRET: &str = "TOKEN_AUTHORITY_CANARY";
fn token(expired: bool) -> TokenSet {
    TokenSet {
        access_token: SECRET.into(),
        refresh_token: format!("refresh-{SECRET}"),
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
#[derive(Clone)]
enum State {
    Ready(TokenSet),
    Missing,
    Pending,
}
struct Authority {
    state: Mutex<State>,
    lock: Arc<tokio::sync::Mutex<()>>,
    fail_load: AtomicBool,
    fail_lock: AtomicBool,
    fail_begin: AtomicBool,
    fail_commit: AtomicBool,
    fail_clear: AtomicBool,
    readback_bad: AtomicBool,
    fail_readback: AtomicBool,
    commit_seen: tokio::sync::Notify,
    committed: AtomicBool,
    hang_begin: AtomicBool,
    hang_commit: AtomicBool,
    hang_clear: AtomicBool,
    reads: AtomicUsize,
    log: Arc<Mutex<Vec<&'static str>>>,
}
impl Authority {
    fn new(state: State) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(state),
            lock: Arc::new(tokio::sync::Mutex::new(())),
            fail_load: AtomicBool::new(false),
            fail_lock: AtomicBool::new(false),
            fail_begin: AtomicBool::new(false),
            fail_commit: AtomicBool::new(false),
            fail_clear: AtomicBool::new(false),
            readback_bad: AtomicBool::new(false),
            fail_readback: AtomicBool::new(false),
            commit_seen: tokio::sync::Notify::new(),
            committed: AtomicBool::new(false),
            hang_begin: AtomicBool::new(false),
            hang_commit: AtomicBool::new(false),
            hang_clear: AtomicBool::new(false),
            reads: AtomicUsize::new(0),
            log: Arc::new(Mutex::new(vec![])),
        })
    }
}
#[async_trait]
impl StrictTokenAuthority for Authority {
    async fn lock(&self) -> AuthorityResult<Box<dyn Send>> {
        if self.fail_lock.load(Ordering::SeqCst) {
            return Err(TokenAuthorityError::Unavailable);
        }
        Ok(Box::new(self.lock.clone().lock_owned().await))
    }
    async fn load(&self) -> AuthorityResult<AuthorityState> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.log.lock().unwrap().push("load");
        if self.fail_load.load(Ordering::SeqCst)
            || (self.fail_readback.load(Ordering::SeqCst) && self.committed.load(Ordering::SeqCst))
        {
            return Err(TokenAuthorityError::Unavailable);
        }
        if self.readback_bad.load(Ordering::SeqCst) && self.committed.load(Ordering::SeqCst) {
            return Ok(AuthorityState::Ready(token(false)));
        }
        Ok(match self.state.lock().unwrap().clone() {
            State::Ready(t) => AuthorityState::Ready(t),
            State::Missing => AuthorityState::Missing,
            State::Pending => AuthorityState::RotationPending,
        })
    }
    async fn begin_rotation(&self) -> AuthorityResult<()> {
        self.log.lock().unwrap().push("begin");
        *self.state.lock().unwrap() = State::Pending;
        if self.hang_begin.load(Ordering::SeqCst) {
            return futures::future::pending().await;
        }
        if self.fail_begin.load(Ordering::SeqCst) {
            return Err(TokenAuthorityError::Unavailable);
        }
        Ok(())
    }
    async fn commit_rotation(&self, t: &TokenSet) -> AuthorityResult<()> {
        self.log.lock().unwrap().push("commit");
        self.commit_seen.notify_one();
        if self.hang_commit.load(Ordering::SeqCst) {
            return futures::future::pending().await;
        }
        if self.fail_commit.load(Ordering::SeqCst) {
            return Err(TokenAuthorityError::Unavailable);
        }
        *self.state.lock().unwrap() = State::Ready(t.clone());
        self.committed.store(true, Ordering::SeqCst);
        Ok(())
    }
    async fn clear(&self) -> AuthorityResult<()> {
        self.log.lock().unwrap().push("clear");
        if self.hang_clear.load(Ordering::SeqCst) {
            return futures::future::pending().await;
        }
        if self.fail_clear.load(Ordering::SeqCst) {
            return Err(TokenAuthorityError::Unavailable);
        }
        *self.state.lock().unwrap() = State::Missing;
        Ok(())
    }
}
struct Transport {
    authority: Arc<Authority>,
    tokens: AtomicUsize,
    calls: AtomicUsize,
    hang_token: AtomicBool,
}
#[async_trait]
impl HttpTransport for Transport {
    async fn execute(
        &self,
        r: HttpRequest,
        _: CancellationToken,
    ) -> std::result::Result<HttpResponse, TransportError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let body = match r.context.purpose {
            HttpPurpose::OAuthDiscovery => {
                self.authority.log.lock().unwrap().push("discover");
                serde_json::json!({"issuer":BASE,"authorization_endpoint":format!("{BASE}/authorize"),
                    "token_endpoint":format!("{BASE}/token"),"revocation_endpoint":format!("{BASE}/revoke"),
                    "registration_endpoint":format!("{BASE}/register"),"scopes_supported":[]}).to_string()
            }
            HttpPurpose::OAuthToken => {
                assert!(
                    matches!(*self.authority.state.lock().unwrap(), State::Pending),
                    "pending must precede network effect"
                );
                self.authority.log.lock().unwrap().push("token");
                self.tokens.fetch_add(1, Ordering::SeqCst);
                if self.hang_token.load(Ordering::SeqCst) {
                    return futures::future::pending().await;
                }
                r#"{"access_token":"new-access","refresh_token":"new-refresh","token_type":"Bearer","expires_in":3600,"scope":"ai"}"#.into()
            }
            HttpPurpose::OAuthRevocation => "{}".into(),
            HttpPurpose::OAuthRegistration => r#"{"client_id":"client"}"#.into(),
            _ => return Err(TransportError::Rejected),
        };
        Ok(HttpResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: Box::pin(stream::iter([Ok(Bytes::from(body))])),
        })
    }
}
struct ForbiddenLegacy;
#[async_trait]
impl TokenStore for ForbiddenLegacy {
    async fn load(&self) -> acosmi::Result<Option<TokenSet>> {
        panic!("strict constructor used Config.store")
    }
    async fn save(&self, _: &TokenSet) -> acosmi::Result<()> {
        panic!("strict constructor used Config.store")
    }
    async fn clear(&self) -> acosmi::Result<()> {
        panic!("strict constructor used Config.store")
    }
}
fn transport(a: &Arc<Authority>) -> Arc<Transport> {
    Arc::new(Transport {
        authority: a.clone(),
        tokens: AtomicUsize::new(0),
        calls: AtomicUsize::new(0),
        hang_token: AtomicBool::new(false),
    })
}
async fn create(a: &Arc<Authority>, t: &Arc<Transport>) -> acosmi::Result<Client> {
    Client::create_with_authority(
        Config {
            server_url: Some(BASE.into()),
            store: Some(Arc::new(ForbiddenLegacy)),
            ..Default::default()
        },
        t.clone(),
        a.clone(),
        None,
    )
    .await
}

#[tokio::test]
async fn unexpired_token_is_reloaded_and_deletion_or_failure_never_uses_cache() {
    let a = Authority::new(State::Ready(token(false)));
    let t = transport(&a);
    let c = create(&a, &t).await.unwrap();
    assert_eq!(c.ensure_token(None).await.unwrap(), SECRET);
    let mut replacement = token(false);
    replacement.access_token = "replacement".into();
    *a.state.lock().unwrap() = State::Ready(replacement);
    assert_eq!(c.ensure_token(None).await.unwrap(), "replacement");
    a.fail_load.store(true, Ordering::SeqCst);
    let e = c.ensure_token(None).await.unwrap_err();
    assert!(!format!("{e:?} {e}").contains(SECRET));
    assert!(c.token_set().is_none());
    a.fail_load.store(false, Ordering::SeqCst);
    *a.state.lock().unwrap() = State::Missing;
    assert!(c.ensure_token(None).await.is_err());
    assert!(!c.is_authorized());
    assert_eq!(t.calls.load(Ordering::SeqCst), 0);
    assert!(a.reads.load(Ordering::SeqCst) >= 5);
}

#[tokio::test]
async fn rotation_is_durable_before_network_and_committed_before_token_publication() {
    let a = Authority::new(State::Ready(token(true)));
    let t = transport(&a);
    let c = create(&a, &t).await.unwrap();
    a.log.lock().unwrap().clear();
    assert_eq!(c.ensure_token(None).await.unwrap(), "new-access");
    assert_eq!(
        *a.log.lock().unwrap(),
        ["load", "begin", "discover", "token", "commit", "load"]
    );
    assert_eq!(t.tokens.load(Ordering::SeqCst), 1);
    assert!(matches!(&*a.state.lock().unwrap(),State::Ready(t) if t.access_token=="new-access"));
    assert_eq!(c.ensure_token(None).await.unwrap(), "new-access");
    assert_eq!(t.tokens.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn begin_or_commit_failure_blocks_this_client_and_recreated_clients() {
    for fail_begin in [false, true] {
        let a = Authority::new(State::Ready(token(true)));
        let t = transport(&a);
        let c = create(&a, &t).await.unwrap();
        a.fail_begin.store(fail_begin, Ordering::SeqCst);
        a.fail_commit.store(!fail_begin, Ordering::SeqCst);
        assert!(c.ensure_token(None).await.is_err());
        assert!(c.token_set().is_none());
        assert!(c.ensure_token(None).await.is_err());
        assert!(c.force_refresh(None).await.is_err());
        assert!(create(&a, &t).await.is_err());
        assert!(c.reconcile_authority(None).await.is_err());
        assert!(matches!(*a.state.lock().unwrap(), State::Pending));
        assert_eq!(t.tokens.load(Ordering::SeqCst), usize::from(!fail_begin));
    }
}

#[tokio::test]
async fn mismatched_readback_blocks_until_explicit_host_reconciliation_without_refresh() {
    let a = Authority::new(State::Ready(token(true)));
    let t = transport(&a);
    let c = create(&a, &t).await.unwrap();
    a.readback_bad.store(true, Ordering::SeqCst);
    assert!(c.ensure_token(None).await.is_err());
    assert!(c.token_set().is_none());
    a.readback_bad.store(false, Ordering::SeqCst);
    assert!(c.ensure_token(None).await.is_err());
    assert_eq!(t.tokens.load(Ordering::SeqCst), 1);
    assert!(c.reconcile_authority(None).await.unwrap());
    assert_eq!(t.tokens.load(Ordering::SeqCst), 1);
    assert_eq!(c.ensure_token(None).await.unwrap(), "new-access");
    assert_eq!(t.tokens.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancellation_during_begin_token_or_commit_keeps_pending_across_clients() {
    for stage in ["begin", "token", "commit"] {
        let a = Authority::new(State::Ready(token(true)));
        let t = transport(&a);
        let c = create(&a, &t).await.unwrap();
        a.hang_begin.store(stage == "begin", Ordering::SeqCst);
        t.hang_token.store(stage == "token", Ordering::SeqCst);
        a.hang_commit.store(stage == "commit", Ordering::SeqCst);
        let cancel = CancellationToken::new();
        let run = c.ensure_token(Some(cancel.clone()));
        tokio::pin!(run);
        assert!(tokio::time::timeout(Duration::from_millis(20), &mut run)
            .await
            .is_err());
        assert!(matches!(*a.state.lock().unwrap(), State::Pending));
        cancel.cancel();
        assert!(run.await.is_err());
        assert!(c.token_set().is_none());
        assert!(create(&a, &t).await.is_err());
        assert!(c.force_refresh(None).await.is_err());
        assert_eq!(
            t.tokens.load(Ordering::SeqCst),
            usize::from(stage != "begin")
        );
    }
}

#[tokio::test]
async fn logout_failure_or_cancel_is_fail_closed_and_pending_is_not_cleared() {
    for cancel_clear in [false, true] {
        let a = Authority::new(State::Ready(token(false)));
        let t = transport(&a);
        let c = create(&a, &t).await.unwrap();
        a.fail_clear.store(!cancel_clear, Ordering::SeqCst);
        a.hang_clear.store(cancel_clear, Ordering::SeqCst);
        let cancel = CancellationToken::new();
        let run = c.logout(Some(cancel.clone()));
        tokio::pin!(run);
        if cancel_clear {
            assert!(tokio::time::timeout(Duration::from_millis(20), &mut run)
                .await
                .is_err());
            cancel.cancel();
        }
        assert!(run.await.is_err());
        assert!(c.ensure_token(None).await.is_err());
        assert!(c.token_set().is_none());
        assert_eq!(t.calls.load(Ordering::SeqCst), 0);
    }
    let a = Authority::new(State::Ready(token(false)));
    let t = transport(&a);
    let c = create(&a, &t).await.unwrap();
    *a.state.lock().unwrap() = State::Pending;
    assert!(c.logout(None).await.is_err());
    assert!(matches!(*a.state.lock().unwrap(), State::Pending));
    assert!(!a.log.lock().unwrap().contains(&"clear"));
}

#[tokio::test]
async fn logout_success_reads_back_missing_and_never_uses_config_store() {
    let a = Authority::new(State::Ready(token(false)));
    let t = transport(&a);
    let c = create(&a, &t).await.unwrap();
    c.logout(None).await.unwrap();
    assert!(!c.is_authorized());
    assert!(c.ensure_token(None).await.is_err());
    assert!(matches!(*a.state.lock().unwrap(), State::Missing));
    assert_eq!(t.tokens.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn concurrent_refresh_is_single_flight_under_required_authority_lock() {
    let a = Authority::new(State::Ready(token(true)));
    let t = transport(&a);
    let c = create(&a, &t).await.unwrap();
    let other = create(&a, &t).await.unwrap();
    let (first, second) = tokio::join!(c.ensure_token(None), other.ensure_token(None));
    assert_eq!(first.unwrap(), "new-access");
    assert_eq!(second.unwrap(), "new-access");
    assert_eq!(t.tokens.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn strict_constructor_rejects_load_error_and_pending_and_accepts_unauthenticated_missing() {
    let a = Authority::new(State::Missing);
    let t = transport(&a);
    assert!(!create(&a, &t).await.unwrap().is_authorized());
    a.fail_load.store(true, Ordering::SeqCst);
    assert!(create(&a, &t).await.is_err());
    a.fail_load.store(false, Ordering::SeqCst);
    *a.state.lock().unwrap() = State::Pending;
    assert!(create(&a, &t).await.is_err());
    assert_eq!(t.calls.load(Ordering::SeqCst), 0);
}

#[cfg(feature = "desktop-loopback")]
#[tokio::test]
async fn strict_login_exchange_persists_pending_then_commit_or_remains_blocked() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for mode in 0..3 {
        let fail_commit = mode == 1;
        let cancel = CancellationToken::new();
        let a = Authority::new(State::Missing);
        let t = transport(&a);
        let c = create(&a, &t).await.unwrap();
        a.fail_commit.store(fail_commit, Ordering::SeqCst);
        a.hang_commit.store(mode == 2, Ordering::SeqCst);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let tx = Mutex::new(Some(tx));
        let handler = |event: acosmi::LoginEvent| {
            if event.r#type == acosmi::auth::auth::EVENT_AUTH_URL {
                if let Some(tx) = tx.lock().unwrap().take() {
                    tx.send(event.url.unwrap()).unwrap();
                }
            }
        };
        let opts = acosmi::auth::LoginOptions {
            skip_browser: true,
            ..Default::default()
        };
        let scopes = vec!["ai".into()];
        let login =
            c.login_with_handler("test", &scopes, Some(&handler), &opts, Some(cancel.clone()));
        let callback = async {
            let auth = url::Url::parse(&rx.await.unwrap()).unwrap();
            let params: std::collections::HashMap<_, _> = auth.query_pairs().into_owned().collect();
            let mut redirect = url::Url::parse(&params["redirect_uri"]).unwrap();
            redirect
                .query_pairs_mut()
                .append_pair("code", "test-code")
                .append_pair("state", &params["state"]);
            let mut socket = tokio::net::TcpStream::connect((
                redirect.host_str().unwrap(),
                redirect.port().unwrap(),
            ))
            .await
            .unwrap();
            let request = format!(
                "GET {}?{} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                redirect.path(),
                redirect.query().unwrap()
            );
            socket.write_all(request.as_bytes()).await.unwrap();
            let mut reply = Vec::new();
            socket.read_to_end(&mut reply).await.unwrap();
            if mode == 2 {
                a.commit_seen.notified().await;
                cancel.cancel();
            }
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(login, callback)
        })
        .await
        .unwrap();
        assert_eq!(result.is_err(), mode != 0);
        assert_eq!(t.tokens.load(Ordering::SeqCst), 1);
        if mode != 0 {
            assert!(c.token_set().is_none());
            assert!(matches!(*a.state.lock().unwrap(), State::Pending));
            assert!(c.ensure_token(None).await.is_err());
            assert!(create(&a, &t).await.is_err());
        } else {
            assert_eq!(c.ensure_token(None).await.unwrap(), "new-access");
        }
        let log = a.log.lock().unwrap();
        assert!(
            log.iter().position(|e| *e == "begin").unwrap()
                < log.iter().position(|e| *e == "token").unwrap()
        );
    }
}

struct LegacyStore {
    value: Mutex<Option<TokenSet>>,
    fail_load: AtomicBool,
    fail_save: AtomicBool,
}
#[async_trait]
impl TokenStore for LegacyStore {
    async fn load(&self) -> acosmi::Result<Option<TokenSet>> {
        if self.fail_load.load(Ordering::SeqCst) {
            return Err(acosmi::Error::other(SECRET));
        }
        Ok(self.value.lock().unwrap().clone())
    }
    async fn save(&self, t: &TokenSet) -> acosmi::Result<()> {
        if self.fail_save.load(Ordering::SeqCst) {
            return Err(acosmi::Error::other(SECRET));
        }
        *self.value.lock().unwrap() = Some(t.clone());
        Ok(())
    }
    async fn clear(&self) -> acosmi::Result<()> {
        *self.value.lock().unwrap() = None;
        Ok(())
    }
}
#[tokio::test]
async fn legacy_token_store_tolerance_remains_explicitly_unchanged() {
    let a = Authority::new(State::Pending);
    let t = transport(&a);
    let store = Arc::new(LegacyStore {
        value: Mutex::new(Some(token(true))),
        fail_load: AtomicBool::new(false),
        fail_save: AtomicBool::new(false),
    });
    let c = Client::create_with_transport(
        Config {
            server_url: Some(BASE.into()),
            store: Some(store.clone()),
            ..Default::default()
        },
        t.clone(),
    )
    .await
    .unwrap();
    store.fail_load.store(true, Ordering::SeqCst);
    store.fail_save.store(true, Ordering::SeqCst);
    c.force_refresh(None).await.unwrap();
    assert_eq!(c.ensure_token(None).await.unwrap(), "new-access");
    assert_eq!(
        store.value.lock().unwrap().as_ref().unwrap().access_token,
        SECRET
    );
    store.fail_load.store(false, Ordering::SeqCst);
    *store.value.lock().unwrap() = None;
    assert_eq!(c.ensure_token(None).await.unwrap(), "new-access");
}

#[tokio::test]
async fn readback_error_after_commit_does_not_publish_or_implicitly_reconcile() {
    let a = Authority::new(State::Ready(token(true)));
    let t = transport(&a);
    let c = create(&a, &t).await.unwrap();
    a.fail_readback.store(true, Ordering::SeqCst);
    assert!(c.ensure_token(None).await.is_err());
    assert!(c.token_set().is_none());
    a.fail_readback.store(false, Ordering::SeqCst);
    assert!(c.ensure_token(None).await.is_err());
    assert!(c.reconcile_authority(None).await.unwrap());
    assert_eq!(c.ensure_token(None).await.unwrap(), "new-access");
    assert_eq!(t.tokens.load(Ordering::SeqCst), 1);
}
