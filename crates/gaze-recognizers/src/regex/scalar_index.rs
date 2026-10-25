use super::*;
use std::collections::HashMap;
use std::ops::Range;

/// One cache per recognizer invocation, shared by every field capture. Each decoded
/// JSON frame owns another cache; no state survives the input or crosses requests.
#[derive(Default)]
pub(super) struct ScalarBoundaryIndex {
    encoded: Option<EncodedIndex>,
    prefix: PrefixCursor,
    strings: HashMap<(usize, Option<usize>), StringEndIndex>,
    quotes: HashMap<(usize, u8, usize, ScalarQuoteDelimiter), QuoteCloseIndex>,
}

impl ScalarBoundaryIndex {
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
            *encoded
                .valid_json
                .get_or_insert_with(|| serde_json::from_str::<serde_json::Value>(input).is_ok())
                || input[close + 1..]
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
        if !field_wrapper_opens(opener) {
            let end = decoded
                .view
                .source
                .partition_point(|range| range.end < capture.end);
            let scan = decoded.index.scan_encoded_wrapper(
                &decoded.view.text,
                anchor..end.saturating_add(1).min(decoded.view.text.len()),
            )?;
            return Some(project_scan(scan, &decoded.view.source, &capture));
        }
        let start = opening + opener.len_utf8();
        let mut seed_end = decoded
            .view
            .source
            .partition_point(|range| range.end < capture.end)
            .saturating_add(1)
            .min(decoded.view.text.len())
            .max(start);
        while !decoded.view.text.is_char_boundary(seed_end) {
            seed_end += 1;
        }
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
            .entry((start, quote, escapes, delimiter))
            .or_insert_with(|| QuoteCloseIndex::new(input, start, quote, escapes, delimiter));
        index.close(input, limit, quote, escapes, delimiter)
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
        let mut state = if self.prefix.at <= prefix_end {
            self.prefix.clone()
        } else {
            PrefixCursor::default()
        };
        let mut stable = true;
        while state.at < prefix_end {
            scanner_work(1);
            let at = state.at;
            let byte = input.as_bytes()[at];
            if byte == b'\\' {
                state.escapes += 1;
                state.at += 1;
                if stable {
                    self.prefix = state.clone();
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
                    stable &= (prior_close.is_some() || limit < prefix_end)
                        && prior_close == full_close
                        && quote_run_end(input, opening, byte, escapes, input.len()) == run_end;
                    state.at = prior_close.map_or_else(
                        || uncertain_scalar_end(input, content, limit),
                        |close| close + count * width,
                    );
                    state.escapes = 0;
                    if stable {
                        self.prefix = state.clone();
                    }
                    continue;
                }
                // A query can cut a quote run before its grammar is known.
                stable &= quote_run_end(input, opening, byte, escapes, input.len()) == run_end;
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
            if stable {
                self.prefix = state.clone();
            }
        }
        let depth = state.enclosing.last().map(|(depth, _)| *depth);
        let absent_value = input.as_bytes()[start - 1] == b'"'
            && depth.is_some_and(|depth| opening_escapes <= depth)
            && serialized_context_is_json(input, &state.enclosing, opening_escapes);
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
    let bytes = input.as_bytes();
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
                    while at < bytes.len() && !matches!(bytes[at], b'\r' | b'\n') {
                        scanner_work(1);
                        at += 1;
                    }
                } else if bytes.get(at..at + 2) == Some(b"/*") {
                    at += 2;
                    while at < bytes.len() && bytes.get(at..at + 2) != Some(b"*/") {
                        scanner_work(1);
                        at += 1;
                    }
                    at += 2;
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

struct QuoteCloseIndex {
    runs: Vec<Range<usize>>,
    terminal: usize,
    close: Option<usize>,
}
impl QuoteCloseIndex {
    fn new(
        input: &str,
        start: usize,
        quote: u8,
        opening_escapes: usize,
        delimiter: ScalarQuoteDelimiter,
    ) -> Self {
        let width = opening_escapes + 1;
        let Some(stride) = width.checked_mul(2) else {
            return Self {
                runs: Vec::new(),
                terminal: input.len(),
                close: None,
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
            } else {
                if byte == quote {
                    let residue = escapes % stride;
                    if residue == opening_escapes {
                        let run_start = at - opening_escapes;
                        let run_end =
                            quote_run_end(input, run_start, quote, opening_escapes, input.len());
                        runs.push(run_start..run_end);
                        let run = (run_end - run_start) / width;
                        let terminal = match delimiter {
                            ScalarQuoteDelimiter::Single => run % 2 == 1,
                            ScalarQuoteDelimiter::DoubledSingle => run % 4 == 2,
                            ScalarQuoteDelimiter::DoubledTriple => {
                                !run.is_multiple_of(2) || run >= 6
                            }
                            ScalarQuoteDelimiter::Triple => run >= 3,
                        };
                        if terminal {
                            let close = scalar_quote_close(
                                input,
                                run_start,
                                run_end,
                                quote,
                                opening_escapes,
                                delimiter,
                            );
                            return Self {
                                runs,
                                terminal: run_end,
                                close,
                            };
                        }
                        at = run_end;
                        escapes = 0;
                        continue;
                    }
                    if residue != stride - 1 {
                        return Self {
                            runs,
                            terminal: at + 1,
                            close: None,
                        };
                    }
                }
                escapes = 0;
            }
            at += 1;
        }
        Self {
            runs,
            terminal: input.len(),
            close: None,
        }
    }
    fn close(
        &self,
        input: &str,
        limit: usize,
        quote: u8,
        escapes: usize,
        delimiter: ScalarQuoteDelimiter,
    ) -> Option<usize> {
        let at = self.runs.partition_point(|run| run.start < limit);
        if let Some(run) = at
            .checked_sub(1)
            .map(|at| &self.runs[at])
            .filter(|run| limit < run.end)
        {
            return scalar_quote_close(input, run.start, limit, quote, escapes, delimiter);
        }
        (self.terminal <= limit).then_some(self.close).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            ];
            for mut input in variants {
                for _depth in 0..=5 {
                    checked += compare(&input);
                    input = serde_json::to_string(&input).unwrap();
                }
            }
        }
        assert!(checked >= 5000, "{checked} captures");
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
        assert!(checked > 3000, "{checked} captures");
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
        assert!(checked > 10000, "{checked} captures");
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
        for surface in 0..5 {
            let mut previous = 0;
            for fields in [128, 512, 2048] {
                let body = "customerId: \"SYN731284\"; ".repeat(fields);
                let input = match surface {
                    0 => body,
                    1 => serde_json::to_string(&body).unwrap(),
                    2 => serde_json::to_string(&serde_json::to_string(&body).unwrap()).unwrap(),
                    3 => format!("\"{}\",public", body.replace('"', "\"\"")),
                    _ => format!("SELECT '{body}', 7;"),
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
                previous = steps;
            }
        }
    }
}
