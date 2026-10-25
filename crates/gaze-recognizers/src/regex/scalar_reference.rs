// Frozen pre-index scanner from 8f63672bc, used only as a differential oracle.
// Unchanged scalar primitives are shared with the production scanner.
use super::*;

pub(super) fn scan_field_owned_value(
    input: &str,
    capture: std::ops::Range<usize>,
) -> LabelledValueScan {
    if input[..capture.start]
        .chars()
        .next_back()
        .is_some_and(field_wrapper_opens)
    {
        return scan_quoted_field_value(input, capture);
    }
    if input.as_bytes().get(capture.start) == Some(&b'\\') {
        if let Some(scan) = scan_json_encoded_wrapper(input, capture.clone()) {
            return scan;
        }
    }
    // The seed includes an unfamiliar wrapper instead of consuming it in the
    // regex. A personal field still owns its value; punctuation cannot turn
    // that evidence into an empty unquoted capture. Leading escape bytes are
    // wrapper syntax (for example Markdown's escaped backtick).
    let escapes = input[capture.start..]
        .bytes()
        .take_while(|byte| *byte == b'\\')
        .count();
    if let Some(opener) = input[capture.start + escapes..]
        .chars()
        .next()
        .filter(|ch| field_wrapper_opens(*ch))
    {
        let start = capture.start + escapes + opener.len_utf8();
        return scan_paired_field_value(input, start..capture.end.max(start), opener, escapes);
    }
    let mut end = capture.start;
    let mut reason = None;
    loop {
        let group_start = end;
        end += input[end..]
            .chars()
            .take_while(|ch| field_value_char(*ch))
            .map(char::len_utf8)
            .sum::<usize>();
        if end == group_start {
            break;
        }
        // A period or colon between components belongs to the identifier; at
        // the end of an unquoted group it is prose syntax. Quoted scalars use
        // their delimiter instead and retain every punctuation byte.
        let value_end = group_start + input[group_start..end].trim_end_matches(['.', ':']).len();
        if value_end != end {
            end = value_end;
            break;
        }
        let spaces = input[end..]
            .chars()
            .take_while(|ch| matches!(ch, ' ' | '\t' | '\u{00a0}' | '\u{202f}'))
            .map(char::len_utf8)
            .sum::<usize>();
        if spaces == 0 {
            break;
        }
        let next = end + spaces;
        if assignment_starts(&input[next..]) {
            reason = Some(LabelledValueScanReason::LabelBoundary);
            break;
        }
        if starts_with_date(&input[next..]) {
            reason = Some(LabelledValueScanReason::DateBoundary);
            break;
        }
        let group: String = input[next..]
            .chars()
            .take_while(|ch| field_value_char(*ch))
            .collect();
        let value_like = group.chars().any(char::is_numeric)
            || (group.chars().any(char::is_alphabetic)
                && group
                    .chars()
                    .filter(|ch| ch.is_alphabetic())
                    .all(char::is_uppercase));
        if !value_like {
            break;
        }
        end = next;
    }
    let span = capture.start..end;
    let repeat_seed = input[span.clone()]
        .rsplit_once('\t')
        .and_then(|(prefix, last)| {
            (prefix.chars().any(char::is_numeric)
                && last.chars().any(char::is_alphabetic)
                && last.chars().all(|ch| {
                    ch.is_alphabetic() || unicode_normalization::char::is_combining_mark(ch)
                }))
            .then_some(gaze_types::LabelledValueRepeatSeed {
                span: span.start..span.start + prefix.len(),
                kind: gaze_types::LabelledValueRepeatSeedKind::UncertainPrefix,
            })
        });
    LabelledValueScan {
        quoted_value: false,
        repeat_seed,
        capture: capture.start..capture.end.min(end),
        extension: capture.end.min(end)..end,
        reason: labelled_value_over_limit(&input[span.clone()])
            .then_some(LabelledValueScanReason::LimitExceeded)
            .or(reason),
        rejected: span.is_empty(),
        span,
    }
}

