//! FROZEN CONTRACT — shared deterministic helpers. Master-owned.
//!
//! These are correctness-critical and shared across tracks (token parsing, the
//! index-alias format, canonicalization whitespace rules, hashing). Keep them here
//! so Track A (projection) and Track B (ingest) canonicalize/alias identically —
//! divergence breaks index lookup.

use gaze::PiiClass;
use sha2::{Digest, Sha256};

/// Parse the class out of a gaze token like `<XXXXXXXX:Name_1>`.
/// Returns `None` for anything not matching the gaze token shape.
pub fn parse_token_class(token: &str) -> Option<PiiClass> {
    let inner = token.strip_prefix('<')?.strip_suffix('>')?;
    let (session_hex, label_and_ordinal) = inner.split_once(':')?;
    if session_hex.len() != 8 || !session_hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let (label, ordinal) = label_and_ordinal.rsplit_once('_')?;
    if ordinal.is_empty() || !ordinal.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    match label {
        "Email" => Some(PiiClass::Email),
        "Name" => Some(PiiClass::Name),
        "Location" => Some(PiiClass::Location),
        "Organization" => Some(PiiClass::Organization),
        custom if custom.starts_with("Custom:") => {
            PiiClass::from_canonical_str(&format!("custom:{}", &custom["Custom:".len()..]))
        }
        _ => None,
    }
}

/// Hex-encode the SHA-256 of `value`. Used for owner-side audit (raw values are
/// only ever persisted as this digest, never in cleartext).
pub fn sha256_hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex(hasher.finalize().as_slice())
}

/// Lowercase hex-encode bytes.
pub fn hex(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(TABLE[(byte >> 4) as usize] as char);
        out.push(TABLE[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Collapse runs of ASCII whitespace to single spaces (canonicalization input).
pub fn collapse_ascii_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The index-internal, owner-side alias format for an entity in a domain. This is
/// NEVER agent-visible — the response translator must replace every occurrence with
/// a current-session token before output.
pub fn domain_alias(class: &PiiClass, fingerprint_hex: &str) -> String {
    let suffix = fingerprint_hex
        .chars()
        .take(4)
        .map(|ch| match ch {
            '0'..='9' => ((ch as u8 - b'0') + b'A') as char,
            'a'..='f' => ((ch as u8 - b'a') + b'K') as char,
            'A'..='F' => ((ch as u8 - b'A') + b'K') as char,
            _ => 'Z',
        })
        .collect::<String>();
    format!("<{}_{}>", class.class_name(), suffix)
}

/// True if `value` still contains an index-domain alias (translator leak guard).
pub fn contains_domain_alias(value: &str) -> bool {
    [
        "<Email_",
        "<Name_",
        "<Location_",
        "<Organization_",
        "<Custom:",
    ]
    .iter()
    .any(|needle| value.contains(needle))
}
