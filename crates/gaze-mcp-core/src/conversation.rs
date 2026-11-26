//! Host-owned registry of per-conversation dispatch hosts.
//!
//! A [`gaze::Session`] is the unit of isolation: its mappings, HMAC labels and
//! restore authority belong to one conversation. [`ConversationRegistry`] maps a
//! host-authenticated [`ConversationId`] to a host built with a fresh Session, so
//! two conversations never share mappings and one conversation's tokens are
//! unowned (and refused by strict restore) in another.
//!
//! The id must come from the host, for example an authenticated HTTP middleware
//! or the transport connection. A model-written value, such as a `_session_id`
//! tool argument or `_meta` field, is never a conversation id.
//!
//! Lifecycle:
//! - **Open:** the first call for an id builds its host through the factory.
//! - **Close:** [`ConversationRegistry::close`] drops the entry. A later call with
//!   the same id starts a new, empty Session.
//! - **Expiry:** an entry unused for [`ConversationLimits::idle_timeout`] is
//!   dropped on the next registry access, with the same effect as close.
//! - **Capacity:** at [`ConversationLimits::max_conversations`] live entries a new
//!   id is refused with [`ConversationError::Capacity`]. Live conversations are
//!   never evicted to make room, because that would silently drop their restore
//!   authority.
//! - **Restart:** the registry is memory-only. After a restart every id starts
//!   empty, and tokens issued before it are unowned.
//!
//! A call already in flight keeps its host until it returns, even when its
//! conversation is closed or expires meanwhile.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::frontend::DispatchHost;

/// Host-authenticated identity of one conversation. `Debug` never prints the id.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ConversationId(Arc<str>);

impl ConversationId {
    /// Wrap an id the host has authenticated. Never pass model-written input.
    pub fn new(id: impl Into<String>) -> Self {
        Self(Arc::from(id.into()))
    }

    /// A fresh unguessable id, for hosts whose conversation is the transport
    /// connection itself.
    pub fn random() -> Self {
        Self::new(ulid::Ulid::new().to_string())
    }

    /// The id as the host supplied it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ConversationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ConversationId([redacted])")
    }
}

/// Bounds on the live conversations a [`ConversationRegistry`] holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConversationLimits {
    /// Most live conversations. Opening one more is refused.
    pub max_conversations: usize,
    /// A conversation unused for this long is closed.
    pub idle_timeout: Duration,
}

impl ConversationLimits {
    /// Default capacity: 64 live conversations.
    pub const DEFAULT_MAX_CONVERSATIONS: usize = 64;
    /// Default idle timeout: 30 minutes.
    pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

    /// Limits with the given capacity and idle timeout.
    pub fn new(max_conversations: usize, idle_timeout: Duration) -> Self {
        Self {
            max_conversations,
            idle_timeout,
        }
    }
}

impl Default for ConversationLimits {
    fn default() -> Self {
        Self::new(Self::DEFAULT_MAX_CONVERSATIONS, Self::DEFAULT_IDLE_TIMEOUT)
    }
}

