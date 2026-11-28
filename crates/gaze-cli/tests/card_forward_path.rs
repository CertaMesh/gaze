//! Forward path (the text sent to the model): a payment card with digits touching it is
//! tokenized whole, with no policy. The same shapes run under the `gaze setup`
//! policy in `src/commands/setup.rs`.

use assert_cmd::Command;
use serde_json::Value;

#[path = "support/token_assertions.rs"]
mod token_assertions;
use token_assertions::without_tokens;

include!("support/card_shapes.rs");

#[test]
fn card_codes_bind_to_the_card_not_to_other_short_numbers() {
    for (text, expected) in [
        ("Karte 4111 1111 1111 1111 123 (CVV)", "Karte \0 \0 (CVV)"),
        ("Karte 4111 1111 1111 1111 １２３", "Karte \0 \0"),
        ("Karte 4111111111111111 123", "Karte \0 \0"),
        ("Karte 4111111111111112 1234", "Karte 4111111111111112 1234"),
        ("Amex 3782 822463 10005 1234", "Amex \0 \0"),
        ("Diners 3056 930902 5904 123", "Diners \0 \0"),
        (
            "Kreditkarte: 4111111111111111, CVC: 094. Seite 094",
            "Kreditkarte: \0, CVC: \0. Seite 094",
        ),
        ("Card: 4111111111111112 0123 (CVV)", "Card: \0 \0 (CVV)"),
        ("Card: 4111111111111112 0123 (page)", "Card: \0 0123 (page)"),
        ("Karte Nr. 4111111111111112 0123", "Karte Nr. \0 \0"),
        (
            "Kreditkarte: 4111111111111112 und die Prüfnummer ist 094",
            "Kreditkarte: \0 und die Prüfnummer ist \0",
        ),
        ("Card: 4111111111111112 (CVV: 094)", "Card: \0 (CVV: \0)"),
        (
            "4111111111111111; CVV is 094 (on the back)",
            "\0; CVV is \0 (on the back)",
        ),
        (
            "Card: 4532-7812-3456-7890-5555",
            "Card: 4532-7812-3456-7890-5555",
        ),
        ("Card: 378282246310005 120.50", "Card: \0 120.50"),
        ("Card: 378282246310005; CVV: 12345", "Card: \0; CVV: 12345"),
    ] {
        let out = Command::cargo_bin("gaze")
            .unwrap()
            .arg("clean")
            .write_stdin(text)
            .output()
            .unwrap();
        assert!(out.status.success());
        let result: Value = serde_json::from_slice(&out.stdout).unwrap();
        let clean = result["clean_text"].as_str().unwrap();
        assert_eq!(without_tokens(clean), expected);
        let request = serde_json::json!({"text": clean, "session_blob": result["session_blob"]});
        let restored = Command::cargo_bin("gaze")
            .unwrap()
            .arg("restore")
            .write_stdin(request.to_string())
            .output()
            .unwrap();
        assert!(restored.status.success());
        let body: Value = serde_json::from_slice(&restored.stdout).unwrap();
        assert_eq!(body["text"], text);
    }
}

#[test]
fn clean_without_policy_tokenizes_a_card_with_touching_digits() {
    let mut failures = Vec::new();
    for (before, card, after) in CARD_SHAPES {
        let input = format!("{before}{card}{after}");
        let out = Command::cargo_bin("gaze")
            .unwrap()
            .arg("clean")
            .write_stdin(input.as_bytes().to_vec())
            .output()
            .unwrap();
        assert!(out.status.success(), "{input:?}: {out:?}");
        let v: Value = serde_json::from_slice(&out.stdout).expect("stdout is JSON");
        let clean = v["clean_text"].as_str().unwrap();
        let expected = format!("{before}\0{after}");
        if without_tokens(clean) != expected || !clean.contains(":Custom:credit_card_") {
            failures.push(format!("{input:?} -> {clean:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "leaked or mis-scoped:\n{}",
        failures.join("\n")
    );
}
