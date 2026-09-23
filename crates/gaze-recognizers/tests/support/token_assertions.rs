/// Ignore emitted token bytes when checking whether raw fixture text survived.
pub fn without_tokens(cleaned: &str) -> String {
    gaze::token_shape::pattern()
        .replace_all(cleaned, "\0")
        .into_owned()
}
