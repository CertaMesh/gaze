//! Nym-small decoding: chunk plan, per-piece labels, word-aligned spans, byte offsets.
//!
//! The backend runs the model over overlapping windows of tokenizer pieces, keeps one
//! probability row per piece, and hands the rows plus the tokenizer's CHARACTER offsets to
//! [`decode_pieces`]. Everything here is pure so it can be tested without the model.

use std::ops::Range;

use gaze_types::nym::{NymLabel, NymOperatingPoint};
use gaze_types::SafetyNetError;

use crate::safety_net::word_pieces::group_word_pieces;

/// `O` plus `B-`/`I-` for each of the 40 labels.
pub(crate) const NUM_LABELS: usize = 1 + 2 * 40;
/// Content pieces per model call, excluding `<bos>`/`<eos>`.
pub(crate) const WINDOW: usize = 512;
/// Pieces shared by consecutive windows.
pub(crate) const OVERLAP: usize = 64;

/// One decoded suspect span in byte offsets of the checked text.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NymSpan {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) label: NymLabel,
    pub(crate) score: f32,
}

/// Piece-index windows that together cover `0..piece_count`.
pub(crate) fn plan_windows(piece_count: usize) -> Vec<Range<usize>> {
    let mut windows = Vec::new();
    let mut start = 0;
    while start < piece_count {
        let end = (start + WINDOW).min(piece_count);
        windows.push(start..end);
        if end == piece_count {
            break;
        }
        start = end - OVERLAP;
    }
    windows
}

/// Merges per-window probability rows into one row per piece.
///
/// A piece seen by several windows keeps the row from the window where it sits furthest from an
/// edge (earliest window on a tie), because context on both sides is what the overlap buys.
/// Every piece must end up with a row; a piece no window scored is a typed error, never a silent
/// gap in the scan.
pub(crate) struct RowMerger {
    rows: Vec<Option<(isize, [f32; NUM_LABELS])>>,
}

impl RowMerger {
    pub(crate) fn new(piece_count: usize) -> Self {
        Self {
            rows: vec![None; piece_count],
        }
    }

    /// Records the probability rows of one window. `probs[j]` belongs to piece
    /// `window.start + j`.
    pub(crate) fn add_window(&mut self, window: Range<usize>, probs: &[[f32; NUM_LABELS]]) {
        let len = window.len();
        let single = window.start == 0 && window.end == self.rows.len();
        for (j, row) in probs.iter().enumerate().take(len) {
            let centrality = if single {
                isize::MAX
            } else {
                j.min(len - 1 - j) as isize
            };
            let slot = &mut self.rows[window.start + j];
            if slot.is_none_or(|(best, _)| centrality > best) {
                *slot = Some((centrality, *row));
            }
        }
    }

    pub(crate) fn finish(self) -> Result<Vec<[f32; NUM_LABELS]>, SafetyNetError> {
        self.rows
            .into_iter()
            .map(|row| row.map(|(_, probs)| probs))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| SafetyNetError::InvalidOutput {
                message: "nym chunking left a piece unscored".to_string(),
            })
    }
}

/// Softmax of one logit row, computed like the probe (shift by the max for stability).
pub(crate) fn softmax_row(logits: &[f32]) -> Result<[f32; NUM_LABELS], SafetyNetError> {
    if logits.len() != NUM_LABELS || logits.iter().any(|value| !value.is_finite()) {
        return Err(SafetyNetError::InvalidOutput {
            message: "nym returned invalid logits".to_string(),
        });
    }
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut out = [0.0f32; NUM_LABELS];
    let mut sum = 0.0f32;
    for (slot, value) in out.iter_mut().zip(logits) {
        *slot = (value - max).exp();
        sum += *slot;
    }
    for slot in &mut out {
        *slot /= sum;
    }
    Ok(out)
}

