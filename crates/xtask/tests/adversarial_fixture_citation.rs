#![allow(dead_code)]

use std::{collections::HashSet, fs};

#[path = "../src/fixture_citation.rs"]
mod fixture_citation;

use fixture_citation::{scan_root_with_tests, FixtureCitationError};

#[test]
fn fixture_email_in_production_scope_without_citation_fails_with_file_and_line() {
    let temp = tempfile::tempdir().expect("tempdir");
    let src_dir = temp.path().join("crates/gaze/src");
    fs::create_dir_all(&src_dir).expect("create mock production src");
    let fixture = src_dir.join("lib.rs");
    fs::write(
        &fixture,
        "pub fn fixture() {\n    let _ = \"alice@example.invalid\";\n}\n",
    )
    .expect("write seeded production fixture");

    let result = scan_root_with_tests(temp.path(), &HashSet::new());
    fs::remove_file(&fixture).expect("remove seeded production fixture");

    let error = result.expect_err("uncited fixture-shaped literal must fail");
    let output = error.to_string();
    assert!(
        matches!(error, FixtureCitationError::MissingCitation { .. }),
        "expected missing citation, got {output}"
    );
    assert!(
        output.contains("lib.rs:2"),
        "gate output must include file:line pointer: {output}"
    );
    assert!(
        output.contains("@example.invalid"),
        "gate output must mention matched fixture pattern: {output}"
    );
}

#[test]
fn fixture_email_with_exact_existing_test_citation_passes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let src_dir = temp.path().join("crates/gaze/src");
    fs::create_dir_all(&src_dir).expect("create mock production src");
    fs::write(
        src_dir.join("lib.rs"),
        concat!(
            "// fixture-cited(crates/gaze/tests/email.rs:gaze::tests::email_round_trip)\n",
            "pub const EMAIL: &str = \"alice@example.invalid\";\n",
        ),
    )
    .expect("write cited production fixture");
    let listed_tests = HashSet::from(["gaze::tests::email_round_trip".to_string()]);

    scan_root_with_tests(temp.path(), &listed_tests)
        .expect("exact cited test name should satisfy citation gate");
}

#[test]
fn fixture_email_with_suffix_only_test_match_fails() {
    let temp = tempfile::tempdir().expect("tempdir");
    let src_dir = temp.path().join("crates/gaze/src");
    fs::create_dir_all(&src_dir).expect("create mock production src");
    fs::write(
        src_dir.join("lib.rs"),
        concat!(
            "pub const EMAIL: &str = \"alice@example.invalid\"; ",
            "// fixture-cited(crates/gaze/tests/email.rs:gaze::tests::email_round_trip)\n",
        ),
    )
    .expect("write cited production fixture");
    let listed_tests = HashSet::from(["email_round_trip".to_string()]);

    let error = scan_root_with_tests(temp.path(), &listed_tests)
        .expect_err("suffix-only test names must not satisfy citation gate");
    let output = error.to_string();
    assert!(
        matches!(error, FixtureCitationError::MissingTest { .. }),
        "expected missing exact test, got {output}"
    );
    assert!(
        output.contains("gaze::tests::email_round_trip"),
        "missing-test output must mention the cited exact test name: {output}"
    );
}

#[test]
fn fixture_email_with_renamed_cited_test_fails_adversarially() {
    let temp = tempfile::tempdir().expect("tempdir");
    let src_dir = temp.path().join("crates/gaze/src");
    fs::create_dir_all(&src_dir).expect("create mock production src");
    fs::write(
        src_dir.join("lib.rs"),
        concat!(
            "// fixture-cited(crates/gaze/tests/email.rs:gaze::tests::email_round_trip)\n",
            "pub const EMAIL: &str = \"alice@example.invalid\";\n",
        ),
    )
    .expect("write cited production fixture");
    let listed_tests = HashSet::from(["gaze::tests::email_round_trip_renamed".to_string()]);

    let error = scan_root_with_tests(temp.path(), &listed_tests)
        .expect_err("renaming the cited test must fail the citation gate");
    let output = error.to_string();
    assert!(
        matches!(error, FixtureCitationError::MissingTest { .. }),
        "expected missing cited test after rename, got {output}"
    );
    assert!(
        output.contains("FixtureCitationMissingTest"),
        "gate output must identify missing-test failure: {output}"
    );
}

#[test]
fn fixture_strings_inside_cfg_test_modules_are_outside_production_lint_surface() {
    let temp = tempfile::tempdir().expect("tempdir");
    let src_dir = temp.path().join("crates/gaze/src");
    fs::create_dir_all(&src_dir).expect("create mock production src");
    fs::write(
        src_dir.join("lib.rs"),
        "#[cfg(test)]\nmod tests {\n    const EMAIL: &str = \"alice@example.invalid\";\n}\n",
    )
    .expect("write cfg-test fixture");

    scan_root_with_tests(temp.path(), &HashSet::new())
        .expect("fixture-shaped strings in cfg(test) modules are not production code");
}
