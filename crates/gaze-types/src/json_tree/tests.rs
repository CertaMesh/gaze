use serde_json::{json, Map, Value};

use super::*;

fn tree(input: &str) -> JsonTree {
    parse(input.as_bytes(), JsonTreeLimits::default()).unwrap_or_else(|_| panic!("strict parse"))
}

fn error(input: &str, limits: JsonTreeLimits) -> JsonTreeError {
    match parse(input.as_bytes(), limits) {
        Ok(_) => panic!("strict parse must refuse"),
        Err(error) => error,
    }
}

/// The value serde_json reads for this node. Numbers re-parse their exact spelling.
fn to_value(node: &JsonNode) -> Value {
    match &node.kind {
        JsonKind::Object(members) => Value::Object(
            members
                .iter()
                .map(|member| (member.key.value.clone(), to_value(&member.value)))
                .collect::<Map<_, _>>(),
        ),
        JsonKind::Array(values) => Value::Array(values.iter().map(to_value).collect()),
        JsonKind::String(value) => Value::String(value.value.clone()),
        JsonKind::Number(spelling) => serde_json::from_str(spelling).unwrap(),
        JsonKind::Bool(value) => Value::Bool(*value),
        JsonKind::Null => Value::Null,
    }
}

fn pointer(path: &FieldPath) -> String {
    path.segments()
        .iter()
        .map(|segment| format!("/{}", segment.replace('~', "~0").replace('/', "~1")))
        .collect()
}

/// Every node's span re-parses to its value, and its path addresses it from the root.
fn assert_spans_and_paths(input: &str, root: &Value, node: &JsonNode, seen: &mut usize) {
    *seen += 1;
    let at_span: Value = serde_json::from_str(&input[node.span.clone()]).unwrap();
    assert_eq!(at_span, to_value(node), "span must hold exactly the value");
    assert_eq!(
        root.pointer(&pointer(&node.path)),
        Some(&at_span),
        "path must address the value"
    );
    match &node.kind {
        JsonKind::Object(members) => {
            for member in members {
                assert_spans_and_paths(input, root, &member.value, seen);
            }
        }
        JsonKind::Array(values) => {
            for value in values {
                assert_spans_and_paths(input, root, value, seen);
            }
        }
        _ => {}
    }
}

fn assert_oracle(input: &str) {
    let expected: Value = serde_json::from_str(input).unwrap();
    let parsed = tree(input);
    assert_eq!(
        to_value(&parsed.root),
        expected,
        "tree must match serde_json"
    );
    let mut nodes = 0;
    assert_spans_and_paths(input, &expected, &parsed.root, &mut nodes);
    let keys = count_keys(&expected);
    assert_eq!(
        parsed.occurrence_count,
        nodes + keys,
        "one occurrence per value and key"
    );
}

fn count_keys(value: &Value) -> usize {
    match value {
        Value::Object(map) => map.len() + map.values().map(count_keys).sum::<usize>(),
        Value::Array(values) => values.iter().map(count_keys).sum(),
        _ => 0,
    }
}

#[test]
fn tree_spans_and_paths_match_serde_json() {
    for input in [
        r#"{"customer_id":"C-731284","nested":{"list":[1,-2.5e+3,true,false,null,"x"]}}"#,
        " \n\t[ {} , [] , \"\" , 0 ]\r\n",
        r#"{"a/b":{"~c":["slash and tilde keys"]},"":"empty key"}"#,
        r#"{"esc":"q\"b\\s\/n\nt\tuü😀\b\f\r"}"#,
        r#"{"straße":"Müller 😀","日本":["ü",{"ключ":"значение"}]}"#,
        r#"{"big":123456789012345678901234567890,"tiny":-0.0000001E-300}"#,
        "\"lone root string\"",
        "-0",
    ] {
        assert_oracle(input);
    }
}