fn enclosing_csv_cell(input: &str, anchor: usize) -> Option<(usize, bool)> {
    let bytes = input.as_bytes();
    let mut at = 0;
    while at <= anchor && at < bytes.len() {
        scanner_work(1);
        while at < bytes.len() && matches!(bytes[at], b' ' | b'\t') {
            scanner_work(1);
            at += 1;
        }
        if at < bytes.len() && bytes[at] == b'"' {
            let opening = at;
            at += 1;
            let mut close = None;
            while at < bytes.len() {
                scanner_work(1);
                if bytes[at] == b'"' {
                    if bytes.get(at + 1) == Some(&b'"') {
                        at += 2;
                        continue;
                    }
                    close = Some(at);
                    break;
                }
                at += 1;
            }
            let end = close.unwrap_or(input.len());
            let suffix = input
                .get(end + usize::from(close.is_some())..)?
                .trim_start_matches([' ', '\t']);
            let cell_end = suffix
                .chars()
                .next()
                .is_none_or(|ch| matches!(ch, ',' | '\r' | '\n'));
            if cell_end && opening < anchor && anchor <= end {
                return Some((end, anchor == end));
            }
            if cell_end {
                at = end + usize::from(close.is_some());
            } else {
                at = opening + 1;
            }
        }
        while at < bytes.len() && !matches!(bytes[at], b',' | b'\r' | b'\n') {
            scanner_work(1);
            at += 1;
        }
        at += 1;
    }
    None
}

fn json_string_frame(input: &str, anchor: usize) -> Option<(std::ops::Range<usize>, bool)> {
    let bytes = input.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        scanner_work(1);
        // Escaped quotes in surrounding prose (including a prior raw-value
        // repeat) cannot open an enclosing JSON string.
        if bytes[at] == b'\\' {
            at += 2;
            continue;
        }
        if bytes[at] != b'"' {
            at += 1;
            continue;
        }
        let start = at;
        at += 1;
        while at < bytes.len() {
            scanner_work(1);
            match bytes[at] {
                b'\\' => at += 2,
                b'"' => break,
                _ => at += 1,
            }
        }
        let closed = at < bytes.len();
        if start < anchor && anchor < at {
            return Some((start..if closed { at + 1 } else { bytes.len() }, closed));
        }
        at += 1;
    }
    None
}

fn json_string_around(input: &str, anchor: usize) -> Option<std::ops::Range<usize>> {
    json_string_frame(input, anchor)
        .filter(|(_, closed)| *closed)
        .map(|(range, _)| range)
}

// Decode validated enclosing JSON strings or stream prefixes, keeping every emitted range
// in the original byte domain. Escaped Unicode punctuation is wrapper syntax
// just as the literal glyph is; the serializer cannot change field ownership.
fn scan_json_encoded_wrapper(
    input: &str,
    capture: std::ops::Range<usize>,
) -> Option<LabelledValueScan> {
    let mut view = ScalarSourceView {
        text: input.to_owned(),
        source: (0..input.len()).map(|at| at..at + 1).collect(),
    };
    loop {
        let anchor = view
            .source
            .iter()
            .position(|range| range.contains(&capture.start))?;
        let (token, closed) = json_string_frame(&view.text, anchor)?;
        view = view.json_frame(token, closed)?;
        let anchor = view
            .source
            .iter()
            .position(|range| range.contains(&capture.start))?;
        let escapes = view.text[anchor..]
            .bytes()
            .take_while(|byte| *byte == b'\\')
            .count();
        let opening = anchor + escapes;
        let opener = view.text[opening..].chars().next()?;
        if !field_wrapper_opens(opener) {
            continue;
        }
        let start = opening + opener.len_utf8();
        let seed_end = view
            .text
            .char_indices()
            .map(|(at, ch)| at + ch.len_utf8())
            .find(|end| view.source[end - 1].end >= capture.end)
            .unwrap_or(view.text.len())
            .max(start);
        let mut scan = if matches!(opener, '\'' | '"') {
            scan_quoted_field_value(&view.text, start..seed_end)
        } else {
            scan_paired_field_value(&view.text, start..seed_end, opener, escapes)
        };
        if scan.reason == Some(LabelledValueScanReason::UncertainQuoteBoundary) {
            // A decoded line break was escaped content in the proven enclosing
            // JSON string. Only an inner scalar/cell boundary can end ownership.
            let boundary = enclosing_scalar_boundary(&view.text, opening, start, escapes);
            let end = boundary.end.max(scan.span.end);
            scan.span.end = end;
            scan.extension.end = end;
            scan.rejected = scan.span.is_empty();
            scan.repeat_seed = (!scan.rejected).then_some(gaze_types::LabelledValueRepeatSeed {
                span: scan.span.clone(),
                kind: gaze_types::LabelledValueRepeatSeedKind::ExactScalar,
            });
        }
        let project = |range: std::ops::Range<usize>| {
            let start = view.source.get(range.start).map_or_else(
                || view.source.last().map_or(capture.start, |range| range.end),
                |range| range.start,
            );
            let end = if range.is_empty() {
                start
            } else {
                view.source[range.end - 1].end
            };
            start..end
        };
        scan.capture = project(scan.capture);
        scan.extension = project(scan.extension);
        scan.span = project(scan.span);
        if let Some(seed) = &mut scan.repeat_seed {
            seed.span = project(seed.span.clone());
        }
        scan.reason = scan.reason.or_else(|| {
            labelled_value_over_limit(&input[scan.span.clone()])
                .then_some(LabelledValueScanReason::LimitExceeded)
        });
        return Some(scan);
    }
}

