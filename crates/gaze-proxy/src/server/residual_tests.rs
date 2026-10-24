use super::*;
use gaze::{Action, DefaultRule, PiiClass, RawDocument, Scope};
use gaze_recognizers::RegexDetector;

fn oracle(pipeline: &Pipeline, transaction: &SessionTransaction<'_>, wire: &str) -> bool {
    let full = transaction.restore_strict_text(wire).unwrap();
    let entries = transaction.snapshot_entries();
    let mut covered = vec![false; full.len()];
    let mut cursor = 0;
    let mut expanded_cursor = 0;
    for range in owned_token_ranges(wire, &transaction.tokens()) {
        expanded_cursor += range.start - cursor;
        let value = entries
            .iter()
            .find(|entry| entry.token == wire[range.clone()])
            .unwrap();
        covered[expanded_cursor..expanded_cursor + value.raw.len()].fill(true);
        expanded_cursor += value.raw.len();
        cursor = range.end;
    }
    pipeline
        .staged_rule_repeat_candidates(transaction, &full)
        .unwrap()
        .iter()
        .all(|hit| rule_hit_is_covered(&full, &covered, hit.span.clone()))
}

#[test]
fn reviewer_31944_unicode_and_many_token_cases_equal_full_matcher() {
    let pipeline = Pipeline::builder()
        .detector(RegexDetector::new(".+", PiiClass::Name).unwrap())
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let values = [
        "Alfa",
        "Beta",
        "Gamma",
        "Delta",
        "Alfa Beta Gamma Delta",
        "Straße",
        "İpek Öztürk",
        "Ａｌｆａ Ｂｅｔａ",
        "Vasquez",
        "Synthetic Person",
        "Person Synthetic",
    ];
    for value in values {
        pipeline
            .redact(&session, RawDocument::Text(value.to_owned()))
            .unwrap();
    }
    let transaction = session.begin_transaction();
    let before = transaction.snapshot_entries();
    let token = |name: &str| {
        before
            .iter()
            .find(|entry| entry.raw == name)
            .unwrap()
            .token
            .clone()
    };
    let surfaced = HashSet::new();
    let dictionaries = DictionaryBundle::default();
    let scan = RequestResidualScan {
        pipeline: &pipeline,
        transaction: &transaction,
        expansion_limit_bytes: 32 * 1024 * 1024,
        surfaced: &surfaced,
        locale_chain: &[LocaleTag::Global],
        dictionaries: &dictionaries,
    };
    let tails = [
        "",
        " delta",
        "-Vasquez",
        "-Ellery-Ellery",
        " STRASSE",
        " i\u{307}pek öztürk",
        "\u{200c}Gamma",
        " .",
    ];
    let gaps = [
        " ", "\t\n", "\u{3000}", "\u{a0}", "\u{200c}", "\u{200d}", " x ", " S ", "-", "’", "",
    ];
    let mut count = 0;
    for left in values {
        for right in values {
            for gap in gaps {
                for suffix in tails {
                    for prefix in ["", "https://example.invalid/", "www."] {
                        let wire = format!(
                            "{prefix}{} {gap}{} {gap}{} {gap}{}{suffix}",
                            token(left),
                            token("Beta"),
                            token("Gamma"),
                            token(right)
                        );
                        assert_eq!(
                            scan.rule_repeats_are_covered(&wire).unwrap(),
                            oracle(&pipeline, &transaction, &wire),
                            "{wire:?}"
                        );
                        count += 1;
                    }
                }
            }
        }
    }
    assert_eq!(count, 31_944);
    assert_eq!(transaction.snapshot_entries(), before);
    println!("Reviewer differential cases: {count}");
}

#[test]
fn randomized_owned_carriers_equal_full_matcher_below_resource_cap() {
    let pipeline = Pipeline::builder()
        .detector(RegexDetector::new(".+", PiiClass::Name).unwrap())
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let values = [
        "Alfa".to_owned(),
        "Beta".to_owned(),
        "Synthetic Person".to_owned(),
        "Person Synthetic".to_owned(),
        "İpek Öztürk".to_owned(),
        "Ａｌｆａ Ｂｅｔａ".to_owned(),
        "Vasquez".to_owned(),
        "Straße".to_owned(),
    ];
    for value in &values {
        pipeline
            .redact(&session, RawDocument::Text(value.clone()))
            .unwrap();
    }
    session
        .tokenize(&PiiClass::Name, &"X".repeat(2048))
        .unwrap();
    let transaction = session.begin_transaction();
    let before = transaction.snapshot_entries();
    let surfaced = HashSet::new();
    let dictionaries = DictionaryBundle::default();
    let scan = RequestResidualScan {
        pipeline: &pipeline,
        transaction: &transaction,
        expansion_limit_bytes: 32 * 1024 * 1024,
        surfaced: &surfaced,
        locale_chain: &[LocaleTag::Global],
        dictionaries: &dictionaries,
    };
    let public = [
        "",
        " ",
        "\t\n",
        "\u{3000}",
        ".",
        ",",
        "42",
        "https://example.invalid/",
    ];
    let private = [
        " synthetic person",
        "-Vasquez",
        "\u{200c}",
        "\u{200d}",
        " i\u{307}pek öztürk",
        "-Ellery",
    ];
    let mut state = 20261024_u64;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 32) as usize
    };
    let short: Vec<_> = before
        .iter()
        .filter(|entry| entry.raw.len() < 100)
        .collect();
    let long = before
        .iter()
        .find(|entry| entry.raw == "X".repeat(2048))
        .unwrap();
    let mut safe = 0;
    for case in 0..4096 {
        let public_only = case % 2 == 0;
        let count = if case < 4 { 1025 } else { 1 + next() % 24 };
        let mut wire = String::new();
        for _ in 0..count {
            wire.push_str(if case < 4 || (case % 64 == 0 && next() % 8 == 0) {
                &long.token
            } else {
                &short[next() % short.len()].token
            });
            wire.push_str(if case < 4 {
                if public_only {
                    ""
                } else {
                    " "
                }
            } else if public_only {
                public[next() % public.len()]
            } else {
                private[next() % private.len()]
            });
        }
        if case < 4 {
            wire.push_str(if public_only {
                "."
            } else {
                " synthetic person"
            });
        }
        let expected = oracle(&pipeline, &transaction, &wire);
        assert_eq!(
            scan.rule_repeats_are_covered(&wire).unwrap(),
            expected,
            "case {case}"
        );
        if public_only {
            assert!(expected, "public-only carrier {case} refused");
            safe += 1;
        }
    }
    assert_eq!(safe, 2048);
    assert_eq!(transaction.snapshot_entries(), before);
    assert_eq!(session.prefix_cache_entry_count(), 0);
    println!("Randomized differential cases: 4096; public-only admission checks: {safe}");
}
