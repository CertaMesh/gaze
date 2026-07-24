use std::error::Error as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Barrier};
use std::time::{Duration, Instant};

use gaze::{PiiClass, Scope, Session};
use gaze_proxy::SessionRegistryConfig;

mod adapter {
    pub use gaze_proxy::SessionRegistryConfig;
}

#[allow(
    dead_code,
    reason = "the focused test compiles the exact private implementation"
)]
#[path = "../src/principal.rs"]
mod principal;
#[allow(
    dead_code,
    reason = "the focused test compiles the exact private implementation"
)]
#[path = "../src/session_registry.rs"]
mod session_registry;

use principal::{
    invoke_resolver, lock_panic_hook_for_test, AuthenticatedPrincipal, ListenerScope, PeerScope,
    PrincipalContext, PrincipalResolveError, PrincipalResolver, ProcessLocalLoopbackResolver,
    MAX_AUTHENTICATED_PRINCIPAL_BYTES, MAX_LOCAL_AUTH_CREDENTIAL_BYTES,
};
use session_registry::{
    derive_framed_for_test, derive_namespace_for_test, RegistryError, SessionRegistry,
    TOMBSTONE_ACCOUNTED_BYTES,
};

const SESSION_A: &str = "123e4567-e89b-42d3-a456-426614174000";
const SESSION_B: &str = "123e4567-e89b-42d3-a456-426614174001";
const SESSION_C: &str = "123e4567-e89b-42d3-a456-426614174002";
const SESSION_D: &str = "123e4567-e89b-42d3-a456-426614174003";

fn principal(value: &[u8]) -> AuthenticatedPrincipal {
    AuthenticatedPrincipal::try_from_canonical_bytes(value.to_vec()).unwrap()
}

fn config(
    max_active: usize,
    session_ttl: Duration,
    max_tombstones: usize,
    max_tombstone_bytes: usize,
    tombstone_ttl: Duration,
) -> SessionRegistryConfig {
    SessionRegistryConfig::try_new(
        max_active,
        session_ttl,
        max_tombstones,
        max_tombstone_bytes,
        tombstone_ttl,
    )
    .unwrap()
}

fn test_registry(config: SessionRegistryConfig) -> SessionRegistry {
    SessionRegistry::with_secret(config, [0x5a; 32])
}

fn loopback_context<'a>(
    certificate_hash: Option<&'a [u8; 32]>,
    local_auth: Option<&'a [u8]>,
) -> Result<PrincipalContext<'a>, PrincipalResolveError> {
    PrincipalContext::new(
        ListenerScope::Loopback,
        PeerScope::Loopback,
        certificate_hash,
        local_auth,
    )
}

#[test]
fn namespace_hmac_matches_frozen_vector_and_length_framing_defeats_naive_collision() {
    let secret = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
        0x1e, 0x1f,
    ];
    let key = derive_namespace_for_test(&secret, &principal(b"tenant/alice"), SESSION_A).unwrap();
    assert_eq!(
        key.as_bytes(),
        &[
            0x43, 0xdf, 0x8c, 0xfe, 0x95, 0x99, 0x9a, 0x31, 0x08, 0xf8, 0xbf, 0xf4, 0x0b, 0xb0,
            0xee, 0x03, 0x58, 0x92, 0x47, 0x95, 0x85, 0xae, 0xb5, 0xe3, 0x91, 0xb7, 0x30, 0x68,
            0x4d, 0x06, 0x41, 0xb5,
        ]
    );

    let naive_left = [b"a".as_slice(), b"bc".as_slice()].concat();
    let naive_right = [b"ab".as_slice(), b"c".as_slice()].concat();
    assert_eq!(naive_left, naive_right);
    let framed_left = derive_framed_for_test(&secret, b"a", b"bc").unwrap();
    let framed_right = derive_framed_for_test(&secret, b"ab", b"c").unwrap();
    assert!(framed_left != framed_right);
}

