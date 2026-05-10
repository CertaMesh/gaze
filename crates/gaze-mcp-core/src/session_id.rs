//! Session-id format + entropy policy for the chokepoint dispatcher.
//!
//! Adopters sometimes pass an external session id from the transport into
//! the manifest (so audit rows correlate with an upstream conversation,
//! request, or trace id). [`SessionIdPolicy`] gives gaze-mcp-core a fail-closed
//! way to validate those ids before they reach the [`crate::manifest::ManifestStore`].
//!
//! The default policy ([`SessionIdPolicy::default_strict`]) accepts only
//! ULID and canonical UUID strings and requires at least 80 bits of effective
//! entropy. Adopters can relax via [`SessionIdFormat::Custom`] but the entropy
//! floor still applies — Custom formats contribute zero estimated entropy by
//! default, so a strict policy will reject them unless the adopter lowers
//! `min_entropy_bits` explicitly.

use regex::Regex;
use thiserror::Error;

/// Format whitelist entry for [`SessionIdPolicy`].
///
/// Built-in [`Self::Ulid`] and [`Self::Uuid`] cases carry conservative entropy
/// estimates; [`Self::Custom`] takes an adopter-supplied regex and contributes
/// zero entropy by default (the policy's `min_entropy_bits` decides whether
/// it passes).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum SessionIdFormat {
    /// Crockford Base32 ULID (26 ASCII characters; first character `0..='7'`).
    /// Effective entropy: 80 bits (random component).
    Ulid,
    /// Canonical hyphenated UUID (`8-4-4-4-12` lowercase or uppercase hex).
    /// Effective entropy: 122 bits (assuming v4).
    Uuid,
    /// Adopter-defined format. Validation is just the regex match; entropy
    /// estimation is left to the adopter (defaults to 0 bits — combine with
    /// a lower `min_entropy_bits` or an explicit override on the policy).
    Custom(Regex),
}

impl SessionIdFormat {
    /// True if `id` matches this format.
    pub fn matches(&self, id: &str) -> bool {
        match self {
            Self::Ulid => is_canonical_ulid(id),
            Self::Uuid => is_canonical_uuid(id),
            Self::Custom(re) => re.is_match(id),
        }
    }

    /// Conservative lower bound on the effective entropy of an id matching
    /// this format. `Custom` returns 0 — adopters with a tight regex can lower
    /// `SessionIdPolicy::min_entropy_bits` to a value the regex's structure
    /// guarantees, or wrap their own pre-validation upstream.
    pub fn effective_entropy_bits(&self) -> u32 {
        match self {
            Self::Ulid => 80,
            Self::Uuid => 122,
            Self::Custom(_) => 0,
        }
    }
}

/// Policy for accepting transport-supplied session ids.
///
/// Build with [`Self::default_strict`] for the recommended posture (ULID +
/// UUID only, 80-bit entropy floor) or construct manually for custom shapes.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SessionIdPolicy {
    /// Minimum effective entropy (bits) required for the matched format.
    pub min_entropy_bits: u32,
    /// Ordered list of acceptable formats. The first matching format wins.
    pub format_whitelist: Vec<SessionIdFormat>,
}

impl SessionIdPolicy {
    /// Construct an explicit policy.
    pub fn new(min_entropy_bits: u32, format_whitelist: Vec<SessionIdFormat>) -> Self {
        Self {
            min_entropy_bits,
            format_whitelist,
        }
    }

    /// Strict fail-closed default: ULID + UUID only, 80-bit entropy floor.
    pub fn default_strict() -> Self {
        Self {
            min_entropy_bits: 80,
            format_whitelist: vec![SessionIdFormat::Ulid, SessionIdFormat::Uuid],
        }
    }