fn enclosing_sql_literal(input: &str, anchor: usize) -> Option<(usize, bool, bool)> {
    static SQL: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let statements = SQL.get_or_init(|| {
        Regex::new(r"(?i)(?:\A|[\r\n])[ \t]*(?:select|insert|update|replace|values)\b")
            .expect("SQL statement record prefix")
    });
    fn literal(input: &str, anchor: usize, backslash_escapes: bool) -> Option<(usize, bool, bool)> {
        let bytes = input.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            scanner_work(1);
            if bytes.get(at..at + 2) == Some(b"--") {
                while at < bytes.len() && !matches!(bytes[at], b'\r' | b'\n') {
                    scanner_work(1);
                    at += 1;
                }
                continue;
            }
            if bytes.get(at..at + 2) == Some(b"/*") {
                at += 2;
                while at < bytes.len() && bytes.get(at..at + 2) != Some(b"*/") {
                    scanner_work(1);
                    at += 1;
                }
                at += 2;
                continue;
            }
            if matches!(bytes[at], b'"' | b'`' | b'[') {
                let quote = if bytes[at] == b'[' { b']' } else { bytes[at] };
                at += 1;
                while at < bytes.len() {
                    scanner_work(1);
                    if backslash_escapes && bytes[at] == b'\\' {
                        at += 2;
                        continue;
                    }
                    if bytes[at] == quote {
                        if bytes.get(at + 1) == Some(&quote) {
                            at += 2;
                            continue;
                        }
                        at += 1;
                        break;
                    }
                    at += 1;
                }
                continue;
            }
            if bytes[at] != b'\'' {
                at += 1;
                continue;
            }
            let opening = at;
            at += 1;
            while at < bytes.len() {
                scanner_work(1);
                if backslash_escapes && bytes[at] == b'\\' {
                    at += 2;
                    continue;
                }
                if bytes[at] == b'\'' {
                    if bytes.get(at + 1) == Some(&b'\'') {
                        at += 2;
                        continue;
                    }
                    if bytes
                        .get(at + 1)
                        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                    {
                        at += 1;
                        continue;
                    }
                    break;
                }
                at += 1;
            }
            let close = at.min(bytes.len());
            if opening <= anchor && anchor <= close {
                return Some((close, anchor == close, anchor == opening));
            }
            at += 1;
        }
        None
    }
    // A tool result can include prose or an earlier raw-value repeat before
    // the SQL record. Start its lexer at each statement, preserving literal
    // lines before the field cue instead of resetting at those content lines.
    statements
        .find_iter(&input[..anchor])
        .flat_map(|statement| {
            let start = statement.start();
            [false, true].into_iter().filter_map(move |escapes| {
                literal(&input[start..], anchor - start, escapes)
                    .map(|(end, absent, own)| (start + end, absent, own))
            })
        })
        .max_by_key(|(end, _, _)| *end)
}