/// Checks that the tokenizer pieces cover every non-whitespace character of `chars`.
pub(crate) fn check_char_coverage(
    chars: &[char],
    char_offsets: &[(usize, usize)],
) -> Result<(), SafetyNetError> {
    let mut covered = vec![false; chars.len()];
    for &(start, end) in char_offsets {
        if start > end || end > chars.len() {
            return Err(SafetyNetError::InvalidOutput {
                message: "nym tokenizer returned out-of-bounds offsets".to_string(),
            });
        }
        covered[start..end].iter_mut().for_each(|slot| *slot = true);
    }
    if chars
        .iter()
        .zip(&covered)
        .any(|(ch, covered)| !covered && !ch.is_whitespace())
    {
        return Err(SafetyNetError::InvalidOutput {
            message: "nym tokenizer left input uncovered".to_string(),
        });
    }
    Ok(())
}

/// Byte offset of every character index in `text`, plus the end.
pub(crate) fn char_to_byte_table(text: &str) -> Vec<usize> {
    text.char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(text.len()))
        .collect()
}

/// The per-piece model verdict the decoder needs: the label with the largest entity mass, that
/// mass, and whether `B-` outweighs `I-` for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PieceScore {
    pub label: NymLabel,
    pub mass: f32,
    pub is_begin: bool,
}

impl PieceScore {
    /// Reads one probability row. The entity mass of a label is `P(B-label) + P(I-label)`; the
    /// argmax runs over all 40 labels (first on a tie), so a piece that looks most like a disabled
    /// label is never relabelled into an enabled one.
    pub(crate) fn from_row(row: &[f32; NUM_LABELS]) -> Self {
        let mut best = (0usize, f32::NEG_INFINITY);
        for index in 0..NymLabel::ALL.len() {
            let mass = row[2 * index + 1] + row[2 * index + 2];
            if mass > best.1 {
                best = (index, mass);
            }
        }
        let (index, mass) = best;
        Self {
            label: NymLabel::ALL[index],
            mass,
            is_begin: row[2 * index + 1] >= row[2 * index + 2],
        }
    }
}

/// A scored piece in byte offsets.
#[derive(Debug, Clone, Copy)]
struct Piece {
    start: usize,
    end: usize,
    score: PieceScore,
}

