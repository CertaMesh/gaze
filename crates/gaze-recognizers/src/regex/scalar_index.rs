#[cfg(test)]
fn legacy_capture_pattern(pattern: &str) -> String {
    let (prefix, _) = pattern.rsplit_once("|(").expect("record scalar seed");
    let prefix = prefix.replace("|[-–—]", "");
    format!(r"{prefix}|([^\r\n]))")
}

use super::*;
use std::collections::HashMap;
use std::ops::Range;

/// One cache per recognizer invocation, shared by every field capture. Each decoded
/// JSON frame owns another cache; no state survives the input or crosses requests.
#[derive(Default)]
pub(super) struct ScalarBoundaryIndex {
    encoded: Option<EncodedIndex>,
    markup_assignments: Option<gaze_types::markup_regions::MarkupRegions>,
    sql_schema: Option<Vec<Range<usize>>>,
    sql_statements: Option<Vec<Range<usize>>>,
    json_label_values: Option<JsonLabelValues>,
    embedded_json: Option<Option<Box<DecodedFrame>>>,
    paired: Option<PairedCloseIndex>,
    prefix: PrefixCursor,
    provisional: Option<(usize, PrefixCursor)>,
    json_ancestors: HashMap<(usize, usize), bool>,
    record_breaks: Option<Vec<usize>>,
    strings: HashMap<(usize, Option<usize>), StringEndIndex>,
    quotes: HashMap<(u8, usize, ScalarQuoteDelimiter), QuoteCloseIndex>,
}

