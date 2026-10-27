// Synthetic carrier matrix: compare syntax byte-for-byte after normalizing only ID tokens.
#[cfg(feature = "bundled-recognizers")]
#[test]
fn record_value_shape_preserves_carrier_syntax() {
    use crate::{CleanDocument, RawDocument, Scope, Session};
    let pipeline = labelled_pipeline();
    let labels = [
        "Customer ID",
        "customerId",
        "customer_id",
        "Kundennummer",
        "Employee ID",
        "employee_id",
        "Member ID",
        "contact_id",
    ];
    let carriers = [
        "<td>{L}:</td><td>{V}</td>",
        "<th>{L}:</th> {V}",
        "<b>{L}:</b> {V}",
        "<label>{L}:</label> {V}",
        "<span>{L}:</span> {V}",
        "<p>{L}:</p> {V}",
        "{L}: <!-- x --> {V}",
        "{L}:&nbsp;{V}",
        "{L}:&#160;{V}",
        "{L}:&quot;{V}&quot;",
        "<field name=\"{L}\" value=\"{V}\"/>",
        "\"{L}\" value=\"{V}\"",
        "\"{L}\" value=",
        "{L} = value=\"{V}\"",
        "<input name=\"{L}\" value=\"{V}\">",
        "<input {L}={V}>",
        "<input\n {L}={V}>",
        "<meta name=\"{L}\" content=\"{V}\">",
        "<field key=\"{L}\" val=\"{V}\"/>",
        "<{L} value=\"{V}\"/>",
        "{L}: >-\n  {V}",
        "{L}: |\n  {V}",
        "{L}: &a {V}",
        "{L}: !!str {V}",
        "{L} = [ \"{V}\" ]",
        "{L} = { value = \"{V}\" }",
        "WHERE {L} = ? AND x = 1",
        "GROUP BY {L} ORDER BY 2",
        "{L}=$1",
        "if {L} == other.{L}:",
        "'{L}' => $id,",
        "{L} -> orders.{L}",
        "{L}: z.string().uuid(),",
        "{L} = os.environ[\"X\"]",
        "<Order {L}={id} />",
        "<Order {L}={id731284} />",
        "{L} = ${X}",
        "{L} = {{customer_id}}",
        "{L} = req.params.id;",
        "{L} = self.customer_id",
        "{L} = row.customer_id",
    ];
    let mut checked = 0;
    for label in labels {
        for carrier in carriers {
            let input = carrier.replace("{L}", label).replace("{V}", "C-731284");
            let session = Session::new(Scope::Ephemeral).unwrap();
            let CleanDocument::Text(clean) = pipeline
                .redact(&session, RawDocument::Text(input.clone()))
                .unwrap()
            else {
                panic!("text")
            };
            let mut structure = clean.clone();
            for entry in session.snapshot_entries() {
                assert_eq!(
                    entry.raw, "C-731284",
                    "non-ID owned in {input:?}: {:?}",
                    entry.raw
                );
                structure = structure.replace(&entry.token, &entry.raw);
            }
            assert_eq!(structure, input, "carrier changed: {clean}");
            assert_eq!(clean, input, "uncovered carrier syntax must stay exact");
            assert_eq!(
                pipeline.restore_strict_text(&session, &clean).unwrap(),
                input
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 328);
    let mut placeholders = 0;
    for label in labels {
        for value in [
            "string",
            "uuid",
            "N/A",
            "-",
            "...",
            "{{customer_id}}",
            "${X}",
            "$CUSTOMER_ID",
            "<customer-id>",
            "req.params.id",
            "self.customer_id",
            "req2.params.id",
        ] {
            for quoted in [false, true] {
                let input = if quoted {
                    format!("{label}: \"{value}\"")
                } else {
                    format!("{label}: {value}")
                };
                let session = Session::new(Scope::Ephemeral).unwrap();
                let CleanDocument::Text(clean) = pipeline
                    .redact(&session, RawDocument::Text(input.clone()))
                    .unwrap()
                else {
                    panic!("text")
                };
                assert_eq!(clean, input, "placeholder changed");
                placeholders += 1;
            }
        }
    }
    assert_eq!(placeholders, 192);
}

#[cfg(feature = "bundled-recognizers")]
#[test]
fn lexical_sweep_preserves_non_strict_keys_numbers_and_literals() {
    use crate::{CleanDocument, RawDocument, Scope, Session};
    let pipeline = labelled_pipeline();
    let carriers = [
        "{\"{V}\":1, // c\n \"n\":731284, \"copy\":\"{V}\"}",
        "{\"{V}\":1,\"n\":731284,\"copy\":\"{V}\",}",
        "{\"{V}\":NaN,\"n\":731284,\"copy\":\"{V}\"}",
        "{'{V}':True,'n':731284,'copy':'{V}'}",
        "{unquoted:1,\"{V}\":1,n:731284,copy:\"{V}\"}",
        "{\"{V}\":1,\"n\":0731284,\"copy\":\"{V}\"}",
        "{\"{V}\":1,\"copy\":\"{V}\",\"rows\":[{\"n\":731284",
        "[731284,\"{V}\",{\"{V}\":true,\"n\":731284 ...[truncated]",
    ];
    let mut checked = 0;
    for value in ["C-731284", "731284"] {
        for carrier in carriers {
            for separate_turn in [false, true] {
                let session = Session::new(Scope::Ephemeral).unwrap();
                let seed = format!("customerId: {value}");
                let body = carrier.replace("{V}", value);
                let input = if separate_turn {
                    pipeline.redact(&session, RawDocument::Text(seed)).unwrap();
                    format!("CARRIER {body}")
                } else {
                    format!("{seed}\nCARRIER {body}")
                };
                let CleanDocument::Text(clean) = pipeline
                    .redact(&session, RawDocument::Text(input.clone()))
                    .unwrap()
                else {
                    panic!("text")
                };
                let out = clean.split_once("CARRIER ").unwrap().1;
                assert!(
                    out.contains(&format!("\"{value}\":")) || out.contains(&format!("'{value}':")),
                    "key changed: {clean}"
                );
                assert!(out.contains("731284"), "number changed: {clean}");
                // Only the value-string copy may change; keys and bare atoms must remain exact.
                let entry = session
                    .snapshot_entries()
                    .into_iter()
                    .find(|entry| entry.raw == value)
                    .unwrap();
                let expected = if body.contains("copy") {
                    body.replace(
                        &format!("\"copy\":\"{value}\""),
                        &format!("\"copy\":\"{}\"", entry.token),
                    )
                    .replace(
                        &format!("'copy':'{value}'"),
                        &format!("'copy':'{}'", entry.token),
                    )
                    .replace(
                        &format!("copy:\"{value}\""),
                        &format!("copy:\"{}\"", entry.token),
                    )
                } else {
                    body.replacen(
                        &format!("[731284,\"{value}\""),
                        &format!("[731284,\"{}\"", entry.token),
                        1,
                    )
                };
                assert_eq!(out, expected, "lexical structure changed");
                assert_eq!(
                    pipeline.restore_strict_text(&session, &clean).unwrap(),
                    input
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 32);
    // Literals are protected even when they came from another deterministic source.
    for literal in ["null", "true", "false"] {
        let matcher = SweepMatcher::build([SweepSource {
            family: "test".into(),
            class: PiiClass::custom("test").unwrap(),
            raw: literal.into(),
            repeat_seed: None,
        }])
        .unwrap()
        .unwrap();
        assert!(matcher.find(&format!("{{bad: {literal},}}")).is_empty());
    }
}

#[test]
fn minified_record_url_lookup_work_is_linear() {
    let source = SweepSource {
        family: "synthetic.counter".into(),
        class: PiiClass::custom("customer_id").unwrap(),
        raw: "C-731284".into(),
        repeat_seed: None,
    };
    let matcher = SweepMatcher::build([source]).unwrap().unwrap();
    for size in [500, 1000, 2000, 4000] {
        let text = format!(
            "[{}]",
            vec![r#"{"customerId":"C-731284","copy":"C-731284"}"#; size].join(",")
        );
        SWEEP_WORK.with(|work| work.set(SweepWork::default()));
        let hits = matcher.find(&text);
        assert_eq!(hits.len(), size * 2, "every requested lookup ran");
        let count = SWEEP_WORK.with(|work| work.get());
        assert_eq!(
            count.url_scan_bytes,
            text.len(),
            "one run-index construction: {count:?}"
        );
        assert!(
            count.url_queries >= size * 2 && count.url_queries <= size * 4,
            "{count:?}"
        );
    }
    let text = "WWW.example.invalid/C-731284 https://example.invalid/C-731284 C-731284";
    let hits = matcher.find(text);
    assert_eq!(hits.len(), 1);
    assert_eq!(&text[hits[0].span.clone()], "C-731284");
}

#[cfg(feature = "bundled-recognizers")]
#[test]
fn owned_identifier_relaxations_preserve_covered_shapes() {
    use crate::{CleanDocument, RawDocument, Scope, Session};
    let pipeline = labelled_pipeline();
    for value in [
        "-731284",
        "@FABLE984321",
        "_opaque/REF-01",
        "FABLE 407",
        "AB12 CD34 EF56 GH78",
    ] {
        let input = format!("customerId: {value}");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = pipeline
            .redact(&session, RawDocument::Text(input.clone()))
            .unwrap()
        else {
            panic!("text")
        };
        let entries = session.snapshot_entries();
        assert_eq!(entries.len(), 1, "{input} -> {clean}");
        assert_eq!(entries[0].raw, value);
        assert_eq!(
            pipeline.restore_strict_text(&session, &clean).unwrap(),
            input
        );
    }
}

#[cfg(feature = "bundled-recognizers")]
#[test]
fn unterminated_id_quotes_never_own_next_line_syntax() {
    use crate::{CleanDocument, RawDocument, Scope, Session};
    let pipeline = labelled_pipeline();
    let mut checked = 0;
    for label in ["customerId", "employee_id", "Member ID"] {
        for quote in ['"', '\''] {
            for syntax in [
                "SELECT 1 WHERE public_column = ?;",
                "if (public_flag) { return 1; }",
                "<td data-kind=public>display</td>",
            ] {
                for newline in ["\n", "\r\n"] {
                    let input = format!("{label}: {quote}C-731284{newline}{syntax}");
                    let session = Session::new(Scope::Ephemeral).unwrap();
                    let CleanDocument::Text(clean) = pipeline
                        .redact(&session, RawDocument::Text(input.clone()))
                        .unwrap()
                    else {
                        panic!("text")
                    };
                    let entries = session.snapshot_entries();
                    assert_eq!(entries.len(), 1, "{input:?} -> {clean:?}");
                    assert_eq!(entries[0].raw, "C-731284", "syntax owned: {input:?}");
                    assert_eq!(
                        clean,
                        format!("{label}: {quote}{}{newline}{syntax}", entries[0].token)
                    );
                    assert_eq!(
                        pipeline.restore_strict_text(&session, &clean).unwrap(),
                        input
                    );
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 36);
}

#[test]
fn lexical_sweep_gate_survives_stray_prose_quotes() {
    let mut checked = 0;
    for prefix in ["", "msg \"oops ", "TV 55\" screen ", "path C:\\\" "] {
        for body in [
            r#"{"731284":1,"n":731284,"copy":"731284",}"#,
            r#"{"731284":NaN,"n":731284,"copy":"731284"}"#,
            r#"{"731284":1,"n":731284,"copy":"731284" // c"#,
            "{'731284':True,'n':731284,'copy':'731284'}",
        ] {
            let input = format!("{prefix}{body}");
            let index = JsonSweepIndex::new(&input);
            for (occurrence, (at, _)) in input.match_indices("731284").enumerate() {
                assert_eq!(
                    index.allows(&(at..at + 6)),
                    occurrence == 2,
                    "{input:?}: {occurrence}"
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 48);
    let input = "{\"731284\"\u{00a0}:1,}";
    let index = JsonSweepIndex::new(input);
    assert!(!index.allows(&(2..8)));
}

#[cfg(feature = "bundled-recognizers")]
#[test]
fn lexical_sweep_keeps_whole_private_values_with_key_looking_content() {
    use crate::{CleanDocument, RawDocument, Scope, Session};
    let pipeline = labelled_pipeline();
    for value in [
        r#"NX734498; "private_code": É769982"#,
        r#"'NX734498; "private_code": É769982"#,
        r#"NX734498\"\\769982"#,
    ] {
        let input = format!("Lookup {value} again.\ncustomerId: \"{value}\npublic_version=FRAME");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = pipeline
            .redact(&session, RawDocument::Text(input.clone()))
            .unwrap()
        else {
            panic!("text")
        };
        let entries = session.snapshot_entries();
        let entry = entries
            .iter()
            .find(|entry| entry.raw == value)
            .unwrap_or_else(|| panic!("{input:?}: {entries:?}"));
        assert_eq!(clean, input.replace(value, &entry.token));
        assert_eq!(
            pipeline.restore_strict_text(&session, &clean).unwrap(),
            input
        );
    }
}

#[cfg(feature = "bundled-recognizers")]
#[test]
fn closed_wrapper_set_requires_id_shape_outside_data() {
    use crate::{CleanDocument, RawDocument, Scope, Session};
    let pipeline = labelled_pipeline();
    let mut checked = 0;
    for (open, close) in [('(', ')'), ('⟨', '⟩'), ('§', '§'), ('🔐', '🔐'), ('[', ']')] {
        for value in ["C731284", "731284", "placeholder"] {
            let input = format!("customerId: {open}{value}{close}");
            let session = Session::new(Scope::Ephemeral).unwrap();
            let CleanDocument::Text(clean) = pipeline
                .redact(&session, RawDocument::Text(input.clone()))
                .unwrap()
            else {
                panic!("text")
            };
            if value != "placeholder" {
                assert_eq!(session.snapshot_entries()[0].raw, value);
                assert!(!clean.contains(value));
            } else {
                assert_eq!(clean, input);
            }
            assert_eq!(
                pipeline.restore_strict_text(&session, &clean).unwrap(),
                input
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 15);
}

#[cfg(feature = "bundled-recognizers")]
#[test]
fn base64_record_ids_keep_padding_without_owning_operators() {
    use crate::{CleanDocument, RawDocument, Scope, Session};
    let pipeline = labelled_pipeline();
    let mut checked = 0;
    for value in ["+YQ==", "opaque+YQ==", "/YQ=", "YWJjZA==", "abc/def"] {
        for quoted in [false, true] {
            for suffix in ["", "; status=public", "\npublic_version=FRAME"] {
                let input = if quoted {
                    format!("customerId: \"{value}\"{suffix}")
                } else {
                    format!("customerId={value}{suffix}")
                };
                let session = Session::new(Scope::Ephemeral).unwrap();
                let CleanDocument::Text(clean) = pipeline
                    .redact(&session, RawDocument::Text(input.clone()))
                    .unwrap()
                else {
                    panic!("text")
                };
                assert_eq!(
                    session.snapshot_entries()[0].raw,
                    value,
                    "{input} -> {clean}"
                );
                assert!(!clean.contains(value));
                assert_eq!(
                    pipeline.restore_strict_text(&session, &clean).unwrap(),
                    input
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 30);
    for value in [
        "a == b",
        "x=+y",
        "!=",
        "=>",
        "+y",
        "N/A",
        "foo=bar",
        "+YQ===",
        "abc/def(x)",
    ] {
        let input = format!("customerId = {value}");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = pipeline
            .redact(&session, RawDocument::Text(input.clone()))
            .unwrap()
        else {
            panic!("text")
        };
        assert_eq!(clean, input);
    }
}

#[cfg(feature = "bundled-recognizers")]
#[test]
fn quoted_html_attributes_and_json_data_remain_owned() {
    use crate::{CleanDocument, RawDocument, Scope, Session};
    let pipeline = labelled_pipeline();
    let mut checked = 0;
    for label in ["customerId", "employee_id", "member_id"] {
        for input in [
            format!("<input {label}=\"C-731284\">"),
            format!("<input {label}='C-731284'>"),
            format!("<div data='{{\"{label}\":\"C-731284\"}}'>"),
            format!(r#"{{"note":"<input {label}=C-731284>"}}"#),
            format!("SELECT '<input {label}=C-731284>';"),
            format!(r#"FRAME,"<input {label}=C-731284>",FRAME"#),
        ] {
            let session = Session::new(Scope::Ephemeral).unwrap();
            let CleanDocument::Text(clean) = pipeline
                .redact(&session, RawDocument::Text(input.clone()))
                .unwrap()
            else {
                panic!("text")
            };
            let entries = session.snapshot_entries();
            assert_eq!(entries.len(), 1, "{input} -> {clean}");
            assert_eq!(entries[0].raw, "C-731284");
            assert_eq!(clean, input.replace("C-731284", &entries[0].token));
            assert_eq!(
                pipeline.restore_strict_text(&session, &clean).unwrap(),
                input
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 18);
    for prefix in ["x < 2; ", "x < 2\n", "x <= y; "] {
        let input = format!("{prefix}customerId=C-731284");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = pipeline
            .redact(&session, RawDocument::Text(input.clone()))
            .unwrap()
        else {
            panic!("text")
        };
        assert_eq!(session.snapshot_entries()[0].raw, "C-731284");
        assert!(!clean.contains("C-731284"));
        assert_eq!(
            pipeline.restore_strict_text(&session, &clean).unwrap(),
            input
        );
    }
}

#[cfg(feature = "bundled-recognizers")]
#[test]
fn seeded_html_repeats_preserve_unquoted_attribute_syntax() {
    use crate::{CleanDocument, RawDocument, Scope, Session};
    let pipeline = labelled_pipeline();
    let mut checked = 0;
    for (carrier, protected) in [
        ("<input data-id=C-731284>", false),
        ("<input\n data-id=C-731284>", false),
        ("<input data-id=\"C-731284\">", true),
        ("<input data-id='C-731284'>", true),
        (r#"{"note":"<input data-id=C-731284>"}"#, true),
        (r#""<input title=\"x\" data-id=C-731284>""#, true),
        (r#"payload={\"note\":\"<input data-id=C-731284>\"}"#, true),
        (r#"FRAME,"<input data-id=C-731284>",FRAME"#, true),
        ("\nSELECT '<input data-id=C-731284>';", true),
    ] {
        for separate_turn in [false, true] {
            let session = Session::new(Scope::Ephemeral).unwrap();
            let input = if separate_turn {
                pipeline
                    .redact(&session, RawDocument::Text("customerId: C-731284".into()))
                    .unwrap();
                format!("CARRIER {carrier}")
            } else {
                format!("customerId: C-731284\nCARRIER {carrier}")
            };
            let CleanDocument::Text(clean) = pipeline
                .redact(&session, RawDocument::Text(input.clone()))
                .unwrap()
            else {
                panic!("text")
            };
            let output = clean.split_once("CARRIER ").unwrap().1;
            let entries = session.snapshot_entries();
            let entry = entries
                .iter()
                .find(|entry| entry.raw == "C-731284")
                .unwrap();
            let expected = if protected {
                carrier.replace("C-731284", &entry.token)
            } else {
                carrier.to_owned()
            };
            assert_eq!(output, expected, "{input:?} -> {clean:?}");
            assert_eq!(
                pipeline.restore_strict_text(&session, &clean).unwrap(),
                input
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 18);
}

#[cfg(feature = "bundled-recognizers")]
#[test]
fn non_tags_never_hide_record_values_or_repeats() {
    use crate::{CleanDocument, RawDocument, Scope, Session};
    let pipeline = labelled_pipeline();
    for (input, value) in [
        ("if a<b:\n    customer_id = 731284", "731284"),
        ("sort <input.txt\ncustomer_id=731284", "731284"),
        ("cat <<EOF\ncustomer_id=731284\nEOF", "731284"),
        ("let m: HashMap<String, u32\ncustomer_id = 731284", "731284"),
        ("template <typename T = int\nemployee_id = E-4471", "E-4471"),
        (
            "x <y = 2;\nCustomer ID: C-731284\nreturn a > b;",
            "C-731284",
        ),
        ("if n <limit then customer_id = 731284", "731284"),
        ("Scores <a few points; employee_id=E-4471 noted", "E-4471"),
        (
            "Use List<T when needed, e.g. x=1.\n\nCustomer ID: C-731284",
            "C-731284",
        ),
        (
            "a <b c=1 and later Customer ID: C-731284 then x > y",
            "C-731284",
        ),
        ("<a href='x>\nCustomer ID: C-731284", "C-731284"),
        ("<a title='it>\nemployee_id: E-4471\nlater ' end", "E-4471"),
        (
            "Customer ID: C-731284\nwhile i <n: x=1\nagain C-731284 here",
            "C-731284",
        ),
    ] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = pipeline
            .redact(&session, RawDocument::Text(input.into()))
            .unwrap()
        else {
            panic!("text")
        };
        let entries = session.snapshot_entries();
        let entry = entries
            .iter()
            .find(|entry| entry.raw == value)
            .unwrap_or_else(|| panic!("{input:?} -> {clean:?}; {entries:?}"));
        assert_eq!(clean, input.replace(value, &entry.token));
        assert_eq!(
            pipeline.restore_strict_text(&session, &clean).unwrap(),
            input
        );
    }
    let prefixes = [
        "if a<b:\n    ",
        "sort <input.txt\n",
        "cat <<EOF\n",
        "let m: HashMap<String, u32\n",
        "template <typename T = int\n",
        "x <y = 2;\n",
        "if n <limit then ",
        "Scores <a few points; ",
        "Use List<T when needed, e.g. x=1.\n\n",
        "a <b c=1 and later ",
        "<a href='x>\n",
        "<a title='it>\n",
        "while i <n: x=1\n",
    ];
    let mut checked = 0;
    for prefix in prefixes.into_iter().map(str::to_owned).chain([
        format!("<a title='{}' ", "x".repeat(2048)),
        "<a title='x'\n ; ".to_owned(),
    ]) {
        for (label, value) in [("Customer ID", "C-731284"), ("employee_id", "E-4471")] {
            for separate_turn in [false, true] {
                let session = Session::new(Scope::Ephemeral).unwrap();
                let seed = format!("{label}: {value}");
                if separate_turn {
                    pipeline.redact(&session, RawDocument::Text(seed)).unwrap();
                }
                let input = if separate_turn {
                    format!("{prefix}again {value} here; then x > y;")
                } else {
                    format!("{prefix}{label}: {value} then x > y;\nagain {value} here")
                };
                let CleanDocument::Text(clean) = pipeline
                    .redact(&session, RawDocument::Text(input.clone()))
                    .unwrap()
                else {
                    panic!("text")
                };
                let entries = session.snapshot_entries();
                let entry = entries
                    .iter()
                    .find(|entry| entry.raw == value)
                    .unwrap_or_else(|| panic!("ID leaked: {input:?} -> {clean:?}; {entries:?}"));
                assert_eq!(clean, input.replace(value, &entry.token), "{input:?}");
                assert_eq!(
                    pipeline.restore_strict_text(&session, &clean).unwrap(),
                    input
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 60);
    for prefix in [
        format!("<a title='{}' ", "x".repeat(2048)),
        "<a data=<x ".into(),
        "</a ".into(),
    ] {
        for separate_turn in [false, true] {
            let session = Session::new(Scope::Ephemeral).unwrap();
            let input = if separate_turn {
                pipeline
                    .redact(&session, RawDocument::Text("customerId: C-731284".into()))
                    .unwrap();
                format!("{prefix}data-id=C-731284>")
            } else {
                format!("{prefix}customerId=C-731284>")
            };
            let CleanDocument::Text(clean) = pipeline
                .redact(&session, RawDocument::Text(input.clone()))
                .unwrap()
            else {
                panic!("text")
            };
            let entries = session.snapshot_entries();
            let entry = entries
                .iter()
                .find(|entry| entry.raw == "C-731284")
                .unwrap_or_else(|| panic!("{input:?} -> {clean:?}"));
            assert_eq!(clean, input.replace("C-731284", &entry.token));
            assert_eq!(
                pipeline.restore_strict_text(&session, &clean).unwrap(),
                input
            );
        }
    }
}

#[cfg(feature = "bundled-recognizers")]
#[test]
fn labelled_value_starts_keep_connectors_hashes_equals_and_numeric_wrappers() {
    use crate::{CleanDocument, RawDocument, Scope, Session};
    let pipeline = labelled_pipeline();
    let mut checked = 0;
    for label in ["Customer ID", "employee_id", "Member ID"] {
        for (start, value, end) in [
            (" - ", "C-731284", " was merged."),
            (" – ", "731284", ""),
            (" — ", "C-731284", ""),
            (" ", "#731284", " called."),
            (": ", "#FABLE731284", ""),
            (": ", "#4521", ""),
            (": ", "AB=12345", ""),
            (" (", "731284", ") called"),
            (": [", "731284", "]"),
            ("=⟨", "731284", "⟩"),
            (": ", "C731284", " NOT NULL"),
        ] {
            let input = format!("{label}{start}{value}{end}");
            let session = Session::new(Scope::Ephemeral).unwrap();
            let CleanDocument::Text(clean) = pipeline
                .redact(&session, RawDocument::Text(input.clone()))
                .unwrap()
            else {
                panic!("text")
            };
            let entries = session.snapshot_entries();
            assert_eq!(entries.len(), 1, "{input:?} -> {clean:?}");
            assert_eq!(entries[0].raw, value, "{input:?}");
            assert_eq!(clean, input.replace(value, &entries[0].token));
            assert_eq!(
                pipeline.restore_strict_text(&session, &clean).unwrap(),
                input
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 33);
}

#[cfg(feature = "bundled-recognizers")]
#[test]
fn code_and_sql_record_fields_never_seed_the_sweep() {
    use crate::{CleanDocument, RawDocument, Scope, Session};
    let pipeline = labelled_pipeline();
    let cases: Vec<String> = serde_json::from_str(include_str!(
        "../../tests/fixtures/record-id-code-counterweights.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 37);
    for case in cases {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let input = format!("{case}\nPUBLIC COPY {case}");
        let CleanDocument::Text(clean) = pipeline
            .redact(&session, RawDocument::Text(input.clone()))
            .unwrap()
        else {
            panic!("text")
        };
        assert_eq!(clean, input, "code or SQL changed");
        assert!(
            session.snapshot_entries().is_empty(),
            "rejected value seeded sweep"
        );
        assert_eq!(
            pipeline.restore_strict_text(&session, &clean).unwrap(),
            input
        );
    }
    let mut checked = 0;
    for label in ["customer_id", "employee_id", "member_id"] {
        for dtype in [
            "i8", "u16", "i32", "u64", "i128", "int", "uint", "int4", "INT8", "Int32", "float32",
            "float64", "f64",
        ] {
            for (connector, suffix) in [(": ", ","), (" ", " NOT NULL,"), ("=", "")] {
                let input = format!("{label}{connector}{dtype}{suffix}\ncopy {dtype}");
                let session = Session::new(Scope::Ephemeral).unwrap();
                let CleanDocument::Text(clean) = pipeline
                    .redact(&session, RawDocument::Text(input.clone()))
                    .unwrap()
                else {
                    panic!("text")
                };
                assert_eq!(clean, input, "type or its copy changed");
                assert!(session.snapshot_entries().is_empty());
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 117);
}

#[cfg(feature = "bundled-recognizers")]
include!("rule_boundary_tests.rs");

#[cfg(feature = "bundled-recognizers")]
#[test]
fn semicolon_unquoted_html_fragments_stay_raw() {
    let pipeline = labelled_pipeline();
    for label in ["customerId", "data-id"] {
        check_boundary(&pipeline, &format!("<a data=x; {label}=C-731284>"), &[]);
    }
}
