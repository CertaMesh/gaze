#[test]
fn readme_nym_example_matches_compiled_source() {
    let readme = include_str!("../../../README.md");
    let start = "<!-- setup-nym-rust-example -->\n```rust\n";
    let end = "\n```\n<!-- /setup-nym-rust-example -->";
    let body = readme
        .split_once(start)
        .expect("README example start")
        .1
        .split_once(end)
        .expect("README example end")
        .0;
    assert_eq!(body, include_str!("../examples/setup_nym.rs").trim_end());
}
