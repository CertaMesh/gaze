use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use rand::RngCore;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::config::{SessionCfg, SessionMode};
use crate::error::{BridgeError, BridgeResult};

const MAGIC: &[u8] = b"gaze-bridge-session-v1\n";
const NONCE_LEN: usize = 12;

pub type SharedSession = Arc<Mutex<gaze::Session>>;

#[derive(Clone)]
pub enum SessionStoreMode {
    Ephemeral,
    File { dir: PathBuf, key: [u8; 32] },
}

pub struct BridgeSessionStore {
    mode: SessionStoreMode,
    sessions: Mutex<HashMap<String, SharedSession>>,
    // One entry per distinct session id ever observed in File mode. Not bounded
    // by `max_sessions`; evicting a `sessions` entry does not drop its lock entry
    // because callers may hold the `Arc` clone across long-running load/persist
    // work. Tracked as a known secondary leak with a small (~60-80 byte) per-entry
    // cost; safe eviction requires an `Arc::strong_count` sweep and is deferred.
    file_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    lru: Mutex<VecDeque<String>>,
    max_sessions: usize,
}

impl BridgeSessionStore {
    pub fn from_config(config: &SessionCfg) -> BridgeResult<Self> {
        let mode = match config.mode {
            SessionMode::Ephemeral => SessionStoreMode::Ephemeral,
            SessionMode::File => {
                let dir = config
                    .dir
                    .clone()
                    .ok_or_else(|| BridgeError::Config("file session dir missing".to_string()))?;
                let key_env = config.key_env.as_deref().ok_or_else(|| {
                    BridgeError::Config("file session key_env missing".to_string())
                })?;
                let key_raw = std::env::var(key_env)
                    .map_err(|_| BridgeError::Config(format!("session key `{key_env}` missing")))?;
                let key = parse_key(&key_raw)?;
                SessionStoreMode::File { dir, key }
            }
        };
        Ok(Self {
            mode,
            sessions: Mutex::new(HashMap::new()),
            file_locks: Mutex::new(HashMap::new()),
            lru: Mutex::new(VecDeque::new()),
            max_sessions: config.max_sessions,
        })
    }

    pub fn ephemeral() -> Self {
        Self {
            mode: SessionStoreMode::Ephemeral,
            sessions: Mutex::new(HashMap::new()),
            file_locks: Mutex::new(HashMap::new()),
            lru: Mutex::new(VecDeque::new()),
            max_sessions: crate::config::DEFAULT_MAX_SESSIONS,
        }
    }

    pub async fn len(&self) -> usize {
        self.sessions.lock().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.sessions.lock().await.is_empty()
    }

    pub async fn get(&self, validated_session_id: &str) -> BridgeResult<SharedSession> {
        let mut sessions = self.sessions.lock().await;
        let mut lru = self.lru.lock().await;

        if let Some(session) = sessions.get(validated_session_id) {
            lru.retain(|id| id != validated_session_id);
            lru.push_back(validated_session_id.to_string());
            return Ok(Arc::clone(session));
        }

        if matches!(self.mode, SessionStoreMode::Ephemeral)
            && self.max_sessions > 0
            && sessions.len() >= self.max_sessions
        {
            return Err(BridgeError::LimitExceeded(
                "session cap reached; use file mode for long-lived bridge deployments".to_string(),
            ));
        }

        let session = match &self.mode {
            SessionStoreMode::Ephemeral => gaze::Session::new(gaze::Scope::Ephemeral)
                .map_err(|err| BridgeError::SessionStore(err.to_string()))?,
            SessionStoreMode::File { dir, key } => {
                self.load_file_session(dir, key, validated_session_id)
                    .await?
            }
        };
        let shared = Arc::new(Mutex::new(session));
        sessions.insert(validated_session_id.to_string(), Arc::clone(&shared));
        lru.push_back(validated_session_id.to_string());

        if matches!(self.mode, SessionStoreMode::File { .. }) && self.max_sessions > 0 {
            // Evict LRU entries that are not currently held by any caller.
            // An entry is "active" when Arc::strong_count > 1 (the cache holds
            // one reference; any additional references belong to in-flight
            // callers). Evicting an active session would leave the caller's Arc
            // pointing at a live Session object that is no longer canonical —
            // a subsequent get() for the same id would load the older on-disk
            // snapshot into a second independent Session, allowing two callers
            // to hold conflicting state and overwrite each other's persisted
            // token mappings.
            //
            // When all entries are active (no safe eviction candidate), reject
            // the new session rather than racing with an in-flight caller.
            let mut evicted_any = true;
            while sessions.len() > self.max_sessions && evicted_any {
                evicted_any = false;
                let mut skipped = VecDeque::new();
                while let Some(candidate) = lru.pop_front() {
                    if sessions.len() <= self.max_sessions {
                        skipped.push_back(candidate);
                        break;
                    }
                    match sessions.get(&candidate) {
                        Some(arc) if Arc::strong_count(arc) == 1 => {
                            sessions.remove(&candidate);
                            evicted_any = true;
                        }
                        _ => {
                            // Active or already-gone — put back at the front to
                            // preserve LRU order and try the next oldest entry.
                            skipped.push_front(candidate);
                        }
                    }
                }
                // Restore any entries we peeked over back into the LRU.
                for entry in skipped {
                    lru.push_front(entry);
                }
            }
            if sessions.len() > self.max_sessions {
                // All cached sessions are active. Undo the insert we just did
                // and reject the request to preserve session ownership invariants.
                sessions.remove(validated_session_id);
                lru.retain(|id| id != validated_session_id);
                return Err(BridgeError::LimitExceeded(
                    "session cap reached; all cached sessions are active".to_string(),
                ));
            }
        }

        Ok(shared)
    }

