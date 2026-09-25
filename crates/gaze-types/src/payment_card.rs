//! Payment-card numbers inside a run of digits.
//!
//! One implementation for both directions: the forward `card.structural` recognizer
//! (`gaze-recognizers`, the text sent to the model) and the restore-boundary DLP scan (`gaze`,
//! the text coming back). Solo todo 3843.
//!
//! A card is rarely alone in its digit run. A CVV or an expiry follows it (`4111 1111 1111 1111
//! 123`), a number precedes it (`Nr 7 4111 …`, `Order 5678 4111 …`), or normalization glued
//! digits onto it (a fullwidth group, a dropped ZERO WIDTH JOINER). The whole run then fails
//! Luhn, so [`scan_card_run`] also tries every group-aligned window written in a card layout.

use std::ops::Range;

/// The digit runs [`scan_card_run`] takes: digits joined by at most one whitespace or `-`
/// between two digits, starting and ending on a word boundary. Callers compile it (this crate
/// carries no regex engine) and pass each match. It is the card pattern
/// `\b\d(?:[\s-]?\d){12,18}\b` with no length bound, so every match of that pattern lies inside
/// one run.
pub const CARD_RUN_PATTERN: &str = r"\b\d(?:[\s-]?\d)*\b";

/// What [`scan_card_run`] found in one digit run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CardRunScan {
    /// Card numbers, as byte ranges of the text, in text order. They never overlap. Where
    /// Luhn-valid windows overlap, the range is their union, so it may hold digits around the
    /// card.
    pub cards: Vec<Range<usize>>,
    /// The windows the card pattern `\b\d(?:[\s-]?\d){12,18}\b` matches in the run that fail
    /// Luhn and overlap no card, in text order. The forward recognizer emits them unchanged so
    /// validator veto still records a Luhn failure for them.
    pub rejected: Vec<Range<usize>>,
}

/// The card numbers in `text[run]`, a match of [`CARD_RUN_PATTERN`].
///
/// Two kinds of window are card candidates when they pass Luhn:
///
/// 1. every window the card pattern `\b\d(?:[\s-]?\d){12,18}\b` matches (greedy, left to
///    right), whatever its grouping: the scan before the retry;
/// 2. every group-aligned window written in a card layout (compact 13 to 19 digits, 4-4-4-4,
///    4-4-4-4-3, 4-6-5, 4-6-4).
///
/// Overlapping candidates become one card spanning their union. Which of two overlapping
/// Luhn-valid windows is the card cannot be told from the digits (`0 4111 1111 1111 1111` passes
/// Luhn as a 17-digit window and as the 16-digit card; a random prefix makes such a window one
/// time in ten), so the scan fails closed and covers both: no digit of a Luhn-valid candidate is
/// left out. Every card found before the retry, and every card a single best window would find,
/// lies inside a union.
///
/// A random digit run passes Luhn one time in ten, so every extra window costs precision. The
/// retry never splits inside a group, and the layout filter keeps grouped amounts, timestamps
/// and phone numbers (groups of 3, 2 or 8 digits) out. A pattern window holds at most 19 digits
/// and a layout window at most five groups, so there are O(n) windows in a run of n bytes; the
/// union sorts them (O(n log n)) and the rejected windows are matched against the cards in one
/// sorted walk. A long run needs no length cap (pinned by
/// `the_rejected_walk_is_linear_in_its_inputs`).
///
/// A group ends at a whitespace or `-` separator and, when `source_spans` is given, wherever the
/// source text breaks between two digits: at a character normalization dropped (ZWJ, ZWNJ) and
/// where the source digits change width (fullwidth next to ASCII). `source_spans[i]` is the
/// source byte range that byte `i` of `text` came from, as the normalized detection view
/// records it; without it, touching digits are one group.
///
/// A range outside `text`, or one not on character boundaries, finds nothing.
pub fn scan_card_run(
    text: &str,
    run: Range<usize>,
    source_spans: Option<&[(usize, usize)]>,
) -> CardRunScan {
    let Some(run_text) = text.get(run.clone()) else {
        return CardRunScan::default();
    };
    let mut candidates = Vec::new();
    let mut failed = Vec::new();
    for window in pattern_windows(run_text, run.start) {
        if crate::luhn_check(&text[window.clone()]) {
            candidates.push(window);
        } else {
            failed.push(window);
        }
    }

    let groups = digit_groups(run_text, run.start, source_spans);
    let widths: Vec<usize> = groups
        .iter()
        .map(|group| text[group.clone()].chars().count())
        .collect();
    for first in 0..groups.len() {
        for last in first..groups.len().min(first + MAX_LAYOUT_GROUPS) {
            if !is_card_layout(&widths[first..=last]) {
                continue;
            }
            let window = groups[first].start..groups[last].end;
            if crate::luhn_check(&text[window.clone()]) {
                candidates.push(window);
            }
        }
    }

    let cards = union_of_overlapping(candidates);
    let (rejected, _) = without_overlap(failed, &cards);
    CardRunScan { cards, rejected }
}