impl ScalarBoundaryIndex {
    pub(super) fn inside_sql_schema(&mut self, input: &str, at: usize) -> bool {
        let regions = self.sql_schema.get_or_insert_with(|| {
            scanner_work(input.len());
            static DDL: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
            let ddl = DDL.get_or_init(|| Regex::new(r#"(?i)\b(?:create[ \t]+(?:(?:temp|temporary|unlogged)[ \t]+)?table|alter[ \t]+table)\s+(?:[a-z_][a-z0-9_.]*|"[^"]+"|`[^`]+`)\s*\("#).expect("SQL schema statement"));
            static TOKENS: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
            let tokens = TOKENS.get_or_init(|| Regex::new(r#"(?s)'(?:''|[^'])*'|"(?:""|[^"])*"|`(?:``|[^`])*`|--[^\r\n]*|/\*.*?\*/|[()]"#).expect("SQL schema delimiters"));
            let mut stack = Vec::new();
            let mut closes = HashMap::new();
            for token in tokens.find_iter(input) {
                match token.as_str() {
                    "(" => stack.push(token.start()),
                    ")" => if let Some(opening) = stack.pop() { closes.insert(opening, token.end()); },
                    _ => {}
                }
            }
            let mut regions: Vec<Range<usize>> = Vec::new();
            for statement in ddl.find_iter(input) {
                if let Some(&end) = closes.get(&(statement.end()-1)) {
                    if let Some(last) = regions.last_mut().filter(|last| last.end > statement.start()) { last.end = last.end.max(end); }
                    else { regions.push(statement.start()..end); }
                }
            }
            regions
        });
        let index = regions.partition_point(|range| range.end <= at);
        regions.get(index).is_some_and(|range| range.contains(&at))
    }
    pub(super) fn inside_sql_statement(&mut self, input: &str, at: usize) -> bool {
        let regions = self.sql_statements.get_or_insert_with(|| {
            scanner_work(input.len());
            static START: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
            let start = START.get_or_init(|| Regex::new(r"(?im)(?:\A|[;\r\n])[ \t]*(?:[a-z_]+[ \t]*:[ \t]*)?(?:select\b|update\s+[a-z_][a-z0-9_.]*\s+set\b|insert\s+into\b|replace\s+into\b|values\s*\(|alter\s+table\s+[a-z_][a-z0-9_.]*\s+add\s+column\b)").expect("SQL statement context"));
            static FROM: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
            let from = FROM.get_or_init(|| Regex::new(r"(?i)\bfrom\s+[^\s;]+").expect("SELECT source"));
            let ends: Vec<_> = input.match_indices(';').map(|(at,_)| at+1).collect();
            let mut regions: Vec<Range<usize>> = Vec::new();
            for statement in start.find_iter(input) {
                let end = ends.get(ends.partition_point(|&end| end <= statement.end())).copied().unwrap_or(input.len());
                let select = statement.as_str().trim_start_matches([';', '\r', '\n']).trim_start().to_ascii_lowercase().starts_with("select");
                if select && !from.is_match(&input[statement.end()..end]) { continue; }
                if let Some(last) = regions.last_mut().filter(|last| last.end >= statement.start()) { last.end = last.end.max(end); }
                else { regions.push(statement.start()..end); }
            }
            regions
        });
        let index = regions.partition_point(|range| range.end <= at);
        regions.get(index).is_some_and(|range| range.contains(&at))
    }
    pub(super) fn inside_markup_assignment(&mut self, input: &str, at: usize) -> bool {
        self.markup_assignments
            .get_or_insert_with(|| {
                scanner_work(input.len());
                gaze_types::markup_regions::MarkupRegions::new(input)
            })
            .contains_assignment(at)
    }
    pub(super) fn parent_scalar(&mut self, input: &str, opening: usize) -> bool {
        let encoded = self.encoded(input);
        let json = encoded.json_frame_at(opening);
        json.is_some_and(|frame| encoded.decoded(input, frame).is_some())
            || self.encoded_boundary(input, opening).is_some()
    }
    pub(super) fn boundary(
        &mut self,
        input: &str,
        opening: usize,
        start: usize,
        opening_escapes: usize,
    ) -> EnclosingScalarBoundary {
        if let Some(boundary) = self.encoded_boundary(input, opening) {
            return boundary;
        }
        self.prefix_boundary(input, opening, start, opening_escapes)
    }

    fn encoded(&mut self, input: &str) -> &mut EncodedIndex {
        self.encoded.get_or_insert_with(|| EncodedIndex::new(input))
    }

    fn encoded_boundary(&mut self, input: &str, opening: usize) -> Option<EnclosingScalarBoundary> {
        let encoded = self.encoded(input);
        if let Some(literal) = encoded.sql_at(opening) {
            return Some(EnclosingScalarBoundary {
                end: literal.end,
                absent_value: opening == literal.end,
                own_sql_literal: opening == literal.start,
                doubled_quote: (opening != literal.start).then_some(b'\''),
                record_lines_are_content: true,
            });
        }
        let frame = encoded.json_at(opening);
        let json = frame.and_then(|frame| {
            let close = encoded.json[frame].range.end - 1;
            encoded.decoded(input, frame).map(|_| (frame, close))
        });
        let closed_json_frame = json.is_some_and(|(_, close)| {
            *encoded.valid_json.get_or_insert_with(|| {
                scanner_work(input.len());
                serde_json::from_str::<serde_json::Value>(input).is_ok()
            }) || input[close + 1..]
                .trim_start_matches([' ', '\t'])
                .chars()
                .next()
                .is_none_or(|ch| matches!(ch, '\r' | '\n'))
        });
        if let Some(cell) = encoded.csv_at(opening).filter(|cell| {
            !(cell.end == input.len() && closed_json_frame)
                && json.is_none_or(|(_, close)| cell.end > close)
        }) {
            return Some(EnclosingScalarBoundary {
                end: cell.end,
                absent_value: opening == cell.end,
                own_sql_literal: false,
                doubled_quote: Some(b'"'),
                record_lines_are_content: true,
            });
        }
        let (frame, _) = json?;
        let decoded = encoded.decoded(input, frame)?;
        let anchor = source_anchor(&decoded.view.source, opening)?;
        decoded
            .index
            .encoded_boundary(&decoded.view.text, anchor)
            .map(|mut boundary| {
                boundary.end = decoded.view.source.get(boundary.end).map_or_else(
                    || {
                        decoded
                            .view
                            .source
                            .last()
                            .map_or(input.len(), |range| range.end)
                    },
                    |range| range.start,
                );
                boundary
            })
    }

    /// A colon inside a JSON label belongs to the label, not its value.
    /// Object keys own the following value; array label cells own the next cell.
    /// Ordinary object value strings never own a sibling field.
    pub(super) fn scan_json_label_cell(
        &mut self,
        input: &str,
        capture: Range<usize>,
    ) -> Option<LabelledValueScan> {
        self.scan_json_label_cells(input, capture)?
            .into_iter()
            .next()
    }

    pub(super) fn scan_json_label_cells(
        &mut self,
        input: &str,
        capture: Range<usize>,
    ) -> Option<Vec<LabelledValueScan>> {
        if self.json_label_values.is_none() {
            let encoded = self.encoded(input);
            self.json_label_values = Some(json_label_values(input, &encoded.json));
        }
        let enclosing_key = {
            let values = self
                .json_label_values
                .as_ref()
                .expect("initialized JSON labels");
            let at = values
                .frames
                .partition_point(|frame| frame.range.start < capture.start);
            at.checked_sub(1).and_then(|at| {
                let frame = &values.frames[at];
                let close = frame.range.end - 1;
                (capture.start <= close && values.keys.contains(&close)).then_some(close)
            })
        };
        let enclosing_close = {
            let values = self
                .json_label_values
                .as_ref()
                .expect("initialized JSON labels");
            let at = values
                .frames
                .partition_point(|frame| frame.range.start < capture.start);
            at.checked_sub(1).and_then(|at| {
                let frame = &values.frames[at];
                let close = frame.range.end - 1;
                (frame.closed
                    && capture.start <= close
                    && json_label_tail_is_whitespace(&input[capture.start..close]))
                .then_some(close)
            })
        };
        let values = self
            .json_label_values
            .as_ref()
            .expect("initialized JSON labels");
        if let Some(mut span) = enclosing_close
            .and_then(|close| values.labels.get(&close))
            .or_else(|| values.labels.get(&capture.start))
            .or_else(|| {
                capture
                    .start
                    .checked_sub(1)
                    .and_then(|at| values.labels.get(&at))
            })
            .or_else(|| values.direct.get(&capture.start))
            .or_else(|| {
                capture
                    .start
                    .checked_sub(1)
                    .and_then(|at| values.direct.get(&at))
            })
            .cloned()
            .or_else(|| {
                enclosing_key.map(|_| std::iter::once(capture.start..capture.start).collect())
            })
        {
            span.retain(|span| !span.is_empty());
            let span = if span.is_empty() {
                std::iter::once(capture.start..capture.start).collect()
            } else {
                span
            };
            return Some(
                span.into_iter()
                    .map(|span| {
                        let span = if span.is_empty() {
                            capture.start..capture.start
                        } else {
                            span
                        };
                        LabelledValueScan {
                            quoted_value: true,
                            parent_scalar: true,
                            capture: span.clone(),
                            extension: span.end..span.end,
                            repeat_seed: (!span.is_empty()).then_some(
                                gaze_types::LabelledValueRepeatSeed {
                                    span: span.clone(),
                                    kind: gaze_types::LabelledValueRepeatSeedKind::ExactScalar,
                                },
                            ),
                            reason: labelled_value_over_limit(&input[span.clone()])
                                .then_some(LabelledValueScanReason::LimitExceeded),
                            rejected: span.is_empty(),
                            span,
                        }
                    })
                    .collect(),
            );
        }
        // Escaped JSON can be a log fragment with no enclosing JSON string.
        let embedded = self.embedded_json.get_or_insert_with(|| {
            scanner_work(input.len());
            let (text, source) = gaze_types::json_regions::unescape_json_fragments(input)?;
            (!gaze_types::json_regions::json_regions(&text).is_empty()).then(|| {
                Box::new(DecodedFrame {
                    view: ScalarSourceView { text, source },
                    index: ScalarBoundaryIndex::default(),
                })
            })
        });
        if let Some(decoded) = embedded {
            if let Some(start) = source_anchor(&decoded.view.source, capture.start) {
                let end = decoded
                    .view
                    .source
                    .partition_point(|range| range.end < capture.end)
                    .saturating_add(1)
                    .min(decoded.view.text.len());
                if let Some(scans) = decoded
                    .index
                    .scan_json_label_cells(&decoded.view.text, start..end)
                {
                    return Some(
                        scans
                            .into_iter()
                            .map(|scan| {
                                let mut scan = project_scan(scan, &decoded.view.source, &capture);
                                scan.reason = scan.reason.or_else(|| {
                                    labelled_value_over_limit(&input[scan.span.clone()])
                                        .then_some(LabelledValueScanReason::LimitExceeded)
                                });
                                scan
                            })
                            .collect(),
                    );
                }
            }
        }
        let encoded = self.encoded(input);
        let frame = encoded.json_frame_at(capture.start)?;
        let decoded = encoded.decoded(input, frame)?;
        let start = source_anchor(&decoded.view.source, capture.start)?;
        let end = decoded
            .view
            .source
            .partition_point(|range| range.end < capture.end)
            .saturating_add(1)
            .min(decoded.view.text.len());
        let scans = decoded
            .index
            .scan_json_label_cells(&decoded.view.text, start..end)?;
        Some(
            scans
                .into_iter()
                .map(|scan| {
                    let mut scan = project_scan(scan, &decoded.view.source, &capture);
                    scan.reason = scan.reason.or_else(|| {
                        labelled_value_over_limit(&input[scan.span.clone()])
                            .then_some(LabelledValueScanReason::LimitExceeded)
                    });
                    scan
                })
                .collect(),
        )
    }

    pub(super) fn scan_encoded_wrapper(
        &mut self,
        input: &str,
        capture: Range<usize>,
    ) -> Option<LabelledValueScan> {
        let encoded = self.encoded(input);
        let frame = encoded.json_frame_at(capture.start)?;
        let decoded = encoded.decoded(input, frame)?;
        let anchor = source_anchor(&decoded.view.source, capture.start)?;
        let escapes = decoded.view.text[anchor..]
            .bytes()
            .take_while(|byte| *byte == b'\\')
            .count();
        let opening = anchor + escapes;
        let opener = decoded.view.text[opening..].chars().next()?;
        let mut seed_end = decoded
            .view
            .source
            .partition_point(|range| range.end < capture.end)
            .saturating_add(1)
            .min(decoded.view.text.len());
        while !decoded.view.text.is_char_boundary(seed_end) {
            seed_end += 1;
        }
        if !field_wrapper_opens(opener) {
            let scan = decoded
                .index
                .scan_encoded_wrapper(&decoded.view.text, anchor..seed_end)?;
            let mut scan = project_scan(scan, &decoded.view.source, &capture);
            scan.reason = scan.reason.or_else(|| {
                labelled_value_over_limit(&input[scan.span.clone()])
                    .then_some(LabelledValueScanReason::LimitExceeded)
            });
            return Some(scan);
        }
        let start = opening + opener.len_utf8();
        let seed_end = seed_end.max(start);
        let mut scan = if matches!(opener, '\'' | '"') {
            scan_quoted_field_value(&decoded.view.text, start..seed_end, &mut decoded.index)
        } else {
            scan_paired_field_value(
                &decoded.view.text,
                start..seed_end,
                opener,
                escapes,
                &mut decoded.index,
            )
        };
        if scan.reason == Some(LabelledValueScanReason::UncertainQuoteBoundary) {
            let boundary = decoded
                .index
                .boundary(&decoded.view.text, opening, start, escapes);
            let end = boundary.end.max(scan.span.end);
            scan.span.end = end;
            scan.extension.end = end;
            scan.rejected = scan.span.is_empty();
            scan.repeat_seed = (!scan.rejected).then_some(gaze_types::LabelledValueRepeatSeed {
                span: scan.span.clone(),
                kind: gaze_types::LabelledValueRepeatSeedKind::ExactScalar,
            });
        }
        let mut scan = project_scan(scan, &decoded.view.source, &capture);
        scan.reason = scan.reason.or_else(|| {
            labelled_value_over_limit(&input[scan.span.clone()])
                .then_some(LabelledValueScanReason::LimitExceeded)
        });
        Some(scan)
    }

    fn string_end(
        &mut self,
        input: &str,
        start: usize,
        limit: usize,
        depth: Option<usize>,
    ) -> usize {
        let index = self
            .strings
            .entry((start, depth))
            .or_insert_with(|| StringEndIndex::new(input, start, depth));
        index.end(input, limit, depth)
    }

    fn quote_close(
        &mut self,
        input: &str,
        start: usize,
        limit: usize,
        quote: u8,
        escapes: usize,
        delimiter: ScalarQuoteDelimiter,
    ) -> Option<usize> {
        let index = self
            .quotes
            .entry((quote, escapes, delimiter))
            .or_insert_with(|| QuoteCloseIndex::new(input, quote, escapes, delimiter));
        index.close(input, start, limit, escapes, delimiter)
    }

    fn context_is_json(
        &mut self,
        input: &str,
        enclosing: &[(usize, usize)],
        escapes: usize,
    ) -> bool {
        if !(escapes + 1).is_power_of_two() {
            return false;
        }
        let valid = *self.encoded(input).valid_json.get_or_insert_with(|| {
            scanner_work(input.len());
            serde_json::from_str::<serde_json::Value>(input).is_ok()
        });
        if !valid {
            return false;
        }
        let mut levels = 0;
        for &(depth, opening) in enclosing.iter().filter(|(depth, _)| *depth < escapes) {
            if (depth + 1).trailing_zeros() != levels {
                return false;
            }
            let valid = if let Some(valid) = self.json_ancestors.get(&(depth, opening)) {
                *valid
            } else {
                let close = self.string_end(input, opening + depth + 1, input.len(), Some(depth));
                let valid = input
                    .get(opening..close + depth + 1)
                    .is_some_and(|raw| serialized_ancestor_contains_json(raw, levels));
                self.json_ancestors.insert((depth, opening), valid);
                valid
            };
            if !valid {
                return false;
            }
            levels += 1;
        }
        levels == (escapes + 1).trailing_zeros()
    }

    pub(super) fn is_json_scalar_close(&mut self, input: &str, opening: usize) -> bool {
        let encoded = self.encoded(input);
        let at = encoded
            .json
            .partition_point(|frame| frame.range.end <= opening);
        if !encoded
            .json
            .get(at)
            .is_some_and(|frame| frame.closed && frame.range.end == opening + 1)
        {
            return false;
        }
        *encoded.valid_json.get_or_insert_with(|| {
            scanner_work(input.len());
            serde_json::from_str::<serde_json::Value>(input).is_ok()
        })
    }

    pub(super) fn paired_close(
        &mut self,
        input: &str,
        start: usize,
        limit: usize,
        opener: char,
        escapes: usize,
    ) -> Option<usize> {
        self.paired
            .get_or_insert_with(|| PairedCloseIndex::new(input))
            .close(input, start, limit, opener, escapes)
    }

    pub(super) fn uncertain_end(&mut self, input: &str, start: usize, limit: usize) -> usize {
        let breaks = self.record_breaks.get_or_insert_with(|| {
            scanner_work(input.len());
            let mut escapes = 0usize;
            let mut breaks = Vec::new();
            for (at, byte) in input.bytes().enumerate() {
                if matches!(byte, b'\n' | b'\r') && escapes.is_multiple_of(2) {
                    breaks.push(at);
                }
                escapes = if byte == b'\\' { escapes + 1 } else { 0 };
            }
            breaks
        });
        breaks
            .get(breaks.partition_point(|at| *at < start))
            .copied()
            .unwrap_or(limit)
            .min(limit)
    }

    fn save_prefix(
        &mut self,
        state: &PrefixCursor,
        stable: bool,
        until: Option<usize>,
        prefix_end: usize,
    ) {
        if stable {
            self.prefix = state.clone();
        } else if let Some(until) = until.filter(|until| *until > prefix_end) {
            self.provisional = Some((until, state.clone()));
        }
    }

    fn prefix_boundary(
        &mut self,
        input: &str,
        opening: usize,
        start: usize,
        opening_escapes: usize,
    ) -> EnclosingScalarBoundary {
        let prefix_end = opening - opening_escapes;
        // Encoded-wrapper fallback queries can revisit an earlier anchor. Keep
        // forward state only when it is a prefix of the requested replay.
        if self
            .provisional
            .as_ref()
            .is_some_and(|(until, _)| prefix_end >= *until)
        {
            self.provisional = None;
        }
        let provisional = self
            .provisional
            .as_ref()
            .filter(|(_, cursor)| cursor.at <= prefix_end && cursor.at >= self.prefix.at);
        let mut state = provisional.map_or_else(
            || {
                if self.prefix.at <= prefix_end {
                    self.prefix.clone()
                } else {
                    PrefixCursor::default()
                }
            },
            |(_, cursor)| cursor.clone(),
        );
        let mut stable = provisional.is_none();
        let mut valid_until = provisional.map(|(until, _)| *until);
        let mut cacheable = true;
        while state.at < prefix_end {
            scanner_work(1);
            let at = state.at;
            let byte = input.as_bytes()[at];
            if byte == b'\\' {
                state.escapes += 1;
                state.at += 1;
                if cacheable {
                    self.save_prefix(&state, stable, valid_until, prefix_end);
                }
                continue;
            }
            if matches!(byte, b'\n' | b'\r') && state.escapes.is_multiple_of(2) {
                state.enclosing.clear();
                state.literal_limit = None;
                state.enclosing_literal = None;
            }
            let apostrophe = byte == b'\''
                && state.escapes.is_multiple_of(2)
                && !input[..at]
                    .chars()
                    .next_back()
                    .is_some_and(|ch| ch.is_alphanumeric() || ch == '_');
            let nested_double = byte == b'"'
                && (state.escapes + 1).is_power_of_two()
                && state
                    .enclosing
                    .last()
                    .is_none_or(|(depth, _)| state.escapes > *depth);
            if apostrophe || nested_double {
                let escapes = if apostrophe { 0 } else { state.escapes };
                let width = escapes + 1;
                let opening = at - escapes;
                let run_end = quote_run_end(input, opening, byte, escapes, prefix_end);
                let run = (run_end - opening) / width;
                let delimiter = match run {
                    1 if apostrophe => Some(ScalarQuoteDelimiter::Single),
                    3..=5 => Some(ScalarQuoteDelimiter::Triple),
                    _ => None,
                };
                if let Some(delimiter) = delimiter {
                    let count = if matches!(delimiter, ScalarQuoteDelimiter::Single) {
                        1
                    } else {
                        3
                    };
                    let content = opening + count * width;
                    let (limit_start, depth) = *state
                        .literal_limit
                        .get_or_insert((content, state.enclosing.last().map(|(depth, _)| *depth)));
                    let limit = self.string_end(input, limit_start, prefix_end, depth);
                    let prior_close =
                        self.quote_close(input, content, limit, byte, escapes, delimiter);
                    if apostrophe && prior_close.is_none() {
                        let full_limit = self.string_end(
                            input,
                            content,
                            input.len(),
                            state.enclosing.last().map(|(depth, _)| *depth),
                        );
                        state.enclosing_literal =
                            self.quote_close(input, content, full_limit, byte, escapes, delimiter);
                    }
                    let full_limit = self.string_end(input, limit_start, input.len(), depth);
                    let full_close =
                        self.quote_close(input, content, full_limit, byte, escapes, delimiter);
                    let next_at = prior_close.map_or_else(
                        || self.uncertain_end(input, content, limit),
                        |close| close + count * width,
                    );
                    let complete_run =
                        quote_run_end(input, opening, byte, escapes, input.len()) == run_end;
                    let independent = prior_close == full_close
                        && complete_run
                        && (prior_close.is_some() || next_at < prefix_end);
                    if !independent {
                        stable = false;
                        if complete_run
                            && next_at < prefix_end
                            && prior_close.is_none()
                            && full_close.is_some()
                        {
                            // Before the next quote event, extending the replay
                            // still takes this same newline fallback. Save it
                            // conditionally and rewind once that event is reached.
                            let index = &self.quotes[&(byte, escapes, delimiter)];
                            let until = index.next_event_after(limit);
                            valid_until = Some(valid_until.map_or(until, |prior| prior.min(until)));
                            cacheable &= until > prefix_end;
                        } else {
                            cacheable = false;
                        }
                    }
                    state.at = next_at;
                    state.escapes = 0;
                    if cacheable {
                        self.save_prefix(&state, stable, valid_until, prefix_end);
                    }
                    continue;
                }
                // A query can cut a quote run before its grammar is known.
                cacheable &= quote_run_end(input, opening, byte, escapes, input.len()) == run_end;
            }
            if byte == b'"' && (state.escapes + 1).is_power_of_two() {
                state.literal_limit = None;
                while state
                    .enclosing
                    .last()
                    .is_some_and(|(depth, _)| *depth > state.escapes)
                {
                    state.enclosing.pop();
                }
                if state
                    .enclosing
                    .last()
                    .is_some_and(|(depth, _)| *depth == state.escapes)
                {
                    state.enclosing.pop();
                } else {
                    state.enclosing.push((state.escapes, at - state.escapes));
                }
            }
            state.escapes = 0;
            state.at += 1;
            if cacheable {
                self.save_prefix(&state, stable, valid_until, prefix_end);
            }
        }
        let depth = state.enclosing.last().map(|(depth, _)| *depth);
        let absent_value = input.as_bytes()[start - 1] == b'"'
            && depth.is_some_and(|depth| opening_escapes <= depth)
            && self.context_is_json(input, &state.enclosing, opening_escapes);
        let end = state
            .enclosing
            .last()
            .map_or(input.len(), |&(depth, opening)| {
                self.string_end(input, opening + depth + 1, input.len(), Some(depth))
            });
        let enclosing_literal = state.enclosing_literal.filter(|end| *end >= start);
        let record_lines_are_content = enclosing_literal.is_some()
            || state.enclosing.last().is_some_and(|&(depth, opening)| {
                depth == 0 && end >= start && quoted_csv_cell_encloses(input, opening, end)
            });
        let end = if end < start && !absent_value {
            self.string_end(input, start, input.len(), depth)
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
}

struct PairedRun {
    span: Range<usize>,
    escapes: usize,
    next_valid: usize,
}

#[derive(Default)]
struct PairedCloseIndex {
    runs: HashMap<char, Vec<PairedRun>>,
}

impl PairedCloseIndex {
    fn new(input: &str) -> Self {
        let mut index = Self::default();
        let mut chars = input.char_indices().peekable();
        let mut escapes = 0;
        while let Some((start, ch)) = chars.next() {
            scanner_work(1);
            let preceding_escapes = escapes;
            let mut end = start + ch.len_utf8();
            while chars.peek().is_some_and(|(_, next)| *next == ch) {
                scanner_work(1);
                end = chars.next().expect("peeked character").0 + ch.len_utf8();
            }
            if !ch.is_alphanumeric() && !ch.is_whitespace() {
                index.runs.entry(ch).or_default().push(PairedRun {
                    span: start..end,
                    escapes: preceding_escapes,
                    next_valid: 0,
                });
            }
            escapes = if ch == '\\' { end - start } else { 0 };
        }
        for (&ch, runs) in &mut index.runs {
            let mut next = runs.len();
            for i in (0..runs.len()).rev() {
                scanner_work(1);
                if Self::delimiter(input, &runs[i], runs[i].span.start, runs[i].span.end, ch, 0)
                    .is_some()
                {
                    next = i;
                }
                runs[i].next_valid = next;
            }
        }
        index
    }

    fn delimiter(
        input: &str,
        run: &PairedRun,
        start: usize,
        end: usize,
        ch: char,
        escapes: usize,
    ) -> Option<usize> {
        let width = ch.len_utf8();
        if !((end - start) / width).is_multiple_of(2)
            && !input[end..]
                .chars()
                .next()
                .is_some_and(|next| next.is_alphanumeric() || next == '_')
        {
            let delimiter = end - width;
            let preceding = if delimiter == run.span.start {
                run.escapes
            } else {
                0
            };
            Some(if preceding == escapes {
                delimiter - escapes
            } else {
                delimiter
            })
        } else {
            None
        }
    }

    fn close_char(
        &self,
        input: &str,
        start: usize,
        limit: usize,
        ch: char,
        escapes: usize,
    ) -> Option<usize> {
        let runs = self.runs.get(&ch)?;
        let first = runs.partition_point(|run| run.span.end <= start);
        let run = runs.get(first).filter(|run| run.span.start < limit)?;
        scanner_work(1);
        let end = run.span.end.min(limit);
        if let Some(close) =
            Self::delimiter(input, run, run.span.start.max(start), end, ch, escapes)
        {
            return Some(close);
        }
        if end == limit {
            return None;
        }
        // After the potentially truncated first run, jump over every invalid
        // full run. A limit inside a later run can change its parity too.
        let next = runs.get(first + 1).map_or(runs.len(), |run| run.next_valid);
        if let Some(run) = runs.get(next).filter(|run| run.span.end <= limit) {
            return Self::delimiter(input, run, run.span.start, run.span.end, ch, escapes);
        }
        let last = runs
            .partition_point(|run| run.span.start < limit)
            .checked_sub(1)?;
        let run = &runs[last];
        (run.span.end > limit)
            .then(|| Self::delimiter(input, run, run.span.start, limit, ch, escapes))
            .flatten()
    }

    fn close(
        &self,
        input: &str,
        start: usize,
        limit: usize,
        opener: char,
        escapes: usize,
    ) -> Option<usize> {
        let (first, second) = field_wrapper_closing_chars(opener);
        let a = self.close_char(input, start, limit, first, escapes);
        let b = second.and_then(|ch| self.close_char(input, start, limit, ch, escapes));
        a.into_iter().chain(b).min()
    }
}

#[derive(Clone, Default)]
struct PrefixCursor {
    at: usize,
    escapes: usize,
    enclosing: Vec<(usize, usize)>,
    literal_limit: Option<(usize, Option<usize>)>,
    enclosing_literal: Option<usize>,
}

fn source_anchor(source: &[Range<usize>], opening: usize) -> Option<usize> {
    let at = source.partition_point(|range| range.end <= opening);
    source
        .get(at)
        .filter(|range| range.contains(&opening))
        .map(|_| at)
}

fn project_scan(
    mut scan: LabelledValueScan,
    source: &[Range<usize>],
    capture: &Range<usize>,
) -> LabelledValueScan {
    let project = |range: Range<usize>| {
        let start = source.get(range.start).map_or_else(
            || source.last().map_or(capture.start, |range| range.end),
            |range| range.start,
        );
        let end = if range.is_empty() {
            start
        } else {
            source[range.end - 1].end
        };
        start..end
    };
    scan.capture = project(scan.capture);
    scan.extension = project(scan.extension);
    scan.span = project(scan.span);
    if let Some(seed) = &mut scan.repeat_seed {
        seed.span = project(seed.span.clone());
    }
    scan
}

struct DecodedFrame {
    view: ScalarSourceView,
    index: ScalarBoundaryIndex,
}
struct JsonFrame {
    range: Range<usize>,
    closed: bool,
    decoded: Option<Option<Box<DecodedFrame>>>,
}
struct EncodedIndex {
    sql: Vec<(usize, Range<usize>)>,
    csv: Vec<Range<usize>>,
    json: Vec<JsonFrame>,
    valid_json: Option<bool>,
}
impl EncodedIndex {
    fn new(input: &str) -> Self {
        scanner_work(input.len());
        Self {
            sql: sql_frames(input),
            csv: csv_frames(input),
            json: json_frames(input),
            valid_json: None,
        }
    }
    fn sql_at(&self, anchor: usize) -> Option<&Range<usize>> {
        let at = self.sql.partition_point(|(start, _)| *start <= anchor);
        at.checked_sub(1)
            .and_then(|at| self.sql.get(at))
            .map(|(_, range)| range)
            .filter(|range| anchor <= range.end)
    }
    fn csv_at(&self, anchor: usize) -> Option<&Range<usize>> {
        let at = self.csv.partition_point(|range| range.start < anchor);
        at.checked_sub(1)
            .and_then(|at| self.csv.get(at))
            .filter(|range| anchor <= range.end)
    }
    fn json_frame_at(&self, anchor: usize) -> Option<usize> {
        let at = self
            .json
            .partition_point(|frame| frame.range.start < anchor)
            .checked_sub(1)?;
        let frame = &self.json[at];
        (anchor < frame.range.end - usize::from(frame.closed)).then_some(at)
    }
    fn json_at(&self, anchor: usize) -> Option<usize> {
        self.json_frame_at(anchor)
            .filter(|at| self.json[*at].closed)
    }
    fn decoded(&mut self, input: &str, at: usize) -> Option<&mut DecodedFrame> {
        let frame = &mut self.json[at];
        frame
            .decoded
            .get_or_insert_with(|| {
                // Build mappings only for this frame, rather than copying the whole
                // input and a byte map for every quoted capture.
                scanner_work(frame.range.len());
                let view = ScalarSourceView {
                    text: input[frame.range.clone()].to_owned(),
                    source: (frame.range.clone()).map(|at| at..at + 1).collect(),
                };
                view.json_frame(0..view.text.len(), frame.closed)
                    .map(|view| {
                        Box::new(DecodedFrame {
                            view,
                            index: ScalarBoundaryIndex::default(),
                        })
                    })
            })
            .as_deref_mut()
    }
}

// Resolve locally from indexed string frames. Tool output may embed JSON in
// NDJSON, fences, or prose; the rest of the document need not be valid JSON.
struct JsonLabelValues {
    keys: std::collections::HashSet<usize>,
    labels: HashMap<usize, Vec<Range<usize>>>,
    direct: HashMap<usize, Vec<Range<usize>>>,
    frames: Vec<JsonFrame>,
}

fn json_label_values(input: &str, _frames: &[JsonFrame]) -> JsonLabelValues {
    let regions = json_regions(input, _frames);
    let mut frames = Vec::new();
    for region in &regions {
        for mut frame in json_frames(&input[region.clone()]) {
            frame.range = frame.range.start + region.start..frame.range.end + region.start;
            frames.push(frame);
        }
    }
    let scalars = json_first_scalars(input, &frames);
    let mut values = JsonLabelValues {
        keys: std::collections::HashSet::new(),
        labels: HashMap::new(),
        direct: HashMap::new(),
        frames,
    };
    let mut containers = Vec::new();
    let mut at = 0;
    let mut strings = values.frames.iter().peekable();
    while at < input.len() {
        scanner_work(1);
        if let Some(frame) = strings.peek().filter(|frame| frame.range.start == at) {
            let close = frame.range.end - 1;
            let next = json_skip_space(input, frame.range.end);
            let connector = input.as_bytes().get(next);
            let array_cell = connector == Some(&b',') && containers.last() == Some(&b'[');
            let key = connector == Some(&b':') && containers.last() == Some(&b'{');
            if key || array_cell {
                let start = json_skip_space(input, next + 1);
                let span = scalars
                    .get(&start)
                    .cloned()
                    .filter(|span| {
                        !array_cell
                            || !span
                                .iter()
                                .any(|span| json_scalar_is_label(input, &values.frames, span))
                    })
                    .unwrap_or_else(|| std::iter::once(close..close).collect());
                values.labels.insert(close, span.clone());
                if key {
                    values.keys.insert(close);
                    if matches!(
                        input.as_bytes().get(start),
                        Some(b'{' | b'[' | b'n' | b't' | b'f')
                    ) {
                        values.direct.insert(start, span.clone());
                        values.direct.insert(start + 1, span);
                    }
                }
            }
            at = frame.range.end;
            strings.next();
            continue;
        }
        match input.as_bytes()[at] {
            b'{' | b'[' => containers.push(input.as_bytes()[at]),
            b'}' | b']' => {
                containers.pop();
            }
            _ => {}
        }
        at += 1;
    }
    values
}

fn json_regions(input: &str, _frames: &[JsonFrame]) -> Vec<Range<usize>> {
    scanner_work(input.len());
    gaze_types::json_regions::json_regions(input)
}

fn json_skip_space(input: &str, mut at: usize) -> usize {
    while input
        .as_bytes()
        .get(at)
        .is_some_and(u8::is_ascii_whitespace)
    {
        scanner_work(1);
        at += 1;
    }
    at
}

fn json_label_tail_is_whitespace(mut tail: &str) -> bool {
    while let Some(ch) = tail.chars().next() {
        scanner_work(1);
        if ch.is_whitespace() || ch == '\u{200b}' {
            tail = &tail[ch.len_utf8()..];
        } else if let Some(len) = gaze_types::json_regions::json_whitespace_escape_len(tail) {
            tail = &tail[len..];
        } else {
            return false;
        }
    }
    true
}

fn json_scalar_is_label(input: &str, frames: &[JsonFrame], span: &Range<usize>) -> bool {
    let at = frames.partition_point(|frame| frame.range.start < span.start);
    let Some(frame) = at.checked_sub(1).and_then(|at| frames.get(at)) else {
        return false;
    };
    if frame.range.start + 1 != span.start || frame.range.end - 1 != span.end {
        return false;
    }
    let Ok(value) = serde_json::from_str::<String>(&input[frame.range.clone()]) else {
        return false;
    };
    scanner_work(frame.range.len());
    let value = value.trim();
    let Some(label) = value.strip_suffix([':', '=']) else {
        return false;
    };
    !label.is_empty()
        && label.chars().any(char::is_alphabetic)
        && label
            .chars()
            .all(|ch| ch.is_alphabetic() || ch.is_whitespace() || "_-.".contains(ch))
}

struct JsonContainer {
    opening: usize,
    kind: u8,
    id_key: bool,
    schema: bool,
    spans: Vec<Range<usize>>,
}

fn json_schema_keyword(key: &str) -> bool {
    matches!(
        key,
        "type"
            | "$ref"
            | "properties"
            | "format"
            | "pattern"
            | "enum"
            | "examples"
            | "const"
            | "default"
    ) || key.starts_with("min")
        || key.starts_with("max")
}

// A container owns direct array strings or explicit id-like object children.
fn json_first_scalars(input: &str, frames: &[JsonFrame]) -> HashMap<usize, Vec<Range<usize>>> {
    let mut values = HashMap::new();
    let mut containers: Vec<JsonContainer> = Vec::new();
    let mut at = 0;
    let mut strings = frames.iter().peekable();
    while at < input.len() {
        scanner_work(1);
        let start = at;
        let mut eligible = false;
        let spans = if let Some(frame) = strings.peek().filter(|frame| frame.range.start == at) {
            at = frame.range.end;
            strings.next();
            if input.as_bytes().get(json_skip_space(input, at)) == Some(&b':') {
                let key = serde_json::from_str::<String>(&input[start..at]).unwrap_or_default();
                if let Some(container) = containers.last_mut() {
                    container.id_key = ["id", "value", "number", "no", "nr"]
                        .iter()
                        .any(|id| key.eq_ignore_ascii_case(id));
                    container.schema |= json_schema_keyword(&key);
                }
                None
            } else {
                eligible = containers
                    .last()
                    .is_some_and(|container| container.kind == b'[' || container.id_key);
                Some(std::iter::once(start + 1..at - 1).collect())
            }
        } else {
            match input.as_bytes()[at] {
                b'{' | b'[' => {
                    containers.push(JsonContainer {
                        opening: at,
                        kind: input.as_bytes()[at],
                        id_key: false,
                        schema: false,
                        spans: Vec::new(),
                    });
                    at += 1;
                    None
                }
                b'}' | b']' => {
                    at += 1;
                    if let Some(mut container) = containers.pop() {
                        if container.schema {
                            container.spans.clear();
                        }
                        values.insert(container.opening, container.spans.clone());
                        eligible = containers.last().is_some_and(|parent| {
                            parent.id_key || (parent.kind == b'[' && container.kind == b'{')
                        });
                        Some(container.spans)
                    } else {
                        None
                    }
                }
                b'-' | b'0'..=b'9' => {
                    at += 1;
                    while input.as_bytes().get(at).is_some_and(|byte| {
                        matches!(byte, b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
                    }) {
                        scanner_work(1);
                        at += 1;
                    }
                    eligible = containers
                        .last()
                        .is_some_and(|container| container.kind == b'{' && container.id_key);
                    Some(std::iter::once(start..at).collect())
                }
                _ => {
                    at += 1;
                    None
                }
            }
        };
        if let Some(spans) = spans {
            values.insert(start, spans.clone());
            if let Some(container) = containers.last_mut() {
                if eligible {
                    container.spans.extend(spans);
                }
                container.id_key = false;
            }
        }
    }
    values
}

fn json_frames(input: &str) -> Vec<JsonFrame> {
    let bytes = input.as_bytes();
    let mut frames = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        scanner_work(1);
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
        frames.push(JsonFrame {
            range: start..if closed { at + 1 } else { bytes.len() },
            closed,
            decoded: None,
        });
        at += 1;
    }
    frames
}

fn csv_frames(input: &str) -> Vec<Range<usize>> {
    let bytes = input.as_bytes();
    let mut frames = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        scanner_work(1);
        while at < bytes.len() && matches!(bytes[at], b' ' | b'\t') {
            scanner_work(1);
            at += 1;
        }
        if at < bytes.len() && bytes[at] == b'"' {
            let opening = at;
            at += 1;
            while at < bytes.len() {
                scanner_work(1);
                if bytes[at] == b'"' {
                    if bytes.get(at + 1) == Some(&b'"') {
                        at += 2;
                        continue;
                    }
                    break;
                }
                at += 1;
            }
            let end = at.min(bytes.len());
            let closed = at < bytes.len();
            let suffix = input[end + usize::from(closed)..].trim_start_matches([' ', '\t']);
            if suffix
                .chars()
                .next()
                .is_none_or(|ch| matches!(ch, ',' | '\r' | '\n'))
            {
                frames.push(opening..end);
                at = end + usize::from(closed);
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
    frames
}

/// SQL statement starts share suffix lexing: a byte already reached from an
/// earlier statement has the same future in that dialect. Quoted-token ends are
/// memoized too, including statements that occur inside earlier literal content.
fn sql_frames(input: &str) -> Vec<(usize, Range<usize>)> {
    static SQL: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let starts: Vec<_> = SQL
        .get_or_init(|| {
            Regex::new(r"(?i)(?:\A|[\r\n])[ \t]*(?:select|insert|update|replace|values)\b")
                .expect("SQL statement record prefix")
        })
        .find_iter(input)
        .map(|statement| statement.start())
        .collect();
    if starts.is_empty() {
        return Vec::new();
    }
    let bytes = input.as_bytes();
    scanner_work(bytes.len() * 2);
    let line_ends: Vec<_> = bytes
        .iter()
        .enumerate()
        .filter_map(|(at, byte)| matches!(byte, b'\r' | b'\n').then_some(at))
        .collect();
    let block_ends: Vec<_> = bytes
        .windows(2)
        .enumerate()
        .filter_map(|(at, pair)| (pair == b"*/").then_some(at))
        .collect();
    let mut frames = Vec::new();
    for (dialect, escapes) in [false, true].into_iter().enumerate() {
        let mut visited = vec![false; bytes.len()];
        let mut ends: HashMap<u8, Vec<Option<usize>>> = HashMap::new();
        for (statement, &start) in starts.iter().enumerate().rev() {
            let mut at = start;
            while at < bytes.len() && !visited[at] {
                scanner_work(1);
                visited[at] = true;
                if bytes.get(at..at + 2) == Some(b"--") {
                    at = line_ends
                        .get(line_ends.partition_point(|end| *end < at))
                        .copied()
                        .unwrap_or(bytes.len());
                } else if bytes.get(at..at + 2) == Some(b"/*") {
                    at += 2;
                    at = block_ends
                        .get(block_ends.partition_point(|end| *end < at))
                        .copied()
                        .unwrap_or(bytes.len())
                        + 2;
                } else if matches!(bytes[at], b'\'' | b'"' | b'`' | b'[') {
                    let opening = at;
                    let quote = if bytes[at] == b'[' { b']' } else { bytes[at] };
                    let cache = ends.entry(quote).or_insert_with(|| vec![None; bytes.len()]);
                    at = sql_quote_end(bytes, at + 1, quote, escapes, cache);
                    if bytes[opening] == b'\'' {
                        frames.push((opening..at.min(bytes.len()), (statement, dialect)));
                    }
                    at += 1;
                } else {
                    at += 1;
                }
            }
        }
    }
    frames.sort_by_key(|(range, _)| range.start);
    let mut best: Option<(Range<usize>, (usize, usize))> = None;
    frames
        .into_iter()
        .map(|(frame, priority)| {
            if best
                .as_ref()
                .is_none_or(|(prior, rank)| (frame.end, priority) >= (prior.end, *rank))
            {
                best = Some((frame.clone(), priority));
            }
            (
                frame.start,
                best.as_ref().expect("current SQL frame").0.clone(),
            )
        })
        .collect()
}

fn sql_quote_end(
    bytes: &[u8],
    start: usize,
    quote: u8,
    escapes: bool,
    cache: &mut [Option<usize>],
) -> usize {
    let mut path = Vec::new();
    let mut at = start;
    let end = loop {
        scanner_work(1);
        if at >= bytes.len() {
            break bytes.len();
        }
        if let Some(end) = cache[at] {
            break end;
        }
        path.push(at);
        if escapes && bytes[at] == b'\\' {
            at += 2;
        } else if bytes[at] == quote {
            if bytes.get(at + 1) == Some(&quote) {
                at += 2;
            } else if quote == b'\''
                && bytes
                    .get(at + 1)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                at += 1;
            } else {
                break at;
            }
        } else {
            at += 1;
        }
    };
    for at in path {
        cache[at] = Some(end);
    }
    end
}

struct StringEndIndex {
    runs: Vec<Range<usize>>,
    end: usize,
}
impl StringEndIndex {
    fn new(input: &str, start: usize, depth: Option<usize>) -> Self {
        let Some(depth) = depth else {
            return Self {
                runs: Vec::new(),
                end: input.len(),
            };
        };
        let mut runs = Vec::new();
        let mut escapes = 0usize;
        let mut at = start;
        while at < input.len() {
            scanner_work(1);
            let byte = input.as_bytes()[at];
            if byte == b'\\' {
                escapes += 1;
                at += 1;
                continue;
            }
            if byte == b'"' && (escapes + 1).is_power_of_two() && escapes <= depth {
                let opening = at - escapes;
                let run_end = quote_run_end(input, opening, byte, escapes, input.len());
                runs.push(opening..run_end);
                let width = escapes + 1;
                let run = (run_end - opening) / width;
                if escapes == depth && run >= 2 {
                    if run.is_multiple_of(2) {
                        at = run_end;
                        escapes = 0;
                        continue;
                    }
                    return Self {
                        runs,
                        end: run_end - width,
                    };
                }
                return Self { runs, end: opening };
            }
            escapes = 0;
            at += 1;
        }
        Self {
            runs,
            end: input.len(),
        }
    }
    fn end(&self, input: &str, limit: usize, depth: Option<usize>) -> usize {
        let at = self.runs.partition_point(|run| run.start < limit);
        if let Some(run) = at
            .checked_sub(1)
            .map(|at| &self.runs[at])
            .filter(|run| limit < run.end)
        {
            return serialized_string_end(input, run.start, limit, depth);
        }
        self.end.min(limit)
    }
}

struct QuoteEvent {
    span: Range<usize>,
    valid_run: bool,
    next_terminal: usize,
}

struct QuoteCloseIndex {
    events: Vec<QuoteEvent>,
    end: usize,
}

impl QuoteCloseIndex {
    // Every capture shares the same suffix events for this quote grammar.
    fn new(input: &str, quote: u8, escapes: usize, delimiter: ScalarQuoteDelimiter) -> Self {
        let width = escapes + 1;
        let stride = width * 2;
        let mut events = Vec::new();
        let mut preceding = 0;
        let mut at = 0;
        while at < input.len() {
            scanner_work(1);
            let byte = input.as_bytes()[at];
            if byte == b'\\' {
                preceding += 1;
            } else {
                if byte == quote {
                    let residue = preceding % stride;
                    if residue == escapes {
                        let start = at - escapes;
                        let end = quote_run_end(input, start, quote, escapes, input.len());
                        events.push(QuoteEvent {
                            span: start..end,
                            valid_run: true,
                            next_terminal: 0,
                        });
                        at = end;
                        preceding = 0;
                        continue;
                    }
                    if residue != stride - 1 {
                        events.push(QuoteEvent {
                            span: at..at + 1,
                            valid_run: false,
                            next_terminal: 0,
                        });
                    }
                }
                preceding = 0;
            }
            at += 1;
        }
        let mut next = events.len();
        for i in (0..events.len()).rev() {
            scanner_work(1);
            if !events[i].valid_run
                || Self::run_result(input, events[i].span.clone(), width, delimiter).0
            {
                next = i;
            }
            events[i].next_terminal = next;
        }
        Self {
            events,
            end: input.len(),
        }
    }

    // A terminal run can reject the boundary (odd/oversized doubled triples).
    fn run_result(
        input: &str,
        span: Range<usize>,
        width: usize,
        delimiter: ScalarQuoteDelimiter,
    ) -> (bool, Option<usize>) {
        let run = span.len() / width;
        let end = span.start + run * width;
        let (terminal, close) = match delimiter {
            ScalarQuoteDelimiter::Single if run % 2 == 1 => (true, Some(end - width)),
            ScalarQuoteDelimiter::DoubledSingle if run % 4 == 2 => (true, Some(end - 2 * width)),
            ScalarQuoteDelimiter::DoubledTriple if !run.is_multiple_of(2) || run > 10 => {
                (true, None)
            }
            ScalarQuoteDelimiter::DoubledTriple if run >= 6 => (true, Some(end - 6 * width)),
            ScalarQuoteDelimiter::Triple if run > 5 => (true, None),
            ScalarQuoteDelimiter::Triple if run >= 3 => (true, Some(end - 3 * width)),
            _ => (false, None),
        };
        (
            terminal,
            close.filter(|_| {
                !input[end..]
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
            }),
        )
    }

    fn next_event_after(&self, limit: usize) -> usize {
        self.events
            .get(self.events.partition_point(|event| event.span.end <= limit))
            .map_or(self.end, |event| event.span.start)
    }

    fn close(
        &self,
        input: &str,
        start: usize,
        limit: usize,
        escapes: usize,
        delimiter: ScalarQuoteDelimiter,
    ) -> Option<usize> {
        let width = escapes + 1;
        let first = self.events.partition_point(|event| event.span.end <= start);
        let event = self
            .events
            .get(first)
            .filter(|event| event.span.start < limit)?;
        scanner_work(1);
        if !event.valid_run {
            return None;
        }
        let end = event.span.end.min(limit);
        let (terminal, close) =
            Self::run_result(input, event.span.start.max(start)..end, width, delimiter);
        if terminal || end == limit {
            return close;
        }
        let next = self
            .events
            .get(first + 1)
            .map_or(self.events.len(), |event| event.next_terminal);
        if let Some(event) = self
            .events
            .get(next)
            .filter(|event| event.span.end <= limit)
        {
            return event
                .valid_run
                .then(|| Self::run_result(input, event.span.clone(), width, delimiter).1)
                .flatten();
        }
        // Clipping a nonterminal run can change its parity into a close.
        let last = self
            .events
            .partition_point(|event| event.span.start < limit)
            .checked_sub(1)?;
        let event = &self.events[last];
        if event.valid_run && event.span.end > limit {
            return Self::run_result(input, event.span.start..limit, width, delimiter).1;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_attribute_names_and_values_share_carrier_context() {
        let input = "<Customer ID value=\"C-731284\"/>";
        let mut index = ScalarBoundaryIndex::default();
        for needle in ["value", "C-731284"] {
            assert!(
                index.inside_markup_assignment(input, input.find(needle).unwrap()),
                "{needle}"
            );
        }
        let wrapper = "Customer ID=<C-731284>";
        assert!(!ScalarBoundaryIndex::default()
            .inside_markup_assignment(wrapper, wrapper.find("C-731284").unwrap()));
    }

    fn compare(input: &str) -> usize {
        static RULES: std::sync::OnceLock<Vec<RegexDetector>> = std::sync::OnceLock::new();
        let rules = RULES.get_or_init(|| {
            let core = gaze::Rulepack::load(gaze::RulepackSource::Embedded(
                crate::embedded("core").unwrap(),
            ))
            .unwrap();
            core.recognizers
                .into_iter()
                .filter(|rule| {
                    matches!(
                        rule.id.as_str(),
                        "customer_id.labelled" | "employee_id.labelled" | "record_id.labelled"
                    )
                })
                .map(|rule| {
                    let gaze::RawMatch::Regex {
                        pattern: Some(pattern),
                        capture_groups,
                        ..
                    } = rule.matcher
                    else {
                        panic!("record rule must have a regex");
                    };
                    let pattern = legacy_capture_pattern(&pattern);
                    let mut detector = RegexDetector::new(&pattern, rule.class).unwrap();
                    detector.capture_groups = capture_groups;
                    detector
                })
                .collect()
        });
        let mut checked = 0;
        for rule in rules {
            let mut index = ScalarBoundaryIndex::default();
            for caps in rule.regex.captures_iter(input) {
                let capture = rule.span_from_captures(&caps).unwrap();
                let old = scalar_reference::scan_field_owned_value(input, capture.clone());
                let new = scan_field_owned_value_indexed(input, capture.clone(), &mut index);
                assert_eq!(new, old, "input {input:?}, capture {capture:?}");
                checked += 1;
            }
        }
        checked
    }

    #[test]
    fn shared_quote_events_match_frozen_grammar_at_clipped_runs() {
        for body in [
            "''SYN''x''\n",
            "'''SYN''é'",
            "'a'_'b' 'c'",
            "\\\"a\\\"b\"c",
            "''''''x''''''''''' ",
        ] {
            let mut input = body.to_owned();
            for _depth in 0..=3 {
                for quote in [b'\'', b'"'] {
                    for escapes in [0, 1, 3, 7] {
                        for delimiter in [
                            ScalarQuoteDelimiter::Single,
                            ScalarQuoteDelimiter::Triple,
                            ScalarQuoteDelimiter::DoubledSingle,
                            ScalarQuoteDelimiter::DoubledTriple,
                        ] {
                            let index = QuoteCloseIndex::new(&input, quote, escapes, delimiter);
                            // Real starts follow an opener, never split its escape prefix.
                            for start in (0..=input.len())
                                .filter(|&at| at == 0 || input.as_bytes()[at - 1] == quote)
                            {
                                for limit in
                                    (start..=input.len()).filter(|&at| input.is_char_boundary(at))
                                {
                                    assert_eq!(index.close(&input, start, limit, escapes, delimiter),
                                        scalar_quote_close(&input, start, limit, quote, escapes, delimiter),
                                        "{input:?}, start {start}, limit {limit}, quote {quote}, escapes {escapes}, delimiter {}", delimiter as u8);
                                }
                            }
                        }
                    }
                }
                input = serde_json::to_string(&input).unwrap();
            }
        }
    }

    #[test]
    fn indexed_scanner_matches_frozen_scanner_on_generated_serializations() {
        let mut checked = 0;
        for seed in 0..128 {
            let value = format!("SYN{seed:06} É{}", seed + 731284);
            let variants = [
                format!(
                    "customerId: \"{value}\"; recordId: 'R{seed:06}'; employeeId: `E{seed:06}`"
                ),
                format!(
                    "customerId: '{value}'; recordId: \"R{seed:06}\"\nemployeeId: “E{seed:06}”"
                ),
                format!("customerId: \"\"\"{value}\nmore \"content\"\"\"; recordId: 'R{seed:06}'"),
                format!("customerId: \"{value}\\\" embedded; employeeId: 'E{seed:06}'"),
                format!("customerId: '{value}; suffix\nrecordId: 'R{seed:06}'"),
                format!("SELECT 'customerId: \"{value}\"; recordId: `R{seed:06}`', 7;"),
                format!(
                    "before\nSELECT 'customerId: {value}\nmore; employeeId: \"E{seed:06}\"', 7;"
                ),
                format!("\"customerId: \"\"{value}\"\"; employeeId: 'E{seed:06}'\",public"),
                format!("customerId: “{value}”; recordId: «R{seed:06}»; employeeId: `E{seed:06}`"),
                serde_json::to_string(&format!("customerId: “SYN{seed:06} \\a\\b\\c”"))
                    .unwrap()
                    .replace('“', r"\u201c")
                    .replace('”', r"\u201d"),
            ];
            for mut input in variants {
                for _depth in 0..=5 {
                    checked += compare(&input);
                    input = serde_json::to_string(&input).unwrap();
                }
            }
        }
        assert_eq!(checked, 12_288);
        eprintln!("serialization differential: {checked} captures");
    }

    #[test]
    fn indexed_scanner_matches_frozen_scanner_on_existing_boundary_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/record_id_boundaries_v27.json"
        ))
        .unwrap();
        let mut checked = 0;
        for case in fixture["records"].as_array().unwrap() {
            checked += compare(case["text"].as_str().unwrap());
        }
        assert_eq!(checked, 3_630);
        eprintln!(
            "fixture differential: {} records, {checked} captures",
            fixture["records"].as_array().unwrap().len()
        );
    }

    #[test]
    fn indexed_scanner_matches_frozen_scanner_after_mixed_prior_fields() {
        let chunks = [
            "public: 'earlier \"text\"'; ",
            "public: \"\"\"earlier 'text'\"\"\"; ",
            "public: ''\n",
            "public: 'unclosed\nnext=7; ",
            "public: \"escaped\\\" text\"; ",
            "SELECT 'public\nSELECT \"identifier\"'; ",
            "\"public,\"\"cell\"\"\",7\n",
            "public: `text`; ",
            "public: “text”; ",
        ];
        let mut checked = 0;
        for seed in 0..2048 {
            let mut state = seed;
            let mut input = String::new();
            for index in 0..4 {
                state = state * 17 % 4093 + 23;
                input.push_str(chunks[state % chunks.len()]);
                input.push_str(match index % 3 {
                    0 => "customerId: \"SYN731284\"; ",
                    1 => "employeeId: 'SYN842395'; ",
                    _ => "recordId: `SYN953406`; ",
                });
            }
            checked += compare(&input);
            checked += compare(&serde_json::to_string(&input).unwrap());
        }
        assert_eq!(checked, 12_288);
        eprintln!("mixed-prefix differential: {checked} captures");
    }

    #[test]
    fn quoted_field_enclosure_work_is_linear() {
        for quote in ["\"", "'", "`", "“"] {
            let close = if quote == "“" { "”" } else { quote };
            let detector = RegexDetector::with_rulepack_fields(
                &format!(r"customerId: {quote}(SYN[0-9]+)"),
                PiiClass::custom("customer_id").unwrap(),
                "customer_id.labelled",
                vec![LocaleTag::Global],
                0.7,
                0,
                "counter",
                Some(vec![1]),
                Vec::new(),
                None,
                None,
            )
            .unwrap()
            .with_complete_labelled_value(true)
            .with_labelled_value_mode(LabelledValueMode::FieldOwned);
            let mut previous = 0;
            for fields in [128, 512, 2048] {
                let input = format!("customerId: {quote}SYN731284{close}\n").repeat(fields);
                SCANNER_WORK.with(|work| work.set(0));
                let scans = detector.scanned_spans(&input, None);
                let steps = SCANNER_WORK.with(|work| work.get());
                assert_eq!(scans.len(), fields);
                assert!(scans
                    .iter()
                    .all(|scan| !scan.rejected && &input[scan.span.clone()] == "SYN731284"));
                assert!(
                    steps <= input.len() * 40,
                    "{quote}: {steps} steps for {} bytes",
                    input.len()
                );
                if previous > 0 {
                    assert!(steps <= previous * 5, "{quote}: {previous} -> {steps}");
                }
                previous = steps;
            }
        }
    }
    #[test]
    fn enclosure_work_stays_linear_in_serialized_records() {
        let core = gaze::Rulepack::load(gaze::RulepackSource::Embedded(
            crate::embedded("core").unwrap(),
        ))
        .unwrap();
        let rule = core
            .recognizers
            .into_iter()
            .find(|rule| rule.id == "customer_id.labelled")
            .unwrap();
        let gaze::RawMatch::Regex {
            pattern: Some(pattern),
            capture_groups,
            ..
        } = rule.matcher
        else {
            panic!("record regex");
        };
        let mut detector = RegexDetector::new(&pattern, rule.class).unwrap();
        detector.capture_groups = capture_groups;
        detector.complete_labelled_value = true;
        detector.labelled_value_mode = LabelledValueMode::FieldOwned;
        let mut compared = 0;
        for surface in 0..9 {
            let mut previous = 0;
            for fields in [128, 512, 2048] {
                let body = "customerId: \"SYN731284\"; ".repeat(fields);
                let input = match surface {
                    0 => body,
                    1 => serde_json::to_string(&body).unwrap(),
                    2 => serde_json::to_string(&serde_json::to_string(&body).unwrap()).unwrap(),
                    3 => format!("\"{}\",public", body.replace('"', "\"\"")),
                    4 => format!("SELECT '{body}', 7;"),
                    5 => format!("public: 'unclosed\n{body}"),
                    6 => format!("public: 'later close\n{body}\n'; public=7"),
                    7 => format!("public: 'unclosed {body}"),
                    _ => format!("public: 'later close {body}'; public=7"),
                };
                SCANNER_WORK.with(|work| work.set(0));
                let scans = detector.scanned_spans(&input, None);
                let steps = SCANNER_WORK.with(|work| work.get());
                assert_eq!(scans.len(), fields, "surface {surface}");
                assert!(scans
                    .iter()
                    .all(|scan| !scan.rejected && &input[scan.span.clone()] == "SYN731284"));
                assert!(
                    steps <= input.len() * 60,
                    "surface {surface}: {steps} steps for {} bytes",
                    input.len()
                );
                if previous > 0 {
                    assert!(
                        steps <= previous * 5,
                        "surface {surface}: {previous} -> {steps}"
                    );
                }
                eprintln!(
                    "surface {surface}, fields {fields}, bytes {}, scanner work {steps}",
                    input.len()
                );
                if fields == 128 {
                    let checked = compare(&input);
                    assert_eq!(checked, fields);
                    compared += checked;
                }
                previous = steps;
            }
        }
        assert_eq!(compared, 1_152);
        eprintln!("scaling inputs differential: {compared} captures");
    }
    #[test]
    fn sibling_encoded_absent_values_share_whole_input_validation() {
        let core = gaze::Rulepack::load(gaze::RulepackSource::Embedded(
            crate::embedded("core").unwrap(),
        ))
        .unwrap();
        let rule = core
            .recognizers
            .into_iter()
            .find(|rule| rule.id == "customer_id.labelled")
            .unwrap();
        let gaze::RawMatch::Regex {
            pattern: Some(pattern),
            capture_groups,
            ..
        } = rule.matcher
        else {
            panic!("record regex");
        };
        let mut detector = RegexDetector::new(&pattern, rule.class).unwrap();
        detector.capture_groups = capture_groups;
        detector.complete_labelled_value = true;
        detector.labelled_value_mode = LabelledValueMode::FieldOwned;
        for depth in 0..=3 {
            let mut oracle_input =
                serde_json::to_string(&vec![r#"{"message":"customerId: "}"#; 8]).unwrap();
            for _ in 0..depth {
                oracle_input = serde_json::to_string(&oracle_input).unwrap();
            }
            assert_eq!(compare(&oracle_input), 8);
            let mut previous = 0;
            for fields in [128, 512, 2048] {
                let mut input =
                    serde_json::to_string(&vec![r#"{"message":"customerId: "}"#; fields]).unwrap();
                for _ in 0..depth {
                    input = serde_json::to_string(&input).unwrap();
                }
                SCANNER_WORK.with(|work| work.set(0));
                let scans = detector.scanned_spans(&input, None);
                let steps = SCANNER_WORK.with(|work| work.get());
                assert_eq!(scans.len(), fields);
                assert!(scans
                    .iter()
                    .all(|scan| scan.rejected && scan.span.is_empty()));
                assert!(
                    steps <= input.len() * 60,
                    "depth {depth}: {steps} steps for {} bytes",
                    input.len()
                );
                if previous > 0 {
                    assert!(
                        steps <= previous * 5,
                        "depth {depth}: {previous} -> {steps}"
                    );
                }
                eprintln!("absent siblings depth {depth}, fields {fields}, bytes {}, scanner work {steps}", input.len());
                previous = steps;
            }
        }
    }
}
#[cfg(test)]
mod regression_tests {
    use super::*;

    pub(super) fn rules() -> Vec<RegexDetector> {
        let core = gaze::Rulepack::load(gaze::RulepackSource::Embedded(
            crate::embedded("core").unwrap(),
        ))
        .unwrap();
        core.recognizers
            .into_iter()
            .filter(|rule| {
                matches!(
                    rule.id.as_str(),
                    "customer_id.labelled" | "employee_id.labelled" | "record_id.labelled"
                )
            })
            .map(|rule| {
                let gaze::RawMatch::Regex {
                    pattern: Some(pattern),
                    capture_groups,
                    ..
                } = rule.matcher
                else {
                    panic!()
                };
                let mut d = RegexDetector::new(&pattern, rule.class).unwrap();
                d.capture_groups = capture_groups;
                d.complete_labelled_value = true;
                d.labelled_value_mode = LabelledValueMode::FieldOwned;
                d
            })
            .collect()
    }

    // Independent oracle: Serde supplies token boundaries and validates values.
    // This does not use the production frame index or its following-value map.
    fn json_successor(input: &str, close: usize) -> Option<Range<usize>> {
        serde_json::from_str::<serde_json::Value>(input).ok()?;
        let mut containers = Vec::new();
        let mut at = 0;
        while at <= close {
            let byte = *input.as_bytes().get(at)?;
            if byte == b'"' {
                let mut stream =
                    serde_json::Deserializer::from_str(&input[at..]).into_iter::<String>();
                stream.next()?.ok()?;
                let end = at + stream.byte_offset();
                let tail = input
                    .get(close..end - 1)
                    .and_then(|tail| serde_json::from_str::<String>(&format!("\"{tail}\"")).ok());
                if end - 1 == close
                    || (at < close && tail.is_some_and(|tail| tail.trim().is_empty()))
                {
                    let rest = input[end..].trim_start();
                    let marker = rest.as_bytes().first()?;
                    if *marker != b':' && !(*marker == b',' && containers.last() == Some(&b'[')) {
                        return None;
                    }
                    let value = rest[1..].trim_start();
                    let start = input.len() - value.len();
                    let mut stream =
                        serde_json::Deserializer::from_str(value).into_iter::<serde_json::Value>();
                    let parsed = stream.next()?.ok()?;
                    let end = start + stream.byte_offset();
                    return match parsed {
                        serde_json::Value::String(_) => Some(start + 1..end - 1),
                        serde_json::Value::Number(_) => Some(start..end),
                        _ => None,
                    };
                }
                at = end;
                continue;
            }
            match byte {
                b'[' | b'{' => containers.push(byte),
                b']' | b'}' => {
                    containers.pop();
                }
                _ => {}
            }
            at += 1;
        }
        None
    }

    fn assert_recovered(input: &str, capture: &Range<usize>, scan: &LabelledValueScan) {
        if let Some(expected) = json_successor(input, capture.start).or_else(|| {
            capture
                .start
                .checked_sub(1)
                .and_then(|at| json_successor(input, at))
        }) {
            assert_eq!(scan.span, expected, "following scalar {input:?}");
            assert!(!scan.rejected);
            RECOVERED_NEXT.with(|count| count.set(count.get() + 1));
        } else if scan.rejected {
            RECOVERED_ABSENT.with(|count| count.set(count.get() + 1));
            assert!(scan.span.is_empty());
            assert_eq!(scan.span.start, capture.start);
            assert!(serde_json::from_str::<serde_json::Value>(input).is_ok());
            assert_eq!(input.as_bytes()[scan.span.start], b'"');
            assert!(
                json_successor(input, scan.span.start).is_none(),
                "a real next value is not absent"
            );
            let mut inside = false;
            let mut escaped = false;
            for byte in input[..scan.span.start].bytes() {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    inside = !inside;
                }
            }
            assert!(
                inside && !escaped,
                "absent value must be at a real JSON scalar close"
            );
        } else {
            RECOVERED_OWNED.with(|count| count.set(count.get() + 1));
            assert!(scan.span.start <= capture.start && scan.span.end >= capture.end);
        }
    }

    pub(super) fn legacy_rules() -> Vec<RegexDetector> {
        rules()
            .into_iter()
            .map(|mut rule| {
                let pattern = legacy_capture_pattern(rule.regex.as_str());
                rule.regex = Regex::new(&pattern).unwrap();
                rule
            })
            .collect()
    }

    fn compare_all(rules: &[RegexDetector], input: &str) -> usize {
        let mut n = 0;
        for rule in rules {
            let mut index = ScalarBoundaryIndex::default();
            for caps in rule.regex.captures_iter(input) {
                let capture = rule.span_from_captures(&caps).unwrap();
                let o = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    scalar_reference::scan_field_owned_value(input, capture.clone())
                }));
                let mut fresh = ScalarBoundaryIndex::default();
                let nw = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    scan_field_owned_value_indexed(input, capture.clone(), &mut fresh)
                }));
                let shared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    scan_field_owned_value_indexed(input, capture.clone(), &mut index)
                }));
                assert!(
                    nw.is_ok() && shared.is_ok(),
                    "candidate panicked: {input:?}, {capture:?}"
                );
                match (&o, &nw, &shared) {
                    (Ok(a), Ok(b), Ok(c)) => {
                        assert_eq!(b, c, "fresh/shared input {input:?}");
                        if a.span.start > a.span.end {
                            assert!(a.rejected);
                            assert_recovered(input, &capture, b);
                            REVERSED.with(|count| count.set(count.get() + 1));
                        } else if b != a {
                            let expected = json_successor(input, capture.start).or_else(|| capture.start.checked_sub(1).and_then(|at| json_successor(input, at)))
                                .unwrap_or_else(|| panic!("unexplained span change {input:?}, capture {capture:?}, predecessor {:?}: {a:?} -> {b:?}", json_successor(input, capture.start.saturating_sub(1))));
                            assert!(!b.rejected);
                            assert_eq!(b.span, expected);
                            if a.rejected {
                                VALID_NEXT.with(|count| count.set(count.get() + 1));
                            } else {
                                // Relocation may drop an empty wrapper or structural
                                // delimiters, but never previously protected value bytes.
                                let whitespace: std::collections::HashSet<_> = input
                                    .char_indices()
                                    .filter(|(_, ch)| ch.is_whitespace())
                                    .flat_map(|(at, ch)| at..at + ch.len_utf8())
                                    .collect();
                                let mut outside_value =
                                    a.span.clone().filter(|at| !expected.contains(at));
                                assert!(outside_value.all(|at| {
                                    let byte = input.as_bytes()[at];
                                    whitespace.contains(&at) || b"\\\"',:[]{}()<>`".contains(&byte)
                                        || (b"nrt".contains(&byte) && at > 0 && input.as_bytes()[at - 1] == b'\\')
                                }),
                                    "old private bytes must stay owned: {input:?}, {capture:?}: {a:?} -> {b:?}");
                                WRAPPER_NEXT.with(|count| count.set(count.get() + 1));
                            }
                        }
                    }
                    _ => {
                        PANICS.with(|p| p.set(p.get() + 1));
                        assert_eq!(nw.as_ref().unwrap(), shared.as_ref().unwrap());
                        let scan = nw.as_ref().unwrap();
                        assert_recovered(input, &capture, scan);
                    }
                }
                if std::env::var_os("GAZE_SCALAR_DIFFERENTIAL_REPORT").is_some()
                    && o.as_ref().map_or(true, |old| old != nw.as_ref().unwrap())
                {
                    eprintln!(
                        "SPAN_CHANGE {}",
                        serde_json::json!({
                            "input": input, "capture": capture,
                            "old": o.as_ref().ok().map(|scan| (&scan.span, scan.rejected)),
                            "new": (&nw.as_ref().unwrap().span, nw.as_ref().unwrap().rejected),
                        })
                    );
                }
                n += 1;
            }
        }
        n
    }
    thread_local! {
    static RECOVERED_NEXT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static WRAPPER_NEXT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static VALID_NEXT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static RECOVERED_ABSENT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; static RECOVERED_OWNED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; static REVERSED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; static PANICS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

    #[test]
    fn review_random_differential() {
        let rules = legacy_rules();
        let atoms = [
            "customerId: ",
            "employee_id=",
            "recordId ",
            "Kundennummer: ",
            "customer no. ",
            "\"",
            "'",
            "`",
            "“",
            "”",
            "«",
            "»",
            "‘",
            "’",
            "\\",
            "\\\"",
            "\\\\\"",
            "\\u201c",
            "\\u201d",
            "\\n",
            "\n",
            "\r\n",
            ",",
            ";",
            " ",
            "\t",
            "\u{00A0}",
            "\u{202F}",
            "SYN123",
            "É9",
            "x",
            "_",
            "\"\"",
            "''",
            "\"\"\"",
            "'''",
            "SELECT ",
            "VALUES (",
            "--",
            "/*",
            "*/",
            "[",
            "]",
            "{",
            "}",
            ":",
            "is ",
            "0",
            "-",
        ];
        let mut state: u64 = 0x9E3779B97F4A7C15;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut checked = 0;
        for _ in 0..40000 {
            let len = 3 + (next() % 24) as usize;
            let mut s = String::new();
            for _ in 0..len {
                let r = next();
                if r % 4 == 0 {
                    s.push_str(atoms[(r as usize / 4) % 5]);
                } else {
                    s.push_str(atoms[(r as usize / 4) % atoms.len()]);
                }
            }
            checked += compare_all(&rules, &s);
            let j = serde_json::to_string(&s).unwrap();
            checked += compare_all(&rules, &j);
            if next() % 3 == 0 {
                checked += compare_all(&rules, &serde_json::to_string(&j).unwrap());
            }
            if next() % 3 == 0 {
                checked += compare_all(&rules, &serde_json::to_string(&serde_json::json!({"a": s, "b": [s.clone(), "customerId: \"Z1\"".to_string()]})).unwrap());
            }
        }
        eprintln!(
            "review random differential: {checked} captures, panics {}",
            PANICS.with(|p| p.get())
        );
        eprintln!(
            "reversed rejected spans repaired: {}",
            REVERSED.with(|p| p.get())
        );
        eprintln!(
            "recovered missing JSON values: {}; recovered ambiguous owned values: {}",
            RECOVERED_ABSENT.with(|p| p.get()),
            RECOVERED_OWNED.with(|p| p.get())
        );
        eprintln!(
            "recovered next JSON values: {}; previously valid rejections repaired: {}",
            RECOVERED_NEXT.with(|p| p.get()),
            VALID_NEXT.with(|p| p.get())
        );
        eprintln!(
            "syntax-only boundary repaired around following scalar: {}",
            WRAPPER_NEXT.with(|p| p.get())
        );
        assert_eq!(checked, 359_152);
        assert_eq!(PANICS.with(|p| p.get()), 5_134);
        assert_eq!(REVERSED.with(|p| p.get()), 3_260);
        assert_eq!(RECOVERED_ABSENT.with(|p| p.get()), 8_344);
        assert_eq!(RECOVERED_OWNED.with(|p| p.get()), 50);
        assert_eq!(RECOVERED_NEXT.with(|p| p.get()), 0);
        assert_eq!(VALID_NEXT.with(|p| p.get()), 2_696);
        assert_eq!(WRAPPER_NEXT.with(|p| p.get()), 395);
    }
}

#[cfg(test)]
mod panic_fixtures {
    use super::*;
    #[test]
    fn unterminated_escaped_quote_preserves_owned_span() {
        let inputs: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/record_id_ambiguous_quotes.json"
        ))
        .unwrap();
        let rules = super::regression_tests::rules();
        let mut checked = 0;
        for fixture in inputs.as_array().unwrap() {
            let input = fixture["text"].as_str().unwrap();
            for rule in &rules {
                for caps in rule.regex.captures_iter(input) {
                    let capture = rule.span_from_captures(&caps).unwrap();
                    let scan = scan_field_owned_value_indexed(
                        input,
                        capture,
                        &mut ScalarBoundaryIndex::default(),
                    );
                    assert!(scan.rejected);
                    assert_eq!(
                        &input[scan.span.clone()],
                        fixture["owned"].as_str().unwrap()
                    );
                    assert_eq!(scan.span.start, input.len() - 1);
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 4);
    }
}

#[cfg(test)]
mod scaling_regressions {
    use super::*;
    #[test]
    fn doubled_apostrophe_prefix_work_is_linear() {
        let rule = super::regression_tests::rules().remove(0);
        for depth in 0..=3 {
            let mut previous = 0;
            for fields in [128, 512, 2048] {
                let mut input = "customerId: ''SYN731284''x''\n".repeat(fields);
                for _ in 0..depth {
                    input = serde_json::to_string(&input).unwrap();
                }
                SCANNER_WORK.with(|w| w.set(0));
                let scans = rule.scanned_spans(&input, None);
                let steps = SCANNER_WORK.with(|w| w.get());
                // At encoded depths the ambiguous quote owns the entire string.
                assert_eq!(scans.len(), if depth == 0 { fields } else { 1 });
                assert!(scans.iter().all(|scan| !scan.rejected));
                assert!(
                    steps <= input.len() * 60,
                    "depth {depth}: {steps}/{}",
                    input.len()
                );
                if previous > 0 {
                    assert!(
                        steps <= previous * 5,
                        "depth {depth}: {previous} -> {steps}"
                    );
                }
                eprintln!("doubled apostrophes depth {depth}, fields {fields}, work {steps}");
                previous = steps;
            }
        }
    }