#[test]
fn generated_documents_round_trip_compact_and_pretty() {
    // Deterministic generator: no external randomness, so failures reproduce exactly.
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    fn build(next: &mut dyn FnMut() -> u64, depth: usize) -> Value {
        let choice = next() % if depth > 4 { 5 } else { 7 };
        match choice {
            0 => Value::Null,
            1 => Value::Bool(next().is_multiple_of(2)),
            2 => json!((next() % 2_000_000) as i64 - 1_000_000),
            3 => json!(f64::from((next() % 10_000) as u32) / 7.0),
            4 => {
                let pool = [
                    "",
                    "plain",
                    "ü\u{1F600}",
                    "q\"\\/\n\t",
                    "\u{0001}",
                    "C-731284",
                ];
                Value::String(pool[(next() % pool.len() as u64) as usize].to_owned())
            }
            5 => Value::Array((0..next() % 4).map(|_| build(next, depth + 1)).collect()),
            _ => Value::Object(
                (0..next() % 4)
                    .map(|index| (format!("k{index}_{}", next() % 3), build(next, depth + 1)))
                    .collect(),
            ),
        }
    }
    for _ in 0..300 {
        let value = build(&mut next, 0);
        assert_oracle(&serde_json::to_string(&value).unwrap());
        assert_oracle(&serde_json::to_string_pretty(&value).unwrap());
    }
}

