use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use gaze::{Scope, Session};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use crate::adapter::SessionRegistryConfig;
use crate::principal::AuthenticatedPrincipal;

type HmacSha256 = Hmac<Sha256>;

const SESSION_NAMESPACE_DOMAIN: &[u8] = b"gaze-session-v1";
pub(crate) const TOMBSTONE_ACCOUNTED_BYTES: usize = 32 + 8;

/// Closed, sanitized finite-registry failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RegistryError {
    InvalidSessionIdentifier,
    ActiveCapacityReached,
    SessionExpired,
    SessionGenerationConflict,
    EntropyUnavailable,
    SessionCreationFailed,
    EpochExhausted,
    InternalFailure,
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidSessionIdentifier => "registry_session_identifier_invalid",
            Self::ActiveCapacityReached => "registry_active_capacity_reached",
            Self::SessionExpired => "registry_session_expired",
            Self::SessionGenerationConflict => "registry_session_generation_conflict",
            Self::EntropyUnavailable => "registry_entropy_unavailable",
            Self::SessionCreationFailed => "registry_session_creation_failed",
            Self::EpochExhausted => "registry_epoch_exhausted",
            Self::InternalFailure => "registry_internal_failure",
        })
    }
}

impl std::error::Error for RegistryError {}

#[derive(Clone, Eq)]
pub(crate) struct NamespaceKey([u8; 32]);

impl PartialEq for NamespaceKey {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Hash for NamespaceKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl NamespaceKey {
    pub(crate) const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryState {
    Active,
}

struct RegistryEntry {
    session: Arc<Session>,
    request_mutex: Arc<AsyncMutex<()>>,
    epoch: u64,
    lease_count: usize,
    expires_at: Instant,
    state: EntryState,
}

struct Tombstone {
    epoch: u64,
    expires_at: Instant,
    last_used_at: Instant,
    lru_order: u64,
}

struct RegistryState {
    active: HashMap<NamespaceKey, RegistryEntry>,
    tombstones: HashMap<NamespaceKey, Tombstone>,
    tombstone_bytes: usize,
    next_epoch: u64,
    lru_order: u64,
}

impl RegistryState {
    fn new() -> Self {
        Self {
            active: HashMap::new(),
            tombstones: HashMap::new(),
            tombstone_bytes: 0,
            next_epoch: 0,
            lru_order: 0,
        }
    }

    fn next_lru_order(&mut self) -> u64 {
        self.lru_order = self.lru_order.saturating_add(1);
        self.lru_order
    }

    fn remove_tombstone(&mut self, key: &NamespaceKey) {
        if self.tombstones.remove(key).is_some() {
            self.tombstone_bytes = self
                .tombstone_bytes
                .saturating_sub(TOMBSTONE_ACCOUNTED_BYTES);
        }
    }

    fn remove_expired_tombstones(&mut self, now: Instant) {
        let expired: Vec<_> = self
            .tombstones
            .iter()
            .filter(|(_, tombstone)| tombstone.expires_at <= now)
            .map(|(key, _)| key.clone())
            .collect();
        for key in expired {
            self.remove_tombstone(&key);
        }
    }

    fn add_tombstone(
        &mut self,
        key: NamespaceKey,
        epoch: u64,
        now: Instant,
        config: SessionRegistryConfig,
    ) {
        self.remove_tombstone(&key);
        let expires_at = checked_deadline(now, config.tombstone_ttl());
        let lru_order = self.next_lru_order();
        self.tombstones.insert(
            key,
            Tombstone {
                epoch,
                expires_at,
                last_used_at: now,
                lru_order,
            },
        );
        self.tombstone_bytes = self
            .tombstone_bytes
            .saturating_add(TOMBSTONE_ACCOUNTED_BYTES);
        self.enforce_tombstone_budgets(config);
    }

    fn enforce_tombstone_budgets(&mut self, config: SessionRegistryConfig) {
        while self.tombstones.len() > config.max_tombstones()
            || self.tombstone_bytes > config.max_tombstone_bytes()
        {
            let oldest = self
                .tombstones
                .iter()
                .min_by_key(|(_, tombstone)| (tombstone.last_used_at, tombstone.lru_order))
                .map(|(key, _)| key.clone());
            let Some(oldest) = oldest else {
                self.tombstone_bytes = 0;
                break;
            };
            self.remove_tombstone(&oldest);
        }
    }

    fn expire_releasable_entries(&mut self, now: Instant, config: SessionRegistryConfig) {
        let expired: Vec<_> = self
            .active
            .iter()
            .filter(|(_, entry)| entry.expires_at <= now && entry.lease_count == 0)
            .map(|(key, _)| key.clone())
            .collect();
        for key in expired {
            if let Some(entry) = self.active.remove(&key) {
                self.add_tombstone(key, entry.epoch, now, config);
            }
        }
    }

