use super::*;
use gaze::{Action, Candidate, ConflictTier, DefaultRule, DetectContext, EvidenceKind, Recognizer};

struct Header;
impl Recognizer for Header {
    fn id(&self) -> &str {
        "synthetic-header"
    }
    fn evidence(&self) -> EvidenceKind {
        EvidenceKind::Rule
    }
    fn supported_class(&self) -> &gaze::PiiClass {
        &gaze::PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(
        &self,
        input: &str,
        _: &DetectContext<'_>,
    ) -> Result<Vec<Candidate>, gaze_types::DetectError> {
        Ok(regex::Regex::new(r"(?m)^From: ([^<\n]+?) <")
            .unwrap()
            .captures_iter(input)
            .map(|caps| {
                Candidate::new(
                    caps.get(1).unwrap().range(),
                    gaze::PiiClass::Name,
                    self.id(),
                    1.0,
                    0,
                    None,
                    "counter",
                    self.id(),
                    ConflictTier::None,
                    vec![],
                )
            })
            .collect())
    }
}

fn pipeline() -> Pipeline {
    Pipeline::builder()
        .recognizer(Header)
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap()
}

/// Deliberately independent of the production matcher, normalization and projection.
/// Rebuild the original and one ownership bit per byte directly from the manifest.
fn ownership(transaction: &SessionTransaction<'_>, protected: &str) -> (String, Vec<bool>) {
    let entries = transaction.snapshot_entries();
    let mut raw = String::new();
    let mut mask = Vec::new();
    for part in regex::Regex::new(r"<[0-9a-f]{8}:[^<>]+>|[^<]+|<")
        .unwrap()
        .find_iter(protected)
    {
        let (value, owned) = match entries.iter().find(|e| e.token == part.as_str()) {
            Some(entry) => (entry.raw.as_str(), true),
            None => (part.as_str(), false),
        };
        raw.push_str(value);
        mask.extend(std::iter::repeat_n(owned, value.len()));
    }
    (raw, mask)
}

#[test]
fn randomized_overlap_union_restores_and_covers_every_known_value() {
    let pipeline = pipeline();
    let dictionaries = DictionaryBundle::default();
    let locales = [LocaleTag::Global];
    let words = [
        "Maria",
        "Schneider",
        "Johanna",
        "Brenner",
        "Tobias",
        "Lorena",
    ];
    let mut random = 0x1c83_aa39_5276_004du64;
    let mut pick = |limit: usize| {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        random as usize % limit
    };
    for case in 0..4096 {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let mut tx = session.begin_transaction();
        // 1..4 independently known values, with contained, prefix, suffix and
        // chained A-B/B-C overlaps. Each has at least two words for folded matching.
        let mut known = vec![];
        for _ in 0..1 + pick(4) {
            let first = pick(words.len() - 1);
            let end = first + 2 + pick(words.len() - first - 1);
            let value = words[first..end].join(" ");
            pipeline
                .discover_staged_rule_sources(
                    &mut tx,
                    &format!("From: {value} <m@example.invalid>"),
                    &locales,
                    &dictionaries,
                )
                .unwrap();
            known.push(first..end);
        }
        let separator = [" ", "\t", "\u{a0}", "\u{200c} "][pick(4)];
        let mut original = String::from("Public ");
        let mut word_ranges = vec![];
        for (i, word) in words.iter().enumerate() {
            if i > 0 {
                original.push_str(separator);
            }
            let start = original.len();
            let spelling = match pick(4) {
                0 => word.to_lowercase(),
                1 => word.to_uppercase(),
                2 => word
                    .chars()
                    .enumerate()
                    .map(|(i, c)| {
                        if i % 2 == 0 {
                            c.to_ascii_uppercase()
                        } else {
                            c.to_ascii_lowercase()
                        }
                    })
                    .collect(),
                _ => word.to_string(),
            };
            original.push_str(&spelling);
            word_ranges.push(start..original.len());
        }
        let benign = "; thanks maria; https://example.invalid/Maria/Schneider; public.";
        original.push_str(benign);
        // 1..3 immutable owned tokens; permit partial-word boundaries too.
        let count = 1 + pick(3);
        let mut input = String::new();
        let mut cursor = 0;
        let mut tokens = vec![];
        for slot in 0..count {
            let index = slot * 2 + pick(2);
            let word = &word_ranges[index];
            let start = word.start + pick(2);
            let end = word.end - pick(2);
            input.push_str(&original[cursor..start]);
            let token = tx
                .tokenize(&gaze::PiiClass::Name, &original[start..end])
                .unwrap();
            input.push_str(&token);
            tokens.push(token);
            cursor = end;
        }
        input.push_str(&original[cursor..]);
        let mut boundary =
            PipelineRequestPseudonymizer::new(&pipeline, &locales, &dictionaries, &mut tx);
        let protected = boundary
            .protect(&input)
            .unwrap_or_else(|e| panic!("case {case}: {e:?}"));
        boundary.verify_rule_coverage(&protected).unwrap();
        assert!(
            protected.ends_with(benign),
            "benign counterweight, case {case}: {protected}"
        );
        for token in tokens {
            assert!(protected.contains(&token), "owned identity, case {case}");
        }
        let (restored, mask) = ownership(&tx, &protected);
        assert_eq!(restored, original, "case {case}");
        assert_eq!(tx.restore_strict_text(&protected).unwrap(), original);
        for range in known {
            let raw = word_ranges[range.start].start..word_ranges[range.end - 1].end;
            assert!(
                mask[raw.clone()].iter().all(|byte| *byte),
                "uncovered {raw:?}, case {case}: {protected}"
            );
        }
    }
}

#[test]
fn final_coverage_rejects_unprotected_contained_and_chained_occurrences() {
    let pipeline = pipeline();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut tx = session.begin_transaction();
    let dictionaries = DictionaryBundle::default();
    let locales = [LocaleTag::Global];
    for value in ["Maria Schneider", "Schneider Johanna", "Johanna Brenner"] {
        pipeline
            .discover_staged_rule_sources(
                &mut tx,
                &format!("From: {value} <m@example.invalid>"),
                &locales,
                &dictionaries,
            )
            .unwrap();
    }
    let token = tx
        .tokenize(&gaze::PiiClass::Name, "Maria Schneider")
        .unwrap();
    let input = format!("Hello {token} johanna brenner");
    let mut boundary =
        PipelineRequestPseudonymizer::new(&pipeline, &locales, &dictionaries, &mut tx);
    assert!(
        boundary.verify_rule_coverage(&input).is_err(),
        "raw overlap accepted"
    );
    let protected = boundary.protect(&input).unwrap();
    assert!(boundary.verify_rule_coverage(&protected).is_ok());
}
