use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::RngCore;
use regex::Regex;
use secrecy::{ExposeSecret, SecretBox};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

use crate::detector::PiiClass;
use crate::policy::{Policy, SessionScope};
use crate::{Error, Result};
use gaze_types::DocumentExtension;

const DEFAULT_PERSISTENT_TTL_SECS: u64 = 86_400;
const DEFAULT_COUNTER_FAMILY: &str = "counter";
const SNAPSHOT_VERSION_V2: u8 = 2;
const SNAPSHOT_VERSION_V3: u8 = 3;
const SNAPSHOT_VERSION_V4: u8 = 4;
const SNAPSHOT_VERSION_V5: u8 = 5;

pub type RestoreError = Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RestoreEventKind {
    ManifestBypass,
    FreshPiiDetected,
}

impl RestoreEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ManifestBypass => "manifest_bypass",
            Self::FreshPiiDetected => "fresh_pii_detected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RestoreEvent {
    pub kind: RestoreEventKind,
    pub class: PiiClass,
    pub raw_sha256: String,
    pub location: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedRestoreToken {
    pub class: PiiClass,
    pub ordinal: u32,
    pub raw: String,
}

/// Persistence scope of a [`Session`]'s token manifest.
///
/// `Scope` chooses *persistence*, not *isolation*. A [`Session`] instance is the
/// pseudonym namespace boundary; two Sessions never share counters or value-keyed
/// lookups, regardless of which `Scope` variant they use.
///
/// | Variant | Use case | `export()` allowed |
/// |---------|----------|--------------------|
/// | `Ephemeral` | Process-bound one-off redaction; namespace lives until the Session is dropped | No |
/// | `Conversation(id)` | Keyed multi-turn LLM sessions that can be re-opened across process restarts, storage backend-dependent | Yes |
/// | `Persistent { ttl: Duration }` | Long-lived sessions across restarts | Yes |
///
/// Do not share one `Scope::Ephemeral` Session across logical conversations if
/// each conversation should start with independent `Email_1` / `Person_1`
/// counters. Use one [`Session`] per logical isolation boundary.
///
/// `Persistent`'s `ttl` is a [`std::time::Duration`]. `SensitiveSnapshot`s exported from a
/// persistent session carry the TTL; [`Session::import`] returns `Error::BlobExpired { .. }` once
/// the deadline has elapsed.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Scope {
    Ephemeral,
    Conversation(String),
    Persistent { ttl: Duration },
}

/// Serializable snapshot of a [`Session`]'s token manifest.
///
/// Produced by [`Session::export`] and consumed by [`Session::import`]. Carries the full
/// token-to-PII mapping plus a session signing key - treat it as sensitive as the original
/// document.
///
/// **Storage:** the snapshot is delivered as raw bytes via [`Self::into_bytes`] and reconstructed
/// via `SensitiveSnapshot::from(Vec<u8>)`. There is no `Serialize`/`Deserialize` impl - bytes are
/// the wire format. Encrypt at rest, bind to a conversation/user, enforce a TTL.
///
/// **Must not:** send to the LLM, analytics, browser clients, logs, or support tickets.
///
/// The audit log (`gaze-audit`) is **not** a restore source; only this snapshot can reconstruct
/// original PII values.
#[derive(Debug, Clone)]
pub struct SensitiveSnapshot(Vec<u8>);

impl SensitiveSnapshot {
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl From<Vec<u8>> for SensitiveSnapshot {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
struct TokenKey {
    family: String,
    class: PiiClass,
    raw: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotEntry {
    class: PiiClass,
    raw: String,
    token: String,
    #[serde(default = "default_counter_family")]
    family: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshotEntry {
    pub class: PiiClass,
    pub raw: String,
    pub token: String,
    pub family: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum SnapshotScope {
    Ephemeral,
    Conversation(String),
    Persistent { ttl_secs: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotPayload {
    scope: SnapshotScope,
    #[serde(deserialize_with = "deserialize_session_hex")]
    session_hex: String,
    entries: Vec<SnapshotEntry>,
    #[serde(default)]
    issued_at: u64,
    #[serde(default)]
    next_by_class: Vec<(PiiClass, usize)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    document: Option<DocumentExtension>,
}

#[derive(Debug, Clone)]
pub(crate) struct PrefixCacheHit {
    pub raw_len: usize,
    pub clean_text: String,
    pub manifest: Vec<gaze_types::EmittedTokenSpan>,
}

#[derive(Debug, Clone)]
struct PrefixCacheEntry {
    raw: String,
    clean_text: String,
    manifest: Vec<gaze_types::EmittedTokenSpan>,
}

#[derive(Clone)]
struct SessionState {
    generation: u64,
    next_by_class: HashMap<PiiClass, usize>,
    token_by_value: HashMap<TokenKey, String>,
    value_by_token: HashMap<String, String>,
    prefix_cache: HashMap<u64, PrefixCacheEntry>,
    restore_regex_cache: Option<(u64, Arc<Regex>)>,
}

impl SessionState {
    fn empty() -> Self {
        Self {
            generation: 0,
            next_by_class: HashMap::new(),
            token_by_value: HashMap::new(),
            value_by_token: HashMap::new(),
            prefix_cache: HashMap::new(),
            restore_regex_cache: None,
        }
    }

    fn advance_generation(&mut self) {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("session generation exhausted");
    }
}

struct SessionIdentity {
    scope: Scope,
    session_hex: [u8; 4],
    audit_session_id: String,
    signing_key: SessionKey,
}

/// A strict restoration result with manifest-authorized output coordinates.
///
/// Each range is a UTF-8 byte range in `text` produced by one or more adjacent
/// known manifest replacements. Ranges never authorize provider-origin bytes.
/// The type intentionally has no `Debug`, `Display`, or serialization surface
/// because `text` may contain restored owner PII.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RestoredTextWithProvenance {
    pub text: String,
    pub authorized_output_ranges: Vec<Range<usize>>,
}

/// Closed failures produced while committing a staged session transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionTransactionError {
    GenerationConflict,
}

impl fmt::Display for SessionTransactionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::GenerationConflict => "session_generation_conflict",
        })
    }
}

impl std::error::Error for SessionTransactionError {}

/// Mutable, isolated staging state captured from one [`Session`] generation.
///
/// Fields are private so callers cannot substitute an unrelated session state
/// or protected identity. Dropping without [`Self::commit`] publishes nothing.
pub struct SessionTransaction<'session> {
    session: &'session Session,
    identity: Arc<SessionIdentity>,
    captured_generation: u64,
    staged: SessionState,
}

/// Immutable restore-only state returned by a successful transaction commit.
///
/// The snapshot has no tokenization, cache, manifest, export, or commit API.
/// Later mutation of the live [`Session`] cannot change this snapshot.
///
/// ```compile_fail
/// use gaze::{PiiClass, Scope, Session};
///
/// let session = Session::new(Scope::Ephemeral)?;
/// let mut transaction = session.begin_transaction();
/// transaction.tokenize(&PiiClass::Name, "Dr. Schmidt")?; // fixture-cited(crates/gaze/src/session.rs:session::tests::committed_snapshot_is_stable_after_later_live_mutation)
/// let snapshot = transaction.commit()?;
/// snapshot.tokenize(&PiiClass::Name, "Another Synthetic Name")?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone)]
pub struct CommittedSessionSnapshot {
    identity: Arc<SessionIdentity>,
    state: Arc<SessionState>,
}

/// Owns the token manifest for one pseudonym namespace.
///
/// A `Session` holds the bidirectional map between PII values and their pseudonymous tokens.
/// Create one per conversation and thread it through every [`Pipeline::redact`] call.
///
/// A `Session` is the boundary of a pseudonym namespace. Each new `Session`
/// starts with fresh per-class counters (`Person_1`, `Email_1`, etc.) and a
/// fresh `session_hex` prefix. Two Sessions never share `next_by_class`,
/// `token_by_value`, or `value_by_token`, regardless of `Scope` variant.
///
/// The [`Scope`] variant chooses *persistence*, not *isolation*:
///
/// - `Scope::Ephemeral` is process-bound. Its namespace lives until the
///   Session is dropped. Use it for ad-hoc one-off redaction. Do not share one
///   `Ephemeral` Session across logical conversations if those conversations
///   should have independent counters and value-to-token mappings.
/// - `Scope::Conversation(String)` is a keyed namespace that can be re-opened
///   across process restarts when the adopter stores and imports the session
///   snapshot. Use it for per-conversation chat or per-thread agents.
/// - `Scope::Persistent { .. }` is a long-lived namespace with a TTL encoded in
///   exported snapshots.
///
/// Picking the wrong variant does not break single-call redaction correctness,
/// but it can produce cross-call linkability: identical raw values fed into one
/// Session always map to the same pseudonym, so downstream consumers can link
/// mentions across contexts that the adopter intended to keep independent.
///
/// # Restore workflow
///
/// 1. Call [`Pipeline::redact`] - returns a [`gaze_types::CleanDocument`] and updates the session
///    map.
/// 2. Call [`Session::export`] to produce a [`SensitiveSnapshot`].
/// 3. Persist the raw bytes (`snapshot.into_bytes()`) **encrypted at rest** - they contain
///    original PII.
/// 4. Send only the cleaned text to the LLM.
/// 5. After the LLM responds, call [`Session::import`] with `SensitiveSnapshot::from(bytes)`.
/// 6. For each token in the LLM response, call [`Session::restore_strict`] (or
///    [`Session::restore`]).
///
/// There is no `Pipeline::restore_text` method - full-text restore is performed by scanning tokens
/// with [`crate::token_shape::pattern`] and calling `restore_strict` per token.
///
/// Document workflows use the same restore root. Call [`Session::export_with_extension`] only when
/// writing a `gaze-document` bundle that needs signed integrity hashes and codec provenance; plain
/// text adopters should keep using [`Session::export`].
///
/// # Round-trip example
///
/// ```rust
/// use gaze::{
///     token_shape, Action, ClassRule, CleanDocument, DefaultRule, Detection, Detector, PiiClass,
///     Pipeline, RawDocument, Scope, SensitiveSnapshot, Session,
/// };
///
/// struct ExampleEmailDetector;
///
/// impl Detector for ExampleEmailDetector {
///     fn detect(&self, input: &str) -> Vec<Detection> {
///         let email = "alice@example.invalid"; // fixture-cited(crates/gaze/src/session.rs:session::tests::snapshot_round_trip_two_families_same_class_raw_preserved_under_shared_counter)
///         input
///             .find(email)
///             .map(|start| Detection::new(start..start + email.len(), PiiClass::Email, "docs"))
///             .into_iter()
///             .collect()
///     }
/// }
///
/// let pipeline = Pipeline::builder()
///     .detector(ExampleEmailDetector)
///     .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
///     .rule(DefaultRule::new(Action::Preserve))
///     .build()?;
/// let session = Session::new(Scope::Conversation("conv-1".into()))?;
///
/// let CleanDocument::Text(clean) = pipeline.redact(
///     &session,
///     RawDocument::Text("alice@example.invalid".into()), // fixture-cited(crates/gaze/src/session.rs:session::tests::snapshot_round_trip_two_families_same_class_raw_preserved_under_shared_counter)
/// )? else {
///     panic!("text variant expected");
/// };
///
/// // Export before sending clean text to the LLM. Persist `blob` encrypted at rest.
/// let snapshot = session.export()?;
/// let blob: Vec<u8> = snapshot.into_bytes();
///
/// // Restore on the owner side after the LLM responds.
/// let restored_session = Session::import(SensitiveSnapshot::from(blob))?;
/// let mut restored = String::new();
/// let mut last = 0;
/// for m in token_shape::pattern().find_iter(&clean) {
///     restored.push_str(&clean[last..m.start()]);
///     restored.push_str(&restored_session.restore_strict(m.as_str())?);
///     last = m.end();
/// }
/// restored.push_str(&clean[last..]);
/// assert_eq!(restored, "alice@example.invalid"); // fixture-cited(crates/gaze/src/session.rs:session::tests::snapshot_round_trip_two_families_same_class_raw_preserved_under_shared_counter)
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// See: `docs/explanation/core/session-contract.md` for the full architecture contract.
///
/// [`Pipeline::redact`]: crate::Pipeline::redact
// intentionally not Debug: contains session signing key and token manifest
pub struct Session {
    identity: Arc<SessionIdentity>,
    state: RwLock<Arc<SessionState>>,
}