fn encoded_scalar_boundary(input: &str, opening: usize) -> Option<EnclosingScalarBoundary> {
    scanner_work(input.len());
    let mut view = ScalarSourceView {
        text: input.to_owned(),
        source: (0..input.len()).map(|at| at..at + 1).collect(),
    };
    loop {
        let anchor = view
            .source
            .iter()
            .position(|range| range.contains(&opening))?;
        if let Some((close, absent_value, own_sql_literal)) =
            enclosing_sql_literal(&view.text, anchor)
        {
            let end = view.source.get(close).map_or_else(
                || view.source.last().map_or(input.len(), |range| range.end),
                |range| range.start,
            );
            return Some(EnclosingScalarBoundary {
                end,
                absent_value,
                own_sql_literal,
                doubled_quote: (!own_sql_literal).then_some(b'\''),
                record_lines_are_content: true,
            });
        }
        let csv = enclosing_csv_cell(&view.text, anchor);
        let json = json_string_around(&view.text, anchor).and_then(|token| {
            let close = token.end - 1;
            view.json_string(token).map(|decoded| (close, decoded))
        });
        // A doubled CSV quote after a literal backslash can resemble a JSON
        // string close. Conversely a JSON-escaped quote can resemble a CSV
        // close. Prefer the enclosing interpretation that retains more value.
        let closed_json_frame = json.as_ref().is_some_and(|(close, _)| {
            serde_json::from_str::<serde_json::Value>(&view.text).is_ok()
                || view.text[close + 1..]
                    .trim_start_matches([' ', '\t'])
                    .chars()
                    .next()
                    .is_none_or(|ch| matches!(ch, '\r' | '\n'))
        });
        if let Some((close, absent_value)) = csv.filter(|(close, _)| {
            !(*close == view.text.len() && closed_json_frame)
                && json
                    .as_ref()
                    .is_none_or(|(json_close, _)| close > json_close)
        }) {
            let end = view.source.get(close).map_or_else(
                || view.source.last().map_or(input.len(), |range| range.end),
                |range| range.start,
            );
            return Some(EnclosingScalarBoundary {
                end,
                absent_value,
                own_sql_literal: false,
                doubled_quote: Some(b'"'),
                record_lines_are_content: true,
            });
        }
        view = json?.1;
    }
}

