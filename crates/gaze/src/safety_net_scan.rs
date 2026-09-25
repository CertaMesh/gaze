use std::borrow::Cow;
use std::ops::Range;

use gaze_types::Manifest;
use sha2::{Digest, Sha256};

use crate::pipeline::{Error, Result};

/// Stable model input. Eight ASCII bytes replace eight ASCII bytes, so scan
/// offsets are identical to observable clean-text offsets, including near UTF-8.
pub(crate) struct SafetyNetScanText<'a> {
    text: Cow<'a, str>,
}

impl<'a> SafetyNetScanText<'a> {
    pub(crate) fn new(
        clean_text: &'a str,
        manifest: &Manifest,
        is_owned: impl Fn(&str) -> bool,
    ) -> Result<Self> {
        let mut stable = None::<Vec<u8>>;
        for emitted in &manifest.spans {
            let token =
                clean_text
                    .get(emitted.clean_span.clone())
                    .ok_or(Error::SafetyNetSpanInvalid {
                        start: emitted.clean_span.start,
                        end: emitted.clean_span.end,
                        text_len: clean_text.len(),
                    })?;
            replace_session_hex(clean_text, &mut stable, emitted.clean_span.start, token);
        }
        // Scan-only APIs have no manifest. The live session can still prove which
        // token-shaped strings it minted; leave every unowned literal unchanged.
        for matched in crate::token_shape::pattern().find_iter(clean_text) {
            if is_owned(matched.as_str()) {
                replace_session_hex(clean_text, &mut stable, matched.start(), matched.as_str());
            }
        }

        let text = match stable {
            Some(bytes) => {
                Cow::Owned(String::from_utf8(bytes).expect("ASCII replacement preserves UTF-8"))
            }
            None => Cow::Borrowed(clean_text),
        };
        Ok(Self { text })
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// The exact offset map is the identity.
    pub(crate) fn to_clean_range(&self, range: Range<usize>) -> Range<usize> {
        range
    }
}

fn replace_session_hex(
    clean_text: &str,
    stable: &mut Option<Vec<u8>>,
    token_start: usize,
    token: &str,
) {
    let Some(offset) = session_hex_offset(token) else {
        return;
    };
    let bytes = stable.get_or_insert_with(|| clean_text.as_bytes().to_vec());
    let start = token_start + offset;
    bytes[start..start + 8].copy_from_slice(b"00000000");
    // Hash only the placeholder shape after removing session entropy. This
    // keeps the model view stable without deriving the prefix from raw PII.
    let mut hasher = Sha256::new();
    hasher.update(b"gaze-safety-net-token-v3\0");
    hasher.update(&bytes[token_start..token_start + token.len()]);
    let digest = hasher.finalize();
    let surrogate = hex::encode(&digest[..4]);
    bytes[start..start + 8].copy_from_slice(surrogate.as_bytes());
}

fn session_hex_offset(token: &str) -> Option<usize> {
    let bytes = token.as_bytes();
    let offset = if bytes.first() == Some(&b'<') && bytes.get(9) == Some(&b':') {
        1
    } else if token.starts_with("email") && token.ends_with("@gaze-fake.invalid") {
        token.find('.')? + 1
    } else if bytes.get(8) == Some(&b':') {
        0
    } else {
        return None;
    };
    bytes
        .get(offset..offset + 8)
        .filter(|hex| {
            hex.iter()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
        .map(|_| offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaze_types::{EmittedTokenSpan, PiiClass};
    use proptest::prelude::*;

    #[test]
    fn canonicalizes_every_emitted_session_hex_shape() {
        let tokens = [
            "<deadbeef:Name_1>",
            "email2.deadbeef@gaze-fake.invalid",
            "deadbeef:custom:tenant_3",
        ];
        let text = tokens.join(" 🦊 ");
        let mut cursor = 0;
        let spans = tokens
            .iter()
            .map(|token| {
                let start = text[cursor..].find(token).unwrap() + cursor;
                cursor = start + token.len();
                EmittedTokenSpan::new(start..cursor, 0..1, PiiClass::Name)
            })
            .collect();
        let scan = SafetyNetScanText::new(&text, &Manifest::from_spans(spans), |_| false).unwrap();
        let prefixes = tokens
            .iter()
            .map(|token| {
                let single = SafetyNetScanText::new(token, &Manifest::default(), |candidate| {
                    candidate == *token
                })
                .unwrap();
                let offset = session_hex_offset(token).unwrap();
                single.text()[offset..offset + 8].to_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            scan.text(),
            format!(
                "<{}:Name_1> 🦊 email2.{}@gaze-fake.invalid 🦊 {}:custom:tenant_3",
                prefixes[0], prefixes[1], prefixes[2]
            )
        );
        assert!(prefixes.iter().all(|prefix| prefix != "00000000"));
        assert_eq!(scan.text().len(), text.len());
    }

    proptest! {
        #[test]
        fn byte_offsets_are_identical_with_utf8_neighbors(
            before in "[a-z🦊é]{0,20}",
            after in "[a-z🦊é]{0,20}",
            ordinal in 1u16..1000,
            hex in "[0-9a-f]{8}",
        ) {
            let token = format!("<{hex}:Name_{ordinal}>");
            let text = format!("{before}{token}{after}");
            let start = before.len();
            let end = start + token.len();
            let manifest = Manifest::from_spans(vec![EmittedTokenSpan::new(start..end, 0..1, PiiClass::Name)]);
            let scan = SafetyNetScanText::new(&text, &manifest, |_| false).unwrap();
            prop_assert_eq!(scan.text().len(), text.len());
            for index in 0..=text.len() {
                prop_assert_eq!(scan.text().is_char_boundary(index), text.is_char_boundary(index));
                if text.is_char_boundary(index) && index < text.len() {
                    let next = text[index..].chars().next().unwrap().len_utf8() + index;
                    prop_assert_eq!(scan.to_clean_range(index..next), index..next);
                }
            }
        }
    }
}