impl Session {
    pub fn new(scope: Scope) -> Result<Self> {
        Ok(Self {
            identity: Arc::new(SessionIdentity {
                scope,
                session_hex: random_session_hex(),
                audit_session_id: new_audit_session_id(),
                signing_key: SessionKey::generate()?,
            }),
            state: RwLock::new(Arc::new(SessionState::empty())),
        })
    }

    pub fn from_policy(policy: &Policy) -> Result<Self> {
        Self::from_policy_with_ttl_override(policy, None)
    }

    pub fn from_policy_with_ttl_override(
        policy: &Policy,
        ttl_secs_override: Option<u64>,
    ) -> Result<Self> {
        let scope = match policy.session.scope {
            SessionScope::Ephemeral => Scope::Ephemeral,
            SessionScope::Conversation => Scope::Conversation("cli".to_string()),
            SessionScope::Persistent => {
                let ttl_secs = ttl_secs_override
                    .or(policy.session.ttl_secs)
                    .unwrap_or(DEFAULT_PERSISTENT_TTL_SECS);
                Scope::Persistent {
                    ttl: Duration::from_secs(ttl_secs),
                }
            }
        };

        Self::new(scope)
    }

    /// Capture the current complete immutable state for isolated mutation.
    pub fn begin_transaction(&self) -> SessionTransaction<'_> {
        let state = self.state_snapshot();
        SessionTransaction {
            session: self,
            identity: Arc::clone(&self.identity),
            captured_generation: state.generation,
            staged: (*state).clone(),
        }
    }

    fn state_snapshot(&self) -> Arc<SessionState> {
        Arc::clone(
            &self
                .state
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }

    fn mutate_state<R>(&self, update: impl FnOnce(&mut SessionState) -> (R, bool)) -> R {
        let mut boundary = self
            .state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut next = (**boundary).clone();
        let (output, changed) = update(&mut next);
        if changed {
            next.advance_generation();
            *boundary = Arc::new(next);
        }
        output
    }

    pub fn tokenize(&self, class: &PiiClass, raw: &str) -> Result<String> {
        self.tokenize_with_family(DEFAULT_COUNTER_FAMILY, class, raw)
    }

    pub fn tokenize_with_family(
        &self,
        family: &str,
        class: &PiiClass,
        raw: &str,
    ) -> Result<String> {
        self.intern_mapping(Some(family), class, raw, |index| {
            format!("<{}:{}_{}>", self.session_hex(), class.class_name(), index)
        })
    }

    pub fn format_preserving_fake(&self, class: &PiiClass, raw: &str) -> Result<String> {
        self.intern_mapping(None, class, raw, |index| match class {
            PiiClass::Email => format!("email{index}.{}@gaze-fake.invalid", self.session_hex()),
            PiiClass::Name | PiiClass::Location | PiiClass::Organization => format!(
                "{}:{}_{}",
                self.session_hex(),
                class.class_name().to_ascii_lowercase(),
                index
            ),
            // Lowercasing preserves the dedicated `custom:` sentinel namespace
            // for format-preserving fakes, so restore can detect them too.
            PiiClass::Custom(name) => format!("{}:custom:{name}_{index}", self.session_hex()),
        })
    }

    fn intern_mapping<F>(
        &self,
        family: Option<&str>,
        class: &PiiClass,
        raw: &str,
        build: F,
    ) -> Result<String>
    where
        F: FnOnce(usize) -> String,
    {
        let family_key = family.unwrap_or(DEFAULT_COUNTER_FAMILY);
        Ok(
            self.mutate_state(|state| {
                intern_mapping_in_state(state, family_key, class, raw, build)
            }),
        )
    }

    /// Enumerate every live token string emitted by this session.
    ///
    /// Intended for restore-side callers that need to build an exact-literal
    /// alternation regex over the session map (Pass 1 of the two-pass restore
    /// strategy): replacing token-shaped strings via
    /// a class-shape regex alone is unsafe because it either (a) straddles
    /// word boundaries into adjacent text, or (b) misses lowercase
    /// FormatPreserve shapes like `location_1`. Feeding these exact strings
    /// into `regex::escape` and sorting longest-first avoids both pitfalls.
    ///
    /// Returned order is unspecified — callers that rely on longest-first
    /// matching must sort the returned vector themselves.
    pub fn tokens(&self) -> Vec<String> {
        self.state_snapshot()
            .value_by_token
            .keys()
            .cloned()
            .collect()
    }

    /// Returns only the prefix-cache cardinality for cross-crate regression tests.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn prefix_cache_entry_count(&self) -> usize {
        self.state_snapshot().prefix_cache.len()
    }

    pub(crate) fn restore_regex(&self) -> Result<Option<Arc<Regex>>> {
        let captured = self.state_snapshot();
        if let Some((cache_generation, regex)) = &captured.restore_regex_cache {
            if *cache_generation == captured.generation {
                return Ok(Some(Arc::clone(regex)));
            }
        }

        let Some(regex) = build_restore_regex(&captured)? else {
            return Ok(None);
        };

        let mut boundary = self
            .state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if boundary.generation == captured.generation {
            let mut next = (**boundary).clone();
            next.restore_regex_cache = Some((next.generation, Arc::clone(&regex)));
            *boundary = Arc::new(next);
        }
        Ok(Some(regex))
    }

    pub fn contains_token(&self, token: &str) -> bool {
        self.state_snapshot().value_by_token.contains_key(token)
    }

    pub fn session_hex(&self) -> String {
        hex::encode(self.identity.session_hex)
    }

    pub fn audit_session_id(&self) -> &str {
        &self.identity.audit_session_id
    }

    pub(crate) fn lookup_prefix_cache(&self, text: &str) -> Option<PrefixCacheHit> {
        self.state_snapshot()
            .prefix_cache
            .values()
            .filter(|cached| {
                text.starts_with(&cached.raw) && text.is_char_boundary(cached.raw.len())
            })
            .map(|cached| PrefixCacheHit {
                raw_len: cached.raw.len(),
                clean_text: cached.clean_text.clone(),
                manifest: cached.manifest.clone(),
            })
            .max_by_key(|hit| hit.raw_len)
    }

    pub(crate) fn store_prefix_cache(
        &self,
        raw: &str,
        clean_text: &str,
        manifest: &[gaze_types::EmittedTokenSpan],
    ) {
        if raw.is_empty() {
            return;
        }
        self.mutate_state(|state| {
            let changed = store_prefix_cache_in_state(state, raw, clean_text, manifest);
            ((), changed)
        });
    }

    pub fn snapshot_entries(&self) -> Vec<SessionSnapshotEntry> {
        snapshot_entries_from_state(&self.state_snapshot())
    }

    // Original byte spans are preserved by recognizer normalizers per
    // research-855 §Rulepack > Normalization (axis-2 invariant).
    pub fn restore_strict(&self, token: &str) -> Result<String> {
        restore_strict_from_state(&self.state_snapshot(), token)
    }

    pub fn restore_strict_text(&self, text: &str) -> std::result::Result<String, RestoreError> {
        restore_strict_text_with_provenance_from_state(&self.state_snapshot(), text)
            .map(|restored| restored.text)
    }

    pub fn restore_strict_text_with_provenance(
        &self,
        text: &str,
    ) -> std::result::Result<RestoredTextWithProvenance, RestoreError> {
        restore_strict_text_with_provenance_from_state(&self.state_snapshot(), text)
    }

    pub fn restore_strict_text_with_events(
        &self,
        text: &str,
    ) -> std::result::Result<(String, Vec<RestoreEvent>), RestoreError> {
        let state = self.state_snapshot();
        let events =
            restore_boundary_events(text, &authorized_structural_values_from_state(&state));
        let restored = restore_strict_text_with_provenance_from_state(&state, text)?.text;
        Ok((restored, events))
    }

    pub fn restore_boundary_events(&self, text: &str) -> Vec<RestoreEvent> {
        restore_boundary_events(
            text,
            &authorized_structural_values_from_state(&self.state_snapshot()),
        )
    }

    pub fn restore(&self, token: &str) -> Option<String> {
        self.state_snapshot().value_by_token.get(token).cloned()
    }

    pub fn export(&self) -> Result<SensitiveSnapshot> {
        self.export_payload(None)
    }

    /// Export a document-extended snapshot for `gaze-document` bundle manifests.
    ///
    /// Use this instead of [`Session::export`] when writing a document bundle with
    /// `<base>-agent/` files (`clean.md`, `layout.json`, `report.json`, optional
    /// `preview-redacted.png`) plus owner-only `<base>-owner/manifest.bin`. The supplied
    /// [`DocumentExtension`] is serialized inside the signed snapshot payload, so its hashes and
    /// codec audit rows become the integrity root for the agent-facing files. Text-only adopters
    /// should use [`Session::export`]. Current snapshots emit v5 so older readers fail closed
    /// before restore while the signature binds the emitted envelope bytes.
    ///
    /// ```rust
    /// use gaze::{
    ///     CodecAuditRow, CodecCapabilitySet, DocumentExtension, ExtractionDensityPolicy, Scope,
    ///     Session, TextOrigin,
    /// };
    ///
    /// let session = Session::new(Scope::Conversation("doc-1".to_string()))?;
    /// let mut codec = CodecAuditRow::new(
    ///     "gaze.codec.pdf",
    ///     "0.7.0",
    ///     "application/pdf",
    ///     TextOrigin::Hybrid,
    /// );
    /// codec.advertised = CodecCapabilitySet::new(true, true, true, false);
    /// codec.delivered = CodecCapabilitySet::new(true, true, false, false);
    /// codec.extraction_density_policy = ExtractionDensityPolicy::Required(1.0);
    /// let extension = DocumentExtension::builder(1)
    ///     .clean_md_sha256([1; 32])
    ///     .layout_json_sha256([2; 32])
    ///     .report_json_sha256([3; 32])
    ///     .page_count(4)
    ///     .audit_session_id(session.audit_session_id())
    ///     .codec_audit(vec![codec])
    ///     .build()?;
    ///
    /// let manifest_bin = session.export_with_extension(extension)?.into_bytes();
    /// # assert_eq!(manifest_bin[0], 5);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Failure modes match [`Session::export`]: ephemeral sessions return
    /// [`Error::ExportForbidden`], JSON encoding failures return [`Error::SnapshotDecode`], and
    /// empty integrity-binding hashes or audit session ids return
    /// [`Error::EmptyDocumentIntegrity`]. `page_count == 0` is allowed because text-only or
    /// degenerate bundles may have no pages while still binding file hashes. Callers must treat the
    /// returned [`SensitiveSnapshot`] as owner-only because it carries the full token-to-PII restore
    /// map. Bundle layout details live in `docs/explanation/document/document-extension.md`.
    pub fn export_with_extension(&self, extension: DocumentExtension) -> Result<SensitiveSnapshot> {
        if extension.clean_md_sha256 == [0; 32]
            || extension.layout_json_sha256 == [0; 32]
            || extension.report_json_sha256 == [0; 32]
            || extension.audit_session_id.is_empty()
        {
            return Err(Error::EmptyDocumentIntegrity);
        }
        self.export_payload(Some(extension))
    }

    fn export_payload(&self, document: Option<DocumentExtension>) -> Result<SensitiveSnapshot> {
        if matches!(&self.identity.scope, Scope::Ephemeral) {
            return Err(Error::ExportForbidden);
        }
        let state = self.state_snapshot();

        // If the host clock is before the Unix epoch, preserve compatibility by
        // exporting `issued_at = 0` rather than failing snapshot export.
        let issued_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);

        let payload = SnapshotPayload {
            scope: snapshot_scope(&self.identity.scope),
            session_hex: self.session_hex(),
            entries: snapshot_entries_from_state(&state)
                .into_iter()
                .map(|entry| SnapshotEntry {
                    family: entry.family,
                    class: entry.class,
                    raw: entry.raw,
                    token: entry.token,
                })
                .collect(),
            next_by_class: state
                .next_by_class
                .iter()
                .map(|(class, index)| (class.clone(), *index))
                .collect(),
            issued_at,
            document,
        };
        let payload_bytes = serde_json::to_vec(&payload).map_err(Error::SnapshotDecode)?;
        let version = SNAPSHOT_VERSION_V5;
        let signing_key = self.identity.signing_key.signing_key();
        let verifying_key = signing_key.verifying_key();
        let verifying_key_bytes = verifying_key.to_bytes();
        let signing_preimage =
            snapshot_signing_preimage(version, &verifying_key_bytes, &payload_bytes);
        let signature = signing_key.sign(&signing_preimage);

        let mut snapshot = Vec::with_capacity(1 + 32 + 64 + payload_bytes.len());
        snapshot.push(version);
        snapshot.extend_from_slice(&verifying_key_bytes);
        snapshot.extend_from_slice(&signature.to_bytes());
        snapshot.extend_from_slice(&payload_bytes);
        Ok(SensitiveSnapshot(snapshot))
    }

    pub fn import(snapshot: SensitiveSnapshot) -> Result<Self> {
        let bytes = snapshot.0;
        if bytes.len() < 97 {
            return Err(Error::InvalidSnapshotSignature);
        }
        let version = bytes[0];
        if version != SNAPSHOT_VERSION_V2
            && version != SNAPSHOT_VERSION_V3
            && version != SNAPSHOT_VERSION_V4
            && version != SNAPSHOT_VERSION_V5
        {
            return Err(Error::InvalidSnapshotVersion(version));
        }

        let verifying_key = VerifyingKey::from_bytes(
            bytes[1..33]
                .try_into()
                .map_err(|_| Error::InvalidSnapshotSignature)?,
        )
        .map_err(|_| Error::InvalidSnapshotSignature)?;
        let signature = Signature::from_bytes(
            bytes[33..97]
                .try_into()
                .map_err(|_| Error::InvalidSnapshotSignature)?,
        );
        let payload_bytes = &bytes[97..];
        let verify_preimage;
        let signed_bytes = if version >= SNAPSHOT_VERSION_V5 {
            let verifying_key_bytes: [u8; 32] = bytes[1..33]
                .try_into()
                .map_err(|_| Error::InvalidSnapshotSignature)?;
            verify_preimage =
                snapshot_signing_preimage(version, &verifying_key_bytes, payload_bytes);
            verify_preimage.as_slice()
        } else {
            payload_bytes
        };
        verifying_key
            .verify(signed_bytes, &signature)
            .map_err(|_| Error::InvalidSnapshotSignature)?;

        let payload = decode_snapshot_payload(payload_bytes)?;
        validate_entry_prefixes(&payload)?;
        let session_hex = session_hex_bytes(&payload.session_hex)?;
        let scope = scope_from_snapshot(payload.scope);
        let issued_at = payload.issued_at;
        if let Scope::Persistent { ttl } = &scope {
            let ttl_secs = ttl.as_secs();
            if issued_at > 0 {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.as_secs())
                    .unwrap_or(0);
                if issued_at > now.saturating_add(60) {
                    return Err(Error::InvalidSnapshotSignature);
                }
                if now.saturating_sub(issued_at) > ttl_secs {
                    return Err(Error::BlobExpired {
                        issued_at,
                        ttl_secs,
                    });
                }
            }
        }

        let mut state = SessionState::empty();
        for entry in payload.entries {
            state.token_by_value.insert(
                TokenKey {
                    family: entry.family,
                    class: entry.class.clone(),
                    raw: entry.raw.clone(),
                },
                entry.token.clone(),
            );
            state.value_by_token.insert(entry.token.clone(), entry.raw);
            if let Some(index) = parse_token_index(&entry.token) {
                let next = state.next_by_class.entry(entry.class).or_insert(0);
                if *next < index {
                    *next = index;
                }
            }
        }
        // Authoritative counter state from the exporter. Overrides any
        // index we reconstructed from parseable token suffixes above so
        // that format-preserving tokens (e.g. `email1.<session>@gaze-fake.invalid`)
        // also round-trip safely.
        for (class, index) in payload.next_by_class {
            let next = state.next_by_class.entry(class).or_insert(0);
            if *next < index {
                *next = index;
            }
        }
        Ok(Self {
            identity: Arc::new(SessionIdentity {
                scope,
                session_hex,
                audit_session_id: new_audit_session_id(),
                signing_key: SessionKey::generate()?,
            }),
            state: RwLock::new(Arc::new(state)),
        })
    }
}