fn enclosing_scalar_boundary(
    input: &str,
    opening: usize,
    start: usize,
    opening_escapes: usize,
) -> EnclosingScalarBoundary {
    if let Some(boundary) = encoded_scalar_boundary(input, opening) {
        return boundary;
    }
    // At each serialization level a delimiter has 2^depth - 1 backslashes.
    // Replay the prefix to find strings enclosing this scalar, excluding its
    // own opening quote and escape bytes. Key-looking content proves nothing.
    let mut enclosing = Vec::new();
    let mut literal_limit = None;
    let mut enclosing_literal = None;
    let prefix_end = opening - opening_escapes;
    let mut escapes = 0usize;
    let mut at = 0;
    while at < prefix_end {
        scanner_work(1);
        let byte = input.as_bytes()[at];
        if byte == b'\\' {
            escapes += 1;
            at += 1;
            continue;
        }
        if matches!(byte, b'\n' | b'\r') && escapes.is_multiple_of(2) {
            enclosing.clear();
            literal_limit = None;
            enclosing_literal = None;
        }
        let apostrophe = byte == b'\''
            && escapes.is_multiple_of(2)
            && !input[..at]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_alphanumeric() || ch == '_');
        let nested_double = byte == b'"'
            && (escapes + 1).is_power_of_two()
            && enclosing.last().is_none_or(|(depth, _)| escapes > *depth);
        if apostrophe || nested_double {
            let literal_escapes = if apostrophe { 0 } else { escapes };
            let width = literal_escapes + 1;
            let opening = at - literal_escapes;
            let run_end = quote_run_end(input, opening, byte, literal_escapes, prefix_end);
            let run = (run_end - opening) / width;
            let delimiter = match run {
                1 if apostrophe => Some(ScalarQuoteDelimiter::Single),
                3..=5 => Some(ScalarQuoteDelimiter::Triple),
                _ => None,
            };
            if let Some(delimiter) = delimiter {
                let count = match delimiter {
                    ScalarQuoteDelimiter::Single => 1,
                    ScalarQuoteDelimiter::Triple => 3,
                    ScalarQuoteDelimiter::DoubledSingle => 2,
                    ScalarQuoteDelimiter::DoubledTriple => 6,
                };
                let content = opening + count * width;
                let limit = *literal_limit.get_or_insert_with(|| {
                    serialized_string_end(
                        input,
                        content,
                        prefix_end,
                        enclosing.last().map(|(depth, _)| *depth),
                    )
                });
                // Prior literal fields use the same delimiter grammar and
                // enclosure bound as the current value. Their content quotes
                // cannot fabricate an enclosing JSON string around it.
                let prior_close =
                    scalar_quote_close(input, content, limit, byte, literal_escapes, delimiter);
                if apostrophe && prior_close.is_none() {
                    let limit = serialized_string_end(
                        input,
                        content,
                        input.len(),
                        enclosing.last().map(|(depth, _)| *depth),
                    );
                    enclosing_literal =
                        scalar_quote_close(input, content, limit, byte, literal_escapes, delimiter);
                }
                at = prior_close.map_or_else(
                    || uncertain_scalar_end(input, content, limit),
                    |close| close + count * width,
                );
                escapes = 0;
                continue;
            }
        }
        if byte == b'"' && (escapes + 1).is_power_of_two() {
            literal_limit = None;
            while enclosing.last().is_some_and(|(depth, _)| *depth > escapes) {
                enclosing.pop();
            }
            if enclosing.last().is_some_and(|(depth, _)| *depth == escapes) {
                enclosing.pop();
            } else {
                enclosing.push((escapes, at - escapes));
            }
        }
        escapes = 0;
        at += 1;
    }
    let depth = enclosing.last().map(|(depth, _)| *depth);
    let absent_value = input.as_bytes()[start - 1] == b'"'
        && depth.is_some_and(|depth| opening_escapes <= depth)
        && serialized_context_is_json(input, &enclosing, opening_escapes);
    // Scan from the enclosing opener, not from the current capture, which
    // may start halfway through a doubled CSV delimiter. JSON escapes and CSV
    // doubled quotes are separate encodings at every serialization depth.
    let end = enclosing.last().map_or(input.len(), |&(depth, opening)| {
        serialized_string_end(input, opening + depth + 1, input.len(), Some(depth))
    });
    let enclosing_literal = enclosing_literal.filter(|end| *end >= start);
    let record_lines_are_content = enclosing_literal.is_some()
        || enclosing.last().is_some_and(|&(depth, opening)| {
            depth == 0 && end >= start && quoted_csv_cell_encloses(input, opening, end)
        });
    // In opaque nested text a same-depth quote can be either the previous
    // string's close or this private scalar's opener. It cannot bound the
    // value before its first byte. Resume at the scalar for the larger valid
    // ownership interpretation; only fully parsed JSON proves absence.
    let end = if end < start && !absent_value {
        serialized_string_end(input, start, input.len(), depth)
    } else {
        end
    };
    EnclosingScalarBoundary {
        end: enclosing_literal.map_or(end, |literal| literal.min(end)),
        absent_value,
        record_lines_are_content,
        own_sql_literal: false,
        doubled_quote: if enclosing_literal.is_some() {
            Some(b'\'')
        } else {
            (depth == Some(opening_escapes) && !absent_value).then_some(b'"')
        },
    }
}