#[test]
fn namespace_isolates_principals_and_session_ids_and_rejects_invalid_identifiers() {
    let secret = [0x33; 32];
    let alice_a = derive_namespace_for_test(&secret, &principal(b"tenant-a"), SESSION_A).unwrap();
    let bob_a = derive_namespace_for_test(&secret, &principal(b"tenant-b"), SESSION_A).unwrap();
    let alice_b = derive_namespace_for_test(&secret, &principal(b"tenant-a"), SESSION_B).unwrap();
    assert!(alice_a != bob_a);
    assert!(alice_a != alice_b);

    for invalid in [
        "123e4567-e89b-12d3-a456-426614174000",
        "123E4567-E89B-42D3-A456-426614174000",
        "123e4567e89b42d3a456426614174000",
        "not-a-session-identifier",
    ] {
        assert!(matches!(
            derive_namespace_for_test(&secret, &principal(b"tenant-a"), invalid),
            Err(RegistryError::InvalidSessionIdentifier)
        ));
    }
}

struct StaticResolver(Vec<u8>);

impl PrincipalResolver for StaticResolver {
    fn resolve(
        &self,
        _context: PrincipalContext<'_>,
    ) -> Result<AuthenticatedPrincipal, PrincipalResolveError> {
        AuthenticatedPrincipal::try_from_canonical_bytes(self.0.clone())
    }
}

struct PanickingResolver;

impl PrincipalResolver for PanickingResolver {
    fn resolve(
        &self,
        _context: PrincipalContext<'_>,
    ) -> Result<AuthenticatedPrincipal, PrincipalResolveError> {
        panic!("synthetic resolver panic containing alice@example.invalid")
    }
}

struct ContextResolver;

impl PrincipalResolver for ContextResolver {
    fn resolve(
        &self,
        context: PrincipalContext<'_>,
    ) -> Result<AuthenticatedPrincipal, PrincipalResolveError> {
        let mut canonical = vec![context.listener_scope() as u8, context.peer_scope() as u8];
        if let Some(hash) = context.tls_client_certificate_hash() {
            hash.with_bytes(|bytes| canonical.extend_from_slice(bytes));
        }
        if let Some(credential) = context.local_auth_credential() {
            credential.with_bytes(|bytes| canonical.extend_from_slice(bytes));
        }
        AuthenticatedPrincipal::try_from_canonical_bytes(canonical)
    }
}

#[test]
fn resolver_boundary_handles_success_empty_oversized_and_panic_without_raw_sources() {
    // Held across the whole test: the counting hook installed below is only
    // observable if no concurrent `invoke_resolver` swaps the global hook.
    let _panic_hook_guard = lock_panic_hook_for_test();
    let resolved = invoke_resolver(
        &StaticResolver(b"tenant-a".to_vec()),
        loopback_context(None, None).unwrap(),
    )
    .unwrap();
    assert_eq!(
        resolved.with_canonical_bytes(|bytes| bytes.to_vec()),
        b"tenant-a"
    );

    assert!(matches!(
        invoke_resolver(
            &StaticResolver(Vec::new()),
            loopback_context(None, None).unwrap()
        ),
        Err(PrincipalResolveError::EmptyPrincipal)
    ));
    assert!(matches!(
        invoke_resolver(
            &StaticResolver(vec![0x41; MAX_AUTHENTICATED_PRINCIPAL_BYTES + 1]),
            loopback_context(None, None).unwrap(),
        ),
        Err(PrincipalResolveError::PrincipalTooLong)
    ));
    let hook_hits = Arc::new(AtomicUsize::new(0));
    let prior_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new({
        let hook_hits = Arc::clone(&hook_hits);
        move |_| {
            hook_hits.fetch_add(1, Ordering::SeqCst);
        }
    }));
    let panic_result = invoke_resolver(&PanickingResolver, loopback_context(None, None).unwrap());
    let _ = std::panic::catch_unwind(|| panic!("post-boundary synthetic panic"));
    let installed_test_hook = std::panic::take_hook();
    std::panic::set_hook(prior_hook);
    drop(installed_test_hook);
    assert_eq!(hook_hits.load(Ordering::SeqCst), 1);
    let panic_error = match panic_result {
        Ok(_) => unreachable!("panicking resolver cannot succeed"),
        Err(error) => error,
    };
    assert_eq!(panic_error, PrincipalResolveError::ResolverPanicked);
    assert!(panic_error.source().is_none());
    let rendered = format!("{panic_error} {panic_error:?}");
    assert!(!rendered.contains("alice@example.invalid"));
    assert!(!rendered.contains("synthetic resolver panic"));
}