impl<'session> SessionTransaction<'session> {
    pub fn tokenize(&mut self, class: &PiiClass, raw: &str) -> Result<String> {
        self.tokenize_with_family(DEFAULT_COUNTER_FAMILY, class, raw)
    }

    pub fn tokenize_with_family(
        &mut self,
        family: &str,
        class: &PiiClass,
        raw: &str,
    ) -> Result<String> {
        let prefix = hex::encode(self.identity.session_hex);
        Ok(
            intern_mapping_in_state(&mut self.staged, family, class, raw, |index| {
                format!("<{prefix}:{}_{}>", class.class_name(), index)
            })
            .0,
        )
    }

    pub fn format_preserving_fake(&mut self, class: &PiiClass, raw: &str) -> Result<String> {
        let prefix = hex::encode(self.identity.session_hex);
        Ok(intern_mapping_in_state(
            &mut self.staged,
            DEFAULT_COUNTER_FAMILY,
            class,
            raw,
            |index| match class {
                PiiClass::Email => format!("email{index}.{prefix}@gaze-fake.invalid"),
                PiiClass::Name | PiiClass::Location | PiiClass::Organization => {
                    format!(
                        "{prefix}:{}_{}",
                        class.class_name().to_ascii_lowercase(),
                        index
                    )
                }
                PiiClass::Custom(name) => format!("{prefix}:custom:{name}_{index}"),
            },
        )
        .0)
    }

    pub fn tokens(&self) -> Vec<String> {
        self.staged.value_by_token.keys().cloned().collect()
    }

    /// Returns only the staged prefix-cache cardinality for cross-crate regression tests.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn prefix_cache_entry_count(&self) -> usize {
        self.staged.prefix_cache.len()
    }

    pub fn contains_token(&self, token: &str) -> bool {
        self.staged.value_by_token.contains_key(token)
    }

    pub fn session_hex(&self) -> String {
        hex::encode(self.identity.session_hex)
    }

    pub fn audit_session_id(&self) -> &str {
        &self.identity.audit_session_id
    }

    /// Validates every Gaze token-shaped substring against staged manifest state.
    ///
    /// This performs shape and membership validation only. It does not build a
    /// restored string or otherwise materialize mapped owner PII.
    pub fn validate_token_shapes(&self, text: &str) -> std::result::Result<(), RestoreError> {
        validate_token_shapes_from_state(&self.staged, text)
    }

    // Kept crate-private: the Pipeline is the sole cache reader/writer, so
    // adopters cannot bypass its trusted prefix-cache population contract.
    pub(crate) fn lookup_prefix_cache(&self, text: &str) -> Option<PrefixCacheHit> {
        self.staged
            .prefix_cache
            .values()
            .filter(|cached| {
                text.starts_with(&cached.raw) && text.is_char_boundary(cached.raw.len())
            })
            .map(|cached| PrefixCacheHit {
                raw_len: cached.raw.len(),
                clean_text: cached.clean_text.clone(),
                manifest: cached.manifest.clone(),
            })
            .max_by_key(|hit| hit.raw_len)
    }

    pub(crate) fn store_prefix_cache(
        &mut self,
        raw: &str,
        clean_text: &str,
        manifest: &[gaze_types::EmittedTokenSpan],
    ) {
        store_prefix_cache_in_state(&mut self.staged, raw, clean_text, manifest);
    }

    pub fn snapshot_entries(&self) -> Vec<SessionSnapshotEntry> {
        snapshot_entries_from_state(&self.staged)
    }

    pub fn restore_strict(&self, token: &str) -> Result<String> {
        restore_strict_from_state(&self.staged, token)
    }

    pub fn restore_strict_text(&self, text: &str) -> std::result::Result<String, RestoreError> {
        restore_strict_text_with_provenance_from_state(&self.staged, text)
            .map(|restored| restored.text)
    }

    pub fn restore_strict_text_with_provenance(
        &self,
        text: &str,
    ) -> std::result::Result<RestoredTextWithProvenance, RestoreError> {
        restore_strict_text_with_provenance_from_state(&self.staged, text)
    }

    pub fn restore_boundary_events(&self, text: &str) -> Vec<RestoreEvent> {
        restore_boundary_events(text, &authorized_structural_values_from_state(&self.staged))
    }

    pub fn restore(&self, token: &str) -> Option<String> {
        self.staged.value_by_token.get(token).cloned()
    }

    /// Atomically publish the entire staged state if the captured generation is current.
    pub fn commit(
        mut self,
    ) -> std::result::Result<CommittedSessionSnapshot, SessionTransactionError> {
        let mut boundary = self
            .session
            .state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if boundary.generation != self.captured_generation {
            return Err(SessionTransactionError::GenerationConflict);
        }

        self.staged.generation = self
            .captured_generation
            .checked_add(1)
            .expect("session generation exhausted");
        self.staged.restore_regex_cache = None;
        let committed = Arc::new(self.staged);
        *boundary = Arc::clone(&committed);
        Ok(CommittedSessionSnapshot {
            identity: self.identity,
            state: committed,
        })
    }
}

impl CommittedSessionSnapshot {
    pub fn tokens(&self) -> Vec<String> {
        self.state.value_by_token.keys().cloned().collect()
    }

    /// Returns only the committed prefix-cache cardinality for cross-crate regression tests.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn prefix_cache_entry_count(&self) -> usize {
        self.state.prefix_cache.len()
    }

    pub fn contains_token(&self, token: &str) -> bool {
        self.state.value_by_token.contains_key(token)
    }

    pub fn session_hex(&self) -> String {
        hex::encode(self.identity.session_hex)
    }

    pub fn audit_session_id(&self) -> &str {
        &self.identity.audit_session_id
    }

    pub fn restore_strict(&self, token: &str) -> Result<String> {
        restore_strict_from_state(&self.state, token)
    }

    pub fn restore_strict_text(&self, text: &str) -> std::result::Result<String, RestoreError> {
        restore_strict_text_with_provenance_from_state(&self.state, text)
            .map(|restored| restored.text)
    }

    pub fn restore_strict_text_with_provenance(
        &self,
        text: &str,
    ) -> std::result::Result<RestoredTextWithProvenance, RestoreError> {
        restore_strict_text_with_provenance_from_state(&self.state, text)
    }

    pub fn restore_boundary_events(&self, text: &str) -> Vec<RestoreEvent> {
        restore_boundary_events(text, &authorized_structural_values_from_state(&self.state))
    }

    pub fn restore(&self, token: &str) -> Option<String> {
        self.state.value_by_token.get(token).cloned()
    }
}

fn intern_mapping_in_state<F>(
    state: &mut SessionState,
    family: &str,
    class: &PiiClass,
    raw: &str,
    build: F,
) -> (String, bool)
where
    F: FnOnce(usize) -> String,
{
    let key = TokenKey {
        family: family.to_string(),
        class: class.clone(),
        raw: raw.to_string(),
    };
    if let Some(existing) = state.token_by_value.get(&key) {
        return (existing.clone(), false);
    }

    let next = state.next_by_class.entry(class.clone()).or_insert(0);
    *next = next
        .checked_add(1)
        .expect("session token counter exhausted");
    let token = build(*next);
    state.token_by_value.insert(key, token.clone());
    state.value_by_token.insert(token.clone(), raw.to_string());
    state.restore_regex_cache = None;
    (token, true)
}

