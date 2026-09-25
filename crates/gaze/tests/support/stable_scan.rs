use sha2::{Digest, Sha256};

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
            let start = matched.start() + offset;
            bytes[start..start + 8].copy_from_slice(b"00000000");
            let mut hasher = Sha256::new();
            hasher.update(b"gaze-safety-net-token-v3\0");
            hasher.update(&bytes[matched.start()..matched.end()]);
            let digest = hasher.finalize();
            let surrogate = hex::encode(&digest[..4]);
            bytes[start..start + 8].copy_from_slice(surrogate.as_bytes());
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
