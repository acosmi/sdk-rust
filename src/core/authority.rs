//! Opt-in external token authority. Legacy TokenStore behavior is unchanged.
use crate::{Error, TokenSet};
use async_trait::async_trait;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// A pending rotation is durable state, including across clients and restarts.
/// Implementations must never return Ready for an unresolved pending rotation.
#[derive(Debug)]
pub enum AuthorityState {
    Ready(TokenSet),
    Missing,
    RotationPending,
}

/// Stable errors without storage, URL, token or backend diagnostic payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TokenAuthorityError {
    #[error("token_authority_unavailable")]
    Unavailable,
    #[error("token_authority_missing")]
    Missing,
    #[error("token_authority_rotation_pending")]
    RotationPending,
    #[error("token_authority_reconciliation_required")]
    ReconciliationRequired,
    #[error("token_authority_commit_unverified")]
    CommitUnverified,
}
impl From<TokenAuthorityError> for Error {
    fn from(value: TokenAuthorityError) -> Self {
        Error::other(value.to_string())
    }
}

pub type AuthorityResult<T> = std::result::Result<T, TokenAuthorityError>;

/// Caller-owned durable authority; there is deliberately no default lock.
///
/// All methods run under lock, including the OAuth request between begin/commit.
/// The returned guard must serialize every client/process sharing this authority.
/// begin_rotation must durably mark RotationPending before returning success;
/// failures/cancellation may have committed and require host reconciliation.
/// commit_rotation must atomically persist the new tokens and change Pending to
/// Ready, returning success only after confirmed durable commit. clear must also
/// confirm durability. load must distinguish errors, Missing and Pending; it may
/// not return a stale Ready snapshot for a deleted or pending authority.
///
/// The SDK verifies call ordering and readback, not an arbitrary implementation's
/// durability. The host owns storage transactions, CAS, actor/revision fencing,
/// revocation and resolution of uncertain transactions. The SDK never resolves
/// durable Pending. Call reconcile_authority only after the host has resolved it.
#[async_trait]
pub trait StrictTokenAuthority: Send + Sync {
    async fn lock(&self) -> AuthorityResult<Box<dyn Send>>;
    async fn load(&self) -> AuthorityResult<AuthorityState>;
    async fn begin_rotation(&self) -> AuthorityResult<()>;
    async fn commit_rotation(&self, tokens: &TokenSet) -> AuthorityResult<()>;
    async fn clear(&self) -> AuthorityResult<()>;
}

pub(crate) struct StrictState {
    pub authority: Arc<dyn StrictTokenAuthority>,
    blocked: AtomicBool,
}
impl StrictState {
    pub fn new(authority: Arc<dyn StrictTokenAuthority>) -> Self {
        Self {
            authority,
            blocked: AtomicBool::new(false),
        }
    }
    pub fn check(&self) -> AuthorityResult<()> {
        if self.blocked.load(Ordering::SeqCst) {
            Err(TokenAuthorityError::ReconciliationRequired)
        } else {
            Ok(())
        }
    }
    pub fn block(&self) {
        self.blocked.store(true, Ordering::SeqCst);
    }
    pub fn reconcile(&self) {
        self.blocked.store(false, Ordering::SeqCst);
    }
}

pub(crate) fn tokens_equal(a: &TokenSet, b: &TokenSet) -> bool {
    a.access_token == b.access_token
        && a.refresh_token == b.refresh_token
        && a.expires_at == b.expires_at
        && a.scope == b.scope
        && a.client_id == b.client_id
        && a.server_url == b.server_url
}
