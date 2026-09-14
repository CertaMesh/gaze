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

#[derive(Default)]
struct SessionCache {
    entries: HashMap<String, SharedSession>,
    lru: VecDeque<String>,
}

pub struct BridgeSessionStore {
    mode: SessionStoreMode,
    cache: Mutex<SessionCache>,
    // One entry per distinct session id ever observed in File mode. Not bounded
    // by `max_sessions`; evicting a cache entry does not drop its lock entry
    // because callers may hold the `Arc` clone across long-running load/persist
    // work. Tracked as a known secondary leak with a small (~60-80 byte) per-entry
    // cost; safe eviction requires an `Arc::strong_count` sweep and is deferred.
    file_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    // Positive at every construction boundary.
    max_sessions: usize,
}

impl BridgeSessionStore {
    pub fn from_config(config: &SessionCfg) -> BridgeResult<Self> {
        config.validate()?;
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
            cache: Mutex::new(SessionCache::default()),
            file_locks: Mutex::new(HashMap::new()),
            max_sessions: config.max_sessions,
        })
    }

    pub fn ephemeral() -> Self {
        Self {
            mode: SessionStoreMode::Ephemeral,
            cache: Mutex::new(SessionCache::default()),
            file_locks: Mutex::new(HashMap::new()),
            max_sessions: crate::config::DEFAULT_MAX_SESSIONS,
        }
    }

    pub async fn len(&self) -> usize {
        self.cache.lock().await.entries.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.cache.lock().await.entries.is_empty()
    }

    pub async fn get(&self, validated_session_id: &str) -> BridgeResult<SharedSession> {
        let mut cache = self.cache.lock().await;

        if let Some(session) = cache.entries.get(validated_session_id).cloned() {
            cache.lru.retain(|id| id != validated_session_id);
            cache.lru.push_back(validated_session_id.to_string());
            return Ok(session);
        }

        let candidate = if cache.entries.len() >= self.max_sessions {
            if matches!(self.mode, SessionStoreMode::Ephemeral) {
                return Err(BridgeError::LimitExceeded(
                    "session cap reached; use file mode for long-lived bridge deployments"
                        .to_string(),
                ));
            }
            let candidate = cache
                .lru
                .iter()
                .find(|id| Arc::strong_count(&cache.entries[*id]) == 1)
                .ok_or_else(|| {
                    BridgeError::LimitExceeded(
                        "session cap reached; all cached sessions are active".to_string(),
                    )
                })?
                .clone();
            Some(candidate)
        } else {
            None
        };

        // Stage the load without changing the cache. Preserve persistence-error
        // precedence if both the incoming load and candidate flush fail.
        let incoming = match &self.mode {
            SessionStoreMode::Ephemeral => gaze::Session::new(gaze::Scope::Ephemeral)
                .map_err(|err| BridgeError::SessionStore(err.to_string())),
            SessionStoreMode::File { dir, key } => {
                self.load_file_session(dir, key, validated_session_id).await
            }
        };
        if let Some(candidate) = &candidate {
            // The earlier count is only a hint: Weak::upgrade bypasses the
            // cache lock. get_mut atomically excludes both strong borrowers and
            // weak handles before persistence. With the cache still locked,
            // no new handle can appear before removal, even across the await.
            let candidate_session = Arc::get_mut(cache.entries.get_mut(candidate).unwrap())
                .ok_or_else(|| {
                    BridgeError::LimitExceeded(
                        "session cap reached; eviction candidate is still shared".to_string(),
                    )
                })?;
            // Persist under exclusive ownership: failed/cancelled callers may
            // have left mappings that never reached disk.
            self.persist(candidate, candidate_session.get_mut()).await?;
        }
        let shared = Arc::new(Mutex::new(incoming?));
        // Commit both indexes only after every fallible/awaiting operation.
        // Failure or cancellation leaves the canonical entry and LRU intact.
        if let Some(candidate) = candidate {
            cache.entries.remove(&candidate);
            cache.lru.retain(|id| id != &candidate);
        }
        cache
            .entries
            .insert(validated_session_id.to_string(), Arc::clone(&shared));
        cache.lru.push_back(validated_session_id.to_string());

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

    #[tokio::test]
    async fn cancelled_exclusive_persist_retains_candidate_and_allows_retry() {
        use std::future::Future;
        use std::task::Poll;

        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let dir = tempfile::TempDir::new().unwrap();
            let store = BridgeSessionStore {
                mode: SessionStoreMode::File {
                    dir: dir.path().to_path_buf(),
                    key: [0x44; 32],
                },
                cache: Mutex::new(SessionCache::default()),
                file_locks: Mutex::new(HashMap::new()),
                max_sessions: 1,
            };
            let a = store.get("session-a").await.unwrap();
            let token = a
                .lock()
                .await
                .tokenize(&gaze_types::PiiClass::Email, "alice@example.invalid")
                .unwrap();
            drop(a);

            // Hold the candidate's file lock until admission reaches persist, after
            // acquiring exclusive session ownership. Its clone is the rendezvous.
            let file_lock = store.file_lock("session-a").await;
            let guard = file_lock.lock().await;
            assert_eq!(Arc::strong_count(&file_lock), 2);
            let mut admission = Box::pin(store.get("session-b"));
            std::future::poll_fn(|cx| {
                assert!(admission.as_mut().poll(cx).is_pending());
                if Arc::strong_count(&file_lock) == 3 {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
            drop(admission);
            drop(guard);
            assert_eq!(store.len().await, 1);
            assert_eq!(
                store
                    .get("session-a")
                    .await
                    .unwrap()
                    .lock()
                    .await
                    .restore(&token),
                Some("alice@example.invalid".to_string())
            );
            drop(
                store
                    .get("session-b")
                    .await
                    .expect("retry after cancellation"),
            );
            assert_eq!(
                store
                    .get("session-a")
                    .await
                    .unwrap()
                    .lock()
                    .await
                    .restore(&token),
                Some("alice@example.invalid".to_string())
            );
        })
        .await
        .expect("cancellation rendezvous and retry must complete within 10 seconds");
    }
}
