use std::io::{self, Write};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;
use zeroize::{Zeroize, Zeroizing};

use crate::{DashboardError, DashboardErrorCode};

/// Exact Authorization scheme spelling.
pub const AUTHORIZATION_SCHEME_V1: &str = "GazeDashboardV1";
const AUTHORIZATION_PREFIX: &[u8] = b"GazeDashboardV1 ";
const TOKEN_LEN: usize = 43;

struct Secret32([u8; 32]);

impl Secret32 {
    fn generate() -> Result<Self, DashboardError> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes)
            .map_err(|_| DashboardError::new(DashboardErrorCode::PairingFailed))?;
        Ok(Self(bytes))
    }

    fn with_bytes<R>(&self, callback: impl for<'a> FnOnce(&'a [u8; 32]) -> R) -> R {
        callback(&self.0)
    }
}

impl Drop for Secret32 {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Per-launch pairing secret with no formatting, cloning, serialization, or conversion surface.
pub struct PairingSecret(Secret32);

impl PairingSecret {
    /// Generates 256 bits from the operating-system CSPRNG.
    pub fn generate() -> Result<Self, DashboardError> {
        Secret32::generate().map(Self)
    }

    pub(crate) fn with_bytes<R>(&self, callback: impl for<'a> FnOnce(&'a [u8; 32]) -> R) -> R {
        self.0.with_bytes(callback)
    }

    pub(crate) fn from_pairing_frame(bytes: [u8; 32]) -> Self {
        Self(Secret32(bytes))
    }

    pub(crate) fn canonical_token(&self) -> Zeroizing<[u8; TOKEN_LEN]> {
        let mut token = Zeroizing::new([0_u8; TOKEN_LEN]);
        self.with_bytes(|bytes| {
            let written = URL_SAFE_NO_PAD
                .encode_slice(bytes, token.as_mut())
                .expect("43-byte base64url destination is exact");
            debug_assert_eq!(written, TOKEN_LEN);
        });
        token
    }
}

/// Per-page session secret, distinct from launch and CSRF secrets.
pub struct PageSessionSecretV1(Secret32);

impl PageSessionSecretV1 {
    pub(crate) fn generate() -> Result<Self, DashboardError> {
        Secret32::generate().map(Self)
    }
}

/// Per-page CSRF secret, distinct from launch and page-session secrets.
pub struct CsrfSecretV1(Secret32);

impl CsrfSecretV1 {
    pub(crate) fn generate() -> Result<Self, DashboardError> {
        Secret32::generate().map(Self)
    }
}

/// Parsed canonical launch authorization. It contains decoded bytes only in zeroizing storage.
pub struct CanonicalAuthorizationV1 {
    bytes: Zeroizing<[u8; 32]>,
}

impl CanonicalAuthorizationV1 {
    /// Parses only the exact scheme, one ASCII space, and a 43-byte unpadded base64url token.
    pub fn parse(header: &[u8]) -> Result<Self, DashboardError> {
        if header.len() != AUTHORIZATION_PREFIX.len() + TOKEN_LEN
            || !header.starts_with(AUTHORIZATION_PREFIX)
        {
            return Err(DashboardError::new(DashboardErrorCode::Unauthorized));
        }
        let encoded = &header[AUTHORIZATION_PREFIX.len()..];
        if encoded
            .iter()
            .any(|byte| !matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'))
        {
            return Err(DashboardError::new(DashboardErrorCode::Unauthorized));
        }
        let mut decoded = Zeroizing::new([0_u8; 32]);
        let written = URL_SAFE_NO_PAD
            .decode_slice(encoded, decoded.as_mut())
            .map_err(|_| DashboardError::new(DashboardErrorCode::Unauthorized))?;
        if written != 32 {
            return Err(DashboardError::new(DashboardErrorCode::Unauthorized));
        }
        let mut canonical = Zeroizing::new([0_u8; TOKEN_LEN]);
        let encoded_len = URL_SAFE_NO_PAD
            .encode_slice(decoded.as_ref(), canonical.as_mut())
            .map_err(|_| DashboardError::new(DashboardErrorCode::Unauthorized))?;
        if encoded_len != TOKEN_LEN || canonical.as_ref().ct_eq(encoded).unwrap_u8() != 1 {
            return Err(DashboardError::new(DashboardErrorCode::Unauthorized));
        }
        Ok(Self { bytes: decoded })
    }

    pub(crate) fn validates(&self, expected_digest: &[u8; 32]) -> bool {
        digest(self.bytes.as_ref())
            .ct_eq(expected_digest)
            .unwrap_u8()
            == 1
    }
}

/// Fixed, manually encoded bootstrap response carrying the page-session and CSRF secrets once.
pub struct SensitiveBootstrapResponseV1 {
    page_session: PageSessionSecretV1,
    csrf: CsrfSecretV1,
}

impl SensitiveBootstrapResponseV1 {
    pub(crate) fn generate() -> Result<Self, DashboardError> {
        Ok(Self {
            page_session: PageSessionSecretV1::generate()?,
            csrf: CsrfSecretV1::generate()?,
        })
    }