fn scan_quoted_field_value(input: &str, capture: std::ops::Range<usize>) -> LabelledValueScan {
    let opener = input[..capture.start]
        .chars()
        .next_back()
        .expect("wrapper opener");
    if !matches!(opener, '"' | '\'') {
        return scan_paired_field_value(input, capture, opener, 0);
    }
    let quote = opener as u8;
    let opening_escapes = input[..capture.start - 1]
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'\\')
        .count();
    // Both normal closes and uncertain fallbacks share this hard enclosure
    // boundary. A quote in a later sibling field can never close this value.
    let enclosing =
        enclosing_scalar_boundary(input, capture.start - 1, capture.start, opening_escapes);
    let limit = enclosing.end;
    let width = opening_escapes + 1;
    let opening_start = capture.start - width;
    let opening_end = quote_run_end(input, opening_start, quote, opening_escapes, limit);
    let opening_run = if enclosing.own_sql_literal {
        1
    } else {
        (opening_end - opening_start) / width
    };
    let doubled_quote = enclosing.doubled_quote == Some(quote);
    let known_empty = enclosing.absent_value
        || (doubled_quote
            && matches!(opening_run, 4 | 12)
            && !input[opening_end..]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_alphanumeric() || ch == '_'))
        || (!doubled_quote
            && width.is_power_of_two()
            && matches!(opening_run, 2 | 6)
            && !input[opening_end..]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_alphanumeric() || ch == '_'));
    let delimiter = match opening_run {
        2 if doubled_quote => Some(ScalarQuoteDelimiter::DoubledSingle),
        6..=10 if doubled_quote && opening_run.is_multiple_of(2) => {
            Some(ScalarQuoteDelimiter::DoubledTriple)
        }
        1 => Some(ScalarQuoteDelimiter::Single),
        3..=5 => Some(ScalarQuoteDelimiter::Triple),
        _ => None,
    }
    .filter(|_| width.is_power_of_two());
    let mut start = match delimiter {
        Some(ScalarQuoteDelimiter::Triple) => opening_start + 3 * width,
        Some(ScalarQuoteDelimiter::DoubledSingle) => opening_start + 2 * width,
        Some(ScalarQuoteDelimiter::DoubledTriple) => opening_start + 6 * width,
        _ => capture.start,
    };
    let known_empty = known_empty || (enclosing.record_lines_are_content && start == limit);
    let mut end = limit;
    let mut reason = Some(LabelledValueScanReason::UncertainQuoteBoundary);
    if known_empty {
        end = start;
        reason = None;
    } else if let Some(close) = delimiter.and_then(|delimiter| {
        scalar_quote_close(input, start, limit, quote, opening_escapes, delimiter)
    }) {
        end = close;
        reason = None;
    }
    if reason.is_some() {
        end = uncertain_field_end(input, start, &enclosing);
        if end <= start {
            // Unclassified opening runs must not turn real private text into
            // an empty rejected capture. Keep available opening bytes owned.
            start = opening_start;
        }
    }
    let span = start..end;
    let capture_end = capture.end.max(start).min(end);
    let repeat_seed = (!span.is_empty()).then_some(gaze_types::LabelledValueRepeatSeed {
        span: span.clone(),
        kind: gaze_types::LabelledValueRepeatSeedKind::ExactScalar,
    });
    LabelledValueScan {
        quoted_value: true,
        repeat_seed,
        capture: start..capture_end,
        extension: capture_end..end,
        reason: reason.or_else(|| {
            labelled_value_over_limit(&input[span.clone()])
                .then_some(LabelledValueScanReason::LimitExceeded)
        }),
        rejected: span.is_empty(),
        span,
    }
}

