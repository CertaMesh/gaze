use std::ops::Range;

use regex::Regex;

use gaze::Session;

use crate::error::{CliError, RestoreMode, RestoreWarning};

/// Pass 1 — exact-literal alternation built from `session.tokens()`.
///
/// Sorts tokens longest-first so a format-preserved email like
/// `email1.<session>@gaze-fake.invalid` wins over a substring match like `<Email_1>`. Bare
/// format-preserving tokens stay wrapped in `\b` word boundaries so a token
/// cannot be swallowed inside an adjacent identifier (the
/// `hostName_1s-record` regression in `docs/roadmap/v0.3/cli.md` §"Test
/// strategy" #5). Wrapped counter tokens intentionally skip `\b`: `<` and
/// `>` are explicit delimiters, and `\b` would miss `See <Email_1>.` because
/// it does not fire across non-word characters. Empty session map is a no-op:
/// `Regex::new("")` would match everywhere, so short-circuit.
pub(crate) struct RestorePass1 {
    pub(crate) text: String,
    pub(crate) substitution_spans: Vec<Range<usize>>,
}

pub(crate) fn restore_pass1(
    session: &Session,
    text: &str,
) -> std::result::Result<RestorePass1, CliError> {
    let mut tokens = session.tokens();
    if tokens.is_empty() {
        return Ok(RestorePass1 {
            text: text.to_string(),
            substitution_spans: Vec::new(),
        });
    }
    tokens.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));

    let pattern = tokens
        .iter()
        .map(|token| {
            let escaped = regex::escape(token);
            if token.starts_with('<') && token.ends_with('>') {
                escaped
            } else {
                format!(r"\b(?:{escaped})\b")
            }
        })
        .collect::<Vec<_>>()
        .join("|");
    let re = Regex::new(&pattern).map_err(|_| CliError::Pipeline)?;

    let mut out = String::with_capacity(text.len());
    let mut substitution_spans = Vec::new();
    let mut last = 0usize;
    for m in re.find_iter(text) {
        out.push_str(&text[last..m.start()]);
        let real = session
            .restore_strict(m.as_str())
            .map_err(|_| CliError::Pipeline)?;
        let substitution_start = out.len();
        out.push_str(&real);
        substitution_spans.push(substitution_start..out.len());
        last = m.end();
    }
    out.push_str(&text[last..]);
    Ok(RestorePass1 {
        text: out,
        substitution_spans,
    })
}

/// Pass 2 — shape-validator over Pass-1 output.
///
/// Any remaining token-shaped substring means the LLM invented a token the
/// session never emitted → `UnknownToken`. The canonical grammar lives in
/// `gaze::token_shape`, so the CLI no longer re-encodes token shapes locally.
pub(crate) fn restore_pass2_validate(
    text: &str,
    substitution_spans: &[Range<usize>],
    session: &Session,
    mode: RestoreMode,
) -> std::result::Result<Vec<RestoreWarning>, CliError> {
    let mut warnings = Vec::new();
    let mut substitution_cursor = 0usize;
    for matched in gaze::token_shape::pattern().find_iter(text) {
        if is_inside_substitution_span(
            matched.start(),
            matched.end(),
            substitution_spans,
            &mut substitution_cursor,
        ) {
            continue;
        }
        let matched_text = matched.as_str();
        if gaze::token_shape::is_trap(matched_text) {
            match mode {
                RestoreMode::Strict => {
                    return Err(CliError::UnknownToken {
                        token: matched_text.to_string(),
                    })
                }
                RestoreMode::Tolerant => warnings.push(RestoreWarning {
                    variant: "UnknownToken".to_string(),
                    token: matched_text.to_string(),
                }),
            }
            continue;
        }
        if session.contains_token(matched_text) {
            continue;
        }
        match mode {
            RestoreMode::Strict => {
                return Err(CliError::UnknownToken {
                    token: matched_text.to_string(),
                })
            }
            RestoreMode::Tolerant => warnings.push(RestoreWarning {
                variant: "UnknownToken".to_string(),
                token: matched_text.to_string(),
            }),
        }
    }
    Ok(warnings)
}

