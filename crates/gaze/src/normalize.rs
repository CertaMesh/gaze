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

fn fullwidth_to_ascii(ch: char) -> char {
    match ch {
        '\u{3000}' => ' ',
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(ch as u32 - 0xFEE0).unwrap_or(ch),
        _ => ch,
    }
}
