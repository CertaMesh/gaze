//! Street-corroborated house numbers (todo 3670).
//!
//! A house number alone is an ordinary number, so no rule may tokenize one on
//! shape. This module only answers a narrower question: given a span the NER
//! model already marked as a location, is that span a whole street whose last
//! word the locale lexicon recognises, and is a house-number shape directly
//! beside it on the side that locale writes it? The pipeline supplies the
//! location spans after conflict resolution; nothing here looks at text the
//! model did not mark.

use std::collections::HashMap;
use std::ops::Range;

use crate::LocaleTag;

/// Recognizer id carried by every house-number candidate and audit row.
pub const HOUSE_NUMBER_RECOGNIZER_ID: &str = "address.house_number.street_corroborated";

/// Longest digit run accepted as one house number. Five digits is a German or
/// US postcode, never a house number in the locales that ship a lexicon.
const MAX_DIGITS: usize = 4;
/// Spaces allowed between street and number. Normalization already folded
/// NBSP and NARROW NBSP into ASCII space; tabs and line breaks never join.
const MAX_SEPARATOR_SPACES: usize = 2;

/// Where a locale writes the house number relative to the street.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StreetNumberOrder {
    /// `Musterweg 17b`: the lexicon lists word endings (`weg`, `straße`) and
    /// the number follows the street.
    NumberAfter,
    /// `17 Example Street`: the lexicon lists whole street-type words
    /// (`street`, `road`) and the number precedes the street.
    NumberBefore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    stem: String,
    /// Declared with a trailing `.` (`str.`, `st.`): a dot may follow the word.
    abbreviation: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct StreetLexicon {
    by_locale: HashMap<LocaleTag, HashMap<StreetNumberOrder, Vec<Entry>>>,
}

impl StreetLexicon {
    pub(crate) fn register(
        &mut self,
        locale: LocaleTag,
        order: StreetNumberOrder,
        names: Vec<String>,
    ) {
        let entries = self
            .by_locale
            .entry(locale)
            .or_default()
            .entry(order)
            .or_default();
        for name in names {
            let lowered = name.trim().to_lowercase();
            let abbreviation = lowered.ends_with('.');
            let stem = lowered.trim_end_matches('.').to_string();
            if stem.is_empty() || entries.iter().any(|entry| entry.stem == stem) {
                continue;
            }
            entries.push(Entry { stem, abbreviation });
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.by_locale.is_empty()
    }

    /// House-number spans licensed by `street`, in `text` byte offsets.
    pub(crate) fn house_numbers(
        &self,
        text: &str,
        street: Range<usize>,
        locale_chain: &[LocaleTag],
    ) -> Vec<Range<usize>> {
        let Some(street_text) = text.get(street.clone()) else {
            return Vec::new();
        };
        if !is_word_boundary(text, street.start, street.end) {
            return Vec::new();
        }
        let multi_word = street_text.split_whitespace().nth(1).is_some();
        let Some(last) = street_text.split_whitespace().next_back() else {
            return Vec::new();
        };
        let span_has_dot = last.ends_with('.');
        let word = last.trim_end_matches('.').to_lowercase();

        let mut found = Vec::new();
        for order in [
            StreetNumberOrder::NumberAfter,
            StreetNumberOrder::NumberBefore,
        ] {
            let entry = locale_chain
                .iter()
                .filter_map(|locale| self.by_locale.get(locale)?.get(&order))
                .flatten()
                .find(|entry| entry_matches(entry, order, &word, multi_word));
            let Some(entry) = entry else {
                continue;
            };
            if span_has_dot && !entry.abbreviation {
                // `Musterweg.` ends a sentence; the next number is not its house.
                continue;
            }
            let number = match order {
                StreetNumberOrder::NumberAfter => {
                    let mut cursor = street.end;
                    if entry.abbreviation && !span_has_dot && text[cursor..].starts_with('.') {
                        cursor += 1;
                    }
                    number_after(text, cursor)
                }
                StreetNumberOrder::NumberBefore => number_before(text, street.start),
            };
            if let Some(number) = number {
                if !found.contains(&number) {
                    found.push(number);
                }
            }
        }
        found
    }
}

fn entry_matches(entry: &Entry, order: StreetNumberOrder, word: &str, multi_word: bool) -> bool {
    match order {
        // A bare `Weg` or `Platz` is a common noun; a street needs a name part,
        // either fused (`Musterweg`) or as an earlier word (`Alter Weg`).
        StreetNumberOrder::NumberAfter => {
            word.ends_with(entry.stem.as_str()) && (word.len() > entry.stem.len() || multi_word)
        }
        // `Street` alone is not a street name; `Example Street` is.
        StreetNumberOrder::NumberBefore => word == entry.stem && multi_word,
    }
}

fn is_word_boundary(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
}

fn separator_len(bytes: impl Iterator<Item = u8>) -> Option<usize> {
    let spaces = bytes.take_while(|&b| b == b' ').count();
    (1..=MAX_SEPARATOR_SPACES)
        .contains(&spaces)
        .then_some(spaces)
}

fn number_after(text: &str, street_end: usize) -> Option<Range<usize>> {
    let start = street_end + separator_len(text[street_end..].bytes())?;
    let len = house_number_len(&text[start..])?;
    let end = start + len;
    ends_cleanly(text, end).then_some(start..end)
}

fn number_before(text: &str, street_start: usize) -> Option<Range<usize>> {
    let end = street_start - separator_len(text[..street_start].bytes().rev())?;
    // The token is the non-space run ending at `end`; it must be exactly one
    // house number, so `Chapter 12` style prefixes are judged on their own.
    let start = text[..end]
        .char_indices()
        .rev()
        .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '–' | '/'))
        .last()
        .map(|(index, _)| index)?;
    if house_number_len(&text[start..end])? != end - start {
        return None;
    }
    let before = text[..start].chars().next_back();
    if before.is_some_and(|ch| ch.is_alphanumeric() || matches!(ch, '.' | ',' | ':')) {
        // `1.200 Example Street` or `v2.17 Example Street`: part of a larger number.
        return None;
    }
    Some(start..end)
}

