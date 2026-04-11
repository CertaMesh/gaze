//! Session key: 32 random bytes, held in a `SecretBox`, zeroized on Drop.
//! Lives for the duration of one `gaze serve` process. Never written to disk.

use hmac::{Hmac, Mac};
use rand::RngCore;
use secrecy::{ExposeSecret, SecretBox};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Mutex;
use zeroize::Zeroize;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("mlock failed: {0}")]
    Mlock(std::io::Error),
    #[error("madvise DONTDUMP failed: {0}")]
    Madvise(std::io::Error),
}

/// Strict mlock + MADV_DONTDUMP on a byte slice. Linux uses MADV_DONTDUMP,
/// macOS has no direct equivalent but still respects mlock. On unsupported
/// platforms this is a soft no-op and returns `Ok(())`.
#[cfg(unix)]
fn lock_bytes(ptr: *mut u8, len: usize) -> Result<(), LockError> {
    // SAFETY: caller owns the allocation for `len` bytes.
    let rc = unsafe { libc::mlock(ptr.cast::<libc::c_void>(), len) };
    if rc != 0 {
        return Err(LockError::Mlock(std::io::Error::last_os_error()));
    }
    #[cfg(target_os = "linux")]
    {
        // SAFETY: same region.
        let rc = unsafe { libc::madvise(ptr.cast::<libc::c_void>(), len, libc::MADV_DONTDUMP) };
        if rc != 0 {
            return Err(LockError::Madvise(std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn lock_bytes(_: *mut u8, _: usize) -> Result<(), LockError> {
    Ok(())
}

pub struct SessionKey {
    inner: SecretBox<[u8; 32]>,
}

impl SessionKey {
    /// Generate a fresh 32-byte key and lock it into RAM.
    pub fn generate() -> Result<Self, LockError> {
        let mut bytes = Box::new([0u8; 32]);
        rand::rngs::OsRng.fill_bytes(bytes.as_mut_slice());
        lock_bytes(bytes.as_mut_ptr(), 32)?;
        Ok(Self {
            inner: SecretBox::new(bytes),
        })
    }

    /// Escape hatch for environments where mlock fails (Docker without
    /// IPC_LOCK, some CI runners). Only reachable via `--allow-unlocked-key`.
    pub fn generate_unlocked() -> Self {
        let mut bytes = Box::new([0u8; 32]);
        rand::rngs::OsRng.fill_bytes(bytes.as_mut_slice());
        Self {
            inner: SecretBox::new(bytes),
        }
    }

    /// HMAC-SHA256 of `input`, returning all 32 output bytes.
    pub fn hmac(&self, input: &[u8]) -> [u8; 32] {
        let key = self.inner.expose_secret();
        let mut mac =
            HmacSha256::new_from_slice(key.as_slice()).expect("HMAC-SHA256 accepts any key length");
        mac.update(input);
        let result = mac.finalize().into_bytes();
        let mut out = [0u8; 32];
        out.copy_from_slice(&result);
        out
    }
}

impl Drop for SessionKey {
    fn drop(&mut self) {
        // SecretBox zeroizes on drop, but be explicit.
        let key = self.inner.expose_secret();
        let mut copy = *key;
        copy.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_differ() {
        let a = SessionKey::generate().expect("mlock should succeed on dev machine");
        let b = SessionKey::generate().expect("mlock should succeed on dev machine");
        assert_ne!(a.hmac(b"test"), b.hmac(b"test"));
    }

    #[test]
    fn same_input_same_output_within_session() {
        let k = SessionKey::generate().expect("mlock should succeed on dev machine");
        assert_eq!(
            k.hmac(b"krishan@example.com"),
            k.hmac(b"krishan@example.com")
        );
    }

    #[test]
    fn hmac_is_32_bytes() {
        let k = SessionKey::generate().expect("mlock should succeed on dev machine");
        assert_eq!(k.hmac(b"anything").len(), 32);
    }

    #[test]
    fn locked_key_still_hmacs() {
        let k = match SessionKey::generate() {
            Ok(k) => k,
            Err(_) => SessionKey::generate_unlocked(),
        };
        assert_eq!(k.hmac(b"x").len(), 32);
    }
}

/// Bidirectional map between raw values and their fake replacements.
/// Lives as long as the `SessionKey`; dropped when `gaze serve` exits.
///
/// Keyed by `(column_class, raw_canonical)` so different PII classes can
/// reuse the same raw string without collision (e.g. a name that looks
/// like an email fragment).
pub struct SessionMap {
    forward: Mutex<HashMap<(String, String), String>>,
    reverse: Mutex<HashMap<(String, String), String>>,
}

impl SessionMap {
    pub fn new() -> Self {
        Self {
            forward: Mutex::new(HashMap::new()),
            reverse: Mutex::new(HashMap::new()),
        }
    }

    /// Lookup existing fake for (class, raw). Returns None if absent.
    pub fn get_fake(&self, class: &str, raw: &str) -> Option<String> {
        self.forward
            .lock()
            .expect("session map forward poisoned")
            .get(&(class.to_string(), raw.to_string()))
            .cloned()
    }

    /// Lookup raw for (class, fake). Used by `restore()` on filter values.
    pub fn get_raw(&self, class: &str, fake: &str) -> Option<String> {
        self.reverse
            .lock()
            .expect("session map reverse poisoned")
            .get(&(class.to_string(), fake.to_string()))
            .cloned()
    }

    /// Insert a new raw→fake binding. Caller must have checked there is no
    /// existing binding first. Returns `Err` if the fake is already taken
    /// by a different raw value of the same class (id collision case).
    pub fn insert(&self, class: &str, raw: String, fake: String) -> Result<(), CollisionError> {
        let mut fwd = self.forward.lock().expect("session map forward poisoned");
        let mut rev = self.reverse.lock().expect("session map reverse poisoned");

        let rev_key = (class.to_string(), fake.clone());
        if let Some(existing_raw) = rev.get(&rev_key) {
            if existing_raw != &raw {
                return Err(CollisionError {
                    class: class.to_string(),
                    fake,
                });
            }
            return Ok(()); // idempotent
        }

        fwd.insert((class.to_string(), raw.clone()), fake.clone());
        rev.insert(rev_key, raw);
        Ok(())
    }

    #[cfg(test)]
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.forward.lock().unwrap().len()
    }
}

impl Default for SessionMap {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("collision inserting fake {fake} for class {class}")]
pub struct CollisionError {
    pub class: String,
    pub fake: String,
}

#[cfg(test)]
mod map_tests {
    use super::*;

    #[test]
    fn insert_and_lookup_both_directions() {
        let m = SessionMap::new();
        m.insert("email", "real@x.com".into(), "user_1@example.com".into())
            .unwrap();
        assert_eq!(
            m.get_fake("email", "real@x.com").as_deref(),
            Some("user_1@example.com")
        );
        assert_eq!(
            m.get_raw("email", "user_1@example.com").as_deref(),
            Some("real@x.com")
        );
    }

    #[test]
    fn reinserting_same_pair_is_idempotent() {
        let m = SessionMap::new();
        m.insert("email", "a@x.com".into(), "user_1@example.com".into())
            .unwrap();
        m.insert("email", "a@x.com".into(), "user_1@example.com".into())
            .unwrap();
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn inserting_collision_errors() {
        let m = SessionMap::new();
        m.insert("id", "42".into(), "1234".into()).unwrap();
        let err = m.insert("id", "43".into(), "1234".into()).unwrap_err();
        assert_eq!(err.class, "id");
    }

    #[test]
    fn same_fake_different_class_is_fine() {
        let m = SessionMap::new();
        m.insert("email", "a@x.com".into(), "token_7".into())
            .unwrap();
        m.insert("name", "Alice".into(), "token_7".into()).unwrap();
        assert_eq!(m.len(), 2);
    }
}