    fn cleanup(&mut self, now: Instant, config: SessionRegistryConfig) {
        self.remove_expired_tombstones(now);
        self.expire_releasable_entries(now, config);
    }
}

struct RegistryInner {
    config: SessionRegistryConfig,
    process_secret: Zeroizing<[u8; 32]>,
    state: Mutex<RegistryState>,
}

impl RegistryInner {
    fn lock_state(&self) -> MutexGuard<'_, RegistryState> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn release(&self, key: &NamespaceKey, epoch: u64) {
        let mut state = self.lock_state();
        if let Some(entry) = state.active.get_mut(key) {
            if entry.epoch == epoch && entry.lease_count > 0 {
                entry.lease_count -= 1;
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct SessionRegistry {
    inner: Arc<RegistryInner>,
}

impl SessionRegistry {
    pub(crate) fn new(config: SessionRegistryConfig) -> Result<Self, RegistryError> {
        let mut process_secret = [0_u8; 32];
        if getrandom::fill(&mut process_secret).is_err() {
            process_secret.zeroize();
            return Err(RegistryError::EntropyUnavailable);
        }
        Ok(Self::with_secret(config, process_secret))
    }

    pub(crate) fn with_secret(config: SessionRegistryConfig, process_secret: [u8; 32]) -> Self {
        Self {
            inner: Arc::new(RegistryInner {
                config,
                process_secret: Zeroizing::new(process_secret),
                state: Mutex::new(RegistryState::new()),
            }),
        }
    }

    pub(crate) fn acquire(
        &self,
        principal: AuthenticatedPrincipal,
        session_identifier: &str,
        expected_epoch: Option<u64>,
    ) -> Result<SessionLease, RegistryError> {
        self.acquire_at(
            principal,
            session_identifier,
            expected_epoch,
            Instant::now(),
        )
    }

    pub(crate) fn acquire_at(
        &self,
        principal: AuthenticatedPrincipal,
        session_identifier: &str,
        expected_epoch: Option<u64>,
        now: Instant,
    ) -> Result<SessionLease, RegistryError> {
        let key = derive_namespace(&self.inner.process_secret, &principal, session_identifier);
        drop(principal);
        let key = key?;

        let mut state = self.inner.lock_state();
        state.cleanup(now, self.inner.config);

        if let Some(entry) = state.active.get_mut(&key) {
            if expected_epoch.is_some_and(|expected| expected != entry.epoch) {
                return Err(RegistryError::SessionGenerationConflict);
            }
            if entry.expires_at <= now {
                return Err(RegistryError::SessionExpired);
            }
            if entry.state != EntryState::Active {
                return Err(RegistryError::InternalFailure);
            }
            entry.lease_count = entry
                .lease_count
                .checked_add(1)
                .ok_or(RegistryError::InternalFailure)?;
            entry.expires_at = checked_deadline(now, self.inner.config.session_ttl());
            return Ok(SessionLease::new(
                Arc::clone(&self.inner),
                key,
                entry.epoch,
                Arc::clone(&entry.session),
                Arc::clone(&entry.request_mutex),
            ));
        }

        if state.tombstones.contains_key(&key) {
            let lru_order = state.next_lru_order();
            let tombstone = state
                .tombstones
                .get_mut(&key)
                .ok_or(RegistryError::InternalFailure)?;
            tombstone.last_used_at = now;
            tombstone.lru_order = lru_order;
            if expected_epoch.is_some_and(|expected| expected != tombstone.epoch) {
                return Err(RegistryError::SessionGenerationConflict);
            }
            return Err(RegistryError::SessionExpired);
        }

        if expected_epoch.is_some() {
            return Err(RegistryError::SessionGenerationConflict);
        }
        if state.active.len() >= self.inner.config.max_active_sessions() {
            return Err(RegistryError::ActiveCapacityReached);
        }
        let epoch = state
            .next_epoch
            .checked_add(1)
            .ok_or(RegistryError::EpochExhausted)?;
        let session = Arc::new(
            Session::new(Scope::Ephemeral).map_err(|_| RegistryError::SessionCreationFailed)?,
        );
        let request_mutex = Arc::new(AsyncMutex::new(()));
        state.next_epoch = epoch;
        state.active.insert(
            key.clone(),
            RegistryEntry {
                session: Arc::clone(&session),
                request_mutex: Arc::clone(&request_mutex),
                epoch,
                lease_count: 1,
                expires_at: checked_deadline(now, self.inner.config.session_ttl()),
                state: EntryState::Active,
            },
        );
        Ok(SessionLease::new(
            Arc::clone(&self.inner),
            key,
            epoch,
            session,
            request_mutex,
        ))
    }

    pub(crate) fn cleanup_at(&self, now: Instant) {
        self.inner.lock_state().cleanup(now, self.inner.config);
    }

    pub(crate) fn stats(&self) -> RegistryStats {
        let state = self.inner.lock_state();
        let total_leases = state.active.values().fold(0_usize, |total, entry| {
            total.saturating_add(entry.lease_count)
        });
        RegistryStats {
            active_entries: state.active.len(),
            tombstones: state.tombstones.len(),
            tombstone_bytes: state.tombstone_bytes,
            total_leases,
        }
    }
}

pub(crate) struct SessionLease {
    registry: Arc<RegistryInner>,
    key: NamespaceKey,
    epoch: u64,
    session: Arc<Session>,
    request_mutex: Arc<AsyncMutex<()>>,
    released: bool,
}

impl SessionLease {
    fn new(
        registry: Arc<RegistryInner>,
        key: NamespaceKey,
        epoch: u64,
        session: Arc<Session>,
        request_mutex: Arc<AsyncMutex<()>>,
    ) -> Self {
        Self {
            registry,
            key,
            epoch,
            session,
            request_mutex,
            released: false,
        }
    }

    pub(crate) const fn epoch(&self) -> u64 {
        self.epoch
    }

    pub(crate) fn session(&self) -> &Session {
        &self.session
    }

    pub(crate) async fn lock_request(&self) -> OwnedMutexGuard<()> {
        Arc::clone(&self.request_mutex).lock_owned().await
    }

    pub(crate) fn try_lock_request(&self) -> Option<OwnedMutexGuard<()>> {
        Arc::clone(&self.request_mutex).try_lock_owned().ok()
    }
}

impl Drop for SessionLease {
    fn drop(&mut self) {
        if !self.released {
            self.registry.release(&self.key, self.epoch);
            self.released = true;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RegistryStats {
    active_entries: usize,
    tombstones: usize,
    tombstone_bytes: usize,
    total_leases: usize,
}

impl RegistryStats {
    pub(crate) const fn active_entries(self) -> usize {
        self.active_entries
    }

    pub(crate) const fn tombstones(self) -> usize {
        self.tombstones
    }

    pub(crate) const fn tombstone_bytes(self) -> usize {
        self.tombstone_bytes
    }

    pub(crate) const fn total_leases(self) -> usize {
        self.total_leases
    }
}

fn checked_deadline(now: Instant, ttl: std::time::Duration) -> Instant {
    match now.checked_add(ttl) {
        Some(deadline) => deadline,
        None => now,
    }
}

fn parse_session_identifier(value: &str) -> Result<[u8; 16], RegistryError> {
    let parsed = Uuid::parse_str(value).map_err(|_| RegistryError::InvalidSessionIdentifier)?;
    if parsed.get_version_num() != 4 || parsed.hyphenated().to_string() != value {
        return Err(RegistryError::InvalidSessionIdentifier);
    }
    Ok(*parsed.as_bytes())
}

fn derive_namespace(
    secret: &[u8; 32],
    principal: &AuthenticatedPrincipal,
    session_identifier: &str,
) -> Result<NamespaceKey, RegistryError> {
    let session_bytes = parse_session_identifier(session_identifier)?;
    principal.with_canonical_bytes(|principal_bytes| {
        derive_framed(secret, principal_bytes, &session_bytes)
    })
}

fn derive_framed(
    secret: &[u8; 32],
    principal_bytes: &[u8],
    session_bytes: &[u8],
) -> Result<NamespaceKey, RegistryError> {
    let principal_len =
        u32::try_from(principal_bytes.len()).map_err(|_| RegistryError::InternalFailure)?;
    let mut hmac =
        <HmacSha256 as Mac>::new_from_slice(secret).map_err(|_| RegistryError::InternalFailure)?;
    hmac.update(SESSION_NAMESPACE_DOMAIN);
    hmac.update(&principal_len.to_be_bytes());
    hmac.update(principal_bytes);
    hmac.update(session_bytes);
    let finalized = hmac.finalize().into_bytes();
    let mut digest = [0_u8; 32];
    digest.copy_from_slice(&finalized);
    Ok(NamespaceKey(digest))
}

pub(crate) fn derive_namespace_for_test(
    secret: &[u8; 32],
    principal: &AuthenticatedPrincipal,
    session_identifier: &str,
) -> Result<NamespaceKey, RegistryError> {
    derive_namespace(secret, principal, session_identifier)
}

pub(crate) fn derive_framed_for_test(
    secret: &[u8; 32],
    principal_bytes: &[u8],
    session_bytes: &[u8],
) -> Result<NamespaceKey, RegistryError> {
    derive_framed(secret, principal_bytes, session_bytes)
}
