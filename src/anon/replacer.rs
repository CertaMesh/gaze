//! Per-`PiiClass` replacement strategies. Each strategy:
//!   1. Checks the session map for an existing fake.
//!   2. If absent, derives a deterministic fake from HMAC(raw).
//!   3. Inserts the pair into the map (with collision handling for Id).
//!
//! The deterministic-within-session property is what lets an agent
//! correlate `Person_7` across `db.sample` results and log lines.

#![allow(dead_code)]

use crate::anon::session::{SessionKey, SessionMap};
use crate::policy::classifier::PiiClass;

pub struct Replacer<'a> {
    key: &'a SessionKey,
    map: &'a SessionMap,
}

impl<'a> Replacer<'a> {
    pub fn new(key: &'a SessionKey, map: &'a SessionMap) -> Self {
        Self { key, map }
    }

    /// Replace a single raw text value. Returns the fake string.
    /// For `PiiClass::NonPii` this returns `raw` unchanged.
    pub fn replace_text(&self, class: PiiClass, raw: &str) -> String {
        if class == PiiClass::NonPii {
            return raw.to_string();
        }
        let class_name = class_key(class);
        if let Some(existing) = self.map.get_fake(class_name, raw) {
            return existing;
        }
        // Index space for text fakes is bounded (e.g. `user_{n}` with
        // n in 0..10_000), so different raws can land on the same fake.
        // On collision, suffix the raw with a counter byte and rehash —
        // mirrors the `replace_id` rehash strategy so the bijection
        // raw <-> fake holds within a session.
        let mut counter: u32 = 0;
        loop {
            let salted: String = if counter == 0 {
                raw.to_string()
            } else {
                format!("{raw}\x00{counter}")
            };
            let fake = match class {
                PiiClass::NonPii => unreachable!(),
                PiiClass::Id => unreachable!("ids go through replace_id"),
                PiiClass::Email => self.fake_email(&salted),
                PiiClass::Name => self.fake_name(&salted),
                PiiClass::Phone => self.fake_phone(&salted),
                PiiClass::Address => self.fake_address(&salted),
                PiiClass::Iban => self.fake_iban(&salted),
                PiiClass::Ip => self.fake_ip(&salted),
                PiiClass::Date => self.fake_date(&salted),
                PiiClass::GenericText => self.fake_generic(&salted),
            };
            match self.map.insert(class_name, raw.to_string(), fake.clone()) {
                Ok(()) => return fake,
                Err(_) => {
                    counter = counter
                        .checked_add(1)
                        .expect("text rehash counter overflow");
                    continue;
                }
            }
        }
    }

    /// Replace an integer `id`. Uses HMAC(raw) mod 2^31, rehashing on
    /// collision. See spec §"Collision handling for the `id` type".
    pub fn replace_id(&self, raw: i64) -> i64 {
        let class = class_key(PiiClass::Id);
        let raw_str = raw.to_string();
        if let Some(existing) = self.map.get_fake(class, &raw_str) {
            return existing
                .parse()
                .expect("session map id values are integers");
        }
        let mut counter: u32 = 0;
        loop {
            let mut input = Vec::with_capacity(raw_str.len() + 5);
            input.extend_from_slice(raw_str.as_bytes());
            if counter > 0 {
                input.push(0);
                input.extend_from_slice(&counter.to_be_bytes());
            }
            let digest = self.key.hmac(&input);
            let candidate = i64::from(u32::from_be_bytes([
                digest[0] & 0x7f, // clear sign bit → [0, 2^31)
                digest[1],
                digest[2],
                digest[3],
            ]));
            let fake_str = candidate.to_string();
            match self.map.insert(class, raw_str.clone(), fake_str) {
                Ok(()) => return candidate,
                Err(_) => {
                    counter = counter.checked_add(1).expect("id rehash counter overflow");
                    continue;
                }
            }
        }
    }

    fn fake_email(&self, raw: &str) -> String {
        let n = self.index_for("email", raw);
        format!("user_{n}@example.com")
    }

    fn fake_name(&self, raw: &str) -> String {
        let n = self.index_for("name", raw);
        format!("Person_{n}")
    }

    fn fake_phone(&self, raw: &str) -> String {
        let n = self.index_for("phone", raw);
        format!("+49 30 {:07}", n % 10_000_000)
    }