    /// Consumes the envelope and writes its fixed 65-byte versioned representation once.
    pub fn write_once(self, writer: &mut impl Write) -> io::Result<()> {
        let bytes = self.encode_for_one_response();
        writer.write_all(bytes.as_ref())
    }

    pub(crate) fn encode_for_one_response(&self) -> Zeroizing<[u8; 65]> {
        let mut encoded = Zeroizing::new([0_u8; 65]);
        encoded[0] = 1;
        self.page_session
            .0
            .with_bytes(|bytes| encoded[1..33].copy_from_slice(bytes));
        self.csrf
            .0
            .with_bytes(|bytes| encoded[33..65].copy_from_slice(bytes));
        encoded
    }

    pub(crate) fn digests(&self) -> ([u8; 32], [u8; 32]) {
        (
            self.page_session.0.with_bytes(|bytes| digest(bytes)),
            self.csrf.0.with_bytes(|bytes| digest(bytes)),
        )
    }
}

pub(crate) struct AuthRegistry {
    launch_digest: [u8; 32],
    generation: u64,
    sessions: Vec<SessionDigest>,
    max_sessions: usize,
}

struct SessionDigest {
    generation: u64,
    page: [u8; 32],
    csrf: [u8; 32],
}

impl AuthRegistry {
    pub(crate) fn new(secret: &PairingSecret, max_sessions: usize) -> Self {
        Self {
            launch_digest: secret.with_bytes(|bytes| digest(bytes)),
            generation: 0,
            sessions: Vec::new(),
            max_sessions,
        }
    }

    pub(crate) fn pair(
        &mut self,
        authorization: &CanonicalAuthorizationV1,
    ) -> Result<SensitiveBootstrapResponseV1, DashboardError> {
        if !authorization.validates(&self.launch_digest) || self.sessions.len() >= self.max_sessions
        {
            return Err(DashboardError::new(DashboardErrorCode::Unauthorized));
        }
        let response = SensitiveBootstrapResponseV1::generate()?;
        let (page, csrf) = response.digests();
        self.sessions.push(SessionDigest {
            generation: self.generation,
            page,
            csrf,
        });
        Ok(response)
    }

    pub(crate) fn validate_session(&self, page: &[u8], csrf: &[u8]) -> bool {
        let page = digest(page);
        let csrf = digest(csrf);
        self.sessions.iter().any(|session| {
            session.generation == self.generation
                && session.page.ct_eq(&page).unwrap_u8() == 1
                && session.csrf.ct_eq(&csrf).unwrap_u8() == 1
        })
    }

    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn purge(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.sessions.clear();
    }

    pub(crate) fn rotate(&mut self, secret: &PairingSecret) {
        self.purge();
        self.launch_digest = secret.with_bytes(|bytes| digest(bytes));
    }
}

pub(crate) fn decode_secondary_header(value: &[u8]) -> Option<Zeroizing<[u8; 32]>> {
    if value.len() != TOKEN_LEN
        || value
            .iter()
            .any(|byte| !matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'))
    {
        return None;
    }
    let mut decoded = Zeroizing::new([0_u8; 32]);
    let written = URL_SAFE_NO_PAD.decode_slice(value, decoded.as_mut()).ok()?;
    if written != 32 {
        return None;
    }
    let mut canonical = Zeroizing::new([0_u8; TOKEN_LEN]);
    let encoded = URL_SAFE_NO_PAD
        .encode_slice(decoded.as_ref(), canonical.as_mut())
        .ok()?;
    if encoded != TOKEN_LEN || canonical.as_ref().ct_eq(value).unwrap_u8() != 1 {
        return None;
    }
    Some(decoded)
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_authorization_rejects_alternates() {
        let secret = PairingSecret::generate().unwrap();
        let token = secret.canonical_token();
        let mut header = Vec::from(AUTHORIZATION_PREFIX);
        header.extend_from_slice(token.as_ref());
        let parsed = CanonicalAuthorizationV1::parse(&header).unwrap();
        assert!(parsed.validates(&secret.with_bytes(|bytes| digest(bytes))));

        let mut padded = header.clone();
        padded.push(b'=');
        assert!(CanonicalAuthorizationV1::parse(&padded).is_err());
        let mut wrong_case = header;
        wrong_case[0] = b'g';
        assert!(CanonicalAuthorizationV1::parse(&wrong_case).is_err());
    }
}
