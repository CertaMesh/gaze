#[test]
fn rust_library_how_to_nym_example_matches_compiled_source() {
    let page = include_str!("../../../docs/how-to/rust-library.md");
    let start = "<!-- setup-nym-rust-example -->\n```rust\n";
    let end = "\n```\n<!-- /setup-nym-rust-example -->";
    assert_eq!(page.matches(start).count(), 1, "exactly one example");
    let body = page
        .split_once(start)
        .expect("rust-library how-to example start")
        .1
        .split_once(end)
        .expect("rust-library how-to example end")
        .0;
    assert_eq!(body, include_str!("../examples/setup_nym.rs").trim_end());
}
