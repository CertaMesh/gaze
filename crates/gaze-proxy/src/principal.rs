use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe, PanicHookInfo};
use std::sync::{Mutex, MutexGuard};

use zeroize::{Zeroize, Zeroizing};

/// Maximum canonical principal size accepted at the trusted resolver boundary.
pub const MAX_AUTHENTICATED_PRINCIPAL_BYTES: usize = 1024;

/// Maximum separately configured local-auth credential size.
pub const MAX_LOCAL_AUTH_CREDENTIAL_BYTES: usize = 8 * 1024;

type PanicHook = Box<dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static>;

static RESOLVER_PANIC_HOOK_BOUNDARY: Mutex<()> = Mutex::new(());

struct PanicHookRestore(Option<PanicHook>);

impl Drop for PanicHookRestore {
    fn drop(&mut self) {
        if let Some(hook) = self.0.take() {
            std::panic::set_hook(hook);
        }
    }
}

fn lock_panic_hook_boundary() -> MutexGuard<'static, ()> {
    match RESOLVER_PANIC_HOOK_BOUNDARY.lock() {
        Ok(boundary) => boundary,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Closed listener exposure supplied to a trusted principal resolver.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ListenerScope {
    Loopback = 0,
    NonLoopback = 1,
}

/// Closed peer exposure supplied to a trusted principal resolver.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PeerScope {
    Loopback = 0,
    NonLoopback = 1,
}

/// Borrowed trusted TLS client-certificate hash.
///
/// This wrapper intentionally has no `Debug`, `Display`, or serialization surface.
pub struct TlsClientCertificateHash<'a> {
    bytes: &'a [u8; 32],
}

impl TlsClientCertificateHash<'_> {
    /// Reveals the fixed-size hash only inside the trusted resolver callback.
    pub fn with_bytes<R>(&self, reveal: impl FnOnce(&[u8; 32]) -> R) -> R {
        reveal(self.bytes)
    }
}

/// Borrowed, separately configured local-auth credential.
///
/// This wrapper intentionally has no `Debug`, `Display`, or serialization surface.
///
/// ```compile_fail
/// use gaze_proxy::LocalAuthCredential;
///
/// fn requires_debug<T: std::fmt::Debug>() {}
/// requires_debug::<LocalAuthCredential<'static>>();
/// ```
pub struct LocalAuthCredential<'a> {
    bytes: &'a [u8],
}

impl LocalAuthCredential<'_> {
    /// Reveals the credential only inside the trusted resolver callback.
    pub fn with_bytes<R>(&self, reveal: impl FnOnce(&[u8]) -> R) -> R {
        reveal(self.bytes)
    }
}

/// Sealed, non-content request context supplied to [`PrincipalResolver`].
///
/// It cannot carry provider credentials, raw headers, a body, URL/query data,
/// or a caller continuity identifier. Fields and construction stay inside the
/// proxy runtime.
///
/// ```compile_fail
/// use gaze_proxy::{ListenerScope, PeerScope, PrincipalContext};
///
/// let _ = PrincipalContext {
///     listener_scope: ListenerScope::Loopback,
///     peer_scope: PeerScope::Loopback,
/// };
/// ```
pub struct PrincipalContext<'a> {
    listener_scope: ListenerScope,
    peer_scope: PeerScope,
    tls_client_certificate_hash: Option<&'a [u8; 32]>,
    local_auth_credential: Option<&'a [u8]>,
}

impl<'a> PrincipalContext<'a> {
    #[allow(
        dead_code,
        reason = "constructed by direct-profile request validation in P3/P4"
    )]
    pub(crate) fn new(
        listener_scope: ListenerScope,
        peer_scope: PeerScope,
        tls_client_certificate_hash: Option<&'a [u8; 32]>,
        local_auth_credential: Option<&'a [u8]>,
    ) -> Result<Self, PrincipalResolveError> {
        if matches!(local_auth_credential, Some(bytes) if bytes.is_empty() || bytes.len() > MAX_LOCAL_AUTH_CREDENTIAL_BYTES)
        {
            return Err(PrincipalResolveError::InvalidLocalAuthCredential);
        }
        Ok(Self {
            listener_scope,
            peer_scope,
            tls_client_certificate_hash,
            local_auth_credential,
        })
    }

    #[must_use]
    pub const fn listener_scope(&self) -> ListenerScope {
        self.listener_scope
    }

    #[must_use]
    pub const fn peer_scope(&self) -> PeerScope {
        self.peer_scope
    }

    #[must_use]
    pub fn tls_client_certificate_hash(&self) -> Option<TlsClientCertificateHash<'a>> {
        self.tls_client_certificate_hash
            .map(|bytes| TlsClientCertificateHash { bytes })
    }

    #[must_use]
    pub fn local_auth_credential(&self) -> Option<LocalAuthCredential<'a>> {
        self.local_auth_credential
            .map(|bytes| LocalAuthCredential { bytes })
    }
}

