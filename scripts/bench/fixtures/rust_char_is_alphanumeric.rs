// Regenerates rust-char-is-alphanumeric.json: every code point Rust's
// `char::is_alphanumeric` accepts, which is what Gaze's `is_inside_word`
// tests. Build it with the repo's pinned toolchain:
//   rustc -O rust_char_is_alphanumeric.rs -o /tmp/alnum
//   /tmp/alnum > rust-char-is-alphanumeric.json

fn main() {
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for cp in 0u32..=0x10FFFF {
        if let Some(c) = char::from_u32(cp) {
            if c.is_alphanumeric() {
                match ranges.last_mut() {
                    Some(last) if last.1 + 1 == cp => last.1 = cp,
                    _ => ranges.push((cp, cp)),
                }
            }
        }
    }
    let total: u32 = ranges.iter().map(|(a, b)| b - a + 1).sum();
    let (major, minor, update) = char::UNICODE_VERSION;
    let body: Vec<String> = ranges.iter().map(|(a, b)| format!("[{a},{b}]")).collect();
    println!(
        "{{\"generator\":\"scripts/bench/fixtures/rust_char_is_alphanumeric.rs\",\
         \"unicode_version\":\"{major}.{minor}.{update}\",\"code_points\":{total},\
         \"ranges\":[{}]}}",
        body.join(",")
    );
}
