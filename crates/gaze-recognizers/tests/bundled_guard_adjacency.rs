//! Adjacency sweep for bundled regexes with a consuming boundary or record guard.
//! `core-extended` aliases `core`; locale packs hold cues, not regex recognizers.

use gaze::{DetectContext, DictionaryBundle, RawMatch, Recognizer, Rulepack, RulepackSource};
use gaze_recognizers::{embedded, RegexDetector};
use gaze_types::LocaleTag;

fn values(id: &str, raw: &str) -> Vec<String> {
    let bundle = if id == "password.field" {
        "secrets"
    } else {
        "core"
    };
    let rulepack = Rulepack::load(RulepackSource::Embedded(embedded(bundle).unwrap())).unwrap();
    let spec = rulepack
        .recognizers
        .into_iter()
        .find(|spec| spec.id == id)
        .unwrap_or_else(|| panic!("missing {id}"));
    let RawMatch::Regex {
        pattern: Some(pattern),
        capture_groups,
        ..
    } = spec.matcher
    else {
        panic!("{id} must have a literal regex");
    };
    let detector = RegexDetector::with_rulepack_fields(
        &pattern,
        spec.class,
        &spec.id,
        spec.locales,
        spec.scoring.base,
        spec.scoring.priority,
        spec.token.family.as_deref().unwrap_or("counter"),
        capture_groups,
        Vec::new(),
        None,
        None,
    )
    .unwrap();
    let dictionaries = DictionaryBundle::default();
    let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);
    detector
        .detect(raw, &ctx)
        .unwrap()
        .into_iter()
        .map(|candidate| raw[candidate.span].to_string())
        .collect()
}

#[test]
fn every_consuming_guard_recognizer_preserves_both_adjacent_values() {
    // Phone and postal guards consume a prefix; IPv6 consumes both edges; birth date and
    // password consume a suffix. IPv4's `\b` is zero-width, included as a control.
    for (id, left, right, separator) in [
        ("phone.structural", "+12025550100", "+12025550101", ","),
        (
            "phone.e164.spaced",
            "+44 7700 900123",
            "+44 7700 900124",
            ",",
        ),
        (
            "phone.national.de",
            "+49 171 3920011",
            "+49 171 3920012",
            ",",
        ),
        ("phone.national.us", "+1 555 0100", "+1 555 0101", ","),
        ("ip.v4", "127.0.0.1", "127.0.0.2", " "),
        ("ip.v6", "::1", "fe80::1", " "),
        ("postal.at_ch", "0000 Narnia", "0001 Utopia", ", "),
        ("postal.ca", "Z1Z 9Z9", "Z2Z 8Z8", ","),
        ("postal.gb", "ZZ9 9ZZ", "ZZ8 8ZZ", ","),
        ("birth_date.cue", "DOB: 1990-02-03", "DOB: 1991-02-03", " "),
        (
            "password.field",
            "password: syntheticA",
            "password: syntheticB",
            "\n",
        ),
    ] {
        for (first, second) in [(left, right), (right, left)] {
            let raw = format!("{first}{separator}{second}");
            let captured = |value: &str| match id {
                "postal.at_ch" => value.split_whitespace().next().unwrap().to_string(),
                "birth_date.cue" | "password.field" => {
                    value.split_once(": ").unwrap().1.to_string()
                }
                _ => value.to_string(),
            };
            assert_eq!(
                values(id, &raw),
                vec![captured(first), captured(second)],
                "{id}: {raw:?}"
            );
        }
    }
}

#[test]
fn locale_packs_add_no_regex_boundary_guards() {
    assert_eq!(embedded("core-extended"), embedded("core"));
    for name in [
        "locale-br",
        "locale-de",
        "locale-en",
        "locale-fr",
        "locale-in",
        "locale-nl",
        "locale-uk",
    ] {
        let rulepack = Rulepack::load(RulepackSource::Embedded(embedded(name).unwrap())).unwrap();
        assert!(
            rulepack.recognizers.is_empty(),
            "{name} gained a recognizer"
        );
    }
}