#[test]
fn context_exposes_only_closed_scopes_and_optional_nonprinting_borrowed_inputs() {
    let _panic_hook_guard = lock_panic_hook_for_test();
    let certificate_hash = [0x42; 32];
    let local_auth = b"synthetic-local-auth";
    let resolved = invoke_resolver(
        &ContextResolver,
        loopback_context(Some(&certificate_hash), Some(local_auth)).unwrap(),
    )
    .unwrap();
    resolved.with_canonical_bytes(|bytes| {
        assert_eq!(
            &bytes[0..2],
            &[ListenerScope::Loopback as u8, PeerScope::Loopback as u8]
        );
        assert_eq!(&bytes[2..34], certificate_hash.as_slice());
        assert_eq!(&bytes[34..], local_auth);
    });

    assert!(matches!(
        loopback_context(None, Some(b"")),
        Err(PrincipalResolveError::InvalidLocalAuthCredential)
    ));
    let oversized = vec![0x41; MAX_LOCAL_AUTH_CREDENTIAL_BYTES + 1];
    assert!(matches!(
        loopback_context(None, Some(&oversized)),
        Err(PrincipalResolveError::InvalidLocalAuthCredential)
    ));
}

#[test]
fn process_local_loopback_resolver_is_stable_per_launch_and_rejects_remote_scope() {
    let _panic_hook_guard = lock_panic_hook_for_test();
    let first = ProcessLocalLoopbackResolver::new().unwrap();
    let second = ProcessLocalLoopbackResolver::new().unwrap();
    let first_a = invoke_resolver(&first, loopback_context(None, None).unwrap()).unwrap();
    let first_b = invoke_resolver(&first, loopback_context(None, None).unwrap()).unwrap();
    let second_a = invoke_resolver(&second, loopback_context(None, None).unwrap()).unwrap();
    assert_eq!(
        first_a.with_canonical_bytes(|bytes| bytes.to_vec()),
        first_b.with_canonical_bytes(|bytes| bytes.to_vec())
    );
    assert_ne!(
        first_a.with_canonical_bytes(|bytes| bytes.to_vec()),
        second_a.with_canonical_bytes(|bytes| bytes.to_vec())
    );

    let remote = PrincipalContext::new(
        ListenerScope::NonLoopback,
        PeerScope::NonLoopback,
        None,
        None,
    )
    .unwrap();
    assert!(matches!(
        invoke_resolver(&first, remote),
        Err(PrincipalResolveError::LoopbackScopeRequired)
    ));
}

#[test]
fn first_use_reuse_capacity_and_raii_lease_accounting_are_finite() {
    let registry = test_registry(config(
        1,
        Duration::from_secs(60),
        4,
        4 * TOMBSTONE_ACCOUNTED_BYTES,
        Duration::from_secs(60),
    ));
    let now = Instant::now();
    let first = registry
        .acquire_at(principal(b"tenant-a"), SESSION_A, None, now)
        .unwrap();
    let second = registry
        .acquire_at(principal(b"tenant-a"), SESSION_A, Some(first.epoch()), now)
        .unwrap();
    assert_eq!(first.epoch(), second.epoch());
    assert_eq!(registry.stats().active_entries(), 1);
    assert_eq!(registry.stats().total_leases(), 2);
    assert!(matches!(
        registry.acquire_at(principal(b"tenant-a"), SESSION_B, None, now),
        Err(RegistryError::ActiveCapacityReached)
    ));
    drop(second);
    assert_eq!(registry.stats().total_leases(), 1);
    drop(first);
    assert_eq!(registry.stats().total_leases(), 0);
}

#[test]
fn expiry_never_replaces_a_leased_entry_then_retains_a_tombstone() {
    let registry = test_registry(config(
        2,
        Duration::from_secs(10),
        4,
        4 * TOMBSTONE_ACCOUNTED_BYTES,
        Duration::from_secs(60),
    ));
    let start = Instant::now();
    let lease = registry
        .acquire_at(principal(b"tenant-a"), SESSION_A, None, start)
        .unwrap();
    let epoch = lease.epoch();
    let expired = start + Duration::from_secs(11);
    registry.cleanup_at(expired);
    assert_eq!(registry.stats().active_entries(), 1);
    assert_eq!(registry.stats().tombstones(), 0);
    assert!(matches!(
        registry.acquire_at(principal(b"tenant-a"), SESSION_A, None, expired),
        Err(RegistryError::SessionExpired)
    ));

    drop(lease);
    registry.cleanup_at(expired);
    assert_eq!(registry.stats().active_entries(), 0);
    assert_eq!(registry.stats().tombstones(), 1);
    assert!(matches!(
        registry.acquire_at(principal(b"tenant-a"), SESSION_A, None, expired),
        Err(RegistryError::SessionExpired)
    ));
    assert!(matches!(
        registry.acquire_at(principal(b"tenant-a"), SESSION_A, Some(epoch + 1), expired,),
        Err(RegistryError::SessionGenerationConflict)
    ));
}

