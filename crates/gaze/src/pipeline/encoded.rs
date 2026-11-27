//! A second, read-only identifier view. Remove complete source spellings only in their
//! carrier's string/text tokens. Keep the primary view (and the repeat sweep) byte-for-byte
//! unchanged; candidates that cross a removed spelling rejoin its ordinary candidate pool.

use super::*;
use crate::normalize::NormalizedText;
use html5gum::{DefaultEmitter, State, Token, Tokenizer};
use std::sync::LazyLock;

fn invisible(cp: u32) -> bool {
    matches!(cp, 0xad | 0x200b..=0x200d | 0x2060 | 0xfeff)
}

struct Edit {
    source: Range<usize>,
    /// Invisible spellings disappear; JSON whitespace retains its lexical boundary.
    whitespace: Option<char>,
}

fn json_edits(raw: &str, literals: &JsonStringLiterals<'_>, edits: &mut Vec<Edit>) {
    for literal in literals.values() {
        let mut at = literal.start;
        while let Some(offset) = raw[at..literal.end].find('\\') {
            at += offset;
            // scan() already validated the JSON. Consuming both bytes of every short
            // escape is essential: a literal backslash followed by `u` isn't Unicode.
            let (width, scalar) = match raw.as_bytes()[at + 1] {
                b'u' => (
                    6,
                    char::from_u32(u32::from_str_radix(&raw[at + 2..at + 6], 16).unwrap()),
                ),
                b'n' => (2, Some('\n')),
                b'r' => (2, Some('\r')),
                b't' => (2, Some('\t')),
                b'f' => (2, Some('\x0c')),
                _ => (2, None),
            };
            if let Some(ch) = scalar.filter(|c| c.is_whitespace() || invisible(*c as u32)) {
                edits.push(Edit {
                    source: at..at + width,
                    whitespace: ch.is_whitespace().then_some(ch),
                });
            }
            at += width;
        }
    }
}