fn scan_paired_field_value(
    input: &str,
    capture: std::ops::Range<usize>,
    opener: char,
    wrapper_escapes: usize,
) -> LabelledValueScan {
    let opening = capture.start - opener.len_utf8();
    let enclosing = enclosing_scalar_boundary(input, opening, capture.start, wrapper_escapes);
    let mut close = None;
    let mut at = capture.start;
    while at < enclosing.end {
        scanner_work(1);
        let ch = input[at..].chars().next().expect("scalar character");
        if field_wrapper_closes(opener, ch) {
            let run = input[at..enclosing.end]
                .chars()
                .take_while(|next| *next == ch)
                .count();
            let run_end = at + run * ch.len_utf8();
            if run % 2 == 1
                && !input[run_end..]
                    .chars()
                    .next()
                    .is_some_and(|next| next.is_alphanumeric() || next == '_')
            {
                let delimiter = run_end - ch.len_utf8();
                let closing_escapes = input[..delimiter]
                    .bytes()
                    .rev()
                    .take_while(|byte| *byte == b'\\')
                    .count();
                close = Some(if closing_escapes == wrapper_escapes {
                    delimiter - wrapper_escapes
                } else {
                    delimiter
                });
                break;
            }
            at = run_end;
        } else {
            at += ch.len_utf8();
        }
    }
    let end = close.unwrap_or_else(|| uncertain_field_end(input, capture.start, &enclosing));
    let span = capture.start..end;
    let capture_end = capture.end.min(end);
    LabelledValueScan {
        quoted_value: true,
        capture: capture.start..capture_end,
        extension: capture_end..end,
        span: span.clone(),
        reason: close
            .is_none()
            .then_some(LabelledValueScanReason::UncertainQuoteBoundary)
            .or_else(|| {
                labelled_value_over_limit(&input[span.clone()])
                    .then_some(LabelledValueScanReason::LimitExceeded)
            }),
        repeat_seed: (!span.is_empty()).then_some(gaze_types::LabelledValueRepeatSeed {
            span: span.clone(),
            kind: gaze_types::LabelledValueRepeatSeedKind::ExactScalar,
        }),
        rejected: span.is_empty(),
    }
}

