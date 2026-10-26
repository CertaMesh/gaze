//! Lexical tag syntax shared by primary record ownership and repeat matching.
use std::ops::Range;

#[derive(Default)]
pub struct MarkupRegions {
    assignments: Vec<Range<usize>>,
    syntax: Vec<Range<usize>>,
}

impl MarkupRegions {
    pub fn new(text: &str) -> Self {
        if !text.contains('<') {
            return Self::default();
        }
        let data = data_scalars(text);
        let bytes = text.as_bytes();
        let mut result = Self::default();
        let mut at = 0;
        while at < bytes.len() {
            let data_at = data.partition_point(|range| range.end <= at);
            if let Some(range) = data.get(data_at).filter(|range| range.contains(&at)) {
                at = range.end;
                continue;
            }
            if bytes[at] != b'<' {
                at += 1;
                continue;
            }
            let tail = text[at + 1..].strip_prefix('/').unwrap_or(&text[at + 1..]);
            if !tail
                .chars()
                .next()
                .is_some_and(|ch| ch.is_alphabetic() || matches!(ch, '_' | ':'))
            {
                at += 1;
                continue;
            }
            let start = at;
            let syntax_before = result.syntax.len();
            let mut syntax = at;
            let mut assignment = false;
            at += 1;
            while at < bytes.len() {
                match bytes[at] {
                    b'=' => {
                        assignment = true;
                        at += 1;
                    }
                    b'"' | b'\'' => {
                        let quote = bytes[at];
                        result.syntax.push(syntax..at + 1);
                        at += 1;
                        let mut escaped = false;
                        while at < bytes.len() {
                            if bytes[at] == quote && !escaped {
                                break;
                            }
                            escaped = bytes[at] == b'\\' && !escaped;
                            at += 1;
                        }
                        syntax = at;
                        at += usize::from(at < bytes.len());
                    }
                    b'>' => {
                        at += 1;
                        break;
                    }
                    _ => at += 1,
                }
            }
            if syntax < at {
                result.syntax.push(syntax..at);
            }
            if assignment {
                result.assignments.push(start..at);
            } else {
                result.syntax.truncate(syntax_before);
            }
        }
        result
    }

    pub fn contains_assignment(&self, at: usize) -> bool {
        let index = self.assignments.partition_point(|range| range.end <= at);
        self.assignments
            .get(index)
            .is_some_and(|range| range.contains(&at))
    }

    pub fn blocks_repeat(&self, span: &Range<usize>) -> bool {
        let index = self.syntax.partition_point(|range| range.end <= span.start);
        self.syntax
            .get(index)
            .is_some_and(|range| range.start <= span.start && span.end <= range.end)
    }
}

fn data_scalars(text: &str) -> Vec<Range<usize>> {
    let regions = crate::json_regions::json_regions(text);
    let mut data: Vec<_> = crate::json_regions::json_strings(text, &regions)
        .into_iter()
        .filter_map(|(span, key)| (!key).then_some(span))
        .collect();
    let trimmed = text.trim();
    if serde_json::from_str::<String>(trimmed).is_ok() {
        let start = text.len() - text.trim_start().len();
        data.push(start + 1..start + trimmed.len() - 1);
    }
    if text.contains('\\') {
        let mut view = text.to_owned();
        let mut source: Vec<_> = (0..text.len()).map(|at| at..at + 1).collect();
        for _ in 0..32 {
            let Some((decoded, projection)) = crate::json_regions::unescape_json_fragments(&view)
            else {
                break;
            };
            source = projection
                .into_iter()
                .map(|range| source[range.start].start..source[range.end - 1].end)
                .collect();
            view = decoded;
            let regions = crate::json_regions::json_regions(&view);
            for (range, key) in crate::json_regions::json_strings(&view, &regions) {
                if !key && !range.is_empty() {
                    data.push(source[range.start].start..source[range.end - 1].end);
                }
            }
        }
    }
    let bytes = text.as_bytes();
    // Closed CSV cells. A quoted cell must start and end at a record delimiter.
    let mut at = 0;
    while at < bytes.len() {
        while at < bytes.len() && matches!(bytes[at], b' ' | b'\t') {
            at += 1;
        }
        if bytes.get(at) == Some(&b'"') {
            let opening = at;
            at = quote_end(bytes, at, b'"', false);
            if at < bytes.len() {
                let suffix = text[at + 1..].trim_start_matches([' ', '\t']);
                if suffix
                    .chars()
                    .next()
                    .is_none_or(|ch| matches!(ch, ',' | '\r' | '\n'))
                {
                    data.push(opening + 1..at);
                }
                at += 1;
            }
        }
        while at < bytes.len() && !matches!(bytes[at], b',' | b'\r' | b'\n') {
            at += 1;
        }
        at += 1;
    }
    // SQL record prefixes establish literal context. Quotes in prose do not.
    let mut at = 0;
    let mut line_start = true;
    let mut sql = false;
    while at < bytes.len() {
        if line_start && !sql {
            let tail = text[at..].trim_start_matches([' ', '\t']);
            sql = ["select", "insert", "update", "replace", "values"]
                .iter()
                .any(|word| {
                    tail.get(..word.len())
                        .is_some_and(|head| head.eq_ignore_ascii_case(word))
                        && tail[word.len()..].chars().next().is_none_or(|ch| {
                            ch.is_ascii() && !ch.is_ascii_alphanumeric() && ch != '_'
                        })
                });
        }
        line_start = false;
        if sql && bytes.get(at..at + 2) == Some(b"--") {
            let start = at;
            while at < bytes.len() && !matches!(bytes[at], b'\r' | b'\n') {
                at += 1;
            }
            data.push(start..at);
            line_start = true;
        } else if sql && bytes.get(at..at + 2) == Some(b"/*") {
            let start = at;
            at += 2;
            while at + 1 < bytes.len() && &bytes[at..at + 2] != b"*/" {
                at += 1;
            }
            at = (at + 2).min(bytes.len());
            data.push(start..at);
        } else if sql && matches!(bytes[at], b'\'' | b'"' | b'`') {
            let opening = at;
            at = quote_end(bytes, at, bytes[at], true);
            if at < bytes.len() {
                data.push(opening + 1..at);
                at += 1;
            }
        } else {
            if matches!(bytes[at], b'\r' | b'\n') {
                line_start = true;
            }
            at += 1;
        }
    }
    data.retain(|range| !range.is_empty());
    data.sort_unstable_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in data {
        if let Some(last) = merged.last_mut().filter(|last| last.end >= range.start) {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

fn quote_end(bytes: &[u8], opening: usize, quote: u8, escapes: bool) -> usize {
    let mut at = opening + 1;
    while at < bytes.len() {
        if escapes && bytes[at] == b'\\' {
            at = (at + 2).min(bytes.len());
        } else if bytes[at] == quote {
            if bytes.get(at + 1) != Some(&quote) {
                break;
            }
            at += 2;
        } else {
            at += 1;
        }
    }
    at
}