fn build_restore_regex(state: &SessionState) -> Result<Option<Arc<Regex>>> {
    let mut tokens = state.value_by_token.keys().cloned().collect::<Vec<_>>();
    if tokens.is_empty() {
        return Ok(None);
    }
    tokens.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    let pattern = tokens
        .iter()
        .map(|token| {
            let escaped = regex::escape(token);
            if token.starts_with('<') && token.ends_with('>') {
                escaped
            } else {
                format!(r"\b(?:{escaped})\b")
            }
        })
        .collect::<Vec<_>>()
        .join("|");
    Regex::new(&pattern)
        .map(Arc::new)
        .map(Some)
        .map_err(Error::InvalidRegex)
}

fn store_prefix_cache_in_state(
    state: &mut SessionState,
    raw: &str,
    clean_text: &str,
    manifest: &[gaze_types::EmittedTokenSpan],
) -> bool {
    if raw.is_empty() {
        return false;
    }
    if state.prefix_cache.len() >= 64 {
        state.prefix_cache.clear();
    }
    let hash = prefix_cache_hash(raw);
    let entry = PrefixCacheEntry {
        raw: raw.to_string(),
        clean_text: clean_text.to_string(),
        manifest: manifest.to_vec(),
    };
    let changed = state.prefix_cache.get(&hash).is_none_or(|existing| {
        existing.raw != entry.raw
            || existing.clean_text != entry.clean_text
            || existing.manifest != entry.manifest
    });
    state.prefix_cache.insert(hash, entry);
    changed
}

fn snapshot_entries_from_state(state: &SessionState) -> Vec<SessionSnapshotEntry> {
    state
        .token_by_value
        .iter()
        .map(|(key, token)| SessionSnapshotEntry {
            family: key.family.clone(),
            class: key.class.clone(),
            raw: key.raw.clone(),
            token: token.clone(),
        })
        .collect()
}

fn restore_strict_from_state(state: &SessionState, token: &str) -> Result<String> {
    state
        .value_by_token
        .get(token)
        .cloned()
        .ok_or_else(|| unknown_token_error(token))
}

fn restore_strict_text_with_provenance_from_state(
    state: &SessionState,
    text: &str,
) -> std::result::Result<RestoredTextWithProvenance, RestoreError> {
    let tokens = validated_restore_tokens(state, text)?;
    let mut restored = String::with_capacity(text.len());
    let mut authorized_output_ranges = Vec::<Range<usize>>::with_capacity(tokens.len());
    let mut cursor = 0usize;
    for token in tokens {
        restored.push_str(&text[cursor..token.start]);
        let raw = state
            .value_by_token
            .get(token.parsed.raw.as_str())
            .expect("validated immutable manifest token must remain present");
        let start = restored.len();
        restored.push_str(raw);
        let end = restored.len();
        if start < end {
            if let Some(previous) = authorized_output_ranges.last_mut() {
                if start <= previous.end {
                    previous.end = previous.end.max(end);
                } else {
                    authorized_output_ranges.push(start..end);
                }
            } else {
                authorized_output_ranges.push(start..end);
            }
        }
        cursor = token.end;
    }
    restored.push_str(&text[cursor..]);
    Ok(RestoredTextWithProvenance {
        text: restored,
        authorized_output_ranges,
    })
}

fn validate_token_shapes_from_state(
    state: &SessionState,
    text: &str,
) -> std::result::Result<(), RestoreError> {
    validated_restore_tokens(state, text).map(|_| ())
}

fn validated_restore_tokens(
    state: &SessionState,
    text: &str,
) -> std::result::Result<Vec<StrictRestoreToken>, RestoreError> {
    let tokens = strict_restore_tokens(text)?;
    for token in &tokens {
        if !state.value_by_token.contains_key(token.parsed.raw.as_str()) {
            return Err(unknown_token_error(token.parsed.raw.as_str()));
        }
    }
    Ok(tokens)
}

fn default_counter_family() -> String {
    DEFAULT_COUNTER_FAMILY.to_string()
}

fn prefix_cache_hash(raw: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    raw.hash(&mut hasher);
    hasher.finish()
}

fn new_audit_session_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(0xffff_ffff_ffff) as u64)
        .unwrap_or(0);
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes[6..]);
    bytes[0] = (millis >> 40) as u8;
    bytes[1] = (millis >> 32) as u8;
    bytes[2] = (millis >> 24) as u8;
    bytes[3] = (millis >> 16) as u8;
    bytes[4] = (millis >> 8) as u8;
    bytes[5] = millis as u8;
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

struct SessionKey {
    secret: SecretBox<[u8; 32]>,
    protection: MemoryProtection,
}

impl SessionKey {
    fn generate() -> Result<Self> {
        let secret = SecretBox::init_with_mut(|bytes: &mut [u8; 32]| {
            rand::thread_rng().fill_bytes(bytes);
        });
        let protection = MemoryProtection::best_effort(secret.expose_secret().as_ptr(), 32);
        Ok(Self { secret, protection })
    }

    fn signing_key(&self) -> SigningKey {
        SigningKey::from_bytes(self.secret.expose_secret())
    }
}

impl Drop for SessionKey {
    fn drop(&mut self) {
        self.protection.unlock();
    }
}

struct MemoryProtection {
    addr: usize,
    len: usize,
    locked: bool,
}

impl MemoryProtection {
    fn best_effort(ptr: *const u8, len: usize) -> Self {
        let locked = lock_memory(ptr, len);
        advise_dontdump(ptr, len);
        Self {
            addr: ptr as usize,
            len,
            locked,
        }
    }

    fn unlock(&mut self) {
        if self.locked {
            unlock_memory(self.addr as *const u8, self.len);
            self.locked = false;
        }
    }
}

fn lock_memory(ptr: *const u8, len: usize) -> bool {
    #[cfg(unix)]
    unsafe {
        if libc::mlock(ptr.cast(), len) == 0 {
            return true;
        }
        tracing::warn!(
            error = %std::io::Error::last_os_error(),
            "session key mlock failed; continuing with unlocked key material"
        );
    }

    false
}

fn unlock_memory(ptr: *const u8, len: usize) {
    #[cfg(unix)]
    unsafe {
        let _ = libc::munlock(ptr.cast(), len);
    }
}

fn advise_dontdump(_ptr: *const u8, _len: usize) {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    unsafe {
        let ptr = _ptr;
        let len = _len;
        let page_size = libc::sysconf(libc::_SC_PAGESIZE);
        if page_size <= 0 {
            return;
        }
        let page_size = page_size as usize;
        let start = (ptr as usize) & !(page_size - 1);
        let end = (ptr as usize + len).div_ceil(page_size) * page_size;
        let aligned_len = end.saturating_sub(start);
        if aligned_len == 0 {
            return;
        }
        let _ = libc::madvise(start as *mut libc::c_void, aligned_len, libc::MADV_DONTDUMP);
    }
}

fn snapshot_scope(scope: &Scope) -> SnapshotScope {
    match scope {
        Scope::Ephemeral => SnapshotScope::Ephemeral,
        Scope::Conversation(id) => SnapshotScope::Conversation(id.clone()),
        Scope::Persistent { ttl } => SnapshotScope::Persistent {
            ttl_secs: ttl.as_secs(),
        },
    }
}

fn scope_from_snapshot(scope: SnapshotScope) -> Scope {
    match scope {
        SnapshotScope::Ephemeral => Scope::Ephemeral,
        SnapshotScope::Conversation(id) => Scope::Conversation(id),
        SnapshotScope::Persistent { ttl_secs } => Scope::Persistent {
            ttl: Duration::from_secs(ttl_secs),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StructuralFinding {
    class: PiiClass,
    raw: String,
    canonical: String,
    location: Range<usize>,
}

fn authorized_structural_values_from_state(state: &SessionState) -> BTreeSet<(PiiClass, String)> {
    let mut authorized = BTreeSet::new();
    for entry in snapshot_entries_from_state(state) {
        for finding in structural_findings(&entry.raw) {
            authorized.insert((finding.class, finding.canonical));
        }
    }
    authorized
}

fn restore_boundary_events(
    text: &str,
    authorized: &BTreeSet<(PiiClass, String)>,
) -> Vec<RestoreEvent> {
    structural_findings(text)
        .into_iter()
        .map(|finding| RestoreEvent {
            kind: if authorized.contains(&(finding.class.clone(), finding.canonical)) {
                RestoreEventKind::ManifestBypass
            } else {
                RestoreEventKind::FreshPiiDetected
            },
            class: finding.class,
            raw_sha256: raw_sha256(&finding.raw),
            location: finding.location,
        })
        .collect()
}

fn structural_findings(text: &str) -> Vec<StructuralFinding> {
    let mut findings = Vec::new();
    collect_email_findings(text, &mut findings);
    collect_phone_findings(text, &mut findings);
    collect_iban_findings(text, &mut findings);
    collect_credit_card_findings(text, &mut findings);
    collect_api_key_findings(text, &mut findings);
    findings.sort_by(|left, right| {
        left.location
            .start
            .cmp(&right.location.start)
            .then_with(|| {
                (right.location.end - right.location.start)
                    .cmp(&(left.location.end - left.location.start))
            })
    });
    let mut accepted = Vec::new();
    for finding in findings {
        if accepted.iter().any(|existing: &StructuralFinding| {
            finding.location.start < existing.location.end
                && existing.location.start < finding.location.end
        }) {
            continue;
        }
        accepted.push(finding);
    }
    accepted
}

fn collect_email_findings(text: &str, findings: &mut Vec<StructuralFinding>) {
    for matched in email_pattern().find_iter(text) {
        if !restore_email_boundary_accepts(text, &matched.range()) {
            continue;
        }
        let raw = matched.as_str();
        if !is_basic_restore_email(raw) {
            continue;
        }
        findings.push(StructuralFinding {
            class: PiiClass::Email,
            raw: raw.to_string(),
            canonical: raw.to_ascii_lowercase(),
            location: matched.range(),
        });
    }
}

fn collect_phone_findings(text: &str, findings: &mut Vec<StructuralFinding>) {
    for matched in phone_pattern().find_iter(text) {
        let raw = matched.as_str();
        findings.push(StructuralFinding {
            class: PiiClass::custom("phone"),
            raw: raw.to_string(),
            canonical: ascii_digits(raw),
            location: matched.range(),
        });
    }
}

fn collect_iban_findings(text: &str, findings: &mut Vec<StructuralFinding>) {
    for matched in iban_pattern().find_iter(text) {
        let raw = matched.as_str();
        let canonical = iban_canonicalize(raw);
        if !iban_mod97_check(&canonical) {
            continue;
        }
        findings.push(StructuralFinding {
            class: PiiClass::custom("iban"),
            raw: raw.to_string(),
            canonical,
            location: matched.range(),
        });
    }
}

fn collect_credit_card_findings(text: &str, findings: &mut Vec<StructuralFinding>) {
    for matched in card_pattern().find_iter(text) {
        let raw = matched.as_str();
        let canonical = ascii_digits(raw);
        if !luhn_check(&canonical) {
            continue;
        }
        findings.push(StructuralFinding {
            class: PiiClass::custom("credit_card"),
            raw: raw.to_string(),
            canonical,
            location: matched.range(),
        });
    }
}

fn collect_api_key_findings(text: &str, findings: &mut Vec<StructuralFinding>) {
    for matched in api_key_pattern().find_iter(text) {
        let raw = matched.as_str();
        findings.push(StructuralFinding {
            class: PiiClass::custom("api_key"),
            raw: raw.to_string(),
            canonical: raw.to_string(),
            location: matched.range(),
        });
    }
}

fn email_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?i)[a-z0-9_][a-z0-9._%+\-]*@[a-z0-9.\-]+\.[a-z]{2,}")
            .expect("email restore DLP regex compiles")
    })
}

