//! Nonfunctional credential shapes assembled at runtime; no live-service checks.
// drift-ack: secrets 0.3.0 adds bare credentials; the existing drift corpus keeps its detections.
use gaze::{CleanDocument, DictionaryBundle, LocaleTag, PiiClass, SafetyNetPolicy, Scope, Session};
use gaze_assembly::CorePipelineConfig;

fn core() -> gaze_assembly::CorePipeline {
    CorePipelineConfig::new()
        .with_bundled_rulepack("secrets")
        .build()
        .unwrap()
}

fn assert_whole(core: &gaze_assembly::CorePipeline, raw: &str, values: &[&str]) {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (clean, spans, _, trace) = core
        .pipeline()
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            raw,
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .unwrap();
    for item in &trace {
        assert!(!item.source_ids().is_empty());
        for source in item.source_ids() {
            assert!(source
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || "._:/-".contains(ch)));
        }
    }
    let observed: Vec<_> = spans
        .iter()
        .map(|span| {
            assert_eq!(
                span.class,
                PiiClass::Custom("security_token".into()),
                "whole credential ownership: class"
            );
            &raw[span.raw_span.clone()]
        })
        .collect();
    assert_eq!(observed, values, "whole credential ownership");
    let CleanDocument::Text(clean) = clean else {
        panic!("text")
    };
    assert_eq!(
        core.pipeline()
            .restore_strict_text(&session, &clean)
            .unwrap(),
        raw
    );
    for value in values {
        assert!(!clean.contains(value));
    }
}

fn synthetic_suffix(size: usize) -> String {
    format!("{}1", "A".repeat(size - 1))
}

fn prefixed_values() -> Vec<String> {
    let mut values = Vec::new();
    for prefix in ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"] {
        for n in [36, 252, 1024] {
            values.push(format!("{prefix}{}", synthetic_suffix(n)));
        }
    }
    values.push(format!(
        "github_pat_{}_{}",
        synthetic_suffix(22),
        synthetic_suffix(59)
    ));
    values.push(format!(
        "ghs_000000_eyJ{}.{}.{}",
        "A".repeat(80),
        "B".repeat(300),
        "C".repeat(100)
    ));
    values.push(format!("sk-{}", synthetic_suffix(48)));
    values.push(format!(
        "sk-proj-{}_-{}",
        synthetic_suffix(32),
        synthetic_suffix(122)
    ));
    values.push(format!("sk-ant-api03-{}AA", synthetic_suffix(93)));
    for prefix in [
        "xoxa-",
        "xoxb-",
        "xoxp-",
        "xoxr-",
        "xoxs-",
        "xapp-",
        "xoxe-",
        "xoxe.xoxp-",
    ] {
        values.push(format!(
            "{prefix}{}-{}-{}",
            "0".repeat(12),
            "0".repeat(12),
            synthetic_suffix(32)
        ));
    }
    for prefix in ["sk_live_", "rk_live_", "sk_test_", "rk_test_", "whsec_"] {
        for n in [24, 99] {
            values.push(format!("{prefix}{}", synthetic_suffix(n)));
        }
    }
    for prefix in ["sk-svcacct-", "sk-admin-", "sk-None-"] {
        values.push(format!(
            "{prefix}{}_-{}",
            synthetic_suffix(32),
            synthetic_suffix(266)
        ));
    }
    values.push(format!("AIzaSy{}", synthetic_suffix(33)));
    values.push(format!("ghp_{}-old", synthetic_suffix(36)));
    values
}

// sha256 of the empty input: public, hex, and 64 base64-alphabet units long.
const SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn long_pem(label: &str, headers: &[&str]) -> Vec<String> {
    // Base64 of synthetic filler text in 64-unit lines, never private-key material.
    let mut lines = vec![format!("-----BEGIN {label}-----")];
    lines.extend(headers.iter().map(|header| header.to_string()));
    lines.extend(
        [
            "c3ludGhldGljLWZpeHR1cmUtbm90LWEta2V5LTAxMjM0NTY3ODktc3ludGhldGlj",
            "LWZpeHR1cmUtbm90LWEta2V5LTAxMjM0NTY3ODktc3ludGhldGljLWZpeHR1cmUt",
            "bm90LWEta2V5LTAxMjM0NTY3ODktc3ludGhldGljLWZpeHR1cmUtbm90LWEta2V5",
            "LTAxMjM0NTY3ODktc3lu",
        ]
        .map(String::from),
    );
    lines.push(format!("-----END {label}-----"));
    lines
}

