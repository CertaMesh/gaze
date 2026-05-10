use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::RngCore;
use secrecy::{ExposeSecret, SecretBox};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value as JsonValue;

use crate::detector::PiiClass;
use crate::policy::{Policy, SessionScope};
use crate::{Error, Result};
use gaze_types::DocumentExtension;

const DEFAULT_PERSISTENT_TTL_SECS: u64 = 86_400;
const DEFAULT_COUNTER_FAMILY: &str = "counter";
const SNAPSHOT_VERSION_V2: u8 = 2;
const SNAPSHOT_VERSION_V3: u8 = 3;
const SNAPSHOT_VERSION_V4: u8 = 4;

/// Lifetime scope of a [`Session`]'s token manifest.
///
/// | Variant | Use case | `export()` allowed |
/// |---------|----------|--------------------|
/// | `Ephemeral` | Single-pass sanitization, no restore needed | No |
/// | `Conversation(id)` | Multi-turn LLM sessions | Yes |
/// | `Persistent { ttl: Duration }` | Long-lived sessions across restarts | Yes |
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

/// Owns the token manifest for one conversation or request.
///
/// A `Session` holds the bidirectional map between PII values and their pseudonymous tokens.
/// Create one per conversation and thread it through every [`Pipeline::redact`] call.
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
/// [`Pipeline::redact`]: crate::Pipeline::redact
// intentionally not Debug: contains session signing key and token manifest
pub struct Session {
    scope: Scope,
    session_hex: [u8; 4],
    audit_session_id: String,
    next_by_class: DashMap<PiiClass, usize>,
    token_by_value: DashMap<TokenKey, String>,
    value_by_token: DashMap<String, String>,
    signing_key: SessionKey,
}

