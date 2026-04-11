#[test]
fn rawrow_is_not_serialize() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/rawrow_no_serialize.rs");
}
