use std::borrow::Cow;
use std::ops::Range;

use gaze_types::Manifest;

use crate::pipeline::{Error, Result};

/// Stable model input. Eight ASCII bytes replace eight ASCII bytes, so scan
/// offsets are identical to observable clean-text offsets, including near UTF-8.
pub(crate) struct SafetyNetScanText<'a> {
    text: Cow<'a, str>,
}

impl<'a> SafetyNetScanText<'a> {
    pub(crate) fn new(clean_text: &'a str, manifest: &Manifest) -> Result<Self> {
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
            let Some(offset) = session_hex_offset(token) else {
                continue;
            };
            let bytes = stable.get_or_insert_with(|| clean_text.as_bytes().to_vec());
            let start = emitted.clean_span.start + offset;
            bytes[start..start + 8].copy_from_slice(b"00000000");
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
    pub(crate) fn to_clean_range(&self, range: Range<usize>) -> Result<Range<usize>> {
        if range.start >= range.end
            || !self.text.is_char_boundary(range.start)
            || !self.text.is_char_boundary(range.end)
        {
            return Err(Error::SafetyNetSpanInvalid {
                start: range.start,
                end: range.end,
                text_len: self.text.len(),
            });
        }
        Ok(range)
    }
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
        let scan = SafetyNetScanText::new(&text, &Manifest::from_spans(spans)).unwrap();
        assert_eq!(
            scan.text(),
            "<00000000:Name_1> 🦊 email2.00000000@gaze-fake.invalid 🦊 00000000:custom:tenant_3"
        );
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
            let scan = SafetyNetScanText::new(&text, &manifest).unwrap();
            prop_assert_eq!(scan.text().len(), text.len());
            for index in 0..=text.len() {
                prop_assert_eq!(scan.text().is_char_boundary(index), text.is_char_boundary(index));
                if text.is_char_boundary(index) && index < text.len() {
                    let next = text[index..].chars().next().unwrap().len_utf8() + index;
                    prop_assert_eq!(scan.to_clean_range(index..next).unwrap(), index..next);
                }
            }
        }
    }
}