    #[test]
    fn unclosed_paired_wrapper_work_is_linear() {
        let rules = super::regression_tests::rules();
        for quote in ["`", "“", "«"] {
            for surface in 0..3 {
                let mut previous = 0;
                for fields in [128, 512, 2048] {
                    let mut input = format!("customerId: {quote}SYN731284\n").repeat(fields);
                    for _ in 0..surface {
                        input = serde_json::to_string(&input).unwrap();
                    }
                    SCANNER_WORK.with(|w| w.set(0));
                    let mut index = ScalarBoundaryIndex::default();
                    let scans: Vec<_> = rules[0]
                        .regex
                        .captures_iter(&input)
                        .map(|caps| {
                            scan_field_owned_value_indexed(
                                &input,
                                rules[0].span_from_captures(&caps).unwrap(),
                                &mut index,
                            )
                        })
                        .collect();
                    let steps = SCANNER_WORK.with(|w| w.get());
                    assert_eq!(scans.len(), fields);
                    assert!(
                        steps <= input.len() * 60,
                        "{quote}, surface {surface}: {steps}/{}",
                        input.len()
                    );
                    if previous > 0 {
                        assert!(
                            steps <= previous * 5,
                            "{quote}, surface {surface}: {previous} -> {steps}"
                        );
                    }
                    eprintln!("unclosed {quote}, surface {surface}, fields {fields}, bytes {}, work {steps}", input.len());
                    previous = steps;
                }
            }
        }
    }