/// Opaque, bounded canonical principal returned by a trusted resolver.
///
/// Canonical bytes are zeroized on drop. The type intentionally implements no
/// `Debug`, `Display`, serialization, cloning, equality, `AsRef`, `Deref`, or
/// implicit conversion traits.
///
/// ```compile_fail
/// use gaze_proxy::AuthenticatedPrincipal;
///
/// let principal = AuthenticatedPrincipal::try_from_canonical_bytes(b"tenant-a".to_vec())?;
/// let _ = format!("{principal:?}");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// ```compile_fail
/// use gaze_proxy::AuthenticatedPrincipal;
///
/// let principal = AuthenticatedPrincipal::try_from_canonical_bytes(b"tenant-a".to_vec())?;
/// let _ = format!("{principal}");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// ```compile_fail
/// use gaze_proxy::AuthenticatedPrincipal;
///
/// let principal = AuthenticatedPrincipal::try_from_canonical_bytes(b"tenant-a".to_vec())?;
/// let _ = principal.clone();
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// ```compile_fail
/// use gaze_proxy::AuthenticatedPrincipal;
///
/// let principal = AuthenticatedPrincipal::try_from_canonical_bytes(b"tenant-a".to_vec())?;
/// let _ = serde_json::to_vec(&principal)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// ```compile_fail
/// use gaze_proxy::AuthenticatedPrincipal;
///
/// fn requires_as_ref<T: AsRef<[u8]>>() {}
/// requires_as_ref::<AuthenticatedPrincipal>();
/// ```
///
/// ```compile_fail
/// use gaze_proxy::AuthenticatedPrincipal;
///
/// let _ = AuthenticatedPrincipal { canonical_bytes: Vec::new() };
/// ```
pub struct AuthenticatedPrincipal {
    canonical_bytes: Zeroizing<Vec<u8>>,
}

impl AuthenticatedPrincipal {
    /// Constructs a bounded canonical identity for return from a trusted resolver.
    pub fn try_from_canonical_bytes(
        mut canonical_bytes: Vec<u8>,
    ) -> Result<Self, PrincipalResolveError> {
        if canonical_bytes.is_empty() {
            canonical_bytes.zeroize();
            return Err(PrincipalResolveError::EmptyPrincipal);
        }
        if canonical_bytes.len() > MAX_AUTHENTICATED_PRINCIPAL_BYTES {
            canonical_bytes.zeroize();
            return Err(PrincipalResolveError::PrincipalTooLong);
        }
        Ok(Self {
            canonical_bytes: Zeroizing::new(canonical_bytes),
        })
    }

    pub(crate) fn with_canonical_bytes<R>(&self, reveal: impl FnOnce(&[u8]) -> R) -> R {
        reveal(self.canonical_bytes.as_slice())
    }
}

/// Closed, sanitized principal resolution failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrincipalResolveError {
    EmptyPrincipal,
    PrincipalTooLong,
    InvalidLocalAuthCredential,
    LoopbackScopeRequired,
    ResolverRejected,
    ResolverPanicked,
    EntropyUnavailable,
}

impl fmt::Display for PrincipalResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyPrincipal => "principal_empty",
            Self::PrincipalTooLong => "principal_too_long",
            Self::InvalidLocalAuthCredential => "principal_local_auth_invalid",
            Self::LoopbackScopeRequired => "principal_loopback_scope_required",
            Self::ResolverRejected => "principal_rejected",
            Self::ResolverPanicked => "principal_resolver_panicked",
            Self::EntropyUnavailable => "principal_entropy_unavailable",
        })
    }
}

impl std::error::Error for PrincipalResolveError {}

/// Trusted synchronous principal authentication boundary.
pub trait PrincipalResolver: Send + Sync + 'static {
    fn resolve(
        &self,
        context: PrincipalContext<'_>,
    ) -> Result<AuthenticatedPrincipal, PrincipalResolveError>;
}