/// Decodes per-piece scores into word-aligned suspect spans.
///
/// * `char_offsets[k]` is piece `k`'s CHARACTER range in `text` (the tokenizer's char offsets);
///   they are trimmed of whitespace and converted to byte offsets here.
/// * A piece counts only if its label ([`PieceScore::from_row`]) is enabled and its mass reaches
///   the label's threshold.
/// * Spans are assembled from whole words: any counted piece labels its word, the strongest
///   piece picks the label, the score is the minimum over the pieces carrying that label, and a
///   word whose first counted piece is `I-` extends an open entity of the same label.
pub(crate) fn decode_pieces(
    text: &str,
    char_offsets: &[(usize, usize)],
    scores: &[PieceScore],
    operating_point: &NymOperatingPoint,
) -> Result<Vec<NymSpan>, SafetyNetError> {
    if char_offsets.len() != scores.len() {
        return Err(SafetyNetError::InvalidOutput {
            message: "nym returned mismatched piece offsets".to_string(),
        });
    }
    let chars = text.chars().collect::<Vec<_>>();
    let bytes = char_to_byte_table(text);

    let mut pieces: Vec<Piece> = Vec::with_capacity(scores.len());
    for (&(mut start, mut end), score) in char_offsets.iter().zip(scores) {
        if start > end || end > chars.len() {
            return Err(SafetyNetError::InvalidOutput {
                message: "nym tokenizer returned out-of-bounds offsets".to_string(),
            });
        }
        // Metaspace pieces carry the space they replaced; the span is the visible part.
        while start < end && chars[start].is_whitespace() {
            start += 1;
        }
        while end > start && chars[end - 1].is_whitespace() {
            end -= 1;
        }
        if start >= end {
            continue;
        }
        let piece = Piece {
            start: bytes[start],
            end: bytes[end],
            score: *score,
        };
        // Byte-fallback pieces of one character share its offsets; keep the strongest.
        match pieces.last_mut() {
            Some(last) if last.start == piece.start && last.end == piece.end => {
                if piece.score.mass > last.score.mass {
                    *last = piece;
                }
            }
            _ => pieces.push(piece),
        }
    }

    let spans = pieces
        .iter()
        .map(|piece| (piece.start, piece.end))
        .collect::<Vec<_>>();
    let mut out = Vec::new();
    let mut open: Option<NymSpan> = None;
    for word in group_word_pieces(text, &spans) {
        let counted = word
            .iter()
            .map(|&index| pieces[index].score)
            .filter(|score| {
                operating_point
                    .threshold(score.label)
                    .is_some_and(|threshold| score.mass >= threshold)
            })
            .collect::<Vec<_>>();
        let Some(first) = counted.first() else {
            out.extend(open.take());
            continue;
        };
        let best = counted.iter().fold(*first, |best, score| {
            if score.mass > best.mass {
                *score
            } else {
                best
            }
        });
        let current = NymSpan {
            start: pieces[word[0]].start,
            end: pieces[word[word.len() - 1]].end,
            label: best.label,
            score: counted
                .iter()
                .filter(|score| score.label == best.label)
                .map(|score| score.mass)
                .fold(f32::INFINITY, f32::min),
        };
        match open.as_mut() {
            Some(span) if !first.is_begin && span.label == current.label => {
                span.end = current.end;
                span.score = span.score.min(current.score);
            }
            _ => out.extend(open.replace(current)),
        }
    }
    out.extend(open);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A probability row where `label` has B/I mass `(b, i)` and `O` holds the rest.
    fn row(label: Option<NymLabel>, b: f32, i: f32) -> [f32; NUM_LABELS] {
        let mut out = [0.0; NUM_LABELS];
        match label {
            Some(label) => {
                let index = NymLabel::ALL.iter().position(|l| *l == label).unwrap();
                out[2 * index + 1] = b;
                out[2 * index + 2] = i;
                out[0] = 1.0 - b - i;
            }
            None => out[0] = 1.0,
        }
        out
    }

    fn scores(rows: &[[f32; NUM_LABELS]]) -> Vec<PieceScore> {
        rows.iter().map(PieceScore::from_row).collect()
    }

    fn o() -> [f32; NUM_LABELS] {
        row(None, 0.0, 0.0)
    }

    fn char_range(text: &str, needle: &str) -> (usize, usize) {
        let byte = text.find(needle).unwrap();
        let start = text[..byte].chars().count();
        (start, start + needle.chars().count())
    }

    fn texts(text: &str, spans: &[NymSpan]) -> Vec<String> {
        spans
            .iter()
            .map(|span| text[span.start..span.end].to_string())
            .collect()
    }

    /// One piece per non-space run, with the preceding whitespace on its front like a metaspace
    /// offset.
    fn pieces(text: &str) -> Vec<(usize, usize)> {
        let chars = text.chars().collect::<Vec<_>>();
        let mut out = Vec::new();
        let mut start = 0;
        while start < chars.len() {
            let mut end = start;
            while end < chars.len() && chars[end].is_whitespace() {
                end += 1;
            }
            while end < chars.len() && !chars[end].is_whitespace() {
                end += 1;
            }
            out.push((start, end));
            start = end;
        }
        out
    }

    fn labelled(
        offsets: &[(usize, usize)],
        target: (usize, usize),
        label: NymLabel,
        mass: f32,
    ) -> Vec<[f32; NUM_LABELS]> {
        offsets
            .iter()
            .map(|&(s, e)| {
                if s < target.1 && target.0 < e {
                    let first = s <= target.0;
                    if first {
                        row(Some(label), mass, 0.0)
                    } else {
                        row(Some(label), 0.0, mass)
                    }
                } else {
                    o()
                }
            })
            .collect()
    }

    #[test]
    fn chunk_plan_covers_every_piece_with_overlap() {
        assert!(plan_windows(0).is_empty());
        assert_eq!(plan_windows(10), vec![0..10]);
        assert_eq!(plan_windows(WINDOW), vec![0..WINDOW]);
        assert_eq!(plan_windows(WINDOW + 1), vec![0..512, 448..513]);
        for count in [1, 511, 512, 513, 960, 961, 1500, 5000] {
            let windows = plan_windows(count);
            assert_eq!(windows.first().unwrap().start, 0);
            assert_eq!(windows.last().unwrap().end, count, "count {count}");
            for pair in windows.windows(2) {
                assert_eq!(pair[0].end - pair[1].start, OVERLAP);
            }
            let mut merger = RowMerger::new(count);
            for window in &windows {
                merger.add_window(window.clone(), &vec![o(); window.len()]);
            }
            assert_eq!(merger.finish().unwrap().len(), count);
        }
    }

    #[test]
    fn an_unscored_piece_is_a_typed_error() {
        let mut merger = RowMerger::new(600);
        merger.add_window(0..512, &vec![o(); 512]);
        assert!(matches!(
            merger.finish(),
            Err(SafetyNetError::InvalidOutput { .. })
        ));
    }

    #[test]
    fn overlap_keeps_the_most_central_window() {
        let windows = plan_windows(600);
        let mut merger = RowMerger::new(600);
        let first = row(Some(NymLabel::Username), 0.9, 0.0);
        let second = row(Some(NymLabel::LicensePlate), 0.9, 0.0);
        merger.add_window(windows[0].clone(), &vec![first; windows[0].len()]);
        merger.add_window(windows[1].clone(), &vec![second; windows[1].len()]);
        let rows = merger.finish().unwrap();
        // Window 2 is 88..600. Piece 100 is 12 from its start but 411 from window 1's end.
        assert_eq!(rows[100], first);
        // Piece 500: 11 from window 1's end, 412 from window 2's start.
        assert_eq!(rows[500], second);
    }

    #[test]
    fn coverage_rejects_a_dropped_character() {
        let chars = "ab cd".chars().collect::<Vec<_>>();
        check_char_coverage(&chars, &[(0, 2), (2, 5)]).unwrap();
        check_char_coverage(&chars, &[(0, 2), (3, 5)]).unwrap();
        assert!(check_char_coverage(&chars, &[(0, 2), (3, 4)]).is_err());
        assert!(check_char_coverage(&chars, &[(0, 9)]).is_err());
    }

    #[test]
    fn argmax_runs_over_all_labels_and_threshold_gates() {
        let text = "Nutzer anna84 hier";
        let offsets = pieces(text);
        let op = NymOperatingPoint::op_b();

        // USERNAME mass 0.6 >= 0.5: a suspect.
        let rows = labelled(&offsets, char_range(text, "anna84"), NymLabel::Username, 0.6);
        let spans = decode_pieces(text, &offsets, &scores(&rows), &op).unwrap();
        assert_eq!(texts(text, &spans), vec!["anna84"]);
        assert_eq!(spans[0].label, NymLabel::Username);
        assert!((spans[0].score - 0.6).abs() < 1e-6);

        // Below threshold: nothing.
        let rows = labelled(&offsets, char_range(text, "anna84"), NymLabel::Username, 0.4);
        assert!(decode_pieces(text, &offsets, &scores(&rows), &op).unwrap().is_empty());

        // GIVEN_NAME outweighs USERNAME on the piece: disabled label wins the argmax, no suspect.
        let mut rows = labelled(&offsets, char_range(text, "anna84"), NymLabel::Username, 0.3);
        let target = offsets
            .iter()
            .position(|&(s, e)| s < 13 && 7 < e)
            .unwrap();
        let index = NymLabel::ALL
            .iter()
            .position(|l| *l == NymLabel::GivenName)
            .unwrap();
        rows[target][2 * index + 1] = 0.6;
        rows[target][0] = 0.1;
        assert!(decode_pieces(text, &offsets, &scores(&rows), &op).unwrap().is_empty());

        // Same shape with USERNAME enabled at 0.3: its 0.35 clears the threshold, but GIVEN_NAME
        // (0.55) is the piece's label, so it still produces nothing. Picking the argmax among
        // enabled labels only would emit a USERNAME suspect here.
        let low = NymOperatingPoint::new([(NymLabel::Username, 0.3)]).unwrap();
        let mut rows = labelled(&offsets, char_range(text, "anna84"), NymLabel::Username, 0.35);
        rows[target][2 * index + 1] = 0.55;
        rows[target][0] = 0.1;
        assert!(decode_pieces(text, &offsets, &scores(&rows), &low)
            .unwrap()
            .is_empty());
        rows[target][2 * index + 1] = 0.0;
        rows[target][0] = 0.65;
        assert_eq!(
            texts(text, &decode_pieces(text, &offsets, &scores(&rows), &low).unwrap()),
            vec!["anna84"]
        );

        // DATE_OF_BIRTH needs 0.9 under op-B.
        let rows = labelled(&offsets, char_range(text, "anna84"), NymLabel::DateOfBirth, 0.85);
        assert!(decode_pieces(text, &offsets, &scores(&rows), &op).unwrap().is_empty());
    }

    #[test]
    fn op_b_never_enables_a_name_label() {
        let text = "Hallo Anna Schmidt";
        let offsets = pieces(text);
        let op = NymOperatingPoint::default();
        for label in [NymLabel::GivenName, NymLabel::Surname, NymLabel::City, NymLabel::ZipCode, NymLabel::TaxId] {
            let rows = labelled(&offsets, char_range(text, "Anna"), label, 0.99);
            assert!(
                decode_pieces(text, &offsets, &scores(&rows), &op).unwrap().is_empty(),
                "{label} fired under the default operating point"
            );
        }
    }

    #[test]
    fn a_partial_word_piece_expands_to_the_whole_word() {
        let text = "Wert abc12345xyz Ende";
        // Word cut into three touching pieces; only the middle one fires.
        let (s, _) = char_range(text, "abc12345xyz");
        let offsets = vec![(0, 4), (4, s + 3), (s + 3, s + 8), (s + 8, s + 11), (s + 11, s + 16)];
        let mut rows = vec![o(); offsets.len()];
        rows[2] = row(Some(NymLabel::Username), 0.0, 0.95);
        let spans = decode_pieces(text, &offsets, &scores(&rows), &NymOperatingPoint::op_b()).unwrap();
        assert_eq!(texts(text, &spans), vec!["abc12345xyz"]);
    }

    #[test]
    fn word_rule_keeps_separate_words_apart() {
        // `-` is not a word character, so `AB` `-` `CD` are three words; a B- on each splits them,
        // I- continuations join them into one plate.
        let text = "Kennzeichen AB-CD 1234 ok";
        let (s, _) = char_range(text, "AB-CD 1234");
        let offsets = vec![(0, 11), (11, s + 2), (s + 2, s + 3), (s + 3, s + 5), (s + 5, s + 10), (s + 10, s + 13)];
        let mut rows = vec![o(); offsets.len()];
        rows[1] = row(Some(NymLabel::LicensePlate), 0.9, 0.0);
        rows[2] = row(Some(NymLabel::LicensePlate), 0.0, 0.9);
        rows[3] = row(Some(NymLabel::LicensePlate), 0.0, 0.8);
        rows[4] = row(Some(NymLabel::LicensePlate), 0.0, 0.7);
        let spans = decode_pieces(text, &offsets, &scores(&rows), &NymOperatingPoint::op_b()).unwrap();
        assert_eq!(texts(text, &spans), vec!["AB-CD 1234"]);
        assert!((spans[0].score - 0.7).abs() < 1e-6);

        rows[3] = row(Some(NymLabel::LicensePlate), 0.8, 0.0);
        let spans = decode_pieces(text, &offsets, &scores(&rows), &NymOperatingPoint::op_b()).unwrap();
        assert_eq!(texts(text, &spans), vec!["AB-", "CD 1234"]);
    }

    /// Offsets are characters: every fixture puts multibyte text before or inside the target so
    /// reading them as bytes lands on the wrong bytes or a non-boundary.
    #[test]
    fn char_offsets_become_byte_offsets_on_multibyte_text() {
        let nfd = "Ko\u{308}nigstraße A\u{308}rger";
        let cases: Vec<(String, &str, NymLabel)> = vec![
            ("Grüße aus Überlingen, Kennung anna.schmidt84 läuft.".into(), "anna.schmidt84", NymLabel::Username),
            (format!("{nfd} Kennung jdoe_1977 öffnet"), "jdoe_1977", NymLabel::Username),
            ("Kennung mu\u{308}ller_x9 ende".into(), "mu\u{308}ller_x9", NymLabel::Username),
            ("🙂👍🏽 Kennzeichen M-AB 1234 folgt".into(), "M-AB 1234", NymLabel::LicensePlate),
            ("Tag 🙂 AB-CD 1234 🚗 ok".into(), "AB-CD 1234", NymLabel::LicensePlate),
            ("Hausnummer\u{a0}12a Ort".into(), "12a", NymLabel::BuildingNumber),
            ("Code 12\u{202f}345 Ende".into(), "12\u{202f}345", NymLabel::BuildingNumber),
        ];
        for (text, target, label) in cases {
            // One piece per non-space run, with the separator kept on the NEXT piece's front
            // like a metaspace offset; NBSP and NARROW NBSP are whitespace and are trimmed.
            let offsets = pieces(&text);
            let (s, e) = char_range(&text, target);
            let rows = offsets
                .iter()
                .map(|&(ps, pe)| {
                    if ps < e && s < pe {
                        let lead = text
                            .chars()
                            .skip(ps)
                            .take(pe - ps)
                            .take_while(|c| c.is_whitespace())
                            .count();
                        let begins = ps + lead <= s;
                        if begins {
                            row(Some(label), 0.95, 0.0)
                        } else {
                            row(Some(label), 0.0, 0.95)
                        }
                    } else {
                        o()
                    }
                })
                .collect::<Vec<_>>();
            let spans = decode_pieces(&text, &offsets, &scores(&rows), &NymOperatingPoint::op_b()).unwrap();
            assert_eq!(texts(&text, &spans), vec![target.to_string()], "{text:?}");
            for span in &spans {
                assert!(text.is_char_boundary(span.start) && text.is_char_boundary(span.end));
                assert!(!gaze_types::is_inside_word(&text, span.start));
                assert!(!gaze_types::is_inside_word(&text, span.end));
            }
        }
    }

    #[test]
    fn byte_fallback_pieces_of_one_character_collapse() {
        let text = "x 🚗 y";
        // The emoji is split into two byte-fallback pieces with identical char offsets.
        let offsets = vec![(0, 1), (1, 3), (2, 3), (3, 5)];
        let mut rows = vec![o(); 4];
        rows[1] = row(Some(NymLabel::Username), 0.2, 0.0);
        rows[2] = row(Some(NymLabel::Username), 0.7, 0.0);
        let spans = decode_pieces(text, &offsets, &scores(&rows), &NymOperatingPoint::op_b()).unwrap();
        assert_eq!(texts(text, &spans), vec!["🚗"]);
        assert!((spans[0].score - 0.7).abs() < 1e-6);
    }

    #[test]
    fn malformed_model_output_is_a_typed_error() {
        let op = NymOperatingPoint::op_b();
        assert!(decode_pieces("ab", &[(0, 2)], &[], &op).is_err());
        assert!(decode_pieces("ab", &[(0, 3)], &scores(&[o()]), &op).is_err());
        assert!(softmax_row(&[0.0; 80]).is_err());
        let mut bad = [0.0; NUM_LABELS];
        bad[3] = f32::NAN;
        assert!(softmax_row(&bad).is_err());
        let probs = softmax_row(&[0.0; NUM_LABELS]).unwrap();
        assert!((probs.iter().sum::<f32>() - 1.0).abs() < 1e-4);
    }
}