/// Why a conversation host could not be returned.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConversationError {
    /// The registry already holds its maximum number of live conversations.
    #[error("conversation limit reached")]
    Capacity,
    /// The factory failed to build a host for a new conversation.
    #[error("conversation host construction failed: {0}")]
    Factory(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Builds one isolated host, with its own fresh [`gaze::Session`], per conversation.
/// Resident pipelines, registries and model handles may be shared between hosts.
pub type ConversationFactory = dyn Fn() -> Result<Arc<dyn DispatchHost>, Box<dyn std::error::Error + Send + Sync>>
    + Send
    + Sync;

struct Entry {
    host: Arc<dyn DispatchHost>,
    last_used: Instant,
}

/// Host-owned map from [`ConversationId`] to that conversation's dispatch host.
/// See the [module documentation](self) for the lifecycle contract.
pub struct ConversationRegistry {
    factory: Box<ConversationFactory>,
    limits: ConversationLimits,
    entries: Mutex<HashMap<ConversationId, Entry>>,
}

impl ConversationRegistry {
    /// A registry that builds each new conversation's host with `factory`.
    pub fn new<F>(factory: F, limits: ConversationLimits) -> Self
    where
        F: Fn() -> Result<Arc<dyn DispatchHost>, Box<dyn std::error::Error + Send + Sync>>
            + Send
            + Sync
            + 'static,
    {
        Self {
            factory: Box::new(factory),
            limits,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// The limits this registry enforces.
    pub fn limits(&self) -> ConversationLimits {
        self.limits
    }

    /// The host for `id`, opening the conversation when it is not live.
    pub fn host(&self, id: &ConversationId) -> Result<Arc<dyn DispatchHost>, ConversationError> {
        let now = Instant::now();
        let mut entries = self.lock();
        entries.retain(|_, entry| now.duration_since(entry.last_used) < self.limits.idle_timeout);
        if let Some(entry) = entries.get_mut(id) {
            entry.last_used = now;
            return Ok(entry.host.clone());
        }
        if entries.len() >= self.limits.max_conversations {
            return Err(ConversationError::Capacity);
        }
        let host = (self.factory)().map_err(ConversationError::Factory)?;
        entries.insert(
            id.clone(),
            Entry {
                host: host.clone(),
                last_used: now,
            },
        );
        Ok(host)
    }

    /// End the conversation and drop its Session. Returns whether it was live.
    pub fn close(&self, id: &ConversationId) -> bool {
        self.lock().remove(id).is_some()
    }

    /// Number of conversations currently held, including any expired ones not
    /// yet swept by an access.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// True when no conversation is held.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<ConversationId, Entry>> {
        // Entries hold no invariant a panicking holder could break halfway.
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl std::fmt::Debug for ConversationRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConversationRegistry")
            .field("limits", &self.limits)
            .field("live", &self.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DispatchError, Principal, ToolDescriptor, ToolResponse};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Host;

    #[async_trait]
    impl DispatchHost for Host {
        async fn dispatch(
            &self,
            _: &Principal,
            _: &str,
            _: serde_json::Value,
            _: Option<&str>,
            _: crate::HostContextSelection,
        ) -> Result<ToolResponse, DispatchError> {
            unreachable!("registry tests never dispatch")
        }

        fn list_tools(&self) -> Vec<ToolDescriptor> {
            Vec::new()
        }
    }

    fn registry(limits: ConversationLimits) -> (ConversationRegistry, Arc<AtomicUsize>) {
        let built = Arc::new(AtomicUsize::new(0));
        let counter = built.clone();
        let registry = ConversationRegistry::new(
            move || {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(Host) as Arc<dyn DispatchHost>)
            },
            limits,
        );
        (registry, built)
    }

    fn same(a: &Arc<dyn DispatchHost>, b: &Arc<dyn DispatchHost>) -> bool {
        Arc::ptr_eq(a, b)
    }

    #[test]
    fn one_host_per_conversation_reused_across_calls() {
        let (registry, built) = registry(ConversationLimits::default());
        let a = ConversationId::new("a");
        let b = ConversationId::new("b");
        let a1 = registry.host(&a).unwrap();
        let b1 = registry.host(&b).unwrap();
        assert!(!same(&a1, &b1));
        assert!(same(&a1, &registry.host(&a).unwrap()));
        assert_eq!(built.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn close_then_reopen_builds_a_fresh_host() {
        let (registry, built) = registry(ConversationLimits::default());
        let a = ConversationId::new("a");
        let first = registry.host(&a).unwrap();
        assert!(registry.close(&a));
        assert!(!registry.close(&a));
        assert!(registry.is_empty());
        assert!(!same(&first, &registry.host(&a).unwrap()));
        assert_eq!(built.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn capacity_refuses_new_ids_but_serves_live_ones() {
        let (registry, built) = registry(ConversationLimits::new(1, Duration::from_secs(60)));
        let a = ConversationId::new("a");
        let live = registry.host(&a).unwrap();
        assert!(matches!(
            registry.host(&ConversationId::new("b")),
            Err(ConversationError::Capacity)
        ));
        assert!(same(&live, &registry.host(&a).unwrap()));
        registry.close(&a);
        registry.host(&ConversationId::new("b")).unwrap();
        assert_eq!(built.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn idle_conversations_expire_and_free_capacity() {
        let (registry, _) = registry(ConversationLimits::new(1, Duration::from_millis(20)));
        let a = ConversationId::new("a");
        let first = registry.host(&a).unwrap();
        std::thread::sleep(Duration::from_millis(40));
        // The expired entry no longer counts against capacity.
        registry.host(&ConversationId::new("b")).unwrap();
        assert!(matches!(
            registry.host(&a),
            Err(ConversationError::Capacity)
        ));
        std::thread::sleep(Duration::from_millis(40));
        assert!(!same(&first, &registry.host(&a).unwrap()));
    }

    #[test]
    fn factory_failure_opens_nothing() {
        let registry = ConversationRegistry::new(
            || Err("synthetic factory failure".into()),
            ConversationLimits::default(),
        );
        assert!(matches!(
            registry.host(&ConversationId::new("a")),
            Err(ConversationError::Factory(_))
        ));
        assert!(registry.is_empty());
    }

    #[test]
    fn debug_never_prints_the_id() {
        let id = ConversationId::new("private-conversation-canary");
        let (registry, _) = registry(ConversationLimits::default());
        registry.host(&id).unwrap();
        assert!(!format!("{id:?} {registry:?}").contains("canary"));
    }
}
