//! Address-block growth (todo 4013).
//!
//! An address is personal data as a unit. When resolution has already
//! protected part of one (a postcode, a NER street or city, a house number),
//! the unit designator, box, state code or military post office written right
//! beside it (`Suite 312`, `Apt. 4B`, `Wohnung 7`, `3. Etage`, `PO Box 417`,
//! `IL`, `PSC 806, Box 9504`, `APO AE`) would otherwise stay raw between
//! tokens. This module grows protection outward from those winners, one piece
//! at a time, across address separators only. Nothing here starts from text the
//! resolver did not already protect: a designator with no address beside it is
//! never tokenized.

use std::collections::HashMap;
use std::ops::Range;

use crate::LocaleTag;

/// Recognizer id of a grown unit designator with its number (`Suite 312`).
pub const ADDRESS_UNIT_RECOGNIZER_ID: &str = "address.block.unit";
/// Recognizer id of a grown number-first designator (`3. Etage`).
pub const ADDRESS_UNIT_NUMBER_BEFORE_RECOGNIZER_ID: &str = "address.block.unit_number_before";
/// Recognizer id of the `Box` of a military line (`PSC 806, Box 9504`).
pub const ADDRESS_MILITARY_BOX_RECOGNIZER_ID: &str = "address.block.military_box";
/// Recognizer id of a grown state or military state code (`IL`, `AE`).
pub const ADDRESS_REGION_CODE_RECOGNIZER_ID: &str = "address.block.region_code";
/// Recognizer id of a grown military post office (`APO`).
pub const ADDRESS_MILITARY_POST_OFFICE_RECOGNIZER_ID: &str = "address.block.military_post_office";

/// Longest chain grown from one side of one address winner.
const MAX_PIECES_PER_SIDE: usize = 5;
/// Longest separator between two pieces: spaces and commas, at most one line break.
const MAX_SEPARATOR_BYTES: usize = 4;
/// Longest unit number: `12345` plus one letter.
const MAX_UNIT_DIGITS: usize = 5;
/// Longest piece read backwards from a separator.
const MAX_PIECE_BYTES: usize = 32;

/// A locale vocabulary the grammar reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AddressVocabulary {
    /// Words written before a unit number (`suite`, `apt.`, `po box`, `wohnung`).
    UnitDesignators,
    /// Words written after an ordinal unit number (`etage` in `3. Etage`).
    UnitDesignatorsNumberBefore,
    /// Exact-case state or region codes (`IL`, military `AE`).
    RegionCodes,
    /// Exact-case military post offices (`APO`).
    MilitaryPostOffices,
}

/// Why a piece joined an address block: the closed reason recorded as the
/// piece's recognizer id in every audit row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AddressGrowth {
    /// A unit designator and its number.
    Unit,
    /// An ordinal number and the designator after it.
    UnitNumberBefore,
    /// `Box N` right after a military `PSC` / `CMR` / `Unit` line.
    MilitaryBox,
    /// A state or military state code.
    RegionCode,
    /// A military post office.
    MilitaryPostOffice,
}

impl AddressGrowth {
    /// Every reason, in declaration order.
    pub const ALL: [AddressGrowth; 5] = [
        AddressGrowth::Unit,
        AddressGrowth::UnitNumberBefore,
        AddressGrowth::MilitaryBox,
        AddressGrowth::RegionCode,
        AddressGrowth::MilitaryPostOffice,
    ];

    /// The recognizer id the grown candidate carries.
    pub const fn recognizer_id(self) -> &'static str {
        match self {
            AddressGrowth::Unit => ADDRESS_UNIT_RECOGNIZER_ID,
            AddressGrowth::UnitNumberBefore => ADDRESS_UNIT_NUMBER_BEFORE_RECOGNIZER_ID,
            AddressGrowth::MilitaryBox => ADDRESS_MILITARY_BOX_RECOGNIZER_ID,
            AddressGrowth::RegionCode => ADDRESS_REGION_CODE_RECOGNIZER_ID,
            AddressGrowth::MilitaryPostOffice => ADDRESS_MILITARY_POST_OFFICE_RECOGNIZER_ID,
        }
    }

    /// The reason for a recognizer id, if it is one of these.
    pub fn from_recognizer_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|growth| growth.recognizer_id() == id)
    }
}