#[test]
fn duplicate_keys_are_refused_where_serde_json_keeps_the_last() {
    let input = r#"{"customer_id":"A","customer_id":"B"}"#;
    assert!(serde_json::from_str::<Value>(input).is_ok());
    assert_eq!(
        error(input, JsonTreeLimits::default()),
        JsonTreeError::DuplicateObjectKey
    );
    // Escapes decode before comparison, so a respelled key is still a duplicate.
    assert_eq!(
        error(
            r#"{"id":1,"nested":{"a":1,"a":2}}"#,
            JsonTreeLimits::default()
        ),
        JsonTreeError::DuplicateObjectKey
    );
    // The same key in different objects is not a duplicate.
    tree(r#"{"id":1,"nested":{"id":2}}"#);
}

#[test]
fn malformed_input_is_refused_with_closed_codes() {
    let limits = JsonTreeLimits::default();
    for input in [
        "",
        "{",
        r#"{"a":1,}"#,
        "[1,]",
        "01",
        "1.",
        "1e",
        "+1",
        "{\"a\":1} trailing",
        "\"unterminated",
        "\"control\u{0001}char\"",
        r#""\ud800""#,
        r#""\udc00""#,
        r#""\ud800A""#,
        r#""\x""#,
        "nul",
        "{a:1}",
        "NaN",
    ] {
        assert_eq!(
            error(input, limits),
            JsonTreeError::InvalidJson,
            "{input:?}"
        );
    }
    assert_eq!(
        match parse(b"\"\xff\"", limits) {
            Err(error) => error,
            Ok(_) => panic!("invalid UTF-8 must be refused"),
        },
        JsonTreeError::InvalidUtf8
    );
}

#[test]
fn limits_fail_closed_at_their_boundaries() {
    let nested = |levels: usize| "[".repeat(levels) + &"]".repeat(levels);
    let shallow = JsonTreeLimits::default().with_max_depth(3);
    parse(nested(4).as_bytes(), shallow).unwrap_or_else(|_| panic!("depth 3 fits"));
    assert_eq!(
        error(&nested(5), shallow),
        JsonTreeError::DepthLimitExceeded
    );

    let deep = nested(10_000);
    assert_eq!(
        error(&deep, JsonTreeLimits::default()),
        JsonTreeError::DepthLimitExceeded,
        "deep nesting is refused before it can exhaust the stack"
    );

    let nodes = JsonTreeLimits::default().with_max_nodes(3);
    parse(b"[1,2]", nodes).unwrap_or_else(|_| panic!("three nodes fit"));
    assert_eq!(error("[1,2,3]", nodes), JsonTreeError::NodeLimitExceeded);

    let strings = JsonTreeLimits::default().with_max_string_bytes(4);
    parse("\"ü12\"".as_bytes(), strings).unwrap_or_else(|_| panic!("four bytes fit"));
    assert_eq!(
        error("\"ü123\"", strings),
        JsonTreeError::StringLimitExceeded
    );

    let numbers = JsonTreeLimits::default().with_max_number_bytes(3);
    parse(b"-12", numbers).unwrap_or_else(|_| panic!("three bytes fit"));
    assert_eq!(error("-123", numbers), JsonTreeError::NumberLimitExceeded);

    let body = JsonTreeLimits::default().with_max_body_bytes(2);
    parse(b"[]", body).unwrap_or_else(|_| panic!("two bytes fit"));
    assert_eq!(error("[ ]", body), JsonTreeError::BodyLimitExceeded);
}

#[test]
fn numbers_keep_their_exact_spelling_beyond_f64() {
    let input = r#"[1e400,123456789012345678901234567890,-0.10E+02]"#;
    let parsed = tree(input);
    let JsonKind::Array(values) = &parsed.root.kind else {
        panic!("array root");
    };
    let spellings: Vec<_> = values
        .iter()
        .map(|value| match &value.kind {
            JsonKind::Number(spelling) => spelling.as_str(),
            _ => panic!("number"),
        })
        .collect();
    assert_eq!(
        spellings,
        ["1e400", "123456789012345678901234567890", "-0.10E+02"]
    );
}

#[test]
fn ascii_key_restriction_is_opt_in() {
    let input = r#"{"straße":"x"}"#;
    tree(input);
    assert_eq!(
        error(input, JsonTreeLimits::default().with_ascii_keys_only(true)),
        JsonTreeError::InvalidJson
    );
    parse(
        r#"{"street":"straße"}"#.as_bytes(),
        JsonTreeLimits::default().with_ascii_keys_only(true),
    )
    .unwrap_or_else(|_| panic!("non-ASCII values stay allowed"));
}

#[test]
fn utf8_offsets_are_bytes_in_the_input() {
    let input = r#"{"ü":"😀x","k":[ "é" ]}"#;
    let parsed = tree(input);
    let JsonKind::Object(members) = &parsed.root.kind else {
        panic!("object root");
    };
    assert_eq!(&input[members[0].value.span.clone()], "\"😀x\"");
    assert_eq!(members[0].value.span.start, "{\"ü\":".len());
    let JsonKind::Array(values) = &members[1].value.kind else {
        panic!("array member");
    };
    assert_eq!(&input[values[0].span.clone()], "\"é\"");
    assert_eq!(values[0].path.segments(), ["k", "0"]);
}

#[test]
fn input_copies_enforce_number_limit() {
    let input = super::input::ValidatedInput::new(b"12345").unwrap();
    assert_eq!(input.copy_range(0..3, 3).unwrap(), "123");
    assert!(matches!(
        input.copy_range(0..5, 3),
        Err(JsonTreeError::NumberLimitExceeded)
    ));
}

/// Count validation plus every input accessor's work, including owned copies and
/// byte-by-byte reconstruction. No timing assertions.
#[cfg(feature = "perf-timers")]
#[test]
fn non_ascii_strings_are_validated_once() {
    for scalar in ["ü", "€", "😀"] {
        let mut work = Vec::new();
        for size in [512, 1024] {
            let text = scalar.repeat(size);
            let input = format!(r#"{{"{scalar}":["{text}",1e400,true,null,"\u00fc"]}}"#);
            let capture = crate::perf::Capture::new();
            let parsed = tree(&input);
            let record = capture.finish();
            let JsonKind::Object(members) = &parsed.root.kind else {
                panic!("object root");
            };
            assert_eq!(members[0].key.value, scalar);
            let JsonKind::Array(values) = &members[0].value.kind else {
                panic!("array member");
            };
            assert!(matches!(&values[0].kind, JsonKind::String(value) if value.value == text));
            assert!(matches!(&values[1].kind, JsonKind::Number(value) if value == "1e400"));
            let bytes = record
                .counters
                .get("json_tree_utf8_validated_bytes")
                .copied()
                .expect("validation must be counted");
            assert_eq!(bytes, input.len() as u64, "{scalar:?}, size {size}");
            let accessor_work = record
                .counters
                .get("json_tree_input_work")
                .copied()
                .expect("input accessor work must be counted");
            work.push(bytes + accessor_work);
        }
        assert!(
            work[1] <= 2 * work[0],
            "doubling scalar count must at most double total input work"
        );
    }
}