    pub async fn persist(
        &self,
        validated_session_id: &str,
        session: &gaze::Session,
    ) -> BridgeResult<()> {
        let SessionStoreMode::File { dir, key } = &self.mode else {
            return Ok(());
        };

        let file_lock = self.file_lock(validated_session_id).await;
        let _guard = file_lock.lock().await;
        tokio::fs::create_dir_all(dir).await.map_err(|err| {
            BridgeError::SessionStore(format!("create session dir failed: {err}"))
        })?;

        let snapshot = session
            .export()
            .map_err(|err| BridgeError::SessionStore(format!("session export failed: {err}")))?;
        let plaintext = snapshot.into_bytes();
        let encrypted = encrypt_snapshot(key, validated_session_id.as_bytes(), &plaintext)?;
        let path = session_path(dir, validated_session_id);
        let tmp = path.with_extension(format!("tmp-{}", random_hex(8)));
        let mut file = tokio::fs::File::create(&tmp).await.map_err(|err| {
            BridgeError::SessionStore(format!("create temp session failed: {err}"))
        })?;
        use tokio::io::AsyncWriteExt;
        file.write_all(&encrypted)
            .await
            .map_err(|err| BridgeError::SessionStore(format!("write session failed: {err}")))?;
        file.sync_all()
            .await
            .map_err(|err| BridgeError::SessionStore(format!("sync session failed: {err}")))?;
        drop(file);
        tokio::fs::rename(&tmp, &path)
            .await
            .map_err(|err| BridgeError::SessionStore(format!("rename session failed: {err}")))
    }

    async fn load_file_session(
        &self,
        dir: &Path,
        key: &[u8; 32],
        validated_session_id: &str,
    ) -> BridgeResult<gaze::Session> {
        let file_lock = self.file_lock(validated_session_id).await;
        let _guard = file_lock.lock().await;
        let path = session_path(dir, validated_session_id);
        match tokio::fs::read(&path).await {
            Ok(bytes) => {
                let plaintext = decrypt_snapshot(key, validated_session_id.as_bytes(), &bytes)?;
                gaze::Session::import(gaze::SensitiveSnapshot::from(plaintext)).map_err(|err| {
                    BridgeError::SessionStore(format!("session import failed: {err}"))
                })
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                gaze::Session::new(gaze::Scope::Conversation(validated_session_id.to_string()))
                    .map_err(|err| BridgeError::SessionStore(err.to_string()))
            }
            Err(err) => Err(BridgeError::SessionStore(format!(
                "read session file failed: {err}"
            ))),
        }
    }

    async fn file_lock(&self, validated_session_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self.file_locks.lock().await;
        Arc::clone(
            locks
                .entry(validated_session_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }
}

fn parse_key(raw: &str) -> BridgeResult<[u8; 32]> {
    let trimmed = raw.trim();
    let bytes = if trimmed.len() == 64 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
        hex::decode(trimmed).map_err(|err| BridgeError::Config(format!("hex key failed: {err}")))?
    } else if trimmed.len() == 32 {
        trimmed.as_bytes().to_vec()
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(trimmed)
            .map_err(|err| BridgeError::Config(format!("base64 session key failed: {err}")))?
    };
    bytes
        .try_into()
        .map_err(|_| BridgeError::Config("session key must decode to 32 bytes".to_string()))
}

fn encrypt_snapshot(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> BridgeResult<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let mut nonce_bytes = [0_u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|err| BridgeError::SessionStore(format!("session encrypt failed: {err}")))?;
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

fn decrypt_snapshot(key: &[u8; 32], aad: &[u8], bytes: &[u8]) -> BridgeResult<Vec<u8>> {
    if bytes.len() < MAGIC.len() + NONCE_LEN || &bytes[..MAGIC.len()] != MAGIC {
        return Err(BridgeError::SessionStore(
            "session file has invalid encrypted header".to_string(),
        ));
    }
    let nonce_start = MAGIC.len();
    let nonce_end = nonce_start + NONCE_LEN;
    let nonce = Nonce::from_slice(&bytes[nonce_start..nonce_end]);
    let cipher = ChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(
            nonce,
            Payload {
                msg: &bytes[nonce_end..],
                aad,
            },
        )
        .map_err(|err| BridgeError::SessionStore(format!("session decrypt failed: {err}")))
}

fn session_path(dir: &Path, validated_session_id: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(validated_session_id.as_bytes());
    dir.join(format!("{}.gaze-session", hex::encode(hasher.finalize())))
}

fn random_hex(len: usize) -> String {
    let mut bytes = vec![0_u8; len];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_key_accepts_32_bytes() {
        let key = parse_key(&"11".repeat(32)).expect("key");
        assert_eq!(key, [0x11; 32]);
    }
}