fn phone_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r"(?x)(?:
                \+?1[\s.-]+555[\s.-]*01\d{2}
                |\+44[\s.-]*7700[\s.-]*900\d{3}
                |\+49[\s.-]*1555[\s.-]*0112233
            )\b",
        )
        .expect("phone restore DLP regex compiles")
    })
}

fn iban_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"\b[A-Z]{2}\d{2}(?: ?[A-Z0-9]{4}){2,7} ?[A-Z0-9]{1,4}\b")
            .expect("IBAN restore DLP regex compiles")
    })
}

fn card_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"\b\d(?:[\s-]?\d){12,18}\b").expect("credit-card restore DLP regex compiles")
    })
}

fn api_key_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"\b(?:sk-test-[A-Za-z0-9]{20,}|gaze_test_[A-Za-z0-9]{16,})\b")
            .expect("API-key restore DLP regex compiles")
    })
}

fn is_basic_restore_email(input: &str) -> bool {
    let Some((local, domain)) = input.split_once('@') else {
        return false;
    };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

fn restore_email_boundary_accepts(input: &str, span: &std::ops::Range<usize>) -> bool {
    let previous_ok = input[..span.start]
        .chars()
        .next_back()
        .is_none_or(|ch| !is_ascii_email_continuation(ch));
    let next_ok = input[span.end..]
        .chars()
        .next()
        .is_none_or(|ch| !is_ascii_email_continuation(ch));

    previous_ok && next_ok
}

fn is_ascii_email_continuation(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn ascii_digits(input: &str) -> String {
    input.chars().filter(char::is_ascii_digit).collect()
}

fn raw_sha256(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    hex::encode(digest)
}

fn iban_canonicalize(input: &str) -> String {
    input
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .flat_map(char::to_uppercase)
        .collect()
}

fn iban_mod97_check(input: &str) -> bool {
    let canonical = iban_canonicalize(input);
    if !(15..=34).contains(&canonical.len()) {
        return false;
    }
    if !canonical.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return false;
    }

    let mut remainder = 0u32;
    for ch in canonical[4..].chars().chain(canonical[..4].chars()) {
        match ch {
            '0'..='9' => {
                remainder = (remainder * 10 + ch.to_digit(10).expect("digit")) % 97;
            }
            'A'..='Z' => {
                let value = u32::from(ch) - u32::from('A') + 10;
                remainder = (remainder * 10 + value / 10) % 97;
                remainder = (remainder * 10 + value % 10) % 97;
            }
            _ => return false,
        }
    }
    remainder == 1
}

fn luhn_check(input: &str) -> bool {
    let mut digits = Vec::new();
    for byte in input.bytes() {
        if !byte.is_ascii_digit() {
            return false;
        }
        digits.push(byte - b'0');
    }
    if !(13..=19).contains(&digits.len()) {
        return false;
    }

    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(index, digit)| {
            let mut value = u32::from(*digit);
            if index % 2 == 1 {
                value *= 2;
                if value > 9 {
                    value -= 9;
                }
            }
            value
        })
        .sum();
    sum.is_multiple_of(10)
}

#[derive(Debug, Clone)]
pub(crate) struct StrictRestoreToken {
    pub start: usize,
    pub end: usize,
    pub parsed: ParsedRestoreToken,
}

pub(crate) fn strict_restore_tokens(text: &str) -> Result<Vec<StrictRestoreToken>> {
    let mut tokens = Vec::new();
    for matched in crate::token_shape::pattern().find_iter(text) {
        let nested_start = matched.start() > 0 && text.as_bytes()[matched.start() - 1] == b'<';
        let nested_end = matched.end() < text.len() && text.as_bytes()[matched.end()] == b'>';
        if nested_start || nested_end {
            let start = matched.start().saturating_sub(usize::from(nested_start));
            let end = matched.end() + usize::from(nested_end);
            return Err(unknown_token_error(&text[start..end]));
        }
        tokens.push(StrictRestoreToken {
            start: matched.start(),
            end: matched.end(),
            parsed: parsed_restore_token(matched.as_str()),
        });
    }

    for matched in malformed_restore_token_pattern().find_iter(text) {
        if tokens
            .iter()
            .any(|token| matched.start() < token.end && token.start < matched.end())
        {
            continue;
        }
        return Err(unknown_token_error(matched.as_str()));
    }

    tokens.sort_by_key(|token| token.start);
    Ok(tokens)
}

pub(crate) fn unknown_token_error(raw: &str) -> Error {
    let parsed = parsed_restore_token(raw);
    Error::UnknownToken {
        class: parsed.class,
        ordinal: parsed.ordinal,
        raw: parsed.raw,
    }
}

pub(crate) fn parsed_restore_token(raw: &str) -> ParsedRestoreToken {
    let (class, ordinal) = parse_restore_token_parts(raw)
        .unwrap_or_else(|| (PiiClass::Custom("unknown".to_string()), 0));
    ParsedRestoreToken {
        class,
        ordinal,
        raw: raw.to_string(),
    }
}

fn parse_restore_token_parts(raw: &str) -> Option<(PiiClass, u32)> {
    if let Some(rest) = raw.strip_prefix("email") {
        let (ordinal, tail) = rest.split_once('.')?;
        if tail.ends_with("@gaze-fake.invalid") || tail.ends_with("@example.test") {
            return Some((PiiClass::Email, parse_ascii_ordinal(ordinal)?));
        }
    }

    let mut body = raw.strip_prefix('<').unwrap_or(raw);
    body = body.strip_suffix('>').unwrap_or(body);
    if body.len() > 9 && body.as_bytes().get(8) == Some(&b':') && is_session_hex(&body[..8]) {
        body = &body[9..];
    }

    if let Some(rest) = body.strip_prefix("Custom:") {
        let (name, ordinal) = rest.rsplit_once('_')?;
        return Some((
            PiiClass::Custom(name.to_string()),
            parse_ascii_ordinal(ordinal)?,
        ));
    }
    if let Some(rest) = body.strip_prefix("custom:") {
        let (name, ordinal) = rest.rsplit_once('_')?;
        return Some((
            PiiClass::Custom(name.to_string()),
            parse_ascii_ordinal(ordinal)?,
        ));
    }

    let (class, ordinal) = body.rsplit_once('_')?;
    let ordinal = parse_ascii_ordinal(ordinal)?;
    let class = PiiClass::from_canonical_str(class).unwrap_or_else(|| PiiClass::custom(class));
    Some((class, ordinal))
}

fn parse_ascii_ordinal(raw: &str) -> Option<u32> {
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    raw.parse().ok()
}

fn malformed_restore_token_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"<(?:[0-9a-f]{8}:)?(?:Email|Name|Location|Organization|email|name|location|organization|Custom:[a-z0-9_]*|custom:[a-z0-9_]*)_?>")
            .expect("malformed restore token regex must compile")
    })
}

fn random_session_hex() -> [u8; 4] {
    let mut bytes = [0u8; 4];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
}

fn deserialize_session_hex<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if is_session_hex(&value) {
        Ok(value)
    } else {
        Err(serde::de::Error::custom(
            "session_hex must be 8 lowercase hex chars",
        ))
    }
}