/// The `windows` that overlap no card. Both lists are sorted by start and the cards do not
/// overlap each other, so one forward walk decides every window: cards that end at or before a
/// window's start cannot overlap it or any later window.
///
/// Also returns the work done, counted in card comparisons: at most one per card passed over
/// plus one per window, so never more than `windows.len() + cards.len()`.
/// `the_rejected_walk_is_linear_in_its_inputs` pins that bound.
fn without_overlap(
    windows: Vec<Range<usize>>,
    cards: &[Range<usize>],
) -> (Vec<Range<usize>>, usize) {
    debug_assert!(windows.is_sorted_by_key(|window| window.start));
    debug_assert!(cards.is_sorted_by_key(|card| card.start));
    let mut comparisons = 0;
    let mut next_card = 0;
    let kept = windows
        .into_iter()
        .filter(|window| {
            while cards.get(next_card).is_some_and(|card| {
                comparisons += 1;
                card.end <= window.start
            }) {
                next_card += 1;
            }
            !cards
                .get(next_card)
                .is_some_and(|card| overlaps(card, window))
        })
        .collect();
    (kept, comparisons)
}

/// True when `text[span]`, read as one digit run, holds a card by [`scan_card_run`]: the check
/// validator veto applies to a `card.structural` candidate, whose span may be the union of
/// overlapping Luhn-valid windows and so fail Luhn as a whole. `source_spans` is as for
/// [`scan_card_run`].
pub fn holds_card(text: &str, span: Range<usize>, source_spans: Option<&[(usize, usize)]>) -> bool {
    !scan_card_run(text, span, source_spans).cards.is_empty()
}

