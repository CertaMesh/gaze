//! Differential enumeration of the `ip.v6` guard change (todo 3710).
//!
//! A corpus pass cannot settle "the new guard loses no real address": it only says the corpus
//! holds no counterexample. This enumerates instead. It reads the shipped `ip.v6` pattern, edits
//! the two guard classes back to the pre-fix hex-only spelling to reconstruct the base rule, and
//! runs both over every generated IPv6 form in every generated context, classifying each
//! divergence by direction.
//!
//! Two claims are asserted over the whole product:
//!
//! 1. **Subset.** Every span the new rule emits, the base rule emitted too. The new guard class
//!    is `[^\w:.]`, the old one `[^0-9a-f:.]`; the first is contained in the second, so the rule
//!    can only ever match less. A divergence in the other direction would mean the edit widened
//!    detection, which nothing here intends.
//! 2. **Zero recall loss.** In every context whose adjacent characters are not identifier
//!    characters, the two rules agree exactly and the whole address is still matched. Every
//!    divergence is therefore identifier-adjacent, which is the defect being fixed.
//!
//! The reconstruction is checked, not assumed: each guard substring must occur exactly once, so
//! a later rewrite of the pattern fails this test loudly instead of silently comparing a rule
//! against itself.

use regex::Regex;
use std::collections::BTreeSet;

/// The `ip.v6` `pattern = '''...'''` body, read out of the shipped rulepack text.
fn shipped_pattern() -> String {
    let toml = gaze_recognizers::embedded("core").expect("core rulepack");
    let rule = toml
        .split("[[recognizers]]")
        .find(|block| block.contains("id = \"ip.v6\""))
        .expect("ip.v6 block");
    let after = rule.split_once("pattern = '''").expect("pattern open").1;
    let body = after.split_once("'''").expect("pattern close").0;
    body.to_string()
}

const NEW_PREFIX_GUARD: &str = r"(?:^|[^\w:.])";
const NEW_SUFFIX_GUARD: &str = r"(?:$|[^\w:.]|\.(?:$|[^0-9]))";
const BASE_PREFIX_GUARD: &str = r"(?:^|[^0-9a-f:.])";
const BASE_SUFFIX_GUARD: &str = r"(?:$|[^0-9a-f:.]|\.(?:$|[^0-9]))";

/// The shipped rule with either guard class swapped for another spelling.
///
/// Restoring both gives the pre-fix rule; restoring one gives a half-fix, which is what proves
/// each guard edit carries its own weight.
fn pattern_with(prefix_guard: &str, suffix_guard: &str) -> String {
    let shipped = shipped_pattern();
    assert_eq!(
        shipped.matches(NEW_PREFIX_GUARD).count(),
        1,
        "the shipped ip.v6 prefix guard is no longer {NEW_PREFIX_GUARD:?}; \
         update this differential before trusting it"
    );
    assert_eq!(
        shipped.matches(NEW_SUFFIX_GUARD).count(),
        1,
        "the shipped ip.v6 suffix guard is no longer {NEW_SUFFIX_GUARD:?}; \
         update this differential before trusting it"
    );
    shipped
        .replace(NEW_PREFIX_GUARD, prefix_guard)
        .replace(NEW_SUFFIX_GUARD, suffix_guard)
}

/// How many of `texts` the two rules disagree on.
fn disagreements(left: &Regex, right: &Regex, texts: &[String]) -> usize {
    texts
        .iter()
        .filter(|text| spans(left, text) != spans(right, text))
        .count()
}

/// The capture-group-1 spans the rule emits, which is what the pipeline tokenizes.
fn spans(rule: &Regex, text: &str) -> BTreeSet<(usize, usize)> {
    rule.captures_iter(text)
        .filter_map(|caps| caps.get(1))
        .map(|m| (m.start(), m.end()))
        .collect()
}

