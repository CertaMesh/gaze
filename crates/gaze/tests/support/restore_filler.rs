use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;

pub fn filler() -> impl Strategy<Value = String> {
    prop_oneof![
        5 => proptest::string::string_regex(r"[\p{L}\p{N}\p{P}\p{Z}\p{M}\x{1F600}-\x{1F64F}]{0,40}")
            .expect("valid filler regex"),
        2 => "[A-Za-z][a-z]{1,8}_[0-9]{1,5}",
    ]
    .prop_filter("no tracked PII or canonical placeholders", |text| {
        !text.contains('@')
            && !text.contains('<')
            && !text.contains('>')
            && !text.contains("+1-555")
            && !text.contains("+44-7700")
            && !text.contains("+49 1555")
            && !text.contains("Dr. Schmidt")
            && !gaze::token_shape::pattern()
                .find_iter(text)
                .any(|matched| gaze::token_shape::starts_with_session_prefix(matched.as_str()))
    })
}

pub fn assert_identifier_coverage(strategy: impl Strategy<Value = String>) {
    let mut runner = TestRunner::deterministic();
    let identifier = regex::Regex::new(r"\b[A-Za-z][A-Za-z0-9_]*_[0-9]+\b").unwrap();
    let count = (0..128)
        .map(|_| {
            strategy
                .new_tree(&mut runner)
                .expect("generated case")
                .current()
        })
        .filter(|text| identifier.is_match(text))
        .count();
    assert!(count > 0, "filtered corpus lost identifier-shaped literals");
    println!("identifier-shaped cases: {count}/128");
}