fn prefixed_lines(lines: &[String], newline: &str, prefix: impl Fn(usize) -> String) -> String {
    let framed: Vec<_> = (1..).zip(lines).map(|(n, line)| prefix(n) + line).collect();
    framed.join(newline)
}

/// The span runs from the first BEGIN to the end of the END line's dashes.
fn armour(raw: &str, label: &str) -> String {
    let start = raw.find("-----BEGIN").unwrap();
    let footer = format!("{label}-----");
    raw[start..raw.rfind(&footer).unwrap() + footer.len()].to_string()
}

fn pem(label: &str, newline: &str) -> String {
    // Base64 decodes to synthetic text, never private-key material.
    format!("-----BEGIN {label}-----{newline}bm90LWEta2V5c3ludGhldGlj{newline}c3ludGhldGlj{newline}-----END {label}-----")
}

#[test]
fn bare_vendor_prefixes_are_whole_restorable_credentials() {
    let core = core();
    for value in prefixed_values() {
        for raw in [
            value.clone(),
            format!("before ({value}), after"),
            format!("Received {value}."),
            format!(r#"{{"value":"{value}"}}"#),
            format!("token = {value}"),
            format!(r#"{{"api_key":"{value}"}}"#),
        ] {
            assert_whole(&core, &raw, &[&value]);
        }
    }
}

#[test]
fn private_pem_blocks_include_headers_body_and_footer() {
    let core = core();
    for label in [
        "PRIVATE KEY",
        "RSA PRIVATE KEY",
        "EC PRIVATE KEY",
        "DSA PRIVATE KEY",
        "PGP PRIVATE KEY BLOCK",
        "OPENSSH PRIVATE KEY",
        "ENCRYPTED PRIVATE KEY",
    ] {
        for newline in ["\n", "\r\n", r"\n", r"\r\n"] {
            let value = pem(label, newline);
            assert_whole(&core, &format!("before {value} after"), &[&value]);
            assert_whole(&core, &format!(r#"{{"value":"{value}"}}"#), &[&value]);
        }
    }
}

#[test]
fn adjacent_bare_credentials_do_not_consume_each_others_boundaries() {
    let core = core();
    let first = &prefixed_values()[0];
    let second = &prefixed_values()[2];
    assert_whole(&core, &format!("{first},{second}"), &[first, second]);
    let first = pem("RSA PRIVATE KEY", "\n");
    let second = pem("EC PRIVATE KEY", "\n");
    assert_whole(&core, &format!("{first}\n{second}"), &[&first, &second]);
}

#[test]
fn prose_and_public_material_remain_untouched() {
    let core = core();
    let mut negatives = vec![
        "sk-learn task- ghp in prose".to_string(),
        "a".repeat(64),
        "c3ludGhldGlj".repeat(8),
        "00000000-0000-4000-8000-000000000000".into(),
        pem("PUBLIC KEY", "\n"),
        pem("CERTIFICATE", "\n"),
        "def network_test_handles_missing_bundle_gracefully():".into(),
        "fn benchmark_test_reports_peak_rss_for_every_arm() {}".into(),
        r#"{"task_test_retry_with_exponential_backoff_ok": true}"#.into(),
        "fn parser_weighs_all_candidate_spans_before_conflict_resolution() {}".into(),
        "key: user_laughs_at_the_joke_and_continues_reading_page_two".into(),
        "disk_live_bytes_written_since_the_last_collection".into(),
        "/desk-admin-panel-settings-overview-page-container".into(),
        r#"class="task-proj-board-column-header-collapsed-state""#.into(),
        "--mask-None-values-in-the-output-stream".into(),
        "Your file starts with\n-----BEGIN OPENSSH PRIVATE KEY-----\nand then contains base64 lines you must never share with anyone at all.".into(),
        "A key starts with -----BEGIN PRIVATE KEY----- and ends with a matching footer.".into(),
        // A generic line prefix needs 32 base64 units and cannot end on a quote.
        format!("BEGIN = \"-----BEGIN PRIVATE KEY-----\"\nHASH = \"{SHA}\""),
        format!("-----BEGIN PRIVATE KEY-----\nHASH = \"{SHA}\""),
        "docs/a.md:3:-----BEGIN PRIVATE KEY-----\ndocs/a.md:4:Paste the key body here, 2026 edition.".into(),
        "-----BEGIN PRIVATE KEY-----\nNote: see the internationalization guide, chapter 12.".into(),
        prefixed_lines(&long_pem("PUBLIC KEY", &[]), "\n", |_| "-".into()),
        format!("BAIza{}", synthetic_suffix(40)),
    ];
    {
        negatives.extend([
            // Short generic prefixes must not turn code identifiers into secrets.
            format!("desk-{}", "A".repeat(64)),
            format!("mask-{}", "A".repeat(64)),
        ]);
    }
    for (prefix, length) in [
        ("ghp_", 35),
        ("github_pat_", 81),
        ("sk-", 31),
        ("sk-proj-", 31),
        ("sk-ant-", 31),
        ("xoxb-", 9),
        ("sk_live_", 23),
    ] {
        negatives.push(format!("{prefix}{}", synthetic_suffix(length)));
    }
    for raw in negatives {
        assert_whole(&core, &raw, &[]);
    }
}

#[test]
fn legacy_encrypted_rsa_headers_and_escaped_base64_stay_in_one_span() {
    let core = core();
    for newline in ["\n", "\r\n", r"\n", r"\r\n"] {
        let value = format!("-----BEGIN RSA PRIVATE KEY-----{newline}Proc-Type: 4,ENCRYPTED{newline}DEK-Info: AES-256-CBC,00000000000000000000000000000000{newline}{newline}bm90L2tleQ==c3ludGhldGlj{newline}-----END RSA PRIVATE KEY-----");
        assert_whole(&core, &value, &[&value]);
    }
    let value = pem("PRIVATE KEY", r"\n").replace("bm90LWEta2V5", r"bm90\/LWEta2V5");
    assert_whole(&core, &format!(r#"{{"value":"{value}"}}"#), &[&value]);
}

#[test]
fn issuer_boundaries_protect_framing_and_preserve_identifiers() {
    let core = core();
    for value in prefixed_values() {
        for (left, right) in [
            ("", ""),
            ("(", ")"),
            (r"\n", r"\n"),
            (r"\u000a", r"\u000a"),
            (r"\/", r"\/"),
            ("%20", "%3D"),
        ] {
            assert_whole(&core, &format!("{left}{value}{right}"), &[&value]);
        }
        if let Some(inner) = value.strip_prefix("xoxe.") {
            assert_whole(&core, &format!("word{value}"), &[inner]);
        } else {
            assert_whole(&core, &format!("word{value}"), &[]);
        }
        for (left, right) in [
            ("task-", ""),
            ("é", "é"),
            ("令牌", "已泄露"),
            ("GH_TOKEN_", ""),
            ("BUILD_", ""),
            ("OPENAI_KEY_", ""),
        ] {
            assert_whole(&core, &format!("{left}{value}{right}"), &[&value]);
        }
    }
    let generic = format!("sk-{}", synthetic_suffix(64));
    for prefix in ["word", "lowercase", "9"] {
        assert_whole(&core, &format!("{prefix}{generic}"), &[]);
    }
}

#[test]
fn truncated_mismatched_and_framed_private_keys_are_whole_and_restorable() {
    let core = core();
    for newline in [
        "\n",
        "\r\n",
        r"\n",
        r"\r\n",
        r"\\n",
        r"\\r\\n",
        r"\u000a",
        r"\u000A",
        r"\u000d",
        "<br>",
        "<br/>",
        "\\\n",
        "\\\r\n",
        "\\n' +\n'",
        "\\n\" + \"",
        " ",
        "\t",
        "\\n\", \"",
        "\\n\",\n    \"",
        "%0A",
        "%0d%0a",
    ] {
        for prefix in ["", "# ", "// ", "> ", "* "] {
            let framing = format!("{newline}{prefix}");
            let body = "bm90LWEta2V5c3ludGhldGlj";
            let start = format!("-----BEGIN RSA PRIVATE KEY-----{framing}{body}");
            assert_whole(&core, &format!(r#"{{"value":"{start}"}}"#), &[&start]);
            let mismatched = format!("{start}{framing}-----END PRIVATE KEY-----");
            assert_whole(&core, &mismatched, &[&mismatched]);
            let complete = format!("{start}{framing}-----END RSA PRIVATE KEY-----");
            assert_whole(&core, &complete, &[&complete]);
        }
    }
    for blank in ["", "\n"] {
        let value = format!("-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\nDEK-Info: AES-128-CBC,00FF\n{blank}bm90LWEta2V5c3ludGhldGlj\n-----END RSA PRIVATE KEY-----");
        assert_whole(&core, &value, &[&value]);
    }
    let pgp = "-----BEGIN PGP PRIVATE KEY BLOCK-----\nVersion: Synthetic\nComment: synthetic fixture\n\nbm90LWEta2V5c3ludGhldGlj\n=ABCD\n-----END PGP PRIVATE KEY BLOCK-----";
    assert_whole(&core, pgp, &[pgp]);
    for newline in ["%0A", "%0a", "%0D%0A", "%0d%0a"] {
        let value = pem("RSA PRIVATE KEY", newline).replace(' ', "%20");
        assert_whole(&core, &format!("key={value}&next=ok"), &[&value]);
    }
    let encoded_body = "-----BEGIN%20PRIVATE%20KEY-----%0Abm90LWEta2V5c3ludGhldGlj%2B%2F%3D%0A-----END%20PRIVATE%20KEY-----";
    assert_whole(&core, encoded_body, &[encoded_body]);
    for placeholder in ["...", "<your key here>", "example", "bm90LWEta2V5"] {
        let raw = format!("-----BEGIN PRIVATE KEY-----\n{placeholder}\n-----END PRIVATE KEY-----");
        assert_whole(&core, &raw, &[]);
    }
}

#[test]
fn word_tails_require_digit_and_contiguous_suffix_evidence() {
    let core = core();
    for prefix in [
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
        "sk-",
        "sk-proj-",
        "sk-svcacct-",
        "sk-admin-",
        "sk-None-",
        "sk-ant-api03-",
        "xoxa-",
        "xoxb-",
        "xoxp-",
        "xoxr-",
        "xoxs-",
        "xapp-",
        "xoxe-",
        "xoxe.xoxp-",
        "sk_live_",
        "rk_live_",
        "sk_test_",
        "rk_test_",
        "whsec_",
        "AIzaSy",
    ] {
        for left in ["_", ".", "$", "-", "/", "#", "::", " ", "(", "\"", "'"] {
            for count in [6, 12, 24] {
                for (index, tail) in [
                    "verify_header_setting_service_".repeat(count),
                    "verify-header-setting-service-".repeat(count),
                    "verifyHeaderSettingService".repeat(count),
                    "VERIFY_HEADER_SETTING_SERVICE_".repeat(count),
                    "verify_header1_setting_service_".repeat(count),
                    "verify-header1-setting-service-".repeat(count),
                ]
                .into_iter()
                .enumerate()
                {
                    let value = format!("{prefix}{tail}");
                    let raw = format!("{left}{value}");
                    let base64url = matches!(
                        prefix,
                        "sk-proj-"
                            | "sk-svcacct-"
                            | "sk-admin-"
                            | "sk-None-"
                            | "sk-ant-api03-"
                            | "AIzaSy"
                    );
                    // Digit-bearing words share the alphabet of these named
                    // issuers. Generic sk-/Stripe/Slack retain alphanumeric runs.
                    if index >= 4 && base64url {
                        assert_whole(&core, &raw, &[&value]);
                    } else {
                        assert_whole(&core, &raw, &[]);
                    }
                }
            }
        }
        // Letter-only bodies are an intentional recall cost. A digit-bearing
        // camel word is the documented precision residual of structural evidence.
        let residual = format!("{prefix}{}1", "verifyHeaderSettingService".repeat(4));
        assert_whole(&core, &residual, &[&residual]);
    }
    // The digit need not be inside the qualifying alphanumeric run.
    for value in [
        format!("whsec_1_{}", "A".repeat(32)),
        format!("whsec_{}_1", "A".repeat(32)),
    ] {
        assert_whole(&core, &value, &[&value]);
    }
}

#[test]
fn numbered_bulleted_and_entity_framed_private_keys_stay_whole() {
    let core = core();
    for framing in [
        "\n1 ",
        "\n  12  ",
        "\n     2\t",
        "\n3:",
        "\n2 | ",
        "\n2|",
        "\n- ",
        "&#10;",
        "&#x0a;",
        "&#x0A;",
        "&#xA;",
        "&#xa;",
        "&#x000A;",
        "&#010;",
        "&#13;&#10;",
        "&#xD;&#xA;",
        "&NewLine;",
    ] {
        let value = pem("PRIVATE KEY", framing);
        assert_whole(&core, &value, &[&value]);
    }
    for placeholder in ["YourPrivateKeyGoesHere", "Base64EncodedKeyMaterial"] {
        let value =
            format!("-----BEGIN PRIVATE KEY-----\n{placeholder}\n-----END PRIVATE KEY-----");
        assert_whole(&core, &value, &[&value]);
    }
}

#[test]
fn tool_diff_and_gutter_prefixed_private_keys_stay_whole() {
    let core = core();
    let key = long_pem("PRIVATE KEY", &[]);
    type Prefix = fn(usize) -> String;
    let shapes: [(&str, Prefix); 20] = [
        ("diff deletion", |_| "-".into()),
        ("grep -n", |n| format!("k.pem:{n}:")),
        ("grep context", |n| format!("k.pem-{n}-")),
        ("rg --no-heading", |n| format!("src/keys/dev.pem:{n}:")),
        ("padded tool arrow", |n| format!("{n:>6}→")),
        ("tool arrow", |n| format!("{n}→")),
        ("bat gutter", |n| format!("{n:>4}   │ ")),
        ("box gutter", |_| "│ ".into()),
        ("pipe gutter", |_| "| ".into()),
        ("git blame", |n| {
            format!("^1a2b3c4 (Synthetic Developer Name 2026-01-01 12:00:00 +0000 {n:>3}) ")
        }),
        ("semicolon comment", |_| "; ".into()),
        ("double semicolon", |_| ";; ".into()),
        ("percent comment", |_| "% ".into()),
        ("bare percent", |_| "%".into()),
        ("sql comment", |_| "-- ".into()),
        ("batch comment", |_| "rem ".into()),
        ("vim comment", |_| "\" ".into()),
        ("bang comment", |_| "! ".into()),
        ("diff deletion in hunk", |_| "-".into()),
        ("cat -n", |n| format!("{n:>6}\t")),
    ];
    for (shape, prefix) in shapes {
        for newline in ["\n", "\r\n"] {
            let raw = prefixed_lines(&key, newline, prefix);
            let raw = if shape == "diff deletion in hunk" {
                format!("@@ -1,6 +0,0 @@{newline}{raw}{newline} context")
            } else {
                raw
            };
            assert_whole(&core, &raw, &[&armour(&raw, "PRIVATE KEY")]);
        }
    }
    let rsa = long_pem(
        "RSA PRIVATE KEY",
        &[
            "Proc-Type: 4,ENCRYPTED",
            "DEK-Info: AES-128-CBC,00FF00FF00FF00FF",
            "",
        ],
    );
    let pgp = long_pem(
        "PGP PRIVATE KEY BLOCK",
        &["Comment: Alice <alice@example.invalid>", ""],
    );
    for (lines, label) in [(&rsa, "RSA PRIVATE KEY"), (&pgp, "PGP PRIVATE KEY BLOCK")] {
        let prefixes: [Prefix; 2] = [|_| "-".into(), |n| format!("k.asc:{n}:")];
        for prefix in prefixes {
            let raw = prefixed_lines(lines, "\n", prefix);
            assert_whole(&core, &raw, &[&armour(&raw, label)]);
        }
    }
    // Trailing borders and word-diff closers after each line.
    let bordered: Vec<_> = key.iter().map(|line| format!("│ {line:<64} │")).collect();
    let table: Vec<_> = (1..)
        .zip(&key)
        .map(|(n, line)| format!("| {n} | {line} |"))
        .collect();
    let word_diff: Vec<_> = key.iter().map(|line| format!("[-{line}-]")).collect();
    for lines in [bordered, table, word_diff] {
        let raw = lines.join("\n");
        assert_whole(&core, &raw, &[&armour(&raw, "PRIVATE KEY")]);
    }
    // Entity tab indentation and HTML paragraph framing.
    for raw in [
        format!("<k>{}</k>", key.join("&#10;&#9;")),
        format!("<k>{}</k>", key.join("&#10;&#9;&#9;")),
        format!("<k>{}</k>", key.join("&#xA;&#x9;")),
        format!("<k>{}</k>", key.join("&#10;&Tab;")),
        key.iter()
            .map(|line| format!("<p>{line}</p>"))
            .collect::<String>(),
        key.iter()
            .map(|line| format!("<p>{line}</p>\n"))
            .collect::<String>(),
    ] {
        assert_whole(&core, &raw, &[&armour(&raw, "PRIVATE KEY")]);
    }
    // The prefix cannot cross the END line into a following hash.
    let diff = prefixed_lines(&key, "\n", |_| "-".into());
    let grep = prefixed_lines(&key, "\n", |n| format!("k.pem:{n}:"));
    for raw in [
        format!("{diff}\n+sha256: {SHA}"),
        format!("{grep}\nk.pem:9:{SHA}"),
        format!("{}<br>sha256: {SHA}", key.join("<br>")),
        format!(r#"{{"k": "{}", "sha": "{SHA}"}}"#, key.join(r"\n")),
    ] {
        assert_whole(&core, &raw, &[&armour(&raw, "PRIVATE KEY")]);
    }
    let ec = long_pem("EC PRIVATE KEY", &[]);
    let first = prefixed_lines(&key, "\n", |_| "-".into());
    let second = prefixed_lines(&ec, "\n", |_| "-".into());
    let raw = format!("{first}\n{second}");
    assert_whole(
        &core,
        &raw,
        &[
            &armour(&first, "PRIVATE KEY"),
            &armour(&second, "EC PRIVATE KEY"),
        ],
    );
}

#[test]
fn issuer_alphabets_keep_separator_dense_bodies_whole() {
    let core = core();
    for prefix in [
        "sk-proj-",
        "sk-svcacct-",
        "sk-admin-",
        "sk-None-",
        "sk-ant-api03-",
        "AIzaSy",
    ] {
        for size in [33, 128] {
            let body: String = "A1_-".repeat(size).chars().take(size).collect();
            let value = format!("{prefix}{body}");
            assert_whole(&core, &value, &[&value]);
        }
    }
    let jwt = format!(
        "ghs_123456_eyJ{}.{}.{}",
        "A_-1".repeat(16),
        "B_-1".repeat(16),
        "C_-1".repeat(16)
    );
    assert_whole(&core, &jwt, &[&jwt]);
    for value in [
        "sk-learn-model-v2-config".to_owned(),
        format!("sk-{}", "verify_header1_setting_".repeat(6)),
        format!("sk_live_{}", "verify_header1_setting_".repeat(6)),
        "xoxa-like-format-is-deprecated-since-2021".to_owned(),
    ] {
        assert_whole(&core, &value, &[]);
    }
}

#[test]
fn existing_credential_cues_and_fields_survive_bare_veto() {
    let core = core();
    for value in [
        format!("AIzaSy{}", "A".repeat(33)),
        format!("ghp_{}", "A".repeat(36)),
    ] {
        assert_whole(&core, &format!("api key: {value}"), &[&value]);
        assert_whole(&core, &format!(r#"{{"api_key":"{value}"}}"#), &[&value]);
    }
}

#[test]
fn literal_armour_headers_accept_free_text_and_extension_names() {
    let core = core();
    for label in ["PGP PRIVATE KEY BLOCK", "RSA PRIVATE KEY"] {
        for newline in ["\n", "\r\n"] {
            for header in [
                "Comment: Alice Example <alice@example.invalid>",
                "Comment: Alice's key",
                "Comment: \"work\"",
                "Hash: SHA256",
                "X-Custom-2: <synthetic> \\ \"quoted\" 'text'",
                "Proc-Type: 4,ENCRYPTED (synthetic)",
                "DEK-Info: AES-256-CBC,00FF (synthetic)",
            ] {
                let value = format!("-----BEGIN {label}-----{newline}{header}{newline}{newline}bm90LWEta2V5c3ludGhldGlj{newline}bm90LWEta2V5c3ludGhldGlj{newline}=ABCD{newline}-----END {label}-----");
                assert_whole(&core, &value, &[&value]);
            }
        }
    }
    let body = "bm90LWEta2V5c3ludGhldGlj";
    for value in [
        format!("-----BEGIN RSA PRIVATE KEY-----\n  Proc-Type: 4,ENCRYPTED\n  DEK-Info: AES-128-CBC,00FF\n\n  {body}\n  {body}\n  -----END RSA PRIVATE KEY-----"),
        format!("-----BEGIN PGP PRIVATE KEY BLOCK-----\n    Version: x\n\n    {body}\n    -----END PGP PRIVATE KEY BLOCK-----"),
        format!("-----BEGIN PGP PRIVATE KEY BLOCK-----\n Version: x\n\n{body}\n-----END PGP PRIVATE KEY BLOCK-----"),
        format!("-----BEGIN PGP PRIVATE KEY BLOCK-----\r\r\nVersion: x\r\r\n\r\r\n{body}\r\r\n-----END PGP PRIVATE KEY BLOCK-----"),
        format!(r"-----BEGIN PGP PRIVATE KEY BLOCK-----\u000d\u000aVersion: x\u000d\u000a\u000d\u000a{body}\u000d\u000a-----END PGP PRIVATE KEY BLOCK-----"),
    ] {
        for raw in [
            value.clone(),
            format!("key: |\n  {value}\n"),
            format!("    {value}"),
        ] {
            assert_whole(&core, &raw, &[&value]);
        }
    }
    // JSON escapes other than newlines stay inside serialized header values.
    for header in [
        r"Comment: J\u00fcrgen",
        r#"Comment: \"work\""#,
        r"Comment: C:\\keys",
        r"Comment: a\/b\tc",
    ] {
        let value = format!(
            r"-----BEGIN PGP PRIVATE KEY BLOCK-----\n{header}\n\n{body}\n-----END PGP PRIVATE KEY BLOCK-----"
        );
        assert_whole(&core, &format!(r#"{{"k": "{value}"}}"#), &[&value]);
    }
    // Markup other than a `<br` terminator stays inside serialized header values.
    for newline in [r"\n", r"\r\n", r"\u000a", "%0a", "&#10;", "<br>"] {
        for header in [
            "Comment: <synthetic>",
            "Comment: Alice <alice@example.invalid>",
            "Comment: <brian@example.invalid>",
            "Comment: a > b",
        ] {
            let value = format!("-----BEGIN PGP PRIVATE KEY BLOCK-----{newline}{header}{newline}{newline}bm90LWEta2V5c3ludGhldGlj{newline}-----END PGP PRIVATE KEY BLOCK-----");
            assert_whole(&core, &value, &[&value]);
        }
    }
    // Raw quotes and invalid escapes in a serialized header keep the WHOLE key raw.
    for newline in [r"\n", r"\r\n", "<br>"] {
        for header in [
            "Comment: Alice's key",
            "Comment: \"work\"",
            "Comment: synthetic\\path",
        ] {
            let value = format!("-----BEGIN PGP PRIVATE KEY BLOCK-----{newline}{header}{newline}{newline}bm90LWEta2V5c3ludGhldGlj{newline}-----END PGP PRIVATE KEY BLOCK-----");
            assert_whole(&core, &value, &[]);
        }
    }
}

#[test]
fn quoted_private_key_line_arrays_remain_a_documented_limit() {
    let core = core();
    for label in ["PRIVATE KEY", "RSA PRIVATE KEY", "PGP PRIVATE KEY BLOCK"] {
        for join in ["\", \"", "', '", "\",\n    \""] {
            let value = pem(label, join);
            assert_whole(&core, &format!("[\"{value}\"]"), &[]);
        }
    }
    for label in ["PUBLIC KEY", "CERTIFICATE"] {
        let value = pem(label, "\", \"");
        assert_whole(&core, &format!("[\"{value}\"]"), &[]);
    }
    let raw = "[\"-----BEGIN PRIVATE KEY-----\", \"YourPrivateKeyGoesHere\", \"-----END PRIVATE KEY-----\"]";
    assert_whole(&core, raw, &[]);
}
