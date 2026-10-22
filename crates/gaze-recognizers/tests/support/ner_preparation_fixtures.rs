pub fn fixtures() -> Vec<(String, String)> {
    let mut fixtures = vec![
        ("short".into(), "Dr. Schmidt visited Berlin.".into()),
        (
            "medium".into(),
            "The example team reviewed the synthetic document. ".repeat(25),
        ),
        ("multi_chunk".into(), "hello world ".repeat(600)),
        (
            "multilingual".into(),
            "grün 東京 مرحبا 🙂 e\u{301} Dr. Schmidt ".repeat(90),
        ),
        (
            "json_tool_call".into(),
            r#"{"tool":"lookup","arguments":{"name":"Dr. Schmidt","email":"alice@example.invalid"}}"#.into(),
        ),
        ("empty".into(), String::new()),
        ("whitespace".into(), " \t\n\r".repeat(100)),
    ];
    for count in [448, 450, 479, 480, 481, 512, 930, 931] {
        fixtures.push((format!("boundary_{count}"), "hello ".repeat(count)));
    }
    for count in [447, 448, 449, 478, 479, 480, 510, 511] {
        fixtures.push((
            format!("wordpiece_edge_{count}"),
            format!(
                "{}pseudonymization {}",
                "hello ".repeat(count),
                "world ".repeat(550)
            ),
        ));
        fixtures.push((
            format!("multibyte_edge_{count}"),
            format!(
                "{}grün東京مرحبا🙂e\u{301} {}",
                "hello ".repeat(count),
                "world ".repeat(550)
            ),
        ));
    }
    let words = [
        "hello",
        "pseudonymization",
        "reversibility",
        "grün",
        "東京",
        "مرحبا",
        "🙂",
        "e\u{301}",
        "alice@example.invalid",
        "[MASK]",
        "a-b",
        "12345",
    ];
    let spaces = [" ", "  ", "\t", "\n", "\r\n", "\u{a0}"];
    let mut state = 20260710u64;
    for index in 0..64 {
        let mut input = String::new();
        for _ in 0..(index * 13) {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            input.push_str(words[(state >> 32) as usize % words.len()]);
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            input.push_str(spaces[(state >> 32) as usize % spaces.len()]);
        }
        fixtures.push((format!("random_{index:02}"), input));
    }
    fixtures
}
