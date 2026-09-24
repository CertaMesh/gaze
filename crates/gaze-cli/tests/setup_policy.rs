#![cfg(feature = "setup")]

use std::collections::BTreeSet;
use std::fs;

use assert_cmd::Command;
use gaze::Rulepack;

#[path = "support/token_assertions.rs"]
mod token_assertions;
use token_assertions::without_tokens;

// Validator-passing synthetic values are shared with the core CLI fixture
// inventory in index_cli.rs. The class set below is checked against the packs.
// fixture-cited(crates/gaze-cli/tests/index_cli.rs:index_ingest_tokenizes_core_identifiers_so_search_never_shows_them_raw)
const CASES: &[(&str, &str, &str)] = &[
    (
        "email",
        "Contact alice@example.invalid",
        "alice@example.invalid",
    ),
    (
        "name",
        "From: Ada Example <ada@example.invalid>",
        "Ada Example",
    ),
    ("phone", "Phone +1-555-0100", "+1-555-0100"),
    (
        "iban",
        "IBAN AT61 1904 3002 3457 3201",
        "AT61 1904 3002 3457 3201",
    ),
    (
        "credit_card",
        "Card 4111 1111 1111 1111",
        "4111 1111 1111 1111",
    ),
    ("ip_address", "Router IP 10.1.2.3", "10.1.2.3"),
    (
        "eth_address",
        "Wallet 0x52908400098527886E0F7030069857D2E4169EE7",
        "0x52908400098527886E0F7030069857D2E4169EE7",
    ),
    ("aadhaar", "Aadhaar 2345 6789 0124", "2345 6789 0124"),
    ("nir", "NIR 190010100100058", "190010100100058"),
    ("steuer_id", "Steuer-ID 48 954 371 207", "48 954 371 207"),
    ("vat_id", "USt-IdNr DE294581776", "DE294581776"),
    ("bsn", "BSN 111222333", "111222333"),
    ("cpf", "CPF 529.982.247-25", "529.982.247-25"),
    ("cnpj", "CNPJ 04.252.011/0001-10", "04.252.011/0001-10"),
    ("nhs_number", "NHS number 943 476 5919", "943 476 5919"),
    ("ssn", "SSN 123-45-6789", "123-45-6789"),
    ("nino", "National Insurance Number AB123456C", "AB123456C"),
    ("pan", "PAN card ABCPA1234F", "ABCPA1234F"),
    ("postal_code", "Mailing code Z1Z 9Z9", "Z1Z 9Z9"),
    (
        "url",
        "Site https://example.invalid/orders",
        "https://example.invalid/orders",
    ),
    ("tax_number", "Tax number 123-456-789", "123-456-789"),
    ("driver_license", "Driver's license D1234567", "D1234567"),
    ("national_id", "National ID number AB123456", "AB123456"),
    ("passport", "Passport ID A1234567", "A1234567"),
    ("birth_date", "DOB: 1990-02-03", "1990-02-03"),
];

#[test]
#[ignore = "requires GAZE_SETUP_TEST_MODEL_DIR pointing to a verified pinned Davlan bundle"]
fn setup_cli_clean_tokenizes_every_bundled_class() {
    let model_dir = std::env::var("GAZE_SETUP_TEST_MODEL_DIR")
        .expect("set GAZE_SETUP_TEST_MODEL_DIR to a verified pinned Davlan bundle");
    let dir = tempfile::tempdir().unwrap();
    let policy_path = dir.path().join("gaze.toml");
    let setup = Command::cargo_bin("gaze")
        .unwrap()
        .args(["setup", "--non-interactive", "--policy-out"])
        .arg(&policy_path)
        .args(["--model-dir", &model_dir, "--force"])
        .output()
        .unwrap();
    assert!(
        setup.status.success(),
        "setup failed: {}",
        String::from_utf8_lossy(&setup.stderr)
    );

    let policy = fs::read_to_string(&policy_path).unwrap();
    assert!(policy.contains("action = \"tokenize\""));
    let declared = gaze_recognizers::embedded_rulepacks()
        .filter(|(name, _)| *name != "secrets")
        .flat_map(|(_, contents)| {
            Rulepack::parse_bundled(contents)
                .unwrap()
                .recognizers
                .into_iter()
                .filter(|recognizer| recognizer.enabled)
                .map(|recognizer| recognizer.class.to_canonical_str())
        })
        .collect::<BTreeSet<_>>();
    let covered = CASES
        .iter()
        .map(|case| {
            if matches!(case.0, "email" | "name") {
                case.0.to_string()
            } else {
                format!("custom:{}", case.0)
            }
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(covered, declared);

    let input = CASES
        .iter()
        .map(|case| case.1)
        .collect::<Vec<_>>()
        .join("\n");
    let output = Command::cargo_bin("gaze")
        .unwrap()
        .args(["clean", "--policy"])
        .arg(&policy_path)
        .write_stdin(input.into_bytes())
        .output()
        .unwrap();
    assert!(output.status.success(), "clean failed");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let clean = without_tokens(value["clean_text"].as_str().unwrap());
    for (class, _, raw) in CASES {
        assert!(!clean.contains(raw), "raw fixture survived for {class}");
    }
}