fn expire_one(registry: &SessionRegistry, session_id: &str, acquired_at: Instant) -> u64 {
    let lease = registry
        .acquire_at(principal(b"tenant-a"), session_id, None, acquired_at)
        .unwrap();
    let epoch = lease.epoch();
    drop(lease);
    registry.cleanup_at(acquired_at + Duration::from_secs(11));
    epoch
}

#[test]
fn tombstone_lru_count_and_byte_budgets_are_separate_and_bounded() {
    let start = Instant::now();
    let count_registry = test_registry(config(
        4,
        Duration::from_secs(10),
        2,
        2 * TOMBSTONE_ACCOUNTED_BYTES,
        Duration::from_secs(100),
    ));
    expire_one(&count_registry, SESSION_A, start);
    expire_one(&count_registry, SESSION_B, start + Duration::from_secs(12));
    assert!(matches!(
        count_registry.acquire_at(
            principal(b"tenant-a"),
            SESSION_A,
            None,
            start + Duration::from_secs(24),
        ),
        Err(RegistryError::SessionExpired)
    ));
    expire_one(&count_registry, SESSION_C, start + Duration::from_secs(25));
    assert_eq!(count_registry.stats().tombstones(), 2);
    assert_eq!(
        count_registry.stats().tombstone_bytes(),
        2 * TOMBSTONE_ACCOUNTED_BYTES
    );
    let reclaimed_b = count_registry
        .acquire_at(
            principal(b"tenant-a"),
            SESSION_B,
            None,
            start + Duration::from_secs(37),
        )
        .unwrap();
    drop(reclaimed_b);
    assert!(matches!(
        count_registry.acquire_at(
            principal(b"tenant-a"),
            SESSION_A,
            None,
            start + Duration::from_secs(37),
        ),
        Err(RegistryError::SessionExpired)
    ));

    let byte_registry = test_registry(config(
        4,
        Duration::from_secs(10),
        10,
        TOMBSTONE_ACCOUNTED_BYTES,
        Duration::from_secs(100),
    ));
    expire_one(&byte_registry, SESSION_A, start);
    expire_one(&byte_registry, SESSION_B, start + Duration::from_secs(12));
    assert_eq!(byte_registry.stats().tombstones(), 1);
    assert_eq!(
        byte_registry.stats().tombstone_bytes(),
        TOMBSTONE_ACCOUNTED_BYTES
    );
    let reclaimed_a = byte_registry
        .acquire_at(
            principal(b"tenant-a"),
            SESSION_A,
            None,
            start + Duration::from_secs(24),
        )
        .unwrap();
    drop(reclaimed_a);
    assert!(matches!(
        byte_registry.acquire_at(
            principal(b"tenant-a"),
            SESSION_B,
            None,
            start + Duration::from_secs(24),
        ),
        Err(RegistryError::SessionExpired)
    ));
}

#[test]
fn tombstone_ttl_reclamation_creates_a_fresh_generation_and_old_tokens_fail() {
    let registry = test_registry(config(
        2,
        Duration::from_secs(10),
        4,
        4 * TOMBSTONE_ACCOUNTED_BYTES,
        Duration::from_secs(5),
    ));
    let start = Instant::now();
    let old = registry
        .acquire_at(principal(b"tenant-a"), SESSION_A, None, start)
        .unwrap();
    let old_epoch = old.epoch();
    let token = old
        .session()
        .tokenize(&PiiClass::Email, "alice@example.invalid")
        .unwrap();
    drop(old);
    registry.cleanup_at(start + Duration::from_secs(11));
    assert_eq!(registry.stats().tombstones(), 1);
    registry.cleanup_at(start + Duration::from_secs(17));
    assert_eq!(registry.stats().tombstones(), 0);
    assert!(matches!(
        registry.acquire_at(
            principal(b"tenant-a"),
            SESSION_A,
            Some(old_epoch),
            start + Duration::from_secs(17),
        ),
        Err(RegistryError::SessionGenerationConflict)
    ));

    let fresh = registry
        .acquire_at(
            principal(b"tenant-a"),
            SESSION_A,
            None,
            start + Duration::from_secs(17),
        )
        .unwrap();
    assert_ne!(fresh.epoch(), old_epoch);
    assert!(fresh.session().restore_strict(&token).is_err());
}

