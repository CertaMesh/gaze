//! Forward path, no policy: every 1- to 4-digit number (`0` to `9999`, leading zeros included)
//! written before a card, separated by a space, glued by a ZERO WIDTH JOINER, or glued in
//! fullwidth digits. No digit of the card may reach the model raw (solo todo 3843 round 2: a
//! prefix whose window passes Luhn by chance used to win over the card and leave its tail raw).

use gaze::{CleanDocument, LocaleTag, PiiClass, RawDocument, Scope, Session};

const CARDS: [&str; 4] = [
    "4111 1111 1111 1111",
    "3782 822463 10005",
    "3056 930902 5904",
    "4111 1111 1111 1111 003",
];

fn fullwidth(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '0'..='9' => char::from_u32(ch as u32 + 0xFEE0).expect("fullwidth digit"),
            _ => ch,
        })
        .collect()
}

/// Every prefixed card as `(text, card byte range)`.
fn cases() -> Vec<(String, std::ops::Range<usize>)> {
    let mut cases = Vec::new();
    for width in 1..=4usize {
        for number in 0..10usize.pow(width as u32) {
            let digits = format!("{number:0width$}");
            for prefix in [
                format!("{digits} "),
                format!("{digits}\u{200D}"),
                fullwidth(&digits),
            ] {
                for card in CARDS {
                    let text = format!("Karte {prefix}{card} ok");
                    let start = "Karte ".len() + prefix.len();
                    cases.push((text, start..start + card.len()));
                }
            }
        }
    }
    cases
}

#[test]
fn no_card_digit_survives_any_prefix() {
    let pipeline = gaze_assembly::CorePipelineConfig::new()
        .build()
        .expect("core pipeline")
        .into_pipeline();
    let card_class = PiiClass::custom("credit_card").expect("valid class");
    let cases = cases();
    let mut leaked = 0usize;
    let mut examples = Vec::new();
    let mut by_shape = std::collections::BTreeMap::<String, usize>::new();
    // Cases go through in batches, one per line; the letters around each keep the digit runs
    // apart.
    for batch in cases.chunks(400) {
        let mut document = String::new();
        let mut offsets = Vec::new();
        for (text, card) in batch {
            offsets.push(document.len() + card.start..document.len() + card.end);
            document.push_str(text);
            document.push('\n');
        }
        let session = Session::new(Scope::Ephemeral).expect("session");
        let (clean, manifest, _) = pipeline
            .clean_with_safety_net(
                &session,
                RawDocument::Text(document.clone()),
                &[LocaleTag::Global],
            )
            .expect("clean");
        assert!(matches!(clean, CleanDocument::Text(_)));
        let covered: Vec<_> = manifest
            .iter()
            .filter(|span| span.class == card_class)
            .map(|span| span.raw_span.clone())
            .collect();
        for ((text, _), card) in batch.iter().zip(offsets) {
            let raw_digit = document[card.clone()]
                .char_indices()
                .filter(|(_, ch)| ch.is_ascii_digit())
                .any(|(at, _)| !covered.iter().any(|span| span.contains(&(card.start + at))));
            if raw_digit {
                leaked += 1;
                let join = if text.contains("\u{200D}") {
                    "zwj"
                } else if text.contains(|ch: char| ('０'..='９').contains(&ch)) {
                    "fullwidth"
                } else {
                    "space"
                };
                let card = CARDS
                    .iter()
                    .filter(|card| text.contains(*card))
                    .max_by_key(|card| card.len())
                    .expect("case holds a card");
                *by_shape.entry(format!("{card}/{join}")).or_default() += 1;
                if examples.len() < 12 {
                    examples.push(text.clone());
                }
            }
        }
    }
    assert_eq!(
        leaked,
        0,
        "{leaked} of {} prefixed cards left a digit raw ({by_shape:?}), e.g. {examples:?}",
        cases.len()
    );
}