fn is_session_hex(value: &str) -> bool {
    value.len() == 8
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn session_hex_bytes(value: &str) -> Result<[u8; 4]> {
    if !is_session_hex(value) {
        return Err(Error::InvalidSnapshotPayload);
    }
    let decoded = hex::decode(value).map_err(|_| Error::InvalidSnapshotPayload)?;
    decoded
        .try_into()
        .map_err(|_| Error::InvalidSnapshotPayload)
}

fn decode_snapshot_payload(payload_bytes: &[u8]) -> Result<SnapshotPayload> {
    let value: JsonValue = serde_json::from_slice(payload_bytes).map_err(Error::SnapshotDecode)?;
    let Some(session_hex) = value.get("session_hex").and_then(JsonValue::as_str) else {
        return Err(Error::InvalidSnapshotPayload);
    };
    if !is_session_hex(session_hex) {
        return Err(Error::InvalidSnapshotPayload);
    }
    serde_json::from_value(value).map_err(Error::SnapshotDecode)
}

fn validate_entry_prefixes(payload: &SnapshotPayload) -> Result<()> {
    let mut tokens_by_value = HashMap::<TokenKey, String>::new();
    let mut values_by_token = HashMap::<String, TokenKey>::new();
    for entry in &payload.entries {
        if !entry_token_matches_session(&entry.token, &payload.session_hex)
            || !crate::token_shape::starts_with_session_prefix(&entry.token)
        {
            return Err(Error::InvalidSnapshotPayload);
        }
        let key = TokenKey {
            family: entry.family.clone(),
            class: entry.class.clone(),
            raw: entry.raw.clone(),
        };
        if tokens_by_value
            .insert(key.clone(), entry.token.clone())
            .is_some()
            || values_by_token.insert(entry.token.clone(), key).is_some()
        {
            return Err(Error::InvalidSnapshotPayload);
        }
    }
    Ok(())
}

fn entry_token_matches_session(token: &str, session_hex: &str) -> bool {
    token.starts_with(&format!("<{session_hex}:"))
        || token.starts_with(&format!("{session_hex}:"))
        || (token.starts_with("email")
            && token
                .split_once('.')
                .and_then(|(_, rest)| rest.strip_suffix("@gaze-fake.invalid"))
                == Some(session_hex))
}

fn parse_token_index(token: &str) -> Option<usize> {
    if let Some(local) = token
        .strip_prefix("email")
        .and_then(|rest| rest.split_once('.').map(|(index, _)| index))
    {
        return parse_ascii_index(local);
    }
    let suffix = token
        .rsplit_once('_')?
        .1
        .strip_suffix('>')
        .unwrap_or(token.rsplit_once('_')?.1);
    parse_ascii_index(suffix)
}

fn parse_ascii_index(raw: &str) -> Option<usize> {
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    raw.parse().ok()
}

fn snapshot_signing_preimage(
    version: u8,
    verifying_key_bytes: &[u8; 32],
    payload_bytes: &[u8],
) -> Vec<u8> {
    let mut preimage = Vec::with_capacity(1 + verifying_key_bytes.len() + payload_bytes.len());
    preimage.push(version);
    preimage.extend_from_slice(verifying_key_bytes);
    preimage.extend_from_slice(payload_bytes);
    preimage
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_snapshot_v03(payload: SnapshotPayload) -> SensitiveSnapshot {
        let payload_bytes = serde_json::to_vec(&payload).expect("serialize payload");
        signed_snapshot_bytes(1, &payload_bytes)
    }

    fn signed_snapshot_bytes(version: u8, payload_bytes: &[u8]) -> SensitiveSnapshot {
        let key = SessionKey::generate().expect("session key");
        let signing_key = key.signing_key();
        let signature = signing_key.sign(payload_bytes);
        let verifying_key = signing_key.verifying_key();

        let mut snapshot = Vec::with_capacity(1 + 32 + 64 + payload_bytes.len());
        snapshot.push(version);
        snapshot.extend_from_slice(&verifying_key.to_bytes());
        snapshot.extend_from_slice(&signature.to_bytes());
        snapshot.extend_from_slice(payload_bytes);
        SensitiveSnapshot::from(snapshot)
    }

    fn signed_snapshot_v2(payload: SnapshotPayload) -> SensitiveSnapshot {
        let mut snapshot = signed_snapshot_v03(payload).into_bytes();
        snapshot[0] = SNAPSHOT_VERSION_V2;
        SensitiveSnapshot::from(snapshot)
    }

    fn signed_snapshot(payload: SnapshotPayload) -> SensitiveSnapshot {
        let mut snapshot = signed_snapshot_v03(payload).into_bytes();
        snapshot[0] = SNAPSHOT_VERSION_V3;
        SensitiveSnapshot::from(snapshot)
    }

    fn snapshot_payload_json(snapshot: &SensitiveSnapshot) -> JsonValue {
        serde_json::from_slice(&snapshot.0[97..]).expect("snapshot payload json")
    }

    fn document_extension(session: &Session) -> DocumentExtension {
        DocumentExtension::builder(1)
            .clean_md_sha256([1; 32])
            .layout_json_sha256([2; 32])
            .report_json_sha256([3; 32])
            .page_count(1)
            .audit_session_id(session.audit_session_id())
            .build()
            .expect("document extension")
    }

    fn legacy_v0_4_0_accepts_only_v2(snapshot: &SensitiveSnapshot) -> Result<()> {
        let bytes = &snapshot.0;
        if bytes.len() < 97 {
            return Err(Error::InvalidSnapshotSignature);
        }
        let version = bytes[0];
        if version != SNAPSHOT_VERSION_V2 {
            return Err(Error::InvalidSnapshotVersion(version));
        }
        Ok(())
    }

    #[test]
    fn session_key_produces_valid_signatures() {
        let key = SessionKey::generate().expect("session key");
        let signing_key = key.signing_key();
        let message = b"gaze";
        let signature = signing_key.sign(message);

        assert!(signing_key
            .verifying_key()
            .verify(message, &signature)
            .is_ok());
    }

    #[test]
    fn import_accepts_persistent_snapshot_within_ttl() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let snapshot = signed_snapshot(SnapshotPayload {
            scope: SnapshotScope::Persistent { ttl_secs: 300 },
            session_hex: "a7f3b8e2".to_string(),
            entries: Vec::new(),
            issued_at: now,
            next_by_class: Vec::new(),
            document: None,
        });

        assert!(Session::import(snapshot).is_ok());
    }

    #[test]
    fn import_rejects_v03_envelope_byte() {
        let snapshot = signed_snapshot_v03(SnapshotPayload {
            scope: SnapshotScope::Persistent { ttl_secs: 300 },
            session_hex: "a7f3b8e2".to_string(),
            entries: Vec::new(),
            issued_at: 0,
            next_by_class: Vec::new(),
            document: None,
        });

        assert!(matches!(
            Session::import(snapshot),
            Err(Error::InvalidSnapshotVersion(1))
        ));
    }

    #[test]
    fn import_rejects_invalid_session_hex() {
        let snapshot = signed_snapshot(SnapshotPayload {
            scope: SnapshotScope::Persistent { ttl_secs: 300 },
            session_hex: "A7F3B8E2".to_string(),
            entries: Vec::new(),
            issued_at: 0,
            next_by_class: Vec::new(),
            document: None,
        });

        assert!(matches!(
            Session::import(snapshot),
            Err(Error::InvalidSnapshotPayload)
        ));
    }

    #[test]
    fn import_rejects_manifest_entry_prefix_mismatch() {
        let snapshot = signed_snapshot(SnapshotPayload {
            scope: SnapshotScope::Persistent { ttl_secs: 300 },
            session_hex: "a7f3b8e2".to_string(),
            entries: vec![SnapshotEntry {
                family: DEFAULT_COUNTER_FAMILY.to_string(),
                class: PiiClass::Name,
                raw: "Dr. Schmidt".to_string(),
                token: "deadbeef:name_1".to_string(),
            }],
            issued_at: 0,
            next_by_class: Vec::new(),
            document: None,
        });

        assert!(matches!(
            Session::import(snapshot),
            Err(Error::InvalidSnapshotPayload)
        ));
    }

    #[test]
    fn import_rejects_non_bijective_manifest_directions() {
        let first = SnapshotEntry {
            family: DEFAULT_COUNTER_FAMILY.to_string(),
            class: PiiClass::Name,
            raw: "Dr. Schmidt".to_string(),
            token: "<a7f3b8e2:Name_1>".to_string(),
        };
        let duplicate_token = SnapshotEntry {
            family: "tool".to_string(),
            class: PiiClass::Name,
            raw: "Synthetic Colleague".to_string(),
            token: first.token.clone(),
        };
        let duplicate_value = SnapshotEntry {
            token: "<a7f3b8e2:Name_2>".to_string(),
            ..first.clone()
        };

        for entries in [
            vec![first.clone(), duplicate_token],
            vec![first.clone(), duplicate_value],
        ] {
            let snapshot = signed_snapshot(SnapshotPayload {
                scope: SnapshotScope::Persistent { ttl_secs: 300 },
                session_hex: "a7f3b8e2".to_string(),
                entries,
                issued_at: 0,
                next_by_class: Vec::new(),
                document: None,
            });
            assert!(matches!(
                Session::import(snapshot),
                Err(Error::InvalidSnapshotPayload)
            ));
        }
    }

    #[test]
    fn import_rejects_expired_persistent_snapshot() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let snapshot = signed_snapshot(SnapshotPayload {
            scope: SnapshotScope::Persistent { ttl_secs: 10 },
            session_hex: "a7f3b8e2".to_string(),
            entries: Vec::new(),
            issued_at: now.saturating_sub(11),
            next_by_class: Vec::new(),
            document: None,
        });

        assert!(matches!(
            Session::import(snapshot),
            Err(Error::BlobExpired {
                issued_at,
                ttl_secs: 10,
            }) if issued_at == now.saturating_sub(11)
        ));
    }

    #[test]
    fn import_accepts_legacy_persistent_snapshot_without_issued_at() {
        let snapshot = signed_snapshot_v2(SnapshotPayload {
            scope: SnapshotScope::Persistent { ttl_secs: 1 },
            session_hex: "a7f3b8e2".to_string(),
            entries: Vec::new(),
            issued_at: 0,
            next_by_class: Vec::new(),
            document: None,
        });

        assert!(Session::import(snapshot).is_ok());
    }

    #[test]
    fn import_v0_4_0_snapshot_version_2_succeeds_with_default_family() {
        let payload = serde_json::json!({
            "scope": { "Persistent": { "ttl_secs": 300 } },
            "session_hex": "a7f3b8e2",
            "entries": [{
                "class": "Name",
                "raw": "Dr. Schmidt",
                "token": "<a7f3b8e2:Name_1>"
            }],
            "issued_at": 0,
            "next_by_class": [["Name", 1]]
        });
        let payload_bytes = serde_json::to_vec(&payload).expect("payload");
        let snapshot = signed_snapshot_bytes(SNAPSHOT_VERSION_V2, &payload_bytes);

        let session = Session::import(snapshot).expect("import v2 snapshot");
        assert_eq!(
            session.restore("<a7f3b8e2:Name_1>").as_deref(),
            Some("Dr. Schmidt")
        );
        let next = session
            .tokenize_with_family("alpha", &PiiClass::Name, "Prof. Weber")
            .expect("next token");
        assert_eq!(next, "<a7f3b8e2:Name_2>");
    }

    #[test]
    fn v0_4_0_rejects_v0_4_1_snapshot_version_3_cleanly() {
        let session = Session::new(Scope::Conversation("test".to_string())).expect("session");
        let _ = session
            .tokenize_with_family("alpha", &PiiClass::Name, "Dr. Schmidt")
            .expect("token");
        let snapshot = session.export().expect("snapshot");

        assert!(matches!(
            legacy_v0_4_0_accepts_only_v2(&snapshot),
            Err(Error::InvalidSnapshotVersion(SNAPSHOT_VERSION_V5))
        ));
    }

    #[test]
    fn snapshot_signature_binds_emitted_envelope_version() {
        let session = Session::new(Scope::Conversation("test".to_string())).expect("session");
        session
            .tokenize(&PiiClass::Name, "Dr. Schmidt")
            .expect("token");

        let mut bytes = session.export().expect("snapshot").into_bytes();
        assert_eq!(bytes[0], SNAPSHOT_VERSION_V5);
        bytes[0] = SNAPSHOT_VERSION_V3;

        assert!(matches!(
            Session::import(SensitiveSnapshot::from(bytes)),
            Err(Error::InvalidSnapshotSignature)
        ));
    }

    #[test]
    fn snapshot_signature_uses_final_envelope_preimage() {
        let session = Session::new(Scope::Conversation("test".to_string())).expect("session");
        session
            .tokenize(&PiiClass::Email, "alice@example.invalid")
            .expect("token");

        let bytes = session.export().expect("snapshot").into_bytes();
        let version = bytes[0];
        let verifying_key_bytes: [u8; 32] = bytes[1..33].try_into().expect("verifying key bytes");
        let verifying_key = VerifyingKey::from_bytes(&verifying_key_bytes).expect("verifying key");
        let signature = Signature::from_bytes(&bytes[33..97].try_into().expect("signature bytes"));
        let payload_bytes = &bytes[97..];
        let preimage = snapshot_signing_preimage(version, &verifying_key_bytes, payload_bytes);

        assert!(verifying_key.verify(&preimage, &signature).is_ok());
        assert!(verifying_key.verify(payload_bytes, &signature).is_err());
    }

    #[test]
    fn snapshot_import_rejects_signature_slot_mutation() {
        let session = Session::new(Scope::Conversation("test".to_string())).expect("session");
        session
            .tokenize(&PiiClass::Name, "Dr. Schmidt")
            .expect("token");

        let mut bytes = session.export().expect("snapshot").into_bytes();
        bytes[33] ^= 0x01;

        assert!(matches!(
            Session::import(SensitiveSnapshot::from(bytes)),
            Err(Error::InvalidSnapshotSignature)
        ));
    }

    #[test]
    fn export_with_extension_round_trips_clean() {
        let session = Session::new(Scope::Conversation("test".to_string())).expect("session");
        let token = session
            .tokenize(&PiiClass::Name, "Dr. Schmidt")
            .expect("token");
        let extension = document_extension(&session);

        let snapshot = session
            .export_with_extension(extension.clone())
            .expect("export with extension");
        let payload = snapshot_payload_json(&snapshot);

        let document: DocumentExtension =
            serde_json::from_value(payload["document"].clone()).expect("document extension");
        assert_eq!(document, extension);

        let imported = Session::import(snapshot).expect("import extended snapshot");
        assert_eq!(imported.restore(&token).as_deref(), Some("Dr. Schmidt"));
    }

    #[test]
    fn export_with_extension_no_pii_leak() {
        let session = Session::new(Scope::Conversation("test".to_string())).expect("session");
        let _ = session
            .tokenize(&PiiClass::Email, "alice@example.invalid")
            .expect("token");

        let snapshot = session
            .export_with_extension(document_extension(&session))
            .expect("export with extension");
        let payload = snapshot_payload_json(&snapshot);
        let document_json = serde_json::to_string(&payload["document"]).expect("document json");

        assert!(!document_json.contains("alice@example.invalid"));
        assert!(!document_json.contains("\"raw\""));
    }

    #[test]
    fn document_extension_zero_clean_md_sha256_rejected() {
        let session = Session::new(Scope::Conversation("test".to_string())).expect("session");
        let mut extension = document_extension(&session);
        extension.clean_md_sha256 = [0; 32];

        assert!(matches!(
            session.export_with_extension(extension),
            Err(Error::EmptyDocumentIntegrity)
        ));
    }

    #[test]
    fn document_extension_zero_layout_json_sha256_rejected() {
        let session = Session::new(Scope::Conversation("test".to_string())).expect("session");
        let mut extension = document_extension(&session);
        extension.layout_json_sha256 = [0; 32];

        assert!(matches!(
            session.export_with_extension(extension),
            Err(Error::EmptyDocumentIntegrity)
        ));
    }

    #[test]
    fn document_extension_zero_report_json_sha256_rejected() {
        let session = Session::new(Scope::Conversation("test".to_string())).expect("session");
        let mut extension = document_extension(&session);
        extension.report_json_sha256 = [0; 32];

        assert!(matches!(
            session.export_with_extension(extension),
            Err(Error::EmptyDocumentIntegrity)
        ));
    }

    #[test]
    fn document_extension_empty_audit_session_id_rejected() {
        let session = Session::new(Scope::Conversation("test".to_string())).expect("session");
        let mut extension = document_extension(&session);
        extension.audit_session_id.clear();

        assert!(matches!(
            session.export_with_extension(extension),
            Err(Error::EmptyDocumentIntegrity)
        ));
    }

    #[test]
    fn document_extension_with_full_integrity_signs_successfully() {
        let session = Session::new(Scope::Conversation("test".to_string())).expect("session");
        let snapshot = session
            .export_with_extension(document_extension(&session))
            .expect("export with full integrity");
        let payload = snapshot_payload_json(&snapshot);

        assert_eq!(payload["document"]["schema_version"], 1);
        assert!(Session::import(snapshot).is_ok());
    }

    #[test]
    fn import_rejects_forward_dated_persistent_snapshot() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let snapshot = signed_snapshot(SnapshotPayload {
            scope: SnapshotScope::Persistent { ttl_secs: 300 },
            session_hex: "a7f3b8e2".to_string(),
            entries: Vec::new(),
            issued_at: now.saturating_add(3_600),
            next_by_class: Vec::new(),
            document: None,
        });

        assert!(matches!(
            Session::import(snapshot),
            Err(Error::InvalidSnapshotSignature)
        ));
    }

    #[test]
    fn tokenize_distinguishes_builtin_and_custom_class_names() {
        let session = Session::new(Scope::Ephemeral).expect("session");
        for (builtin, name) in [
            (PiiClass::Email, "email"),
            (PiiClass::Name, "name"),
            (PiiClass::Location, "location"),
            (PiiClass::Organization, "organization"),
        ] {
            let builtin_value = format!("{name}-builtin");
            let custom_value = format!("{name}-custom");

            let builtin_token = session
                .tokenize(&builtin, &builtin_value)
                .expect("builtin token");
            let custom_class = PiiClass::custom(name);
            let custom_token = session
                .tokenize(&custom_class, &custom_value)
                .expect("custom token");

            assert!(builtin_token.ends_with(&format!(":{}_1>", builtin.class_name())));
            assert!(custom_token.ends_with(&format!(":Custom:{name}_1>")));
            assert_ne!(builtin_token, custom_token);
            assert_eq!(
                session.restore(&builtin_token).as_deref(),
                Some(builtin_value.as_str())
            );
            assert_eq!(
                session.restore(&custom_token).as_deref(),
                Some(custom_value.as_str())
            );
        }
    }

    #[test]
    fn tokenize_distinguishes_custom_classes_with_matching_pascal_case() {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let first_class = PiiClass::custom("email");
        let second_class = PiiClass::custom("custom_email");

        let first_token = session
            .tokenize(&first_class, "alice@corp.com")
            .expect("first custom token");
        let second_token = session
            .tokenize(&second_class, "hello")
            .expect("second custom token");

        assert!(first_token.ends_with(":Custom:email_1>"));
        assert!(second_token.ends_with(":Custom:custom_email_1>"));
        assert_ne!(first_token, second_token);
        assert_eq!(
            session.restore(&first_token).as_deref(),
            Some("alice@corp.com")
        );
        assert_eq!(session.restore(&second_token).as_deref(), Some("hello"));
    }

    #[test]
    fn snapshot_round_trip_two_families_same_class_raw_preserved_under_shared_counter() {
        let session = Session::new(Scope::Conversation("test".to_string())).expect("session");
        let alpha = session
            .tokenize_with_family("alpha", &PiiClass::Name, "Dr. Schmidt")
            .expect("alpha token");
        let beta = session
            .tokenize_with_family("beta", &PiiClass::Name, "Dr. Schmidt")
            .expect("beta token");

        assert_ne!(alpha, beta);
        assert!(alpha.ends_with(":Name_1>"));
        assert!(beta.ends_with(":Name_2>"));

        let snapshot = session.export().expect("snapshot");
        assert_eq!(snapshot.0[0], SNAPSHOT_VERSION_V5);
        let imported = Session::import(snapshot).expect("import");

        assert_eq!(imported.restore(&alpha).as_deref(), Some("Dr. Schmidt"));
        assert_eq!(imported.restore(&beta).as_deref(), Some("Dr. Schmidt"));
        assert_eq!(
            imported
                .tokenize_with_family("alpha", &PiiClass::Name, "Dr. Schmidt")
                .expect("alpha stable"),
            alpha
        );
        assert_eq!(
            imported
                .tokenize_with_family("beta", &PiiClass::Name, "Dr. Schmidt")
                .expect("beta stable"),
            beta
        );
    }

    #[test]
    fn restore_strict_text_rejects_unknown_token() {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let err = session
            .restore_strict_text("Reply to <deadbeef:Email_999>.")
            .expect_err("unknown token must fail closed");

        assert!(matches!(
            err,
            Error::UnknownToken {
                class: PiiClass::Email,
                ordinal: 999,
                raw,
            } if raw == "<deadbeef:Email_999>"
        ));
    }

    #[test]
    fn restore_strict_text_rejects_malformed_token() {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let err = session
            .restore_strict_text("Reply to <Email_>.")
            .expect_err("malformed token must fail closed");

        assert!(matches!(
            err,
            Error::UnknownToken {
                class: PiiClass::Custom(name),
                ordinal: 0,
                raw,
            } if name == "unknown" && raw == "<Email_>"
        ));
    }

    #[test]
    fn restore_strict_text_rejects_cross_session_token_as_unknown() {
        let source = Session::new(Scope::Ephemeral).expect("source session");
        let token = source
            .tokenize(&PiiClass::Email, "alice@example.invalid")
            .expect("token");
        let target = Session::new(Scope::Ephemeral).expect("target session");

        let err = target
            .restore_strict_text(&format!("Reply to {token}."))
            .expect_err("wrong-session token must fail closed");

        assert!(matches!(
            err,
            Error::UnknownToken {
                class: PiiClass::Email,
                ordinal: 1,
                raw,
            } if raw == token
        ));
    }

    #[test]
    fn restore_strict_text_is_atomic_on_failure() {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let token = session
            .tokenize(&PiiClass::Name, "Dr. Schmidt")
            .expect("token");

        let err = session
            .restore_strict_text(&format!("{token} then <deadbeef:Email_9>"))
            .expect_err("unknown token must prevent partial restore output");

        assert!(matches!(
            err,
            Error::UnknownToken {
                class: PiiClass::Email,
                ordinal: 9,
                raw,
            } if raw == "<deadbeef:Email_9>"
        ));
    }

    #[test]
    fn restore_strict_text_preserves_path_adjacency_byte_exact() {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let workspace = session
            .tokenize(&PiiClass::Organization, "Workspace")
            .expect("workspace token");
        let artist = session
            .tokenize(&PiiClass::Organization, "Artist")
            .expect("artist token");
        let email = session
            .tokenize(&PiiClass::Email, "alice@example.invalid")
            .expect("email token");
        let cases = [
            (
                format!("list all folders in ~/{workspace}"),
                "list all folders in ~/Workspace".to_string(),
            ),
            (format!("{workspace}*"), "Workspace*".to_string()),
            (
                format!("~/{workspace}/{artist}"),
                "~/Workspace/Artist".to_string(),
            ),
            (
                format!("owner={email};path=~/{workspace}"),
                "owner=alice@example.invalid;path=~/Workspace".to_string(),
            ),
        ];

        for (clean, expected) in cases {
            let restored = session
                .restore_strict_text(&clean)
                .expect("restore must succeed");

            assert_eq!(restored, expected);
            assert!(!restored.contains("~/ Workspace"));
        }
    }

    #[test]
    fn restore_boundary_events_distinguish_manifest_bypass_from_fresh_pii() {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let token = session
            .tokenize(&PiiClass::Email, "alice@example.invalid")
            .expect("token");

        let (restored, events) = session
            .restore_strict_text_with_events(&format!(
                "{token} raw alice@example.invalid fresh bob@example.invalid"
            ))
            .expect("restore continues in audit-only mode");

        assert!(restored.contains("alice@example.invalid"));
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, RestoreEventKind::ManifestBypass);
        assert_eq!(events[0].class, PiiClass::Email);
        assert_eq!(events[0].raw_sha256.len(), 64);
        assert_ne!(events[0].raw_sha256, "alice@example.invalid");
        assert_eq!(events[1].kind, RestoreEventKind::FreshPiiDetected);
        assert_eq!(events[1].class, PiiClass::Email);
    }

    #[test]
    fn restore_dlp_email_boundary_accepts_non_ascii_and_punctuation_delimiters() {
        for (input, location) in [
            ("a@example.invalidø", 0..17),
            ("a@example.invalid. Thanks", 0..17),
            ("a@example.invalid-", 0..17),
            ("a@example.invalid+", 0..17),
            ("a@example.invalid%", 0..17),
            (".a@example.invalid", 1..18),
        ] {
            let findings = structural_findings(input);
            let finding = findings
                .iter()
                .find(|finding| finding.class == PiiClass::Email)
                .unwrap_or_else(|| panic!("missing email finding for {input}"));

            assert_eq!(finding.raw, "a@example.invalid", "{input}");
            assert_eq!(finding.location, location, "{input}");
        }

        let adjacent = structural_findings("a@example.invalid,b@example.invalid")
            .into_iter()
            .filter(|finding| finding.class == PiiClass::Email)
            .map(|finding| (finding.raw, finding.location))
            .collect::<Vec<_>>();
        assert_eq!(
            adjacent,
            vec![
                ("a@example.invalid".to_string(), 0..17),
                ("b@example.invalid".to_string(), 18..35)
            ]
        );

        for input in ["a@example.invalid1", "a@example.invalid_"] {
            assert!(
                structural_findings(input)
                    .iter()
                    .all(|finding| finding.class != PiiClass::Email),
                "{input}"
            );
        }
    }

    #[test]
    fn restore_boundary_events_cover_structural_identifier_scope() {
        let session = Session::new(Scope::Ephemeral).expect("session");
        session
            .tokenize(&PiiClass::custom("phone"), "+1-555-0101")
            .expect("phone token");
        session
            .tokenize(&PiiClass::custom("iban"), "DE89 3704 0044 0532 0130 00")
            .expect("iban token");
        session
            .tokenize(&PiiClass::custom("credit_card"), "4111 1111 1111 1111")
            .expect("card token");
        session
            .tokenize(&PiiClass::custom("api_key"), "sk-test-00000000000000000000")
            .expect("api key token");

        let events = session.restore_boundary_events(
            "known +1-555-0101 UK +44 7700 900123 DE +49 1555 0112233 \
             known DE89370400440532013000 fresh GB82 WEST 1234 5698 7654 32 \
             known 4111111111111111 fresh 4012-8888-8888-1881 \
             known sk-test-00000000000000000000 fresh gaze_test_0000000000000000",
        );

        assert!(events.iter().any(|event| {
            event.kind == RestoreEventKind::ManifestBypass
                && event.class == PiiClass::custom("phone")
        }));
        assert!(events.iter().any(|event| {
            event.kind == RestoreEventKind::FreshPiiDetected
                && event.class == PiiClass::custom("phone")
        }));
        assert!(events.iter().any(|event| {
            event.kind == RestoreEventKind::ManifestBypass
                && event.class == PiiClass::custom("iban")
        }));
        assert!(events.iter().any(|event| {
            event.kind == RestoreEventKind::FreshPiiDetected
                && event.class == PiiClass::custom("iban")
        }));
        assert!(events.iter().any(|event| {
            event.kind == RestoreEventKind::ManifestBypass
                && event.class == PiiClass::custom("credit_card")
        }));
        assert!(events.iter().any(|event| {
            event.kind == RestoreEventKind::FreshPiiDetected
                && event.class == PiiClass::custom("credit_card")
        }));
        assert!(events.iter().any(|event| {
            event.kind == RestoreEventKind::ManifestBypass
                && event.class == PiiClass::custom("api_key")
        }));
        assert!(events.iter().any(|event| {
            event.kind == RestoreEventKind::FreshPiiDetected
                && event.class == PiiClass::custom("api_key")
        }));
    }

    #[test]
    fn transaction_discard_then_commit_swaps_complete_state_and_shares_identity() {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let initial_entries = session.snapshot_entries();
        let initial_state = session.state_snapshot();

        {
            let mut discarded = session.begin_transaction();
            let discarded_token = discarded
                .tokenize(&PiiClass::Email, "alice@example.invalid")
                .expect("staged token");
            discarded.store_prefix_cache("alice", &discarded_token, &[]);
        }
        assert_eq!(session.snapshot_entries(), initial_entries);
        assert!(Arc::ptr_eq(&initial_state, &session.state_snapshot()));

        let mut transaction = session.begin_transaction();
        assert!(Arc::ptr_eq(&session.identity, &transaction.identity));
        let token = transaction
            .tokenize(&PiiClass::Email, "alice@example.invalid")
            .expect("staged token");
        assert_eq!(
            transaction.restore_strict(&token).expect("staged restore"),
            "alice@example.invalid"
        );
        transaction.store_prefix_cache("alice", &token, &[]);
        let snapshot = transaction.commit().expect("commit");

        assert_eq!(
            session.restore_strict(&token).expect("live restore"),
            "alice@example.invalid"
        );
        assert_eq!(
            snapshot.restore_strict(&token).expect("snapshot restore"),
            "alice@example.invalid"
        );
        let snapshot_restored = snapshot
            .restore_strict_text_with_provenance(&format!("owner={token}"))
            .expect("snapshot provenance");
        assert_eq!(snapshot_restored.text, "owner=alice@example.invalid");
        assert_eq!(snapshot_restored.authorized_output_ranges.len(), 1);
        assert_eq!(
            snapshot_restored.authorized_output_ranges[0],
            "owner=".len().."owner=alice@example.invalid".len()
        );
        assert_eq!(
            session
                .lookup_prefix_cache("alice and more")
                .expect("committed prefix cache")
                .clean_text,
            token
        );
        assert!(Arc::ptr_eq(&session.identity, &snapshot.identity));

        let regex = session
            .restore_regex()
            .expect("restore regex")
            .expect("non-empty restore regex");
        assert!(regex.is_match(&token));
        let state = session.state_snapshot();
        assert_eq!(state.token_by_value.len(), state.value_by_token.len());
        for (key, mapped_token) in &state.token_by_value {
            assert_eq!(
                state.value_by_token.get(mapped_token),
                Some(&key.raw),
                "manifest directions must remain reciprocal"
            );
        }
        assert_eq!(state.next_by_class.get(&PiiClass::Email), Some(&1));
        assert_eq!(
            state
                .restore_regex_cache
                .as_ref()
                .map(|(generation, _)| *generation),
            Some(state.generation)
        );
        assert!(state.generation > 0);
    }

    #[test]
    fn generation_conflicts_never_publish_partial_transaction_state() {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let mut transaction = session.begin_transaction();
        let staged = transaction
            .tokenize(&PiiClass::Name, "Dr. Schmidt")
            .expect("staged token");

        let live = session
            .tokenize(&PiiClass::Email, "alice@example.invalid")
            .expect("live token");
        let before_conflict = session.snapshot_entries();
        let error = match transaction.commit() {
            Err(error) => error,
            Ok(_) => panic!("generation conflict must fail closed"),
        };

        assert_eq!(error, SessionTransactionError::GenerationConflict);
        assert_eq!(session.snapshot_entries(), before_conflict);
        assert!(session.contains_token(&live));
        assert!(!session.contains_token(&staged));
    }

    #[test]
    fn every_existing_session_mutation_route_conflicts_after_capture() {
        let session = Session::new(Scope::Ephemeral).expect("session");

        let transaction = session.begin_transaction();
        session
            .tokenize(&PiiClass::Email, "alice@example.invalid")
            .expect("tokenize");
        assert!(matches!(
            transaction.commit(),
            Err(SessionTransactionError::GenerationConflict)
        ));

        let transaction = session.begin_transaction();
        session
            .tokenize_with_family("tool", &PiiClass::Name, "Dr. Schmidt")
            .expect("family tokenize");
        assert!(matches!(
            transaction.commit(),
            Err(SessionTransactionError::GenerationConflict)
        ));

        let transaction = session.begin_transaction();
        session
            .format_preserving_fake(&PiiClass::Location, "München")
            .expect("format preserving fake");
        assert!(matches!(
            transaction.commit(),
            Err(SessionTransactionError::GenerationConflict)
        ));

        let transaction = session.begin_transaction();
        session.store_prefix_cache("prefix", "clean", &[]);
        assert!(matches!(
            transaction.commit(),
            Err(SessionTransactionError::GenerationConflict)
        ));
    }

    #[test]
    fn concurrent_existing_mutation_routes_serialize_or_conflict_without_partial_state() {
        #[derive(Clone, Copy)]
        enum MutationRoute {
            Tokenize,
            TokenizeWithFamily,
            FormatPreserving,
            PrefixCache,
        }

        enum MutationOutcome {
            Token(String),
            PrefixCache,
        }

        for route in [
            MutationRoute::Tokenize,
            MutationRoute::TokenizeWithFamily,
            MutationRoute::FormatPreserving,
            MutationRoute::PrefixCache,
        ] {
            let session = Session::new(Scope::Ephemeral).expect("session");
            let mut transaction = session.begin_transaction();
            let staged = transaction
                .tokenize(&PiiClass::Organization, "Synthetic Workshop")
                .expect("staged token");
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let mutator_barrier = Arc::clone(&barrier);

            let (commit_result, mutation) = std::thread::scope(|scope| {
                let mutator = scope.spawn(|| {
                    mutator_barrier.wait();
                    match route {
                        MutationRoute::Tokenize => MutationOutcome::Token(
                            session
                                .tokenize(&PiiClass::Email, "alice@example.invalid")
                                .expect("tokenize"),
                        ),
                        MutationRoute::TokenizeWithFamily => MutationOutcome::Token(
                            session
                                .tokenize_with_family("tool", &PiiClass::Name, "Dr. Schmidt")
                                .expect("family tokenize"),
                        ),
                        MutationRoute::FormatPreserving => MutationOutcome::Token(
                            session
                                .format_preserving_fake(&PiiClass::Location, "München")
                                .expect("format preserving fake"),
                        ),
                        MutationRoute::PrefixCache => {
                            session.store_prefix_cache("prefix", "clean", &[]);
                            MutationOutcome::PrefixCache
                        }
                    }
                });
                barrier.wait();
                let commit_result = transaction.commit();
                let mutation = mutator.join().expect("mutator join");
                (commit_result, mutation)
            });

            match commit_result {
                Ok(snapshot) => {
                    assert!(snapshot.contains_token(&staged));
                    assert!(session.contains_token(&staged));
                }
                Err(SessionTransactionError::GenerationConflict) => {
                    assert!(!session.contains_token(&staged));
                }
            }
            match mutation {
                MutationOutcome::Token(token) => assert!(session.contains_token(&token)),
                MutationOutcome::PrefixCache => assert_eq!(
                    session
                        .lookup_prefix_cache("prefix suffix")
                        .expect("prefix cache")
                        .clean_text,
                    "clean"
                ),
            }
        }
    }

    #[test]
    fn two_same_generation_commits_have_exactly_one_winner() {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let mut left = session.begin_transaction();
        let mut right = session.begin_transaction();
        let left_token = left
            .tokenize(&PiiClass::Name, "Dr. Schmidt")
            .expect("left token");
        let right_token = right
            .tokenize(&PiiClass::Email, "alice@example.invalid")
            .expect("right token");

        let (left_result, right_result) = std::thread::scope(|scope| {
            let left = scope.spawn(move || left.commit());
            let right = scope.spawn(move || right.commit());
            (
                left.join().expect("left join"),
                right.join().expect("right join"),
            )
        });

        assert_ne!(left_result.is_ok(), right_result.is_ok());
        assert_ne!(
            session.contains_token(&left_token),
            session.contains_token(&right_token)
        );
    }

    #[test]
    fn committed_snapshot_is_stable_after_later_live_mutation() {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let mut transaction = session.begin_transaction();
        let first = transaction
            .tokenize(&PiiClass::Name, "Dr. Schmidt")
            .expect("first token");
        let snapshot = transaction.commit().expect("commit");

        let later = session
            .tokenize(&PiiClass::Email, "alice@example.invalid")
            .expect("later token");

        assert_eq!(
            snapshot.restore_strict(&first).expect("old restore"),
            "Dr. Schmidt"
        );
        assert!(snapshot.restore_strict(&later).is_err());
        assert_eq!(
            session.restore_strict(&later).expect("live restore"),
            "alice@example.invalid"
        );
    }

    #[test]
    fn restoration_provenance_uses_utf8_output_coordinates_and_coalesces_adjacency() {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let name = session
            .tokenize(&PiiClass::Name, "Dr. Schmidt")
            .expect("name token");
        let location = session
            .tokenize(&PiiClass::Location, "München")
            .expect("location token");
        let input = format!("π/{name}{location};{name}");

        let restored = session
            .restore_strict_text_with_provenance(&input)
            .expect("restore with provenance");
        let first_start = "π/".len();
        let first_end = first_start + "Dr. SchmidtMünchen".len();
        let second_start = first_end + ";".len();

        assert_eq!(restored.text, "π/Dr. SchmidtMünchen;Dr. Schmidt");
        assert_eq!(
            restored.authorized_output_ranges,
            vec![
                first_start..first_end,
                second_start..second_start + "Dr. Schmidt".len()
            ]
        );

        assert!(session
            .restore_strict_text_with_provenance("prefix <deadbeef:Email_999> suffix")
            .is_err());
        assert!(session
            .restore_strict_text_with_provenance("prefix <Email_> suffix")
            .is_err());
        assert!(session
            .restore_strict_text_with_provenance("prefix <deadbeef:Email_1 suffix")
            .is_err());
        assert!(session
            .restore_strict_text_with_provenance(&format!("prefix <{name}> suffix"))
            .is_err());

        let other = Session::new(Scope::Ephemeral).expect("other session");
        let cross_session = other
            .tokenize(&PiiClass::Email, "bob@example.invalid")
            .expect("cross-session token");
        assert!(session
            .restore_strict_text_with_provenance(&cross_session)
            .is_err());
    }

    #[test]
    fn transaction_token_shape_validation_rejects_unknown_malformed_nested_and_cross_session() {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let mut transaction = session.begin_transaction();
        let ordinary = transaction
            .tokenize(&PiiClass::Name, "Dr. Schmidt")
            .expect("ordinary token");
        let format_preserving = transaction
            .format_preserving_fake(&PiiClass::Email, "alice@example.invalid")
            .expect("format-preserving token");

        transaction
            .validate_token_shapes(&format!("known {ordinary} and {format_preserving}"))
            .expect("known staged tokens");
        assert!(transaction.validate_token_shapes("trap <Email_1>").is_err());
        assert!(transaction
            .validate_token_shapes("trap email1@gaze-fake.invalid")
            .is_err());
        assert!(transaction
            .validate_token_shapes(&format!(
                "unknown <{}:Email_999>",
                transaction.session_hex()
            ))
            .is_err());
        assert!(transaction
            .validate_token_shapes(&format!(
                "unknown email999.{}@gaze-fake.invalid",
                transaction.session_hex()
            ))
            .is_err());
        assert!(transaction
            .validate_token_shapes(&format!("nested <{ordinary}>"))
            .is_err());
        assert!(transaction
            .validate_token_shapes("malformed <deadbeef:Email_>")
            .is_err());

        let other = Session::new(Scope::Ephemeral).expect("other session");
        let cross_ordinary = other
            .tokenize(&PiiClass::Location, "München")
            .expect("cross ordinary");
        let cross_format = other
            .format_preserving_fake(&PiiClass::Name, "Synthetic Colleague")
            .expect("cross format-preserving");
        assert!(transaction.validate_token_shapes(&cross_ordinary).is_err());
        assert!(transaction.validate_token_shapes(&cross_format).is_err());
    }

    #[test]
    fn transaction_token_shape_validation_rejects_degenerate_shadow_shape_before_mapping() {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let transaction = session.begin_transaction();
        let would_be_fake = format!("{}:name_1", transaction.session_hex());

        assert!(transaction.validate_token_shapes(&would_be_fake).is_err());
        assert!(transaction.tokens().is_empty());
        assert!(session.tokens().is_empty());
    }
}
