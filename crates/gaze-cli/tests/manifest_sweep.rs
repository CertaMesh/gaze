//! Repeat-value sweep (solo todo 3849), end to end through the `gaze` binary.
//!
//! Once a rule-found value is tokenized, its other copies in the document and
//! in later turns of the same session must be tokenized too. Before the sweep
//! each copy was protected only if a recognizer fired at that exact spot, so a
//! name caught in an email header shipped raw in the body. Cases A-G mirror the
//! v0.15.1 repro table of the concept report; synthetic names only.

use std::io::Write;
use std::process::{Command, Stdio};

use regex::Regex;
use serde_json::{json, Value};
use serial_test::file_serial;
use tempfile::tempdir;

const HEADER: &str = "From: Maria Schneider <maria.schneider@example.invalid>\n";

fn clean(input: &str) -> Value {
    let out = assert_cmd::Command::cargo_bin("gaze")
        .unwrap()
        .args(["clean", "--locale", "en-US"])
        .write_stdin(input.as_bytes().to_vec())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "clean failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("stdout is JSON")
}

fn restore(blob: &str, text: &str) -> String {
    let body = json!({ "session_blob": blob, "text": text }).to_string();
    let out = assert_cmd::Command::cargo_bin("gaze")
        .unwrap()
        .arg("restore")
        .write_stdin(body.into_bytes())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "restore failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).expect("restore stdout is JSON");
    value["text"].as_str().unwrap().to_string()
}

fn name_tokens(text: &str) -> Vec<String> {
    Regex::new(r"<[0-9a-f]{8}:Name_\d+>")
        .unwrap()
        .find_iter(text)
        .map(|m| m.as_str().to_string())
        .collect()
}

/// Cleans `input`, asserts that no forbidden raw substring survives, and that
/// the session blob restores the clean text byte-exact.
fn clean_and_round_trip(input: &str, forbidden: &[&str]) -> String {
    let value = clean(input);
    let clean_text = value["clean_text"].as_str().unwrap().to_string();
    for raw in forbidden {
        assert!(
            !clean_text.contains(raw),
            "{raw:?} shipped raw in {clean_text:?}"
        );
    }
    let blob = value["session_blob"].as_str().unwrap();
    assert_eq!(restore(blob, &clean_text), input, "restore must be exact");
    clean_text
}

#[test]
#[file_serial(gaze_subprocess)]
fn case_a_body_copy_reuses_the_header_token() {
    let input =
        format!("{HEADER}Subject: hello\n\nMy name is Maria Schneider. Please call back.\n");
    let clean = clean_and_round_trip(&input, &["Maria", "Schneider"]);
    let tokens = name_tokens(&clean);
    assert_eq!(tokens.len(), 2, "{clean}");
    assert_eq!(tokens[0], tokens[1], "byte-identical copy keeps the token");
}

#[test]
#[file_serial(gaze_subprocess)]
fn case_b_lowercase_copy_gets_a_sibling_token() {
    let input = format!("{HEADER}\nhi, this is maria schneider again.\n");
    let clean = clean_and_round_trip(&input, &["maria schneider", "Maria Schneider"]);
    let tokens = name_tokens(&clean);
    assert_eq!(tokens.len(), 2, "{clean}");
    assert_ne!(
        tokens[0], tokens[1],
        "a variant spelling restores to its own bytes, so it needs its own token"
    );
}

#[test]
#[file_serial(gaze_subprocess)]
fn case_c_title_case_surname_part_is_swept() {
    // `Schneider` is an everyday German noun (tailor) and on the closed
    // common-word list, so the surname part is probed with another name.
    let input = "From: Lena Kowalski <lena.kowalski@example.invalid>\n\nPlease forward this to Ms Kowalski today.\n";
    clean_and_round_trip(input, &["Kowalski"]);
}

#[test]
#[file_serial(gaze_subprocess)]
fn case_d_title_case_first_name_part_is_swept() {
    let input = format!("{HEADER}\nThanks, Maria\n");
    clean_and_round_trip(&input, &["Maria"]);
}

#[test]
#[file_serial(gaze_subprocess)]
fn case_e_uppercase_copy_is_one_token() {
    let input = format!("{HEADER}\nSIGNED: MARIA SCHNEIDER\n");
    let clean = clean_and_round_trip(&input, &["MARIA", "SCHNEIDER"]);
    assert_eq!(name_tokens(&clean).len(), 2, "{clean}");
    assert!(clean.contains("SIGNED: <"), "{clean}");
}