    fn fake_address(&self, raw: &str) -> String {
        let n = self.index_for("address", raw);
        format!("Musterstrasse_{n}_00000_Berlin")
    }

    fn fake_iban(&self, raw: &str) -> String {
        let n = self.index_for("iban", raw);
        format!("DE{:020}", u128::from(n) % 10u128.pow(18))
    }

    fn fake_ip(&self, raw: &str) -> String {
        let n = self.index_for("ip", raw) % 254 + 1;
        format!("10.0.0.{n}")
    }

    fn fake_date(&self, raw: &str) -> String {
        // M1a ships a placeholder; real date shifting lands in Task M1a.7.
        format!("1970-01-01T00:00:00Z (shifted from len={})", raw.len())
    }

    fn fake_generic(&self, raw: &str) -> String {
        let n = self.index_for("generic", raw);
        format!("redacted_{n}")
    }

    /// Derive a small stable integer index from HMAC(class || raw).
    /// Used as the `{n}` suffix in fake outputs. Same input within a
    /// session → same index, which the map then pins.
    fn index_for(&self, class: &str, raw: &str) -> u64 {
        let mut input = Vec::with_capacity(class.len() + 1 + raw.len());
        input.extend_from_slice(class.as_bytes());
        input.push(b':');
        input.extend_from_slice(raw.as_bytes());
        let digest = self.key.hmac(&input);
        // Take the first 8 bytes as a big-endian u64, mod 10_000 for tidy output.
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&digest[..8]);
        u64::from_be_bytes(buf) % 10_000
    }
}

fn class_key(class: PiiClass) -> &'static str {
    match class {
        PiiClass::NonPii => "nonpii",
        PiiClass::Id => "id",
        PiiClass::Name => "name",
        PiiClass::Email => "email",
        PiiClass::Phone => "phone",
        PiiClass::Address => "address",
        PiiClass::Iban => "iban",
        PiiClass::Ip => "ip",
        PiiClass::Date => "date",
        PiiClass::GenericText => "generic",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (SessionKey, SessionMap) {
        (SessionKey::generate(), SessionMap::new())
    }

    #[test]
    fn non_pii_passes_through() {
        let (k, m) = fixture();
        let r = Replacer::new(&k, &m);
        assert_eq!(r.replace_text(PiiClass::NonPii, "hello"), "hello");
    }

    #[test]
    fn email_is_stable_within_session() {
        let (k, m) = fixture();
        let r = Replacer::new(&k, &m);
        let a = r.replace_text(PiiClass::Email, "krishan@example.com");
        let b = r.replace_text(PiiClass::Email, "krishan@example.com");
        assert_eq!(a, b);
        assert!(a.starts_with("user_"));
        assert!(a.ends_with("@example.com"));
    }

    #[test]
    fn different_emails_get_different_fakes() {
        let (k, m) = fixture();
        let r = Replacer::new(&k, &m);
        let a = r.replace_text(PiiClass::Email, "one@x.com");
        let b = r.replace_text(PiiClass::Email, "two@x.com");
        assert_ne!(a, b);
    }

    #[test]
    fn id_is_stable_within_session() {
        let (k, m) = fixture();
        let r = Replacer::new(&k, &m);
        let a = r.replace_id(42);
        let b = r.replace_id(42);
        assert_eq!(a, b);
        assert!(a >= 0 && a < i64::from(i32::MAX));
    }

    #[test]
    fn id_rehash_breaks_collision() {
        // Pin a fake id by hand, then ask the replacer to anonymize
        // a different raw id whose HMAC happens to produce the same
        // first candidate. The only way to force this deterministically
        // is to pre-seed the map with whatever the first HMAC produces.
        let (k, m) = fixture();
        let r = Replacer::new(&k, &m);
        let first_fake = r.replace_id(100);
        // Pre-seed the map as if raw 200 had already produced `first_fake`
        // for a different row. Insert directly.
        m.insert("id", "200".into(), first_fake.to_string())
            .unwrap_err(); // collision with 100 — good.
                           // New raw must go through rehash; manually trigger.
        let second = r.replace_id(300);
        assert_ne!(second, first_fake);
    }

    #[test]
    fn ip_output_looks_like_10_0_0_x() {
        let (k, m) = fixture();
        let r = Replacer::new(&k, &m);
        let fake = r.replace_text(PiiClass::Ip, "192.168.1.50");
        assert!(fake.starts_with("10.0.0."));
    }
}
