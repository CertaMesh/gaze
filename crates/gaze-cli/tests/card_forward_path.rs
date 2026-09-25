//! Forward path (the text sent to the model): a payment card with digits touching it is
//! tokenized whole, with no policy (solo todo 3843). The same shapes run under the `gaze setup`
//! policy in `src/commands/setup.rs`.

use assert_cmd::Command;
use serde_json::Value;

#[path = "support/token_assertions.rs"]
mod token_assertions;
use token_assertions::without_tokens;

include!("support/card_shapes.rs");

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