fn field_wrapper_closes(open: char, close: char) -> bool {
    match open {
        '\u{28}' => matches!(close, '\u{29}'),
        '\u{5b}' => matches!(close, '\u{5d}'),
        '\u{7b}' => matches!(close, '\u{7d}'),
        '\u{ab}' => matches!(close, '\u{bb}'),
        '\u{f3a}' => matches!(close, '\u{f3b}'),
        '\u{f3c}' => matches!(close, '\u{f3d}'),
        '\u{169b}' => matches!(close, '\u{169c}'),
        '\u{2018}' => matches!(close, '\u{2019}'),
        '\u{201a}' => matches!(close, '\u{2019}' | '\u{2018}'),
        '\u{201b}' => matches!(close, '\u{2019}'),
        '\u{201c}' => matches!(close, '\u{201d}'),
        '\u{201e}' => matches!(close, '\u{201d}' | '\u{201c}'),
        '\u{201f}' => matches!(close, '\u{201d}'),
        '\u{2039}' => matches!(close, '\u{203a}'),
        '\u{2045}' => matches!(close, '\u{2046}'),
        '\u{207d}' => matches!(close, '\u{207e}'),
        '\u{208d}' => matches!(close, '\u{208e}'),
        '\u{2308}' => matches!(close, '\u{2309}'),
        '\u{230a}' => matches!(close, '\u{230b}'),
        '\u{2329}' => matches!(close, '\u{232a}'),
        '\u{2768}' => matches!(close, '\u{2769}'),
        '\u{276a}' => matches!(close, '\u{276b}'),
        '\u{276c}' => matches!(close, '\u{276d}'),
        '\u{276e}' => matches!(close, '\u{276f}'),
        '\u{2770}' => matches!(close, '\u{2771}'),
        '\u{2772}' => matches!(close, '\u{2773}'),
        '\u{2774}' => matches!(close, '\u{2775}'),
        '\u{27c5}' => matches!(close, '\u{27c6}'),
        '\u{27e6}' => matches!(close, '\u{27e7}'),
        '\u{27e8}' => matches!(close, '\u{27e9}'),
        '\u{27ea}' => matches!(close, '\u{27eb}'),
        '\u{27ec}' => matches!(close, '\u{27ed}'),
        '\u{27ee}' => matches!(close, '\u{27ef}'),
        '\u{2983}' => matches!(close, '\u{2984}'),
        '\u{2985}' => matches!(close, '\u{2986}'),
        '\u{2987}' => matches!(close, '\u{2988}'),
        '\u{2989}' => matches!(close, '\u{298a}'),
        '\u{298b}' => matches!(close, '\u{298c}'),
        '\u{298d}' => matches!(close, '\u{2990}'),
        '\u{298f}' => matches!(close, '\u{298e}'),
        '\u{2991}' => matches!(close, '\u{2992}'),
        '\u{2993}' => matches!(close, '\u{2994}'),
        '\u{2995}' => matches!(close, '\u{2996}'),
        '\u{2997}' => matches!(close, '\u{2998}'),
        '\u{29d8}' => matches!(close, '\u{29d9}'),
        '\u{29da}' => matches!(close, '\u{29db}'),
        '\u{29fc}' => matches!(close, '\u{29fd}'),
        '\u{2e02}' => matches!(close, '\u{2e03}'),
        '\u{2e04}' => matches!(close, '\u{2e05}'),
        '\u{2e09}' => matches!(close, '\u{2e0a}'),
        '\u{2e0c}' => matches!(close, '\u{2e0d}'),
        '\u{2e1c}' => matches!(close, '\u{2e1d}'),
        '\u{2e20}' => matches!(close, '\u{2e21}'),
        '\u{2e22}' => matches!(close, '\u{2e23}'),
        '\u{2e24}' => matches!(close, '\u{2e25}'),
        '\u{2e26}' => matches!(close, '\u{2e27}'),
        '\u{2e28}' => matches!(close, '\u{2e29}'),
        '\u{2e42}' => matches!(close, '\u{201d}'),
        '\u{3008}' => matches!(close, '\u{3009}'),
        '\u{300a}' => matches!(close, '\u{300b}'),
        '\u{300c}' => matches!(close, '\u{300d}'),
        '\u{300e}' => matches!(close, '\u{300f}'),
        '\u{3010}' => matches!(close, '\u{3011}'),
        '\u{3014}' => matches!(close, '\u{3015}'),
        '\u{3016}' => matches!(close, '\u{3017}'),
        '\u{3018}' => matches!(close, '\u{3019}'),
        '\u{301a}' => matches!(close, '\u{301b}'),
        '\u{301d}' => matches!(close, '\u{301e}' | '\u{301f}'),
        '\u{fd3f}' => matches!(close, '\u{fd3e}'),
        '\u{fe17}' => matches!(close, '\u{fe18}'),
        '\u{fe35}' => matches!(close, '\u{fe36}'),
        '\u{fe37}' => matches!(close, '\u{fe38}'),
        '\u{fe39}' => matches!(close, '\u{fe3a}'),
        '\u{fe3b}' => matches!(close, '\u{fe3c}'),
        '\u{fe3d}' => matches!(close, '\u{fe3e}'),
        '\u{fe3f}' => matches!(close, '\u{fe40}'),
        '\u{fe41}' => matches!(close, '\u{fe42}'),
        '\u{fe43}' => matches!(close, '\u{fe44}'),
        '\u{fe47}' => matches!(close, '\u{fe48}'),
        '\u{fe59}' => matches!(close, '\u{fe5a}'),
        '\u{fe5b}' => matches!(close, '\u{fe5c}'),
        '\u{fe5d}' => matches!(close, '\u{fe5e}'),
        '\u{ff08}' => matches!(close, '\u{ff09}'),
        '\u{ff3b}' => matches!(close, '\u{ff3d}'),
        '\u{ff5b}' => matches!(close, '\u{ff5d}'),
        '\u{ff5f}' => matches!(close, '\u{ff60}'),
        '\u{ff62}' => matches!(close, '\u{ff63}'),
        '\u{60}' => matches!(close, '\u{60}'),
        '\u{3c}' => matches!(close, '\u{3e}'),
        '\u{2e55}' => close == '\u{2e56}',
        '\u{2e57}' => close == '\u{2e58}',
        '\u{2e59}' => close == '\u{2e5a}',
        '\u{2e5b}' => close == '\u{2e5c}',
        _ => close == open,
    }
}
