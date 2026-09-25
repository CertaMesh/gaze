use std::ops::Range;

use unicode_normalization::UnicodeNormalization;

pub struct NormalizedText {
    pub text: String,
    pub spans: Vec<(usize, usize)>,
}

pub fn normalize(input: &str) -> NormalizedText {
    let mut text = String::new();
    let mut spans = Vec::new();

    for (start, ch) in input.char_indices() {
        let end = start + ch.len_utf8();
        if matches!(ch, '\u{200C}' | '\u{200D}') {
            continue;
        }

        let mapped = fullwidth_to_ascii(ch);
        let normalized = mapped.to_string().nfc().collect::<String>();
        text.push_str(&normalized);
        for _ in 0..normalized.len() {
            spans.push((start, end));
        }
    }

    NormalizedText { text, spans }
}

/// Map a byte range of the normalized text back to the raw bytes it came from. Every
/// normalized byte carries its source char's full raw range, so a folded 2- or 3-byte
/// separator maps back to all of its raw bytes. Returns `None` for an empty or
/// out-of-bounds range.
pub fn raw_range(span: Range<usize>, spans: &[(usize, usize)]) -> Option<Range<usize>> {
    if span.is_empty() || span.end > spans.len() {
        return None;
    }

    Some(spans[span.start].0..spans[span.end - 1].1)
}

fn fullwidth_to_ascii(ch: char) -> char {
    match ch {
        // Every Unicode space separator (general category Zs) is a group separator wherever an
        // ASCII space is. PDFs and banking UIs write IBANs, cards and tax IDs with NBSP, NARROW
        // NBSP or THIN SPACE between groups; patterns written with `\x20` / `[ -]` and validators
        // that strip only ASCII whitespace (Luhn, mod-97) missed them, so the value shipped raw
        // (solo todo #3819). Folding here fixes every recognizer and validator at once; the span
        // map keeps tokens and restore byte-exact to the original separator.
        '\u{00A0}'
        | '\u{1680}'
        | '\u{2000}'..='\u{200A}'
        | '\u{202F}'
        | '\u{205F}'
        | '\u{3000}' => ' ',
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(ch as u32 - 0xFEE0).unwrap_or(ch),
        _ => ch,
    }
}