#[test]
#[file_serial(gaze_subprocess)]
fn case_g_repeated_email_keeps_one_token() {
    let input = "Contact maria.schneider@example.invalid or write again to maria.schneider@example.invalid.\n";
    let clean = clean_and_round_trip(input, &["maria.schneider@"]);
    let email = Regex::new(r"<[0-9a-f]{8}:Email_\d+>").unwrap();
    let tokens = email
        .find_iter(&clean)
        .map(|m| m.as_str())
        .collect::<Vec<_>>();
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0], tokens[1]);
}

/// Stated trade-off: a lone lower-case name part is not swept, because
/// matching lower-case single words would tokenize ordinary words that happen
/// to be names. If this starts failing, the trade-off changed on purpose.
#[test]
#[file_serial(gaze_subprocess)]
fn stated_leak_lone_lowercase_part_stays_raw() {
    let input = format!("{HEADER}\nthanks maria\n");
    let clean = clean(&input)["clean_text"].as_str().unwrap().to_string();
    assert!(clean.ends_with("thanks maria\n"), "{clean}");
}

#[test]
#[file_serial(gaze_subprocess)]
fn common_word_parts_are_not_swept() {
    // `Rose` and `May` are on the closed stoplist: the header name is still
    // tokenized, the ordinary words elsewhere are not.
    let input = "From: Rose May <rose.may@example.invalid>\n\nThe Rose garden opens in May.\n";
    let clean = clean(input)["clean_text"].as_str().unwrap().to_string();
    assert!(
        clean.ends_with("The Rose garden opens in May.\n"),
        "{clean}"
    );
    assert!(!clean.contains("From: Rose May"), "{clean}");
}

/// A four-digit postcode is found through its city anchor. Copying the bare
/// digits would tokenize every year and room number, so short digit runs do
/// not propagate (review 668 F1).
#[test]
#[file_serial(gaze_subprocess)]
fn short_postcode_does_not_sweep_a_year() {
    let input = "Adresse: Rue du Lac 1, 2024 Neuchâtel\nIm Jahr 2024 zogen wir um.\n";
    let out = assert_cmd::Command::cargo_bin("gaze")
        .unwrap()
        .args(["clean", "--locale", "de-CH"])
        .write_stdin(input.as_bytes().to_vec())
        .output()
        .unwrap();
    assert!(out.status.success());
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    let clean = value["clean_text"].as_str().unwrap();
    assert!(
        !clean.contains("2024 Neuchâtel"),
        "the anchored postcode is still found: {clean}"
    );
    assert!(clean.contains("Im Jahr 2024 zogen"), "{clean}");
}

fn write_core_only_policy() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    std::fs::write(
        &path,
        "schema_version = \"0.1.0\"\n[session]\nscope = \"conversation\"\n\
         [policy.rulepacks]\nbundled = [\"core\"]\n\
         [[rule]]\nkind = \"default\"\naction = \"tokenize\"\n",
    )
    .unwrap();
    (dir, path)
}

#[test]
#[file_serial(gaze_subprocess)]
fn daemon_later_turns_are_swept_with_earlier_turn_values() {
    let (_dir, policy) = write_core_only_policy();
    let audit_dir = tempdir().unwrap();
    let audit_db = audit_dir.path().join("audit.db");
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
        .args([
            "daemon",
            "--policy",
            policy.to_str().unwrap(),
            "--audit-db",
            audit_db.to_str().unwrap(),
            "--locale",
            "en-US",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        for text in [
            format!("{HEADER}Hello.\n"),
            "Maria Schneider asked for a callback.\n".to_string(),
            "maria schneider asked again.\n".to_string(),
        ] {
            writeln!(stdin, "{}", json!({ "session_id": "s1", "text": text })).unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let turns = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap()["clean_text"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(turns.len(), 3);
    let first = name_tokens(&turns[0]);
    let second = name_tokens(&turns[1]);
    let third = name_tokens(&turns[2]);
    assert_eq!(first.len(), 1, "{turns:?}");
    assert_eq!(second, first, "turn 2 reuses the turn-1 token: {turns:?}");
    assert_eq!(third.len(), 1, "turn 3 is swept: {turns:?}");
    assert_ne!(third, first, "variant spelling gets a sibling token");
    assert!(!turns[2].contains("maria"), "{turns:?}");

    let conn = rusqlite::Connection::open(audit_db).unwrap();
    let swept: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM redaction_log WHERE decided_by = 'manifest_sweep'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(swept, 2, "each swept copy writes one audit row");
}