    #[test]
    fn odd_openers_before_unterminated_escaped_quotes_never_panic() {
        let mut rules = super::regression_tests::rules();
        rules.extend(super::regression_tests::legacy_rules());
        let mut checked = 0;
        for prefix in ["''", "x", "»", "[[]", "'''", "\"\"\""] {
            for label in ["customerId: ", "employee_id=", "member_id: "] {
                for tail in ["", " ", "SYN731284", "\nSYN842395", "\\", "\""] {
                    let mut input = format!("{prefix}\"{label}{tail}");
                    for _depth in 0..=2 {
                        for rule in &rules {
                            for scan in rule.scanned_spans(&input, None) {
                                assert!(input.get(scan.span).is_some(), "{input:?}");
                                checked += 1;
                            }
                        }
                        input = serde_json::to_string(&input).unwrap();
                    }
                }
            }
        }
        assert!(checked >= 300, "{checked}");
        eprintln!("odd-opener no-panic generator: {checked} captures");
    }
}

#[cfg(test)]
mod json_label_cells {
    use super::{scan_field_owned_value_indexed, ScalarBoundaryIndex};
    fn shape(value: &serde_json::Value) -> serde_json::Value {
        use serde_json::Value;
        match value {
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, value)| (key.clone(), shape(value)))
                    .collect(),
            ),
            Value::Array(values) => Value::Array(values.iter().map(shape).collect()),
            Value::String(text) => {
                if let Ok(Value::String(decoded)) = serde_json::from_str::<Value>(text) {
                    return shape(&Value::String(decoded));
                }
                let regions = gaze_types::json_regions::json_regions(text);
                if regions.is_empty() {
                    Value::String("string".into())
                } else {
                    Value::Array(
                        regions
                            .into_iter()
                            .map(|span| shape(&serde_json::from_str::<Value>(&text[span]).unwrap()))
                            .collect(),
                    )
                }
            }
            Value::Number(_) => Value::String("number".into()),
            other => other.clone(),
        }
    }

    #[test]
    fn json_record_structure_and_covered_values_are_preserved() {
        let rules = super::regression_tests::rules();
        let mut checked = 0;
        for key in [
            "customerId",
            "Customer ID:",
            "Customer ID: ",
            "Customer ID:\t",
            "Customer ID:\n",
            "Kundennummer:",
            "Employee_ID=",
            "member_id:",
        ] {
            for value in [
                r#""SYN731284""#,
                "731284",
                "-731284",
                "731284.5",
                "731284e2",
                r#"["SYN731284"]"#,
                r#"{"value":"SYN731284"}"#,
                r#"{"version":2,"id":"SYN731284"}"#,
                r#"{"type":"string","description":"The id"}"#,
                r##"{"$ref":"#/defs/Id"}"##,
                r#"{"a":[1,2]}"#,
                r#"[{"k":"v"}]"#,
                "null",
                "true",
                "false",
                r#""null""#,
                r#""true""#,
                r#""status""#,
            ] {
                let body = format!(
                    "{{{}:{value},\"status\":\"ok\",\"active\":true,\"empty\":null}}",
                    serde_json::to_string(key).unwrap()
                );
                for container in [
                    body.clone(),
                    format!("[{body}]"),
                    format!("{{\"outer\":{body}}}"),
                    format!("{{\"rows\":[[{body}]]}}"),
                ] {
                    for prefix in [
                        "",
                        "tool output: ",
                        "{\"n\":0}\n",
                        "{\"n\":0\n",
                        "log [truncated\n",
                        "note: \"unbalanced quote\n",
                    ] {
                        for tail in ["", " trailing text"] {
                            let mut input = format!("{prefix}{container}{tail}");
                            for depth in 0..=4 {
                                let mut spans: Vec<_> = rules
                                    .iter()
                                    .flat_map(|rule| rule.scanned_spans(&input, None))
                                    .filter(|scan| !scan.rejected)
                                    .map(|scan| scan.span)
                                    .collect();
                                spans.sort_by_key(|span| (span.start, std::cmp::Reverse(span.end)));
                                spans.dedup();
                                if input.contains("731284") {
                                    let private = input.find("731284").unwrap();
                                    assert!(
                                        spans
                                            .iter()
                                            .any(|span| span.start <= private
                                                && span.end >= private + 6),
                                        "leak depth {depth}: {input:?}: {spans:?}"
                                    );
                                }
                                let regions = if depth == 0 {
                                    let mut regions = std::iter::once(
                                        prefix.len()..prefix.len() + container.len(),
                                    )
                                    .collect::<Vec<_>>();
                                    if prefix == "{\"n\":0}\n" {
                                        regions.insert(0, 0..7);
                                    }
                                    regions
                                } else {
                                    std::iter::once(0..input.len()).collect::<Vec<_>>()
                                };
                                for region in regions {
                                    let original: serde_json::Value =
                                        serde_json::from_str(&input[region.clone()]).unwrap();
                                    let strings = if depth == 0 {
                                        gaze_types::json_regions::json_strings(
                                            &input,
                                            std::slice::from_ref(&region),
                                        )
                                    } else {
                                        vec![(1..input.len() - 1, false)]
                                    };
                                    let mut clean = input[region.clone()].to_owned();
                                    for span in spans.iter().rev().filter(|span| {
                                        region.start <= span.start && span.end <= region.end
                                    }) {
                                        let in_string = strings.iter().any(|(string, key)| {
                                            !key && string.start <= span.start
                                                && span.end <= string.end
                                        });
                                        if !in_string {
                                            let number = serde_json::from_str::<serde_json::Value>(
                                                &input[span.clone()],
                                            )
                                            .unwrap_or(serde_json::Value::Null);
                                            assert!(
                                                number.is_number(),
                                                "non-value span: {input:?}: {span:?}"
                                            );
                                        }
                                        let numeric =
                                            serde_json::from_str::<serde_json::Value>(value)
                                                .unwrap()
                                                .is_number();
                                        // Direct numeric IDs retain the separately tracked
                                        // numeric-emission limitation; preserve their type in
                                        // this recognizer-only span substitution.
                                        let mut marker = if numeric || !in_string {
                                            "0".to_owned()
                                        } else {
                                            "MASK".to_owned()
                                        };
                                        if numeric {
                                            for _ in 0..depth {
                                                let encoded =
                                                    serde_json::to_string(&marker).unwrap();
                                                marker = encoded[1..encoded.len() - 1].to_owned();
                                            }
                                        }
                                        clean.replace_range(
                                            span.start - region.start..span.end - region.start,
                                            &marker,
                                        );
                                    }
                                    let parsed: serde_json::Value = serde_json::from_str(&clean)
                                        .unwrap_or_else(|error| {
                                            panic!("{input:?} -> {clean:?}: {error}")
                                        });
                                    assert_eq!(
                                        shape(&original),
                                        shape(&parsed),
                                        "depth {depth}: {input:?} -> {clean:?}"
                                    );
                                }
                                checked += 1;
                                input = serde_json::to_string(&input).unwrap();
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 34560);
    }

    #[test]
    fn json_object_keys_are_never_private_value_spans() {
        let rules = super::regression_tests::rules();
        for body in [
            r#"{"customerId: SYN731284":"public"}"#,
            r#"{"outer":{"Customer ID: SYN731284":"public"},"type":"string"}"#,
            r#"{"customerId":{"customerId: SYN731284":"public"}}"#,
        ] {
            let mut input = body.to_owned();
            for _ in 0..=4 {
                let scans: Vec<_> = rules
                    .iter()
                    .flat_map(|rule| rule.scanned_spans(&input, None))
                    .collect();
                assert!(
                    scans.iter().all(|scan| scan.rejected),
                    "{input:?}: {scans:?}"
                );
                input = serde_json::to_string(&input).unwrap();
            }
        }
    }

    #[test]
    fn containers_own_all_and_only_explicit_record_values() {
        let rules = super::regression_tests::rules();
        let mut checked = 0;
        for (body, expected) in [
            (
                r#"["SYN731284","SYN731285"]"#,
                vec!["SYN731284", "SYN731285"],
            ),
            (
                r#"[{"id":"SYN731284"},{"ID":"SYN731285"}]"#,
                vec!["SYN731284", "SYN731285"],
            ),
            (
                r#"{"version":2,"id":"SYN731284","value":"SYN731285"}"#,
                vec!["SYN731284", "SYN731285"],
            ),
            (r#"["","SYN731284"]"#, vec!["SYN731284"]),
            (r#"[{"id":""},{"id":"SYN731284"}]"#, vec!["SYN731284"]),
            (r#"{"version":2,"other":"SYN731284"}"#, vec![]),
            (
                r#"{"type":"string","id":"SYN731284","maxLength":36}"#,
                vec![],
            ),
            (r#"{"format":"int64","examples":["SYN731284"]}"#, vec![]),
        ] {
            let mut input = format!(r#"{{"customerId":{body}}}"#);
            for _ in 0..=4 {
                let mut spans: Vec<_> = rules
                    .iter()
                    .flat_map(|rule| rule.scanned_spans(&input, None))
                    .filter(|scan| !scan.rejected)
                    .map(|scan| scan.span)
                    .collect();
                spans.sort_by_key(|span| (span.start, span.end));
                spans.dedup();
                let actual: Vec<_> = spans.iter().map(|span| &input[span.clone()]).collect();
                assert_eq!(actual, expected, "{input}");
                checked += 1;
                input = serde_json::to_string(&input).unwrap();
            }
        }
        assert_eq!(checked, 40);
    }

    #[test]
    fn schema_containers_never_own_metadata_or_id_children() {
        let rules = super::regression_tests::rules();
        let mut checked = 0;
        for key in [
            "type",
            "$ref",
            "properties",
            "format",
            "pattern",
            "minimum",
            "maximum",
            "minItems",
            "maxLength",
            "enum",
            "examples",
            "const",
            "default",
        ] {
            for array in [false, true] {
                let object = serde_json::json!({"id":"SYN731284", key: "int64"});
                let mut input = serde_json::json!({"customerId": if array { serde_json::json!([object]) } else { object }}).to_string();
                for _ in 0..=4 {
                    assert!(
                        rules
                            .iter()
                            .flat_map(|rule| rule.scanned_spans(&input, None))
                            .all(|scan| scan.rejected),
                        "{input}"
                    );
                    checked += 1;
                    input = serde_json::to_string(&input).unwrap();
                }
            }
        }
        assert_eq!(checked, 130);
    }

    #[test]
    fn unicode_label_whitespace_owns_the_value() {
        let rules = super::regression_tests::rules();
        let mut checked = 0;
        for escape in [
            "\\u0009",
            "\\u000a",
            "\\u000A",
            "\\u000d",
            "\\u000D",
            "\\u0020",
            " \\u0009 \\t ",
            "\\u000b",
            "\\u000B",
            "\\u000c",
            "\\u000C",
            "\\u00a0",
            "\\u2003",
            "\\u202F",
            "\\f",
            "\\u200b",
            "\u{200b}",
        ] {
            let mut input = format!(r#"{{"Customer ID:{escape}":"SYN731284"}}"#);
            for _ in 0..=4 {
                let private = input.find("731284").unwrap();
                assert!(
                    rules
                        .iter()
                        .flat_map(|rule| rule.scanned_spans(&input, None))
                        .any(|scan| !scan.rejected && scan.span.contains(&private)),
                    "{input:?}"
                );
                checked += 1;
                input = serde_json::to_string(&input).unwrap();
            }
        }
        assert_eq!(checked, 85);
    }

    #[test]
    fn json_label_regions_preserve_owned_bytes_at_every_depth() {
        let rules = super::regression_tests::rules();
        let mut checked = 0;
        for key in [
            "Customer ID:",
            "Customer ID: ",
            "Customer ID:\t",
            "Customer ID:\n",
            "Employee_ID=",
            "Kundennummer:",
            "member_id:",
        ] {
            for value in [
                r#""SYN731284""#,
                "731284",
                "-731284",
                "731284.5",
                "731284e2",
                r#"["SYN731284"]"#,
                r#"{"value":"SYN731284"}"#,
            ] {
                let body = format!("{{{}:{value}}}", serde_json::to_string(key).unwrap());
                for container in [
                    body.clone(),
                    format!("[{body}]"),
                    format!("{{\"outer\":{body}}}"),
                    format!("{{\"rows\":[[{body}]]}}"),
                ] {
                    for mut input in [
                        container.clone(),
                        format!("{{\"n\":0}}\n{container}"),
                        format!("tool output: {container}"),
                        format!("{container} trailing text"),
                        format!("```json\n{container}\n```"),
                    ] {
                        for depth in 0..=4 {
                            let scans: Vec<_> = rules
                                .iter()
                                .flat_map(|rule| rule.scanned_spans(&input, None))
                                .collect();
                            let private = input.find("731284").unwrap();
                            assert!(
                                scans.iter().any(|scan| !scan.rejected
                                    && scan.span.start <= private
                                    && scan.span.end >= private + 6),
                                "depth {depth}: {input}: {scans:?}"
                            );
                            checked += 1;
                            input = serde_json::to_string(&input).unwrap();
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 4900);
    }

    #[test]
    fn object_value_close_does_not_own_sibling_fields_or_json_keywords() {
        let rules = super::regression_tests::rules();
        for body in [
            r#"{"note":"customerId: ","unrelated":"PUBLIC-000123"}"#,
            r#"{"customerId:":null}"#,
            r#"{"customerId:":true}"#,
            r#"{"customerId:":false}"#,
        ] {
            let mut input = body.to_owned();
            for _depth in 0..=3 {
                let scans: Vec<_> = rules
                    .iter()
                    .flat_map(|rule| rule.scanned_spans(&input, None))
                    .collect();
                assert!(!scans.is_empty(), "{input}");
                assert!(scans.iter().all(|scan| scan.rejected), "{input}: {scans:?}");
                input = serde_json::to_string(&input).unwrap();
            }
        }
    }

    #[test]
    fn escaped_prose_label_index_owns_the_value_cell() {
        let rules = super::regression_tests::rules();
        let input = r#"tool {\"Customer ID:\":\"C-731284\"}"#;
        let captures = rules[0].regex.captures(input).unwrap();
        let capture = rules[0].span_from_captures(&captures).unwrap();
        let decisions = ScalarBoundaryIndex::default()
            .scan_json_label_cells(input, capture)
            .expect("escaped prose JSON needs an ownership decision");
        assert_eq!(decisions.len(), 1);
        assert!(!decisions[0].rejected);
        assert_eq!(&input[decisions[0].span.clone()], "C-731284");
    }

    #[test]
    fn array_header_label_is_not_a_private_value() {
        let rules = super::regression_tests::rules();
        let mut input = r#"[["Customer ID:","Order ID:"],["PUBLIC-COL-1","O-1"]]"#.to_owned();
        for _depth in 0..=4 {
            // Header ownership is rejected before the independent value-shape floor.
            for rule in &rules {
                let mut index = ScalarBoundaryIndex::default();
                for captures in rule.regex.captures_iter(&input) {
                    let capture = rule.span_from_captures(&captures).unwrap();
                    let decision = scan_field_owned_value_indexed(&input, capture, &mut index);
                    assert!(decision.rejected, "header owned: {input}: {decision:?}");
                }
            }
            let scans: Vec<_> = rules
                .iter()
                .flat_map(|rule| rule.scanned_spans(&input, None))
                .collect();
            assert!(scans.iter().all(|scan| scan.rejected), "{input}: {scans:?}");
            input = serde_json::to_string(&input).unwrap();
        }
    }

    #[test]
    fn json_label_close_owns_following_value() {
        let rules = super::regression_tests::rules();
        let fixtures: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/record_id_json_label_cells.json"
        ))
        .unwrap();
        for fixture in fixtures.as_array().unwrap() {
            if fixture["unchanged"] == true {
                continue;
            }
            let mut input = fixture["text"].as_str().unwrap().to_owned();
            let mut expected = fixture["owned"].as_str().unwrap().to_owned();
            for depth in 0..=4 {
                let scans: Vec<_> = rules
                    .iter()
                    .flat_map(|rule| rule.scanned_spans(&input, None))
                    .collect();
                assert!(!scans.is_empty(), "{input}");
                assert!(
                    scans
                        .iter()
                        .any(|scan| !scan.rejected && input[scan.span.clone()] == expected),
                    "depth {depth}: {input:?}: {scans:?}, expected {expected:?}"
                );
                input = serde_json::to_string(&input).unwrap();
                let encoded = serde_json::to_string(&expected).unwrap();
                expected = encoded[1..encoded.len() - 1].to_owned();
            }
        }
    }
}