/// Unit words that start a military line and license a following `Box N`.
const MILITARY_LINE_WORDS: [&str; 3] = ["psc", "cmr", "unit"];

#[derive(Debug, Clone, Default)]
pub(crate) struct AddressGrammar {
    by_locale: HashMap<LocaleTag, HashMap<AddressVocabulary, Vec<String>>>,
}

/// One grown piece in normalized-text byte offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GrownPiece {
    pub(crate) span: Range<usize>,
    pub(crate) growth: AddressGrowth,
    /// The address winner the chain started from.
    pub(crate) anchor: usize,
}

impl AddressGrammar {
    pub(crate) fn register(
        &mut self,
        locale: LocaleTag,
        vocabulary: AddressVocabulary,
        names: Vec<String>,
    ) {
        let words = self
            .by_locale
            .entry(locale)
            .or_default()
            .entry(vocabulary)
            .or_default();
        for name in names {
            let name = name.trim();
            // Designators match case-insensitively; codes are exact-case.
            let word = match vocabulary {
                AddressVocabulary::UnitDesignators
                | AddressVocabulary::UnitDesignatorsNumberBefore => name.to_lowercase(),
                AddressVocabulary::RegionCodes | AddressVocabulary::MilitaryPostOffices => {
                    name.to_string()
                }
            };
            if !word.is_empty() && !words.contains(&word) {
                words.push(word);
            }
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.by_locale.is_empty()
    }

    fn words<'a>(
        &'a self,
        chain: &'a [LocaleTag],
        vocabulary: AddressVocabulary,
    ) -> impl Iterator<Item = &'a str> {
        chain
            .iter()
            .filter_map(move |locale| self.by_locale.get(locale)?.get(&vocabulary))
            .flatten()
            .map(String::as_str)
    }

    /// Pieces grown outward from each anchor (`anchors[i]` is an address
    /// winner's span). A piece never overlaps `claimed` (every settled
    /// selection) nor another grown piece, and growth stops at anything that
    /// is not an address separator followed by a known piece.
    pub(crate) fn grow(
        &self,
        text: &str,
        anchors: &[Range<usize>],
        claimed: &[Range<usize>],
        chain: &[LocaleTag],
    ) -> Vec<GrownPiece> {
        let mut found: Vec<GrownPiece> = Vec::new();
        let taken = |span: &Range<usize>, found: &[GrownPiece]| {
            claimed
                .iter()
                .chain(found.iter().map(|piece| &piece.span))
                .any(|other| overlaps(span, other))
        };
        for (anchor, span) in anchors.iter().enumerate() {
            // Rightward.
            let mut edge = span.end;
            let mut previous: Option<(AddressGrowth, Range<usize>)> = None;
            for _ in 0..MAX_PIECES_PER_SIDE {
                let Some(start) = separator_end(text, edge) else {
                    break;
                };
                let Some((end, growth)) = self.piece_at(text, start, chain) else {
                    break;
                };
                let growth = match (growth, &previous) {
                    (PieceKind::Box, Some((AddressGrowth::Unit, unit)))
                        if is_military_line(&text[unit.clone()]) =>
                    {
                        AddressGrowth::MilitaryBox
                    }
                    (PieceKind::Box, _) => break,
                    (PieceKind::Grown(growth), _) => growth,
                };
                let piece = start..end;
                if taken(&piece, &found) {
                    break;
                }
                found.push(GrownPiece {
                    span: piece.clone(),
                    growth,
                    anchor,
                });
                previous = Some((growth, piece));
                edge = end;
            }
            // Leftward.
            let mut edge = span.start;
            for _ in 0..MAX_PIECES_PER_SIDE {
                let Some(end) = separator_start(text, edge) else {
                    break;
                };
                let Some((start, kind)) = self.piece_ending_at(text, end, chain) else {
                    break;
                };
                let piece = start..end;
                let pieces = match kind {
                    PieceKind::Grown(growth) => vec![(piece, growth)],
                    // `Box N` joins leftward only together with the military line before it.
                    PieceKind::Box => {
                        let Some(unit_end) = separator_start(text, start) else {
                            break;
                        };
                        let Some((unit_start, PieceKind::Grown(AddressGrowth::Unit))) =
                            self.piece_ending_at(text, unit_end, chain)
                        else {
                            break;
                        };
                        if !is_military_line(&text[unit_start..unit_end]) {
                            break;
                        }
                        vec![
                            (piece, AddressGrowth::MilitaryBox),
                            (unit_start..unit_end, AddressGrowth::Unit),
                        ]
                    }
                };
                if pieces.iter().any(|(piece, _)| taken(piece, &found)) {
                    break;
                }
                edge = pieces.last().map(|(piece, _)| piece.start).unwrap_or(edge);
                found.extend(pieces.into_iter().map(|(span, growth)| GrownPiece {
                    span,
                    growth,
                    anchor,
                }));
            }
        }
        found.sort_by_key(|piece| (piece.span.start, piece.span.end));
        found
    }

    /// The piece starting exactly at `start`: its end and kind.
    fn piece_at(
        &self,
        text: &str,
        start: usize,
        chain: &[LocaleTag],
    ) -> Option<(usize, PieceKind)> {
        if !starts_word(text, start) {
            return None;
        }
        let rest = &text[start..];
        if let Some(len) = box_len(rest) {
            return Some((start + len, PieceKind::Box));
        }
        let designator = self
            .words(chain, AddressVocabulary::UnitDesignators)
            .filter_map(|word| designator_len(rest, word))
            .max();
        if let Some(len) = designator {
            return Some((start + len, PieceKind::Grown(AddressGrowth::Unit)));
        }
        if let Some(len) = self
            .words(chain, AddressVocabulary::UnitDesignatorsNumberBefore)
            .filter_map(|word| number_before_len(rest, word))
            .max()
        {
            return Some((
                start + len,
                PieceKind::Grown(AddressGrowth::UnitNumberBefore),
            ));
        }
        // A state code is an address part only in front of its postcode
        // (`IL 00068`); `Paris, OR maybe` is prose.
        if let Some(word) = self
            .words(chain, AddressVocabulary::RegionCodes)
            .find(|word| {
                exact_word(rest, word)
                    && separator_end(text, start + word.len())
                        .is_some_and(|next| text[next..].starts_with(|c: char| c.is_ascii_digit()))
            })
        {
            return Some((
                start + word.len(),
                PieceKind::Grown(AddressGrowth::RegionCode),
            ));
        }
        self.words(chain, AddressVocabulary::MilitaryPostOffices)
            .find(|word| exact_word(rest, word))
            .map(|word| {
                (
                    start + word.len(),
                    PieceKind::Grown(AddressGrowth::MilitaryPostOffice),
                )
            })
    }

    /// The longest piece ending exactly at `end`.
    fn piece_ending_at(
        &self,
        text: &str,
        end: usize,
        chain: &[LocaleTag],
    ) -> Option<(usize, PieceKind)> {
        let floor = end.saturating_sub(MAX_PIECE_BYTES);
        text[..end]
            .char_indices()
            .rev()
            .take_while(|(index, _)| *index >= floor)
            .map(|(index, _)| index)
            .filter_map(|start| {
                let (piece_end, kind) = self.piece_at(text, start, chain)?;
                (piece_end == end).then_some((start, kind))
            })
            .last()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PieceKind {
    Grown(AddressGrowth),
    /// `Box N`: only an address piece right after a military line.
    Box,
}

fn overlaps(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

fn is_military_line(unit: &str) -> bool {
    let word = unit
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase();
    MILITARY_LINE_WORDS.contains(&word.as_str())
}

/// A separator is 1-4 bytes of spaces and commas with at most one line
/// break. A full stop, colon, semicolon, tab, pipe or quote ends the block.
fn is_separator(separator: &str) -> bool {
    !separator.is_empty()
        && separator.len() <= MAX_SEPARATOR_BYTES
        && separator.bytes().all(|b| matches!(b, b' ' | b',' | b'\n'))
        && separator.bytes().filter(|&b| b == b'\n').count() <= 1
        && separator.bytes().filter(|&b| b == b',').count() <= 1
}

/// Where the next piece starts after the separator at `edge`, if one follows.
fn separator_end(text: &str, edge: usize) -> Option<usize> {
    let len = text[edge..]
        .bytes()
        .take(MAX_SEPARATOR_BYTES + 1)
        .take_while(|b| matches!(b, b' ' | b',' | b'\n'))
        .count();
    is_separator(&text[edge..edge + len]).then_some(edge + len)
}

/// Where the previous piece ends before the separator ending at `edge`.
fn separator_start(text: &str, edge: usize) -> Option<usize> {
    let len = text[..edge]
        .bytes()
        .rev()
        .take(MAX_SEPARATOR_BYTES + 1)
        .take_while(|b| matches!(b, b' ' | b',' | b'\n'))
        .count();
    is_separator(&text[edge - len..edge]).then_some(edge - len)
}

fn starts_word(text: &str, start: usize) -> bool {
    text.is_char_boundary(start)
        && !text[..start]
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric)
}

fn ends_word(rest: &str, len: usize) -> bool {
    !rest[len..]
        .chars()
        .next()
        .is_some_and(char::is_alphanumeric)
}

fn exact_word(rest: &str, word: &str) -> bool {
    rest.starts_with(word) && ends_word(rest, word.len())
}

/// `designator` + one space + optional `#` + 1-5 digits + optional letter.
fn designator_len(rest: &str, word: &str) -> Option<usize> {
    let head = rest.get(..word.len())?;
    if head.to_lowercase() != word {
        return None;
    }
    if !word.ends_with('.') && !ends_word(rest, word.len()) {
        return None;
    }
    let mut len = word.len();
    len += usize::from(rest[len..].starts_with(' '));
    if len == word.len() && !word.ends_with('.') && !rest[len..].starts_with('#') {
        return None;
    }
    len += usize::from(rest[len..].starts_with('#'));
    unit_number_len(&rest[len..]).map(|number| len + number)
}

/// Digits, then one optional letter, and nothing alphanumeric after it.
fn unit_number_len(rest: &str) -> Option<usize> {
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > MAX_UNIT_DIGITS {
        return None;
    }
    let letter = rest
        .as_bytes()
        .get(digits)
        .is_some_and(u8::is_ascii_alphabetic)
        && !rest
            .as_bytes()
            .get(digits + 1)
            .is_some_and(u8::is_ascii_alphanumeric);
    let len = digits + usize::from(letter);
    // `12.5` or `3:30` is a quantity or time, never a unit number.
    let decimal = matches!(rest.as_bytes().get(len), Some(b'.' | b':' | b','))
        && rest.as_bytes().get(len + 1).is_some_and(u8::is_ascii_digit);
    (ends_word(rest, len) && !decimal).then_some(len)
}

/// `N. designator`: 1-2 digits, a full stop, one space, the word.
fn number_before_len(rest: &str, word: &str) -> Option<usize> {
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if !(1..=2).contains(&digits) || !rest[digits..].starts_with(". ") {
        return None;
    }
    let start = digits + 2;
    let head = rest.get(start..start + word.len())?;
    (head.to_lowercase() == word && ends_word(rest, start + word.len()))
        .then_some(start + word.len())
}

/// `Box N` with a capital `B`.
fn box_len(rest: &str) -> Option<usize> {
    let after = rest.strip_prefix("Box ")?;
    unit_number_len(after).map(|number| 4 + number)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grammar() -> AddressGrammar {
        let mut grammar = AddressGrammar::default();
        let names = |list: &[&str]| list.iter().map(|name| name.to_string()).collect();
        grammar.register(
            LocaleTag::EnUs,
            AddressVocabulary::UnitDesignators,
            names(&[
                "suite",
                "ste.",
                "ste",
                "apt.",
                "apt",
                "apartment",
                "unit",
                "floor",
                "fl.",
                "po box",
                "p.o. box",
                "psc",
                "cmr",
            ]),
        );
        grammar.register(
            LocaleTag::EnUs,
            AddressVocabulary::RegionCodes,
            names(&["IL", "OR", "AE", "AP", "AA"]),
        );
        grammar.register(
            LocaleTag::EnUs,
            AddressVocabulary::MilitaryPostOffices,
            names(&["APO", "FPO", "DPO"]),
        );
        grammar.register(
            LocaleTag::DeDe,
            AddressVocabulary::UnitDesignators,
            names(&["wohnung", "whg.", "postfach"]),
        );
        grammar.register(
            LocaleTag::DeDe,
            AddressVocabulary::UnitDesignatorsNumberBefore,
            names(&["etage", "stock", "og"]),
        );
        grammar
    }

    const CHAIN: &[LocaleTag] = &[LocaleTag::EnUs, LocaleTag::DeDe];

    /// Grown pieces for the given anchor substrings (all other anchors claimed too).
    fn grown(text: &str, anchors: &[&str]) -> Vec<(String, AddressGrowth)> {
        let spans = anchors
            .iter()
            .map(|anchor| {
                let start = text.find(anchor).expect("anchor in text");
                start..start + anchor.len()
            })
            .collect::<Vec<_>>();
        grammar()
            .grow(text, &spans, &spans, CHAIN)
            .into_iter()
            .map(|piece| (text[piece.span].to_string(), piece.growth))
            .collect()
    }

    use AddressGrowth::*;

    #[test]
    fn unit_designators_between_street_and_city_join_the_block() {
        assert_eq!(
            grown(
                "to 117 Drusk Lane Ste. 522, Westmere Falls, VT 00096 today",
                &["Drusk Lane", "Westmere Falls", "00096"]
            ),
            [("Ste. 522".to_string(), Unit)]
        );
        assert_eq!(
            grown(
                "He moved to 7841 Drusk Lane, Apt 840B, Calder Rise, IL 00088 last month.",
                &["Drusk Lane", "00088"]
            ),
            [
                ("Apt 840B".to_string(), Unit),
                ("IL".to_string(), RegionCode)
            ]
        );
        assert_eq!(
            grown("Kalvik Road Unit #3, Trelling", &["Kalvik Road"]),
            [("Unit #3".to_string(), Unit)]
        );
    }

    #[test]
    fn a_state_between_city_and_zip_joins_from_the_zip() {
        assert_eq!(
            grown("Brinmoor, IL 00068", &["00068"]),
            [("IL".to_string(), RegionCode)]
        );
        assert_eq!(
            grown("Brinmoor\nIL 00068", &["00068"]),
            [("IL".to_string(), RegionCode)]
        );
    }

    #[test]
    fn a_military_line_joins_whole_from_its_zip() {
        assert_eq!(
            grown("Ship to:\nPSC 806, Box 9504\nFPO AA 00090\n", &["00090"]),
            [
                ("PSC 806".to_string(), Unit),
                ("Box 9504".to_string(), MilitaryBox),
                ("FPO".to_string(), MilitaryPostOffice),
                ("AA".to_string(), RegionCode),
            ]
        );
        assert_eq!(
            grown(
                "Send it to CMR 466 Box 6596, APO AP 00090 this week.",
                &["00090"]
            ),
            [
                ("CMR 466".to_string(), Unit),
                ("Box 6596".to_string(), MilitaryBox),
                ("APO".to_string(), MilitaryPostOffice),
                ("AP".to_string(), RegionCode),
            ]
        );
    }

    #[test]
    fn german_designators_join_after_the_house_number() {
        assert_eq!(
            grown(
                "in der Pellinorallee 7a, 2. OG, 00937 Wiesenthal-Nord.",
                &["Pellinorallee 7a", "00937"]
            ),
            [("2. OG".to_string(), UnitNumberBefore)]
        );
        assert_eq!(
            grown(
                "Corvathgasse 54\nWhg. 627\n00572 Kornhelm",
                &["Corvathgasse 54", "00572"]
            ),
            [("Whg. 627".to_string(), Unit)]
        );
        assert_eq!(
            grown("Postfach 505, 00724 Kornhelm", &["00724"]),
            [("Postfach 505".to_string(), Unit)]
        );
    }

    #[test]
    fn a_standalone_designator_never_grows() {
        assert!(grown("Run test Suite 431 before merging.", &[]).is_empty());
        assert!(grown("Minutes from PSC 311 are attached.", &[]).is_empty());
    }

    #[test]
    fn a_sentence_or_field_end_stops_growth() {
        assert!(grown(
            "Deliver to Brinmoor, IL 00068.\nThe regression Suite 810 is still red.",
            &["Brinmoor", "00068"]
        )
        .iter()
        .all(|(piece, _)| piece == "IL"));
        assert!(grown("Tervelau\nDie Antwort liegt in Postfach 123", &["Tervelau"]).is_empty());
        assert!(grown(
            r#"{"street":"Drusk Lane","unit":"Apt. 741"}"#,
            &["Drusk Lane"]
        )
        .is_empty());
        assert!(grown("Drusk Lane;Apt 7", &["Drusk Lane"]).is_empty());
        assert!(grown("Drusk Lane\n\nApt 7", &["Drusk Lane"]).is_empty());
        assert!(grown("Drusk Lane\tApt 7", &["Drusk Lane"]).is_empty());
    }

    #[test]
    fn box_alone_or_after_a_non_military_unit_never_grows() {
        assert!(grown("Order 12345, Box 3 of 5", &["12345"]).is_empty());
        assert_eq!(
            grown("Drusk Lane Suite 4, Box 3", &["Drusk Lane"]),
            [("Suite 4".to_string(), Unit)]
        );
    }

    #[test]
    fn numbers_that_are_quantities_or_codes_are_not_units() {
        assert!(grown("Drusk Lane Suite 12.5 km", &["Drusk Lane"]).is_empty());
        assert!(grown("Drusk Lane Suite 3:30", &["Drusk Lane"]).is_empty());
        assert!(grown("Drusk Lane Suite 123456", &["Drusk Lane"]).is_empty());
        assert!(grown("Drusk Lane Suites 4", &["Drusk Lane"]).is_empty());
        assert!(grown("Drusk Lane Suite", &["Drusk Lane"]).is_empty());
        assert!(grown("Brinmoor, il 00068", &["00068"]).is_empty());
        assert!(grown("Brinmoor, ILL 00068", &["00068"]).is_empty());
        assert!(grown(
            "We saw Brinmoor, OR maybe Calder Rise.",
            &["Brinmoor", "Calder Rise"]
        )
        .is_empty());
    }

    #[test]
    fn a_piece_never_crosses_another_selection() {
        // `Suite 4` is claimed by another winner, so nothing grows over it.
        let text = "Drusk Lane Suite 4, IL 00068";
        let street = 0..10;
        let suite = 11..18;
        let found = grammar().grow(text, &[street.clone()], &[street, suite], CHAIN);
        assert!(found.is_empty());
    }

    #[test]
    fn inactive_locales_contribute_no_words() {
        let text = "Pellinorallee 7a, Wohnung 4";
        let found = grammar().grow(text, &[0..16], &[0..16], &[LocaleTag::EnUs]);
        assert!(found.is_empty());
    }

    #[test]
    fn every_growth_reason_maps_to_a_distinct_recognizer_id() {
        let ids = AddressGrowth::ALL.map(AddressGrowth::recognizer_id);
        for growth in AddressGrowth::ALL {
            assert_eq!(
                AddressGrowth::from_recognizer_id(growth.recognizer_id()),
                Some(growth)
            );
        }
        let mut unique = ids.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len());
        assert_eq!(AddressGrowth::from_recognizer_id("ner"), None);
    }
}