/// Invokes adopter code before body/session creation and sanitizes unwind failures.
#[allow(
    dead_code,
    reason = "called by direct-profile request validation in P3/P4"
)]
pub(crate) fn invoke_resolver(
    resolver: &dyn PrincipalResolver,
    context: PrincipalContext<'_>,
) -> Result<AuthenticatedPrincipal, PrincipalResolveError> {
    let _boundary = lock_panic_hook_boundary();
    let prior_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let restore_hook = PanicHookRestore(Some(prior_hook));
    let result = match catch_unwind(AssertUnwindSafe(|| resolver.resolve(context))) {
        Ok(result) => result,
        Err(_) => Err(PrincipalResolveError::ResolverPanicked),
    };
    drop(restore_hook);
    result
}

/// Launch-local loopback identity source.
///
/// The random identity is non-observable and zeroized when the resolver drops.
pub struct ProcessLocalLoopbackResolver {
    identity: Zeroizing<[u8; 32]>,
}

impl ProcessLocalLoopbackResolver {
    /// Creates a fresh process-local loopback resolver from the operating system RNG.
    pub fn new() -> Result<Self, PrincipalResolveError> {
        let mut identity = [0_u8; 32];
        if getrandom::fill(&mut identity).is_err() {
            identity.zeroize();
            return Err(PrincipalResolveError::EntropyUnavailable);
        }
        Ok(Self {
            identity: Zeroizing::new(identity),
        })
    }
}

impl PrincipalResolver for ProcessLocalLoopbackResolver {
    fn resolve(
        &self,
        context: PrincipalContext<'_>,
    ) -> Result<AuthenticatedPrincipal, PrincipalResolveError> {
        if context.listener_scope() != ListenerScope::Loopback
            || context.peer_scope() != PeerScope::Loopback
        {
            return Err(PrincipalResolveError::LoopbackScopeRequired);
        }
        AuthenticatedPrincipal::try_from_canonical_bytes(self.identity.as_slice().to_vec())
    }
}

/// Test-only serialization for the process-global panic hook.
///
/// [`invoke_resolver`] swaps the global panic hook for the duration of the trusted
/// resolver call, so *every* caller of it is a global-hook writer. A test that
/// installs its own hook and then observes whether it fires therefore races any
/// concurrent `invoke_resolver` in the same test binary: the concurrent boundary
/// window can mask the installed hook (observed hit count too low) or restore a
/// stale hook over it.
///
/// [`RESOLVER_PANIC_HOOK_BOUNDARY`] cannot be reused for this -- `invoke_resolver`
/// takes it internally and [`Mutex`] is not reentrant. This second lock is always
/// acquired strictly *outside* that boundary and is never taken by production code,
/// so the ordering cannot deadlock.
///
/// Every test that touches [`std::panic::set_hook`] / [`std::panic::take_hook`],
/// directly or through [`invoke_resolver`], MUST hold this guard across its whole
/// hook window. That includes tests in `tests/session_registry.rs`, which
/// `#[path]`-includes this module and so shares this exact lock.
///
/// `#[cfg(test)]` keeps this out of every non-test build; production behavior is
/// unchanged.
#[cfg(test)]
pub(crate) static PANIC_HOOK_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Acquires [`PANIC_HOOK_TEST_LOCK`], tolerating poisoning from an unrelated
/// failing test so the real assertion failure is what gets reported.
#[cfg(test)]
pub(crate) fn lock_panic_hook_for_test() -> MutexGuard<'static, ()> {
    match PANIC_HOOK_TEST_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principal_bounds_fail_closed() {
        assert!(matches!(
            AuthenticatedPrincipal::try_from_canonical_bytes(Vec::new()),
            Err(PrincipalResolveError::EmptyPrincipal)
        ));
        assert!(matches!(
            AuthenticatedPrincipal::try_from_canonical_bytes(vec![
                0x41;
                MAX_AUTHENTICATED_PRINCIPAL_BYTES
                    + 1
            ]),
            Err(PrincipalResolveError::PrincipalTooLong)
        ));
    }

    #[test]
    fn loopback_identity_is_stable_for_one_resolver() -> Result<(), PrincipalResolveError> {
        let _panic_hook_guard = lock_panic_hook_for_test();
        let resolver = ProcessLocalLoopbackResolver::new()?;
        let context =
            || PrincipalContext::new(ListenerScope::Loopback, PeerScope::Loopback, None, None);
        let first = invoke_resolver(&resolver, context()?)?;
        let second = invoke_resolver(&resolver, context()?)?;
        assert!(first
            .with_canonical_bytes(|left| { second.with_canonical_bytes(|right| left == right) }));
        Ok(())
    }
}