impl Session {
    pub fn new(scope: Scope) -> Result<Self> {
        Ok(Self {
            scope,
            session_hex: random_session_hex(),
            audit_session_id: new_audit_session_id(),
            next_by_class: DashMap::new(),
            token_by_value: DashMap::new(),
            value_by_token: DashMap::new(),
            signing_key: SessionKey::generate()?,
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
        let key = TokenKey {
            family: family_key.to_string(),
            class: class.clone(),
            raw: raw.to_string(),
        };
        match self.token_by_value.entry(key) {
            Entry::Occupied(existing) => Ok(existing.get().clone()),
            Entry::Vacant(vacant) => {
                let token = {
                    let mut next = self.next_by_class.entry(class.clone()).or_insert(0);
                    *next += 1;
                    build(*next)
                };

                vacant.insert(token.clone());
                self.value_by_token.insert(token.clone(), raw.to_string());
                Ok(token)
            }
        }
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
        self.value_by_token
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    pub fn contains_token(&self, token: &str) -> bool {
        self.value_by_token.contains_key(token)
    }

    pub fn session_hex(&self) -> String {
        hex::encode(self.session_hex)
    }

    pub fn audit_session_id(&self) -> &str {
        &self.audit_session_id
    }

    // Original byte spans are preserved by recognizer normalizers per
    // research-855 §Rulepack > Normalization (axis-2 invariant).
    pub fn restore_strict(&self, token: &str) -> Result<String> {
        self.value_by_token
            .get(token)
            .map(|value| value.value().clone())
            .ok_or_else(|| Error::UnknownToken(token.to_string()))
    }

    pub fn restore(&self, token: &str) -> Option<String> {
        self.value_by_token
            .get(token)
            .map(|value| value.value().clone())
    }

    pub fn export(&self) -> Result<SensitiveSnapshot> {
        self.export_payload(None)
    }

    pub fn export_with_extension(&self, extension: DocumentExtension) -> Result<SensitiveSnapshot> {
        self.export_payload(Some(extension))
    }

    fn export_payload(&self, document: Option<DocumentExtension>) -> Result<SensitiveSnapshot> {
        if matches!(self.scope, Scope::Ephemeral) {
            return Err(Error::ExportForbidden);
        }

        // If the host clock is before the Unix epoch, preserve compatibility by
        // exporting `issued_at = 0` rather than failing snapshot export.
        let issued_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);

        let payload = SnapshotPayload {
            scope: snapshot_scope(&self.scope),
            session_hex: self.session_hex(),
            entries: self
                .token_by_value
                .iter()
                .map(|entry| SnapshotEntry {
                    family: entry.key().family.clone(),
                    class: entry.key().class.clone(),
                    raw: entry.key().raw.clone(),
                    token: entry.value().clone(),
                })
                .collect(),
            next_by_class: self
                .next_by_class
                .iter()
                .map(|entry| (entry.key().clone(), *entry.value()))
                .collect(),
            issued_at,
            document,
        };
        let payload_bytes = serde_json::to_vec(&payload).map_err(Error::SnapshotDecode)?;
        let signing_key = self.signing_key.signing_key();
        let signature = signing_key.sign(&payload_bytes);
        let verifying_key = signing_key.verifying_key();

        let version = if payload.document.is_some() {
            SNAPSHOT_VERSION_V4
        } else {
            SNAPSHOT_VERSION_V3
        };
        let mut snapshot = Vec::with_capacity(1 + 32 + 64 + payload_bytes.len());
        snapshot.push(version);
        snapshot.extend_from_slice(&verifying_key.to_bytes());
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
        verifying_key
            .verify(payload_bytes, &signature)
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

        let session = Self {
            scope,
            session_hex,
            audit_session_id: new_audit_session_id(),
            next_by_class: DashMap::new(),
            token_by_value: DashMap::new(),
            value_by_token: DashMap::new(),
            signing_key: SessionKey::generate()?,
        };
        for entry in payload.entries {
            session.token_by_value.insert(
                TokenKey {
                    family: entry.family,
                    class: entry.class.clone(),
                    raw: entry.raw.clone(),
                },
                entry.token.clone(),
            );
            session
                .value_by_token
                .insert(entry.token.clone(), entry.raw);
            if let Some(index) = parse_token_index(&entry.token) {
                let mut next = session.next_by_class.entry(entry.class).or_insert(0);
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
            let mut next = session.next_by_class.entry(class).or_insert(0);
            if *next < index {
                *next = index;
            }
        }
        Ok(session)
    }
}

fn default_counter_family() -> String {
    DEFAULT_COUNTER_FAMILY.to_string()
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
    for entry in &payload.entries {
        if !entry_token_matches_session(&entry.token, &payload.session_hex)
            || !crate::token_shape::starts_with_session_prefix(&entry.token)
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
        return local.parse().ok();
    }
    let suffix = token
        .rsplit_once('_')?
        .1
        .strip_suffix('>')
        .unwrap_or(token.rsplit_once('_')?.1);
    suffix.parse().ok()
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
            Err(Error::InvalidSnapshotVersion(3))
        ));
    }

    #[test]
    fn export_with_extension_round_trips_clean() {
        let session = Session::new(Scope::Conversation("test".to_string())).expect("session");
        let token = session
            .tokenize(&PiiClass::Name, "Dr. Schmidt")
            .expect("token");
        let extension = DocumentExtension::new(1, gaze_types::TextOrigin::EmbeddedText);

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
            .export_with_extension(DocumentExtension::default())
            .expect("export with extension");
        let payload = snapshot_payload_json(&snapshot);
        let document_json = serde_json::to_string(&payload["document"]).expect("document json");

        assert!(!document_json.contains("alice@example.invalid"));
        assert!(!document_json.contains("\"raw\""));
    }

    #[test]
    fn export_with_extension_handles_empty_extension() {
        let session = Session::new(Scope::Conversation("test".to_string())).expect("session");

        let snapshot = session
            .export_with_extension(DocumentExtension::default())
            .expect("export with default extension");
        let payload = snapshot_payload_json(&snapshot);

        assert_eq!(payload["document"]["schema_version"], 1);
        assert_eq!(payload["document"]["text_origin"], "embedded_text");
        assert_eq!(
            payload["document"]["clean_md_sha256"]
                .as_array()
                .expect("clean hash")
                .len(),
            32
        );
        assert!(payload["document"].get("codec_audit").is_none());
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
        assert_eq!(snapshot.0[0], SNAPSHOT_VERSION_V3);
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
}