/// A deterministic spread of IPv6 forms: every `::` cut position, full eight-hextet forms, and
/// the RFC 4291 IPv4-embedded shapes.
fn addresses() -> Vec<String> {
    const POOL: [&str; 11] = [
        "0", "1", "a", "ff", "db8", "2001", "0db8", "dead", "beef", "abcd", "fe80",
    ];
    let mut out = Vec::new();
    let mut next = 0usize;
    let mut hextet = |bump: usize| {
        next = (next + bump) % POOL.len();
        POOL[next]
    };
    // Every `left::right` split, several hextet fillings each.
    for round in 1..=7usize {
        for left in 0..=7usize {
            for right in 0..=(7 - left) {
                let head: Vec<&str> = (0..left).map(|_| hextet(round)).collect();
                let tail: Vec<&str> = (0..right).map(|_| hextet(round + 1)).collect();
                out.push(format!("{}::{}", head.join(":"), tail.join(":")));
            }
        }
    }
    // Full eight-hextet forms.
    for round in 1..=7usize {
        let all: Vec<&str> = (0..8).map(|_| hextet(round)).collect();
        out.push(all.join(":"));
    }
    out.push("2001:0db8:0000:0000:0000:ff00:0042:8329".to_string());
    // IPv4-embedded (RFC 5737 documentation range).
    out.push("::ffff:192.0.2.1".to_string());
    out.push("::192.0.2.1".to_string());
    out.push("2001:db8::192.0.2.33".to_string());
    out.push("0:0:0:0:0:ffff:192.0.2.1".to_string());
    out.push("fe80:0:0:0:0:0:0:1".to_string());
    out.sort();
    out.dedup();
    out
}

/// Left contexts: delimiters that must keep the address, then identifier characters that must
/// not. `:` and `.` were already outside both guard classes and are kept as controls.
const PREFIXES: [&str; 16] = [
    "", " ", "\n", "\t", "=", "(", "[", "\"", "'", "/", ",", ">", ":", ".", "-", "#",
];
const WORDY_PREFIXES: [&str; 8] = ["a", "s", "Z", "_", "9", "gaze", "Policy", "std"];

const SUFFIXES: [&str; 16] = [
    "", " ", "\n", "\t", ")", "]", "\"", "'", ",", "/", "%", "<", ":", ".", "-", "#",
];
const WORDY_SUFFIXES: [&str; 8] = ["a", "p", "Z", "_", "9", "pply_to", "resolve", "new"];

