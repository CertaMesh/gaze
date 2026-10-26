//! Byte regions of independently valid JSON containers in mixed tool output.
use std::ops::Range;

/// Find maximal valid regions, including complete records after truncated log lines.
/// A raw line break cannot occur inside a JSON string, so it resets quote state.
pub fn json_regions(input: &str) -> Vec<Range<usize>> {
    let bytes = input.as_bytes();
    let mut stack = Vec::new();
    let mut candidates = Vec::new();
    let mut quoted = false;
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if matches!(byte, b'\n' | b'\r') {
            quoted = false;
        } else if quoted {
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
    regions
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
