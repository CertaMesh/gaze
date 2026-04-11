//! Session key: 32 random bytes, held in a `SecretBox`, zeroized on Drop.
//! Lives for the duration of one `gaze serve` process. Never written to disk.

#![allow(dead_code)]

use hmac::{Hmac, Mac};
use rand::RngCore;
use secrecy::{ExposeSecret, SecretBox};
use sha2::Sha256;
use zeroize::Zeroize;

type HmacSha256 = Hmac<Sha256>;

pub struct SessionKey {
    inner: SecretBox<[u8; 32]>,
}

impl SessionKey {
    /// Generate a fresh 32-byte key from the OS CSPRNG.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        Self {
            inner: SecretBox::new(Box::new(bytes)),
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
        let a = SessionKey::generate();
        let b = SessionKey::generate();
        assert_ne!(a.hmac(b"test"), b.hmac(b"test"));
    }

    #[test]
    fn same_input_same_output_within_session() {
        let k = SessionKey::generate();
        assert_eq!(
            k.hmac(b"krishan@example.com"),
            k.hmac(b"krishan@example.com")
        );
    }

    #[test]
    fn hmac_is_32_bytes() {
        let k = SessionKey::generate();
        assert_eq!(k.hmac(b"anything").len(), 32);
    }
}