    /// Validate `id` against the policy. Returns `Ok(())` if any whitelisted
    /// format matches AND that format's effective entropy meets the floor.
    pub fn validate(&self, id: &str) -> Result<(), SessionIdError> {
        if id.is_empty() {
            return Err(SessionIdError::Empty);
        }
        for fmt in &self.format_whitelist {
            if fmt.matches(id) {
                let bits = fmt.effective_entropy_bits();
                if bits < self.min_entropy_bits {
                    return Err(SessionIdError::InsufficientEntropy {
                        required: self.min_entropy_bits,
                        actual: bits,
                    });
                }
                return Ok(());
            }
        }
        Err(SessionIdError::DisallowedFormat)
    }
}

/// Reasons [`SessionIdPolicy::validate`] rejects an id.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionIdError {
    /// The session id was empty. Always fails closed regardless of policy.
    #[error("session id is empty")]
    Empty,
    /// No whitelisted [`SessionIdFormat`] matched the id.
    #[error("session id does not match any whitelisted format")]
    DisallowedFormat,
    /// A whitelisted format matched but its effective entropy is below the
    /// policy's `min_entropy_bits` floor.
    #[error("session id entropy {actual} bits is below required floor {required}")]
    InsufficientEntropy {
        /// Required minimum entropy bits per [`SessionIdPolicy::min_entropy_bits`].
        required: u32,
        /// Effective entropy of the matched format.
        actual: u32,
    },
}

fn is_canonical_ulid(id: &str) -> bool {
    if id.len() != 26 {
        return false;
    }
    let mut chars = id.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if !matches!(first, '0'..='7') {
        return false;
    }
    if !is_crockford_base32(first) {
        return false;
    }
    chars.all(is_crockford_base32)
}

fn is_crockford_base32(c: char) -> bool {
    matches!(c, '0'..='9' | 'A'..='H' | 'J'..='K' | 'M'..='N' | 'P'..='T' | 'V'..='Z')
}

fn is_canonical_uuid(id: &str) -> bool {
    if id.len() != 36 {
        return false;
    }
    let bytes = id.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        let expect_hyphen = matches!(i, 8 | 13 | 18 | 23);
        if expect_hyphen {
            if *b != b'-' {
                return false;
            }
        } else if !b.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_strict_accepts_ulid() {
        let policy = SessionIdPolicy::default_strict();
        assert!(policy.validate("01HRT7K6P6X5Q9M0V8YQ4N7TBC").is_ok());
    }

    #[test]
    fn default_strict_accepts_uuid() {
        let policy = SessionIdPolicy::default_strict();
        assert!(policy
            .validate("550e8400-e29b-41d4-a716-446655440000")
            .is_ok());
    }

    #[test]
    fn default_strict_rejects_disallowed_format() {
        let policy = SessionIdPolicy::default_strict();
        assert_eq!(
            policy.validate("session-1"),
            Err(SessionIdError::DisallowedFormat)
        );
    }

    #[test]
    fn empty_always_rejected() {
        let policy = SessionIdPolicy::default_strict();
        assert_eq!(policy.validate(""), Err(SessionIdError::Empty));
    }

    #[test]
    fn ulid_with_invalid_first_char_is_rejected() {
        let policy = SessionIdPolicy::default_strict();
        // Leading char `Z` exceeds the 0..7 range (ulids overflow guard).
        assert_eq!(
            policy.validate("ZZZZZZZZZZZZZZZZZZZZZZZZZZ"),
            Err(SessionIdError::DisallowedFormat)
        );
    }

    #[test]
    fn custom_format_passes_only_with_lowered_entropy_floor() {
        let re = Regex::new(r"^sess-[a-z0-9]{8}$").unwrap();
        // Strict floor (80 bits) rejects Custom even on a regex match.
        let strict = SessionIdPolicy::new(80, vec![SessionIdFormat::Custom(re.clone())]);
        assert_eq!(
            strict.validate("sess-abcd1234"),
            Err(SessionIdError::InsufficientEntropy {
                required: 80,
                actual: 0
            })
        );
        // Adopter who has an out-of-band entropy guarantee can lower the floor.
        let relaxed = SessionIdPolicy::new(0, vec![SessionIdFormat::Custom(re)]);
        assert!(relaxed.validate("sess-abcd1234").is_ok());
    }
}
