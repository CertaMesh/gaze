use std::time::Duration;

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::RngCore;
use secrecy::{ExposeSecret, SecretBox};
use serde::{Deserialize, Serialize};

use crate::detector::PiiClass;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub enum Scope {
    Ephemeral,
    Conversation(String),
    Persistent { ttl: Duration },
}

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
    class: PiiClass,
    raw: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotEntry {
    class: PiiClass,
    raw: String,
    token: String,
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
    entries: Vec<SnapshotEntry>,
}

pub struct Session {
    scope: Scope,
    next_by_class: DashMap<PiiClass, usize>,
    token_by_value: DashMap<TokenKey, String>,
    value_by_token: DashMap<String, String>,
    signing_key: SessionKey,
}

impl Session {
    pub fn new(scope: Scope) -> Result<Self> {
        Ok(Self {
            scope,
            next_by_class: DashMap::new(),
            token_by_value: DashMap::new(),
            value_by_token: DashMap::new(),
            signing_key: SessionKey::generate()?,
        })
    }

    pub fn tokenize(&self, class: &PiiClass, raw: &str) -> Result<String> {
        self.intern_mapping(class, raw, |index| format!("{}_{}", class_name(class), index))
    }

    pub fn format_preserving_fake(&self, class: &PiiClass, raw: &str) -> Result<String> {
        self.intern_mapping(class, raw, |index| match class {
            PiiClass::Email => format!("email{index}@example.test"),
            _ => format!("{}_{}", class_name(class).to_ascii_lowercase(), index),
        })
    }

    fn intern_mapping<F>(&self, class: &PiiClass, raw: &str, build: F) -> Result<String>
    where
        F: FnOnce(usize) -> String,
    {
        let key = TokenKey {
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

    pub fn restore_strict(&self, token: &str) -> Result<String> {
        self.value_by_token
            .get(token)
            .map(|value| value.value().clone())
            .ok_or_else(|| Error::UnknownToken(token.to_string()))
    }

    pub fn restore(&self, token: &str) -> Option<String> {
        self.value_by_token.get(token).map(|value| value.value().clone())
    }

    pub fn export(&self) -> Result<SensitiveSnapshot> {
        if matches!(self.scope, Scope::Ephemeral) {
            return Err(Error::ExportForbidden);
        }

        let payload = SnapshotPayload {
            scope: snapshot_scope(&self.scope),
            entries: self
                .token_by_value
                .iter()
                .map(|entry| SnapshotEntry {
                    class: entry.key().class.clone(),
                    raw: entry.key().raw.clone(),
                    token: entry.value().clone(),
                })
                .collect(),
        };
        let payload_bytes = serde_json::to_vec(&payload).map_err(Error::SnapshotDecode)?;
        let signing_key = self.signing_key.signing_key();
        let signature = signing_key.sign(&payload_bytes);
        let verifying_key = signing_key.verifying_key();

        let mut snapshot = Vec::with_capacity(1 + 32 + 64 + payload_bytes.len());
        snapshot.push(1);
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
        if version != 1 {
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

        let payload: SnapshotPayload =
            serde_json::from_slice(payload_bytes).map_err(Error::SnapshotDecode)?;
        let session = Self::new(scope_from_snapshot(payload.scope))?;
        for entry in payload.entries {
            session.token_by_value.insert(
                TokenKey {
                    class: entry.class.clone(),
                    raw: entry.raw.clone(),
                },
                entry.token.clone(),
            );
            session.value_by_token.insert(entry.token.clone(), entry.raw);
            if let Some(index) = parse_token_index(&entry.token) {
                let mut next = session.next_by_class.entry(entry.class).or_insert(0);
                if *next < index {
                    *next = index;
                }
            }
        }
        Ok(session)
    }
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
        let end = ((ptr as usize + len + page_size - 1) / page_size) * page_size;
        let aligned_len = end.saturating_sub(start);
        if aligned_len == 0 {
            return;
        }
        let _ = libc::madvise(start as *mut libc::c_void, aligned_len, libc::MADV_DONTDUMP);
    }
}

fn class_name(class: &PiiClass) -> &'static str {
    match class {
        PiiClass::Email => "Email",
        PiiClass::Name => "Name",
        PiiClass::Location => "Location",
        PiiClass::Organization => "Organization",
        PiiClass::Custom(_) => "Custom",
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

fn parse_token_index(token: &str) -> Option<usize> {
    token.rsplit_once('_')?.1.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_key_produces_valid_signatures() {
        let key = SessionKey::generate().expect("session key");
        let signing_key = key.signing_key();
        let message = b"gaze";
        let signature = signing_key.sign(message);

        assert!(signing_key.verifying_key().verify(message, &signature).is_ok());
    }
}