fn html_edits(raw: &str, edits: &mut Vec<Edit>) -> Result<()> {
    static REFERENCE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"&(?:#[xX]([0-9a-fA-F]+)|#([0-9]+)|ZeroWidthSpace|Negative(?:Medium|Thick|Thin|VeryThin)Space|NoBreak|shy|zwn?j);").unwrap()
    });
    let mut tokens = Tokenizer::new_with_emitter(raw, DefaultEmitter::<usize>::new_with_span());
    let mut references_enabled = false;
    while let Some(Ok(token)) = tokens.next() {
        match token {
            Token::StartTag(tag) => {
                let state = if tag.name.as_ref() == b"noframes" {
                    State::RawText
                } else {
                    html5gum::naive_next_state(&tag.name).unwrap_or(State::Data)
                };
                references_enabled = matches!(state, State::Data | State::RcData);
                tokens.set_state(state);
            }
            Token::EndTag(_) => references_enabled = true,
            Token::Doctype(tag) if tag.name.eq_ignore_ascii_case(b"html") => {
                references_enabled = true
            }
            Token::String(s) if references_enabled => {
                let source = raw
                    .get(s.span.start..s.span.end)
                    .ok_or_else(|| clean_to_raw_mapping_error("invalid HTML text geometry"))?;
                for cap in REFERENCE.captures_iter(source) {
                    let cp = cap
                        .get(1)
                        .map(|m| (m, 16))
                        .or_else(|| cap.get(2).map(|m| (m, 10)));
                    if cp.is_some_and(|(m, radix)| {
                        !u32::from_str_radix(m.as_str(), radix).is_ok_and(invisible)
                    }) {
                        continue;
                    }
                    let m = cap.get(0).unwrap();
                    edits.push(Edit {
                        source: s.span.start + m.start()..s.span.start + m.end(),
                        whitespace: None,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

struct View {
    text: String,
    raw_spans: Vec<(usize, usize)>,
    /// Each view byte owns a range in ordinary normalized text. Only a removed
    /// encoded invisible creates a gap; a contracted whitespace escape owns its full range.
    primary: Vec<(usize, usize)>,
}

impl View {
    fn new(
        raw: &str,
        normalized: &NormalizedText,
        literals: &JsonStringLiterals<'_>,
    ) -> Result<Option<Self>> {
        let mut edits = Vec::new();
        if raw.trim_start().starts_with('<') && raw.contains('&') {
            html_edits(raw, &mut edits)?;
        } else if raw.contains("\\u") {
            json_edits(raw, literals, &mut edits);
        }
        if !edits.iter().any(|edit| edit.whitespace.is_none()) {
            return Ok(None);
        }
        let mut view = Self {
            text: String::new(),
            raw_spans: Vec::new(),
            primary: Vec::new(),
        };
        let mut start = 0;
        for edit in edits {
            let end =
                start + normalized.spans[start..].partition_point(|&(s, _)| s < edit.source.start);
            view.text.push_str(&normalized.text[start..end]);
            view.raw_spans
                .extend_from_slice(&normalized.spans[start..end]);
            view.primary.extend((start..end).map(|i| (i, i + 1)));
            // Every source spelling is ASCII, unchanged by primary normalization.
            start = end + edit.source.len();
            if let Some(ch) = edit.whitespace {
                let folded = normalize(ch.encode_utf8(&mut [0; 4])).text;
                view.text.push_str(&folded);
                view.raw_spans.extend(std::iter::repeat_n(
                    (edit.source.start, edit.source.end),
                    folded.len(),
                ));
                view.primary
                    .extend(std::iter::repeat_n((end, start), folded.len()));
            }
        }
        view.text.push_str(&normalized.text[start..]);
        view.raw_spans.extend_from_slice(&normalized.spans[start..]);
        view.primary
            .extend((start..normalized.text.len()).map(|i| (i, i + 1)));
        Ok(Some(view))
    }

    fn crossing(&self, mut candidate: Candidate) -> Result<Option<Candidate>> {
        let mapping_error = || clean_to_raw_mapping_error("invalid encoded identifier geometry");
        let positions = self
            .primary
            .get(candidate.span.clone())
            .ok_or_else(mapping_error)?;
        let span = positions.first().ok_or_else(mapping_error)?.0
            ..positions.last().ok_or_else(mapping_error)?.1;
        if !positions.windows(2).any(|p| p[0].1 < p[1].0) {
            return Ok(None);
        }
        candidate.span = span;
        if let Some(end) = &mut candidate.labelled_value_capture_end {
            *end = self
                .primary
                .get(end.checked_sub(1).ok_or_else(mapping_error)?)
                .ok_or_else(mapping_error)?
                .1;
        }
        Ok(Some(candidate))
    }
}

impl Pipeline {
    pub(super) fn add_encoded_identifiers(
        &self,
        raw: &str,
        normalized: &NormalizedText,
        literals: &JsonStringLiterals<'_>,
        ctx: &DetectContext<'_>,
        pool: &mut crate::resolver::CandidatePool,
        vetoed: &mut Vec<crate::validator_veto::VetoedCandidate>,
    ) -> Result<()> {
        let Some(view) = View::new(raw, normalized, literals)? else {
            return Ok(());
        };
        let view_ctx = DetectContext::new(ctx.locale_chain, ctx.dictionaries)
            .with_source_spans(&view.raw_spans);
        let (mut extra, rejected) = self
            .registry
            .detect_encoded_identifiers(&view.text, &view_ctx)?;
        for mut row in rejected {
            if let Some(candidate) = view.crossing(row.candidate)? {
                row.candidate = candidate;
                vetoed.push(row);
            }
        }
        let mut added = extra
            .take_originals()
            .into_iter()
            .filter_map(|c| view.crossing(c).transpose())
            .peekable();
        if added.peek().is_some() {
            let mut candidates = pool.take_originals();
            for candidate in added {
                candidates.push(candidate?);
            }
            *pool = crate::resolver::CandidatePool::new(candidates);
        }
        Ok(())
    }
}