/// Sort `windows` and merge every chain of overlapping ones into one range.
fn union_of_overlapping(mut windows: Vec<Range<usize>>) -> Vec<Range<usize>> {
    windows.sort_by_key(|window| window.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for window in windows {
        match merged.last_mut() {
            Some(last) if window.start < last.end => last.end = last.end.max(window.end),
            _ => merged.push(window),
        }
    }
    merged
}

/// The most groups a card layout in [`is_card_layout`] has.
const MAX_LAYOUT_GROUPS: usize = 5;

/// The group widths a card number is printed in: compact (13 to 19 digits), 4-4-4-4, the
/// 19-digit 4-4-4-4-3, and the Amex and Diners 4-6-5 and 4-6-4.
fn is_card_layout(widths: &[usize]) -> bool {
    matches!(
        widths,
        [13..=19] | [4, 4, 4, 4] | [4, 4, 4, 4, 3] | [4, 6, 5] | [4, 6, 4]
    )
}

fn is_separator(ch: char) -> bool {
    ch.is_whitespace() || ch == '-'
}

fn overlaps(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

/// The windows `\b\d(?:[\s-]?\d){12,18}\b` matches inside one run, offset by `base`. Inside a run
/// the word boundaries are exactly the separator edges, so a match starts at a
/// separator-delimited group, takes the most whole groups that stay within 19 digits, and must
/// reach 13; a start that cannot moves on to the next group.
fn pattern_windows(run_text: &str, base: usize) -> Vec<Range<usize>> {
    let mut groups: Vec<(Range<usize>, usize)> = Vec::new();
    let mut open = false;
    for (offset, ch) in run_text.char_indices() {
        let at = base + offset;
        if is_separator(ch) {
            open = false;
            continue;
        }
        match groups.last_mut() {
            Some((range, digits)) if open => {
                range.end = at + ch.len_utf8();
                *digits += 1;
            }
            _ => groups.push((at..at + ch.len_utf8(), 1)),
        }
        open = true;
    }

    let mut windows = Vec::new();
    let mut first = 0;
    while first < groups.len() {
        let mut digits = 0;
        let mut end = None;
        for (last, (_, width)) in groups.iter().enumerate().skip(first) {
            digits += width;
            if digits > 19 {
                break;
            }
            if digits >= 13 {
                end = Some(last);
            }
        }
        match end {
            Some(last) => {
                windows.push(groups[first].0.start..groups[last].0.end);
                first = last + 1;
            }
            None => first += 1,
        }
    }
    windows
}

/// Split a run into digit groups (byte ranges of the text, offset by `base`). See
/// [`scan_card_run`] for where a group ends.
fn digit_groups(
    run_text: &str,
    base: usize,
    source_spans: Option<&[(usize, usize)]>,
) -> Vec<Range<usize>> {
    let mut groups: Vec<Range<usize>> = Vec::new();
    let mut previous_digit: Option<usize> = None;
    for (offset, ch) in run_text.char_indices() {
        let at = base + offset;
        if is_separator(ch) {
            previous_digit = None;
            continue;
        }
        let joins_previous = previous_digit.is_some_and(|previous| {
            let (Some(&(previous_start, previous_end)), Some(&(start, end))) = (
                source_spans.and_then(|spans| spans.get(previous)),
                source_spans.and_then(|spans| spans.get(at)),
            ) else {
                return true;
            };
            (previous_start, previous_end) == (start, end)
                || (previous_end == start && previous_end - previous_start == end - start)
        });
        match groups.last_mut() {
            Some(group) if joins_previous => group.end = at + ch.len_utf8(),
            _ => groups.push(at..at + ch.len_utf8()),
        }
        previous_digit = Some(at);
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The normalized detection view the scan runs on: fullwidth digits folded to ASCII, ZWJ
    /// and ZWNJ dropped, with the source span of every byte.
    fn view(source: &str) -> (String, Vec<(usize, usize)>) {
        let mut text = String::new();
        let mut spans = Vec::new();
        for (start, ch) in source.char_indices() {
            let end = start + ch.len_utf8();
            if matches!(ch, '\u{200C}' | '\u{200D}') {
                continue;
            }
            let folded = match ch {
                '０'..='９' => char::from_u32(ch as u32 - 0xFEE0).unwrap(),
                _ => ch,
            };
            text.push(folded);
            spans.extend(std::iter::repeat_n((start, end), folded.len_utf8()));
        }
        (text, spans)
    }

    /// The cards in `source` as source substrings. The run is found by hand: the fixtures hold
    /// one digit run, bounded by letters or spaces.
    fn cards(source: &str) -> Vec<String> {
        let (text, spans) = view(source);
        let start = text.find(|ch: char| ch.is_ascii_digit()).unwrap();
        let end = text.rfind(|ch: char| ch.is_ascii_digit()).unwrap() + 1;
        scan_card_run(&text, start..end, Some(&spans))
            .cards
            .into_iter()
            .map(|card| source[spans[card.start].0..spans[card.end - 1].1].to_string())
            .collect()
    }

    #[test]
    fn every_card_layout_is_found_next_to_a_cvv() {
        // REVIEW 652 round 2 F-B: the layout whitelist is load-bearing beyond 4-4-4-4.
        for (source, card) in [
            ("x 4111111111111111 123 x", "4111111111111111"),
            ("x 3782 822463 10005 1234 x", "3782 822463 10005"),
            ("x 3056 930902 5904 123 x", "3056 930902 5904"),
            ("x 4111 1111 1111 1111 123 x", "4111 1111 1111 1111"),
        ] {
            assert_eq!(cards(source), [card], "{source:?}");
        }
    }

    #[test]
    fn the_longest_card_wins_over_its_luhn_valid_prefix() {
        // `4111 1111 1111 1111 003` passes Luhn and so does its 16-digit prefix. The leading
        // `12` makes the pattern window fail, so the retry has to choose.
        assert_eq!(
            cards("Ref 12 4111 1111 1111 1111 003 45 ok"),
            ["4111 1111 1111 1111 003"]
        );
    }

    #[test]
    fn overlapping_luhn_valid_windows_become_one_card() {
        // Solo todo 3843 round 2: `0 4111 1111 1111 1111` passes Luhn as the pattern window
        // (a leading zero keeps the checksum), so does the 16-digit card and so does the 19-digit
        // card after it. None may win alone: the union leaves no digit of any of them out.
        assert_eq!(
            cards("x 0 4111 1111 1111 1111 003 x"),
            ["0 4111 1111 1111 1111 003"]
        );
        assert_eq!(
            cards("x 0\u{200D}4111 1111 1111 1111 x"),
            ["0\u{200D}4111 1111 1111 1111"]
        );
        // REVIEW 658 F2: the 18-digit pattern window and the 4-4-4-4 card start on the same
        // byte and both pass Luhn; the union must keep the longer end, or ` 00` ships raw.
        assert_eq!(
            cards("Karte 4111 1111 1111 1111 00 ok"),
            ["4111 1111 1111 1111 00"]
        );
    }

    /// One run of `groups` random 4-digit groups (a fixed LCG, so both sizes see the same kind
    /// of text): about one 4-4-4-4 window in ten passes Luhn, so cards and rejected windows are
    /// both dense.
    fn random_grouped_run(groups: usize) -> String {
        let mut state = 0x658_u64;
        let mut text = String::with_capacity(groups * 5);
        for index in 0..groups {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            if index > 0 {
                text.push(' ');
            }
            text.push_str(&format!("{:04}", (state >> 33) % 10_000));
        }
        text
    }

    #[test]
    fn rejected_windows_match_the_plain_overlap_definition() {
        // The sorted walk in `without_overlap` must equal "overlaps no card".
        let text = random_grouped_run(20_000);
        let scan = scan_card_run(&text, 0..text.len(), None);
        let all_failed: Vec<_> = pattern_windows(&text, 0)
            .into_iter()
            .filter(|window| !crate::luhn_check(&text[window.clone()]))
            .collect();
        let expected: Vec<_> = all_failed
            .into_iter()
            .filter(|window| !scan.cards.iter().any(|card| overlaps(card, window)))
            .collect();
        assert!(!scan.cards.is_empty() && !expected.is_empty());
        assert_eq!(scan.rejected, expected);
    }

    #[test]
    fn the_rejected_walk_is_linear_in_its_inputs() {
        // REVIEW 658 F1 / round 4: matching rejected windows against every card made the scan
        // quadratic. Count card comparisons instead of timing (no wall clock): the walk may
        // compare at most once per card passed over and once per window, whatever the size.
        let failing_windows = |text: &str| -> Vec<Range<usize>> {
            pattern_windows(text, 0)
                .into_iter()
                .filter(|window| !crate::luhn_check(&text[window.clone()]))
                .collect()
        };
        // Synthetic lists of n and 4n: a card every third slot, windows in between.
        for n in [1_000usize, 4_000] {
            let cards: Vec<_> = (0..n).map(|index| index * 30..index * 30 + 10).collect();
            let windows: Vec<_> = (0..n)
                .map(|index| index * 30 + 12..index * 30 + 25)
                .collect();
            let (kept, comparisons) = without_overlap(windows.clone(), &cards);
            assert_eq!(kept, windows);
            assert!(
                comparisons <= windows.len() + cards.len(),
                "n {n}: {comparisons}"
            );
        }
        // A 1 MB run of random 4-digit groups, the shape the review timed.
        let text = random_grouped_run(200_000);
        let scan = scan_card_run(&text, 0..text.len(), None);
        let failed = failing_windows(&text);
        let (rejected, comparisons) = without_overlap(failed.clone(), &scan.cards);
        assert_eq!(rejected, scan.rejected);
        assert!(scan.cards.len() > 10_000 && failed.len() > 10_000);
        assert!(
            comparisons <= failed.len() + scan.cards.len(),
            "{comparisons} comparisons for {} windows and {} cards",
            failed.len(),
            scan.cards.len()
        );
    }

    #[test]
    fn a_card_that_ends_where_a_window_starts_does_not_hide_the_next_card() {
        // REVIEW 658 round 4 I1: `0..10` touches the window `10..14` without overlapping it; the
        // walk must move past it and find that `12..15` does overlap.
        let (kept, _) = without_overlap(vec![10..14, 20..24], &[0..10, 12..15]);
        assert_eq!(kept, vec![(20..24)]);
        let touching_window: Vec<Range<usize>> = std::iter::once(10..14).collect();
        let touching_card: Vec<Range<usize>> = std::iter::once(0..10).collect();
        let (kept, _) = without_overlap(touching_window, &touching_card);
        assert_eq!(kept, vec![(10..14)]);
    }

    #[test]
    fn a_union_span_still_holds_a_card() {
        let text = "x 0 4111 1111 1111 1111 003 x";
        assert!(holds_card(text, 2..27, None));
        assert!(!holds_card("x 4012 8888 8888 1882 x", 2..21, None));
    }

    #[test]
    fn a_width_change_or_a_dropped_joiner_ends_a_group() {
        // Normalized, both runs read `4111 1111 1111 11111234`; only the source spans tell where
        // the card ends.
        for source in [
            "x 4111 1111 1111 1111１２３４ x",
            "x 4111 1111 1111 1111\u{200D}1234 x",
        ] {
            assert_eq!(cards(source), ["4111 1111 1111 1111"], "{source:?}");
        }
        // Without source spans the touching digits are one group and no layout fits.
        let text = "x 4111 1111 1111 11111234 x";
        assert!(scan_card_run(text, 2..25, None).cards.is_empty());
    }

    #[test]
    fn a_card_is_found_anywhere_in_a_long_run() {
        // REVIEW 652 round 2 F-A: past the first 19 digits of the run.
        for (source, card) in [
            ("2024 4111 1111 1111 1111", "4111 1111 1111 1111"),
            ("Order 5678 4111 1111 1111 1111 paid", "4111 1111 1111 1111"),
            ("Nr 12345 4111 1111 1111 1111", "4111 1111 1111 1111"),
        ] {
            assert_eq!(cards(source), [card], "{source:?}");
        }
        // Two cards in one run are both found.
        assert_eq!(
            cards("x 4111 1111 1111 1111 5500 0000 0000 0004 x"),
            ["4111 1111 1111 1111", "5500 0000 0000 0004"]
        );
    }

    #[test]
    fn a_luhn_failing_pattern_window_is_rejected_unless_a_card_overlaps_it() {
        let text = "Ref 4012 8888 8888 1882 ok";
        let scan = scan_card_run(text, 4..23, None);
        assert!(scan.cards.is_empty());
        assert_eq!(scan.rejected, vec![(4..23)]);

        let text = "Karte 4111 1111 1111 1111 123";
        let scan = scan_card_run(text, 6..29, None);
        assert_eq!(scan.cards, vec![(6..25)]);
        assert!(scan.rejected.is_empty());
    }

    #[test]
    fn groups_that_are_no_card_layout_stay_clean() {
        // Each fails Luhn as a whole but holds a Luhn-valid 13- or 14-digit group-aligned
        // window (`5 573 835 698 185`, `2018 10 21 06 23 06`) that no card layout allows.
        for text in [
            "Montant : 5 573 835 698 185 105 €",
            "log 2018 10 21 06 23 06 560 ok",
            "Karte 4111 1111 1111 1111123",
        ] {
            let start = text.find(|ch: char| ch.is_ascii_digit()).unwrap();
            let end = text.rfind(|ch: char| ch.is_ascii_digit()).unwrap() + 1;
            assert!(
                scan_card_run(text, start..end, None).cards.is_empty(),
                "{text:?}"
            );
        }
    }

    #[test]
    fn pattern_windows_are_exactly_the_card_pattern_matches() {
        // `pattern_windows` restates `\b\d(?:[\s-]?\d){12,18}\b` without a regex engine.
        // Enumerate generated texts and require the same matches as the regex itself, found
        // through the runs of `CARD_RUN_PATTERN` as callers find them.
        let card = regex::Regex::new(r"\b\d(?:[\s-]?\d){12,18}\b").unwrap();
        let run = regex::Regex::new(CARD_RUN_PATTERN).unwrap();
        let alphabet: Vec<char> = "0123456789 -a_\u{00A0}\u{2028}\u{0663}é".chars().collect();
        let mut state = 0x3843_u64;
        let mut next = |bound: usize| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) as usize % bound
        };
        for _ in 0..20_000 {
            let len = 10 + next(50);
            let text: String = (0..len)
                .map(|_| {
                    if next(10) < 7 {
                        char::from(b'0' + next(10) as u8)
                    } else {
                        alphabet[next(alphabet.len())]
                    }
                })
                .collect();
            let expected: Vec<Range<usize>> = card.find_iter(&text).map(|m| m.range()).collect();
            let actual: Vec<Range<usize>> = run
                .find_iter(&text)
                .flat_map(|m| pattern_windows(m.as_str(), m.start()))
                .collect();
            assert_eq!(actual, expected, "{text:?}");
        }
    }

    #[test]
    fn out_of_range_runs_find_nothing() {
        assert_eq!(scan_card_run("4111", 2..9, None), CardRunScan::default());
        assert_eq!(scan_card_run("é4111", 1..5, None), CardRunScan::default());
    }
}