#[test]
fn simultaneous_first_use_produces_one_entry_and_one_epoch() {
    const WORKERS: usize = 8;
    let registry = Arc::new(test_registry(config(
        2,
        Duration::from_secs(60),
        4,
        4 * TOMBSTONE_ACCOUNTED_BYTES,
        Duration::from_secs(60),
    )));
    let ready = Arc::new(Barrier::new(WORKERS + 1));
    let release = Arc::new(Barrier::new(WORKERS + 1));
    let (send, receive) = mpsc::channel();

    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            let registry = Arc::clone(&registry);
            let ready = Arc::clone(&ready);
            let release = Arc::clone(&release);
            let send = send.clone();
            scope.spawn(move || {
                let lease = registry
                    .acquire(principal(b"tenant-a"), SESSION_A, None)
                    .unwrap();
                send.send(lease.epoch()).unwrap();
                ready.wait();
                release.wait();
                drop(lease);
            });
        }
        drop(send);
        ready.wait();
        assert_eq!(registry.stats().active_entries(), 1);
        assert_eq!(registry.stats().total_leases(), WORKERS);
        let epochs: Vec<_> = (0..WORKERS).map(|_| receive.recv().unwrap()).collect();
        assert_eq!(epochs.len(), WORKERS);
        assert!(epochs.iter().all(|epoch| *epoch == epochs[0]));
        release.wait();
    });
    assert_eq!(registry.stats().total_leases(), 0);
}

#[tokio::test]
async fn per_entry_request_mutex_serializes_same_namespace_only() {
    let registry = test_registry(config(
        2,
        Duration::from_secs(60),
        4,
        4 * TOMBSTONE_ACCOUNTED_BYTES,
        Duration::from_secs(60),
    ));
    let first = registry
        .acquire(principal(b"tenant-a"), SESSION_A, None)
        .unwrap();
    let second = registry
        .acquire(principal(b"tenant-a"), SESSION_A, Some(first.epoch()))
        .unwrap();
    let other = registry
        .acquire(principal(b"tenant-a"), SESSION_D, None)
        .unwrap();
    let first_guard = first.lock_request().await;
    assert!(second.try_lock_request().is_none());
    assert!(other.try_lock_request().is_some());
    drop(first_guard);
    assert!(second.try_lock_request().is_some());
}

#[test]
fn registry_construction_uses_os_randomness_and_errors_are_closed() {
    let registry = SessionRegistry::new(config(
        1,
        Duration::from_secs(60),
        1,
        TOMBSTONE_ACCOUNTED_BYTES,
        Duration::from_secs(60),
    ));
    assert!(registry.is_ok());

    for error in [
        RegistryError::InvalidSessionIdentifier,
        RegistryError::ActiveCapacityReached,
        RegistryError::SessionExpired,
        RegistryError::SessionGenerationConflict,
        RegistryError::EntropyUnavailable,
        RegistryError::SessionCreationFailed,
        RegistryError::EpochExhausted,
        RegistryError::InternalFailure,
    ] {
        assert!(error.source().is_none());
        let rendered = format!("{error} {error:?}");
        for forbidden in [
            "alice@example.invalid",
            "synthetic-local-auth",
            SESSION_A,
            "synthetic-token-fragment",
            "https://example.invalid",
            "x-gaze-session-id",
        ] {
            assert!(!rendered.contains(forbidden));
        }
    }
}

#[test]
fn core_session_fixture_uses_an_independent_ephemeral_namespace() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let token = session
        .tokenize(&PiiClass::Email, "alice@example.invalid")
        .unwrap();
    assert_eq!(
        session.restore_strict(&token).unwrap(),
        "alice@example.invalid"
    );
}
