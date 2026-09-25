/// Test expectation for the fixed-width session prefix presented to safety nets.
pub fn stable_scan(text: &str) -> String {
    let mut bytes = text.as_bytes().to_vec();
    for matched in gaze::token_shape::pattern().find_iter(text) {
        let token = matched.as_str();
        let offset = if token.starts_with('<') && token.as_bytes().get(9) == Some(&b':') {
            Some(1)
        } else if token.starts_with("email") && token.ends_with("@gaze-fake.invalid") {
            token.find('.').map(|dot| dot + 1)
        } else if token.as_bytes().get(8) == Some(&b':') {
            Some(0)
        } else {
            None
        };
        if let Some(offset) = offset {
            bytes[matched.start() + offset..matched.start() + offset + 8]
                .copy_from_slice(b"00000000");
        }
    }
    String::from_utf8(bytes).unwrap()
}

/// Recover the emitted text in fixtures that captured only the scan view.
#[allow(dead_code)]
pub fn emitted_text(scan: &str, tokens: &[String]) -> String {
    tokens.iter().fold(scan.to_owned(), |text, token| {
        text.replace(&stable_scan(token), token)
    })
}