/// True when `c` is an identifier character, which is what the new guard class refuses next to a
/// candidate.
///
/// This is the guard's own rule, and it is a shade stricter than `gaze_types::is_inside_word`: a
/// candidate ending in `::` followed directly by a letter is refused here even though `:` is not
/// alphanumeric. That is deliberate. An address abutting a letter with no delimiter at all is the
/// very shape this fix is about (`std::`, `Foo::<T>::new`), and no written address form ends
/// against a bare letter.
fn is_identifier_char(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

#[test]
fn the_new_guard_is_a_strict_subset_that_loses_no_delimited_address() {
    let new_rule = Regex::new(&shipped_pattern()).expect("shipped ip.v6 compiles");
    let base_rule = Regex::new(&pattern_with(BASE_PREFIX_GUARD, BASE_SUFFIX_GUARD))
        .expect("reconstructed base ip.v6 compiles");

    let addresses = addresses();
    let prefixes: Vec<&str> = PREFIXES.iter().chain(WORDY_PREFIXES.iter()).copied().collect();
    let suffixes: Vec<&str> = SUFFIXES.iter().chain(WORDY_SUFFIXES.iter()).copied().collect();

    // Only forms the base rule actually recognises are "real addresses" for this comparison;
    // anything the base never matched cannot be a loss.
    let recognised: Vec<&String> = addresses
        .iter()
        .filter(|address| {
            let probe = format!(" {address} ");
            spans(&base_rule, &probe).contains(&(1, 1 + address.len()))
        })
        .collect();
    assert!(
        recognised.len() > 200,
        "corpus too small to be evidence: {} forms",
        recognised.len()
    );

    let mut cases = 0usize;
    let mut divergences = 0usize;
    let mut widenings = Vec::new();
    let mut losses_outside_a_word = Vec::new();
    let mut delimited_cases = 0usize;

    for address in &recognised {
        for prefix in &prefixes {
            for suffix in &suffixes {
                let text = format!("{prefix}{address}{suffix}");
                cases += 1;
                let base = spans(&base_rule, &text);
                let new = spans(&new_rule, &text);

                // Claim 1: never wider than the base rule.
                if !new.is_subset(&base) {
                    widenings.push(text.clone());
                    continue;
                }

                let left_edge = is_identifier_char(prefix.chars().next_back());
                let right_edge = is_identifier_char(suffix.chars().next());

                if base != new {
                    divergences += 1;
                    // Claim 1b: every divergence is identifier-adjacent.
                    if !left_edge && !right_edge {
                        losses_outside_a_word.push(text.clone());
                    }
                    continue;
                }

                // Claim 2: with non-identifier delimiters the whole address still matches.
                if !left_edge && !right_edge {
                    delimited_cases += 1;
                    let whole = (prefix.len(), prefix.len() + address.len());
                    if base.contains(&whole) {
                        assert!(
                            new.contains(&whole),
                            "recall loss: {text:?} lost the whole address"
                        );
                    }
                }
            }
        }
    }

    assert!(
        widenings.is_empty(),
        "the new guard matched where the base rule did not, in {} cases, e.g. {:?}",
        widenings.len(),
        &widenings[..widenings.len().min(5)]
    );
    assert!(
        losses_outside_a_word.is_empty(),
        "{} divergences are not identifier-adjacent, e.g. {:?}",
        losses_outside_a_word.len(),
        &losses_outside_a_word[..losses_outside_a_word.len().min(5)]
    );
    // Non-vacuity, attributed to each guard rather than to the context it happened in. Compare
    // the shipped rule against a half-fix that restored only ONE guard: if that half-fix behaves
    // identically, that guard's edit changed nothing. Counting divergences by whether the
    // context was left-wordy or right-wordy cannot see this, because a context is usually both,
    // so a half-reverted rule keeps a large count on either side.
    let wordy: Vec<String> = recognised
        .iter()
        .flat_map(|address| {
            WORDY_PREFIXES.iter().flat_map(move |prefix| {
                WORDY_SUFFIXES
                    .iter()
                    .map(move |suffix| format!("{prefix}{address}{suffix}"))
            })
        })
        .collect();
    let prefix_only_base = Regex::new(&pattern_with(BASE_PREFIX_GUARD, NEW_SUFFIX_GUARD))
        .expect("prefix-only base compiles");
    let suffix_only_base = Regex::new(&pattern_with(NEW_PREFIX_GUARD, BASE_SUFFIX_GUARD))
        .expect("suffix-only base compiles");
    let prefix_guard_effect = disagreements(&new_rule, &prefix_only_base, &wordy);
    let suffix_guard_effect = disagreements(&new_rule, &suffix_only_base, &wordy);
    assert!(
        prefix_guard_effect > 100,
        "restoring only the PREFIX guard changes {prefix_guard_effect} cases; that guard edit is \
         doing nothing and this differential cannot see it"
    );
    assert!(
        suffix_guard_effect > 100,
        "restoring only the SUFFIX guard changes {suffix_guard_effect} cases; that guard edit is \
         doing nothing and this differential cannot see it"
    );
    assert!(
        divergences > 10_000,
        "the reconstructed base rule behaves like the shipped one ({divergences} divergences); \
         this differential is measuring nothing"
    );
    assert!(
        delimited_cases > 10_000,
        "too few delimited cases to be evidence: {delimited_cases}"
    );

    println!(
        "ip.v6 guard differential: {} address forms x {} prefixes x {} suffixes = {cases} cases; \
         {divergences} divergences, all identifier-adjacent; {delimited_cases} delimited cases \
         agree exactly; 0 widenings; 0 recall losses. Guard effect in identifier contexts: \
         prefix {prefix_guard_effect}, suffix {suffix_guard_effect}.",
        recognised.len(),
        prefixes.len(),
        suffixes.len()
    );
}
