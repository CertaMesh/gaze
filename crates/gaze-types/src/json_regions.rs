//! Byte regions of independently valid JSON containers in mixed tool output.
use std::ops::Range;

/// Find maximal valid regions, including complete records after truncated log lines.
/// A raw line break cannot occur inside a JSON string, so it resets quote state.
pub fn json_regions(input: &str) -> Vec<Range<usize>> {
    let bytes = input.as_bytes();
    let mut stack = Vec::new();
    let mut candidates = Vec::new();
    let mut quoted = false;
    let mut quoted_openings = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if matches!(byte, b'\n' | b'\r') {
            quoted = false;
        } else if quoted {
            if matches!(byte, b'{' | b'[') {
                quoted_openings.push(at);
            }
            if byte == b'\\' {
                at += 1;
            } else if byte == b'"' {
                quoted = false;
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'{' | b'[' => stack.push((at, byte)),
                b'}' | b']' => {
                    let expected = if byte == b'}' { b'{' } else { b'[' };
                    if let Some((start, opening)) = stack.pop() {
                        if opening == expected {
                            candidates.push(start..at + 1);
                        } else {
                            stack.clear();
                        }
                    }
                }
                _ => {}
            }
        }
        at += 1;
    }
    candidates.sort_unstable_by_key(|span| (span.start, std::cmp::Reverse(span.end)));
    let mut regions: Vec<Range<usize>> = Vec::new();
    for span in candidates {
        if regions.last().is_some_and(|region| region.end >= span.end) {
            continue;
        }
        if serde_json::from_str::<serde_json::Value>(&input[span.clone()]).is_ok() {
            regions.push(span);
        }
    }
    // Prose quotes have no JSON semantics. Recover complete containers hidden by
    // inch marks or truncated log quotes, while leaving validated string content alone.
    let mut recovered = Vec::new();
    let mut recovered_end = 0;
    for start in quoted_openings {
        let at = regions.partition_point(|region| region.end <= start);
        if start < recovered_end || regions.get(at).is_some_and(|region| region.start <= start) {
            continue;
        }
        let mut stream =
            serde_json::Deserializer::from_str(&input[start..]).into_iter::<serde_json::Value>();
        if stream.next().is_some_and(|value| value.is_ok()) {
            recovered_end = start + stream.byte_offset();
            recovered.push(start..recovered_end);
        }
    }
    regions.extend(recovered);
    regions.sort_unstable_by_key(|span| (span.start, std::cmp::Reverse(span.end)));
    let mut end = 0;
    regions.retain(|span| {
        if span.end <= end {
            false
        } else {
            end = span.end;
            true
        }
    });
    regions
}

/// Decode one layer of valid JSON escapes in mixed output, retaining the source
/// byte range of every decoded byte. Invalid escapes remain literal.
pub fn unescape_json_fragments(input: &str) -> Option<(String, Vec<Range<usize>>)> {
    let mut text = String::with_capacity(input.len());
    let mut source = Vec::with_capacity(input.len());
    let mut at = 0;
    let mut changed = false;
    while at < input.len() {
        let start = at;
        let escape = (input.as_bytes()[at] == b'\\')
            .then(|| {
                let len = if input.as_bytes().get(at + 1) == Some(&b'u') {
                    let code = input
                        .get(at + 2..at + 6)
                        .and_then(|hex| u16::from_str_radix(hex, 16).ok());
                    if code.is_some_and(|code| (0xd800..=0xdbff).contains(&code)) {
                        12
                    } else {
                        6
                    }
                } else {
                    2
                };
                let raw = input.get(at..at + len)?;
                serde_json::from_str::<String>(&format!("\"{raw}\""))
                    .ok()
                    .map(|decoded| (len, decoded))
            })
            .flatten();
        let decoded = if let Some((len, decoded)) = escape {
            at += len;
            changed = true;
            decoded
        } else {
            let ch = input[at..].chars().next().expect("character");
            at += ch.len_utf8();
            ch.to_string()
        };
        text.push_str(&decoded);
        source.extend(std::iter::repeat_n(start..at, decoded.len()));
    }
    changed.then_some((text, source))
}

/// Length of one JSON escape whose decoded character is whitespace.
pub fn json_whitespace_escape_len(text: &str) -> Option<usize> {
    match text.as_bytes().get(..2)? {
        b"\\t" | b"\\n" | b"\\r" | b"\\f" => Some(2),
        b"\\u" => {
            let code = u16::from_str_radix(text.get(2..6)?, 16).ok()?;
            char::from_u32(u32::from(code))
                .filter(|ch| ch.is_whitespace() || *ch == '\u{200b}')
                .map(|_| 6)
        }
        _ => None,
    }
}

/// String interiors and their key/value role inside validated JSON regions.
pub fn json_strings(input: &str, regions: &[Range<usize>]) -> Vec<(Range<usize>, bool)> {
    let bytes = input.as_bytes();
    let mut strings = Vec::new();
    for region in regions {
        let mut at = region.start;
        while at < region.end {
            if bytes[at] != b'"' {
                at += 1;
                continue;
            }
            let start = at + 1;
            at += 1;
            while bytes[at] != b'"' {
                at += if bytes[at] == b'\\' { 2 } else { 1 };
            }
            let end = at;
            at += 1;
            let mut next = at;
            while bytes.get(next).is_some_and(u8::is_ascii_whitespace) {
                next += 1;
            }
            strings.push((start..end, bytes.get(next) == Some(&b':')));
        }
    }
    strings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncated_prefixes_do_not_hide_complete_records() {
        let record = r#"{"Customer ID:":"SYN731284","nested":[{"x":"[public]"}]}"#;
        for prefix in [
            "log [truncated\n",
            "{\"n\":0\n",
            "note: \"unbalanced quote\n",
            "```json\r\n",
            "TV 55\" screen ",
            "path C:\\\" ",
            "msg \"oops ",
        ] {
            let text = format!("{prefix}{record}\n{record}");
            let regions = json_regions(&text);
            assert_eq!(regions.len(), 2, "{text}");
            assert!(regions.iter().all(|span| &text[span.clone()] == record));
            let strings = json_strings(&text, &regions);
            assert_eq!(strings.iter().filter(|(_, key)| *key).count(), 6);
            assert_eq!(strings.iter().filter(|(_, key)| !key).count(), 4);
        }
    }
}