fn is_inside_substitution_span(
    start: usize,
    end: usize,
    spans: &[Range<usize>],
    cursor: &mut usize,
) -> bool {
    while spans.get(*cursor).is_some_and(|span| span.end <= start) {
        *cursor += 1;
    }

    spans
        .get(*cursor)
        .is_some_and(|span| span.start <= start && end <= span.end)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use gaze::Scope;

    fn empty_session() -> Session {
        Session::new(Scope::Ephemeral).expect("session")
    }

    #[test]
    fn pass2_cursor_scan_handles_dense_substitution_spans() {
        let spans = (0..1_000)
            .map(|i| {
                let start = i * 16;
                start..start + 8
            })
            .collect::<Vec<_>>();
        let mut cursor = 0usize;
        let started = Instant::now();

        for (expected_cursor, span) in spans.iter().enumerate() {
            assert!(
                is_inside_substitution_span(span.start, span.end, &spans, &mut cursor),
                "span {expected_cursor} should be exempt"
            );
            assert_eq!(
                cursor, expected_cursor,
                "cursor must advance monotonically to the current span"
            );
        }

        assert!(
            started.elapsed() < Duration::from_millis(50),
            "dense cursor scan exceeded the regression budget"
        );
    }

    #[test]
    fn pass2_cursor_scan_handles_token_shaped_text_inside_substituted_span() {
        let session = empty_session();
        let text = "Order_42";
        let spans = std::iter::once(0..text.len()).collect::<Vec<_>>();
        let mut cursor = 0usize;

        assert!(is_inside_substitution_span(
            0,
            text.len(),
            &spans,
            &mut cursor
        ));
        assert_eq!(cursor, 0, "cursor stays on the containing span");
        let warnings = restore_pass2_validate(text, &spans, &session, RestoreMode::Strict).unwrap();

        assert!(warnings.is_empty());
    }

    #[test]
    fn pass2_cursor_scan_traps_adjacent_hallucinated_tokens_outside_substituted_span() {
        let session = empty_session();
        let text = "Alice_1<Email_999>";
        let spans = std::iter::once(0..7).collect::<Vec<_>>();
        let mut cursor = 0usize;

        assert!(is_inside_substitution_span(0, 7, &spans, &mut cursor));
        assert_eq!(cursor, 0);
        assert!(!is_inside_substitution_span(
            7,
            text.len(),
            &spans,
            &mut cursor
        ));
        assert_eq!(
            cursor, 1,
            "cursor must advance past the adjacent completed substitution span"
        );
        match restore_pass2_validate(text, &spans, &session, RestoreMode::Strict) {
            Err(CliError::UnknownToken { token }) => assert_eq!(token, "<Email_999>"),
            Err(other) => panic!("unexpected error: {other:?}"),
            Ok(_) => panic!("expected adjacent hallucinated token to fail"),
        }
    }

    #[test]
    fn pass2_cursor_scan_tolerant_mode_reports_first_unknown_token() {
        let session = empty_session();
        let text = "Alice_1 <Email_999> <Name_100>";
        let spans = std::iter::once(0..7).collect::<Vec<_>>();
        let mut cursor = 0usize;

        assert!(is_inside_substitution_span(0, 7, &spans, &mut cursor));
        assert!(!is_inside_substitution_span(8, 19, &spans, &mut cursor));
        assert_eq!(
            cursor, 1,
            "cursor remains advanced after leaving the substitution span"
        );
        let warnings =
            restore_pass2_validate(text, &spans, &session, RestoreMode::Tolerant).unwrap();

        assert_eq!(warnings[0].variant, "UnknownToken");
        assert_eq!(warnings[0].token, "<Email_999>");
    }
}
