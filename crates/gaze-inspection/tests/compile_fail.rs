#[test]
fn inspection_capabilities_and_sensitive_values_are_sealed() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/*.rs");
}