/// A number followed by `.`, `,` or `:` and a digit is a decimal, time or
/// ordinal date, not a house number.
fn ends_cleanly(text: &str, end: usize) -> bool {
    let mut rest = text[end..].chars();
    match rest.next() {
        None => true,
        Some(ch) if ch.is_alphanumeric() => false,
        Some('.' | ',' | ':') => !rest.next().is_some_and(|ch| ch.is_ascii_digit()),
        Some(_) => true,
    }
}

/// Byte length of the house number at the start of `s`: 1-4 digits, one
/// optional letter (`17b`), one optional range or unit part (`12-14`, `12/3`).
fn house_number_len(s: &str) -> Option<usize> {
    let first = part_len(s)?;
    let rest = &s[first..];
    let after_space = usize::from(rest.starts_with(' '));
    let joiner = rest[after_space..].chars().next();
    let joined = match joiner {
        Some(ch @ ('-' | '–' | '/')) => after_space + ch.len_utf8(),
        _ => return Some(first),
    };
    let gap = joined + usize::from(rest[joined..].starts_with(' '));
    match part_len(&rest[gap..]) {
        Some(second) => Some(first + gap + second),
        None => Some(first),
    }
}

fn part_len(s: &str) -> Option<usize> {
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > MAX_DIGITS {
        return None;
    }
    let bytes = s.as_bytes();
    let letter = bytes.get(digits).is_some_and(u8::is_ascii_alphabetic)
        && !bytes.get(digits + 1).is_some_and(u8::is_ascii_alphanumeric);
    Some(digits + usize::from(letter))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lexicon() -> StreetLexicon {
        let mut lexicon = StreetLexicon::default();
        lexicon.register(
            LocaleTag::DeDe,
            StreetNumberOrder::NumberAfter,
            ["straße", "strasse", "str.", "weg", "platz", "gasse"]
                .map(String::from)
                .to_vec(),
        );
        lexicon.register(
            LocaleTag::EnUs,
            StreetNumberOrder::NumberBefore,
            ["street", "st.", "road", "drive", "court"]
                .map(String::from)
                .to_vec(),
        );
        lexicon
    }

    const CHAIN: &[LocaleTag] = &[LocaleTag::DeDe, LocaleTag::EnUs];

    /// Numbers licensed by the street `street` inside `text`.
    fn numbers(text: &str, street: &str) -> Vec<String> {
        let start = text.find(street).expect("street in text");
        lexicon()
            .house_numbers(text, start..start + street.len(), CHAIN)
            .into_iter()
            .map(|span| text[span].to_string())
            .collect()
    }

    #[test]
    fn german_street_licenses_the_number_after_it() {
        assert_eq!(numbers("Musterweg 17b, 10115 Berlin", "Musterweg"), ["17b"]);
        assert_eq!(numbers("Lindenstraße 12-14", "Lindenstraße"), ["12-14"]);
        assert_eq!(numbers("Am Hafenplatz 9A", "Am Hafenplatz"), ["9A"]);
        assert_eq!(numbers("Lindenweg 12/3 links", "Lindenweg"), ["12/3"]);
        assert_eq!(numbers("Alte Gasse  4", "Alte Gasse"), ["4"]);
        assert_eq!(numbers("HAUPTSTRASSE 7", "HAUPTSTRASSE"), ["7"]);
    }

    #[test]
    fn abbreviated_street_may_carry_its_dot_outside_the_span() {
        assert_eq!(numbers("Hauptstr. 12", "Hauptstr"), ["12"]);
        assert_eq!(numbers("Hauptstr. 12", "Hauptstr."), ["12"]);
    }

    #[test]
    fn english_street_licenses_the_number_before_it() {
        assert_eq!(numbers("17 Example Street", "Example Street"), ["17"]);
        assert_eq!(
            numbers("at 230 Harbor Road, Springfield", "Harbor Road"),
            ["230"]
        );
        assert_eq!(numbers("12-14 Mill St. today", "Mill St."), ["12-14"]);
        assert_eq!(numbers("Unit 4, 17B Oak Drive", "Oak Drive"), ["17B"]);
    }

    #[test]
    fn street_first_postcode_is_not_consumed() {
        assert_eq!(numbers("Hauptstraße 12 1010 Wien", "Hauptstraße"), ["12"]);
        assert!(numbers("Musterweg 10115 Berlin", "Musterweg").is_empty());
    }

    #[test]
    fn places_without_a_street_word_license_nothing() {
        assert!(numbers("Berlin 2026 Einwohner", "Berlin").is_empty());
        assert!(numbers("Munich 12 people", "Munich").is_empty());
    }

    /// Known limit, disclosed in the concept: the lexicon cannot tell a court
    /// title from a street. Only the NER span decides; if the model marks
    /// `Civil Court` as a location, the chapter number is tokenized.
    #[test]
    fn street_type_words_follow_the_ner_span_they_are_given() {
        assert_eq!(
            numbers("Chapter 12 Civil Court decisions", "Civil Court"),
            ["12"]
        );
    }

    #[test]
    fn bare_street_words_are_not_street_names() {
        assert!(numbers("Weg 12", "Weg").is_empty());
        assert!(numbers("12 Street", "Street").is_empty());
    }

    #[test]
    fn number_must_sit_on_the_locale_side() {
        assert!(numbers("12 Musterweg", "Musterweg").is_empty());
        assert!(numbers("Example Street 12", "Example Street").is_empty());
    }

    #[test]
    fn line_breaks_tabs_and_cell_borders_end_the_match() {
        assert!(numbers("Musterweg\n12 Stück", "Musterweg").is_empty());
        assert!(numbers("Musterweg\t12", "Musterweg").is_empty());
        assert!(numbers("| Lindenweg | 12 |", "Lindenweg").is_empty());
        assert!(numbers("Musterweg   12", "Musterweg").is_empty());
    }

    #[test]
    fn decimals_times_and_longer_tokens_are_not_house_numbers() {
        assert!(numbers("Musterweg 12.5 km", "Musterweg").is_empty());
        assert!(numbers("Musterweg 12:30 Uhr", "Musterweg").is_empty());
        assert!(numbers("Musterweg 12km", "Musterweg").is_empty());
        assert!(numbers("Musterweg 5th", "Musterweg").is_empty());
        assert!(numbers("v2.17 Example Street", "Example Street").is_empty());
        assert!(numbers("1.200 Example Street", "Example Street").is_empty());
        assert!(numbers("A12 Example Street", "Example Street").is_empty());
    }

    #[test]
    fn sentence_end_after_a_full_street_word_licenses_nothing() {
        assert!(numbers("zum Musterweg. 12 Leute kamen", "Musterweg.").is_empty());
        assert!(numbers("zum Musterweg. 12 Leute kamen", "Musterweg").is_empty());
    }

    #[test]
    fn street_must_be_a_whole_word() {
        assert!(numbers("Xmusterweg 12", "musterweg").is_empty());
    }

    #[test]
    fn inactive_locale_lexicon_is_ignored() {
        let text = "Musterweg 17";
        let found = lexicon().house_numbers(text, 0..9, &[LocaleTag::EnUs]);
        assert!(found.is_empty());
    }
}
