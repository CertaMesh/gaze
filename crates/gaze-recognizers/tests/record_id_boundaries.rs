//! Independent application-owned byte ranges from the v27 boundary corpus.
//! Observe only public pipeline, manifest, trace, and strict restore contracts.
use gaze::{
    Action, CleanDocument, Context, DictionaryBundle, LocaleTag, PiiClass, Pipeline, Policy,
    RuleSpec, SafetyNetPolicy, Scope, Session,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    generator_version: u32,
    records: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    family: String,
    surface: String,
    text: String,
    gold: Vec<Gold>,
}

#[derive(Deserialize)]
struct Gold {
    start: usize,
    end: usize,
    label: String,
}

fn record_pipeline() -> Pipeline {
    // Retain bundled assembly and recognizers, while observing only the three
    // record classes in scope. The full-policy gate accounts for other classes.
    let mut policy = Policy::default();
    policy.rulepacks.bundled = vec!["core".to_owned()];
    policy.rules = ["customer_id", "employee_id", "record_id"]
        .into_iter()
        .map(|class| RuleSpec::Class {
            class: PiiClass::custom(class).unwrap(),
            action: Action::Tokenize,
        })
        .chain([RuleSpec::Default {
            action: Action::Preserve,
        }])
        .collect();
    let inputs = gaze_assembly::resolve_policy_inputs(&policy, None, None, None).unwrap();
    let context = Context {
        dictionaries: Default::default(),
        class_map: Default::default(),
        fields: Default::default(),
        record_match_kinds: Default::default(),
        record_value_rejections: Default::default(),
    };
    gaze_assembly::build_pipeline(
        &policy,
        &context,
        &inputs.rulepacks,
        &inputs.locale_chain,
        None,
    )
    .unwrap()
}

fn check_cells(family: &str, surface_prefix: &str) {
    check_selected_cells(family, surface_prefix, false);
}

fn check_selected_cells(family: &str, surface_prefix: &str, benign_only: bool) {
    let fixture: Fixture =
        serde_json::from_str(include_str!("fixtures/record_id_boundaries_v27.json")).unwrap();
    assert_eq!(fixture.generator_version, 27);
    let pipeline = record_pipeline();
    let mut checked = 0;
    for case in fixture.records.iter().filter(|case| {
        case.family == family
            && case.surface.starts_with(surface_prefix)
            && (!benign_only || case.gold.is_empty())
    }) {
        checked += 1;
        let session = Session::new(Scope::Ephemeral).unwrap();
        let (clean, spans, _, trace, _) = pipeline
            .clean_text_with_safety_net_policy_detect_context_and_protection_evidence(
                &session,
                &case.text,
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::default(),
            )
            .unwrap();
        let CleanDocument::Text(clean) = clean else {
            panic!("text")
        };
        assert_eq!(
            pipeline.restore_strict_text(&session, &clean).unwrap(),
            case.text,
            "{}: exact restore",
            case.id
        );
        let entries = session.snapshot_entries();
        // Unquoted tab grouping retains its existing conservative public cost.
        let conservative =
            !case.gold.is_empty() && case.surface.ends_with("tab_group_ambiguous_column");
        let mut expected = case.text.clone();
        for gold in case.gold.iter().rev() {
            let value = &case.text[gold.start..gold.end];
            let class = match gold.label.as_str() {
                "CUSTOMER_ID" => "customer_id",
                "EMPLOYEE_ID" => "employee_id",
                "RECORD_ID" => "record_id",
                label => panic!("unexpected gold class {label}"),
            };
            let class = PiiClass::custom(class).unwrap();
            assert!(
                spans.iter().zip(&trace).any(|(span, evidence)| {
                    span.raw_span.start <= gold.start
                        && span.raw_span.end >= gold.end
                        && evidence.class() == &class
                }),
                "{}: complete typed gold leaked: {:?} -> {clean:?}; entries={entries:?}; trace={trace:?}",
                case.id,
                value
            );
            if conservative {
                assert!(
                    entries
                        .iter()
                        .any(|entry| entry.class == class && entry.raw.contains(value)),
                    "{}: complete original missing from manifest",
                    case.id
                );
                continue;
            }
            let entry = entries
                .iter()
                .find(|entry| entry.raw == value && entry.class == class)
                .unwrap_or_else(|| {
                    panic!(
                        "{}: punctuation or public bytes entered the manifest: {:?}",
                        case.id, value
                    )
                });
            expected.replace_range(gold.start..gold.end, &entry.token);
        }
        if !conservative {
            assert_eq!(clean, expected, "{}: public bytes changed", case.id);
        }
    }
    assert!(checked > 0, "missing independently authored cells");
}

#[test]
fn public_keys_headers_placeholders_and_tsv_counterweights_stay_raw() {
    for family in ["customer_id", "employee_id", "record_id"] {
        check_selected_cells(family, "recordids_boundary_", true);
    }
}

#[test]
fn customer_punctuation_preserves_public_bytes_and_repeat_seeds() {
    check_cells("customer_id", "recordids_boundary_punctuation_");
}

#[test]
fn employee_punctuation_preserves_public_bytes_and_repeat_seeds() {
    check_cells("employee_id", "recordids_boundary_punctuation_");
}

#[test]
fn member_punctuation_preserves_public_bytes_and_repeat_seeds() {
    check_cells("record_id", "recordids_boundary_punctuation_");
}

macro_rules! boundary_test {
    ($name:ident, $family:literal, $surface:literal) => {
        #[test]
        fn $name() {
            check_cells($family, concat!("recordids_boundary_", $surface));
        }
    };
}

boundary_test!(
    customer_dotted_sentence_repeat,
    "customer_id",
    "punctuation_short_dotted"
);
boundary_test!(
    employee_dotted_sentence_repeat,
    "employee_id",
    "punctuation_short_dotted"
);
boundary_test!(
    member_dotted_sentence_repeat,
    "record_id",
    "punctuation_short_dotted"
);
boundary_test!(
    customer_connector_field_boundaries,
    "customer_id",
    "connectors_"
);
boundary_test!(
    employee_connector_field_boundaries,
    "employee_id",
    "connectors_"
);
boundary_test!(
    member_connector_field_boundaries,
    "record_id",
    "connectors_"
);
boundary_test!(
    customer_serialized_raw_scalars,
    "customer_id",
    "serialized_"
);
boundary_test!(
    employee_serialized_raw_scalars,
    "employee_id",
    "serialized_"
);
boundary_test!(member_serialized_raw_scalars, "record_id", "serialized_");
boundary_test!(customer_opening_newlines, "customer_id", "opening_lf_");
boundary_test!(employee_opening_newlines, "employee_id", "opening_lf_");
boundary_test!(member_opening_newlines, "record_id", "opening_lf_");
boundary_test!(
    customer_tab_groups_and_public_columns,
    "customer_id",
    "tab_group_"
);
boundary_test!(
    employee_tab_groups_and_public_columns,
    "employee_id",
    "tab_group_"
);
boundary_test!(
    member_tab_groups_and_public_columns,
    "record_id",
    "tab_group_"
);

fn check_legacy_cue_ownership(cue: &str, class: &str) {
    use gaze_assembly::CorePipelineConfig;
    let core = CorePipelineConfig::new().build().unwrap();
    for connector in ["=", ": ", " is ", " ist ", " lautet ", " "] {
        let input =
            format!("{cue}: Q7312845 BUILD VERSION{connector}FRAME; lookup Q7312845 again.");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap() else {
            panic!("text")
        };
        let entries = session.snapshot_entries();
        let entry = entries
            .iter()
            .find(|entry| {
                entry.class == PiiClass::custom(class).unwrap() && entry.raw == "Q7312845"
            })
            .unwrap_or_else(|| {
                panic!("legacy next cue swallowed: {input:?} -> {clean:?}; {entries:?}")
            });
        assert_eq!(
            clean,
            format!(
                "{cue}: {} BUILD VERSION{connector}FRAME; lookup {} again.",
                entry.token, entry.token
            )
        );
        assert_eq!(
            core.pipeline()
                .restore_strict_text(&session, &clean)
                .unwrap(),
            input
        );
    }
    // A broader legacy regex can already prove a group that resembles a cue.
    // The extension fix must never trim that original capture.
    let input = format!("{cue}: Q731284 BUILD VERSION=FRAME");
    let session = Session::new(Scope::Ephemeral).unwrap();
    let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap() else {
        panic!("text")
    };
    let entries = session.snapshot_entries();
    let entry = entries
        .iter()
        .find(|entry| entry.raw == "Q731284 BUILD")
        .expect("original capture retained");
    assert_eq!(clean, format!("{cue}: {} VERSION=FRAME", entry.token));
    assert_eq!(
        core.pipeline()
            .restore_strict_text(&session, &clean)
            .unwrap(),
        input
    );
    // Uppercase and horizontal grouping alone never prove a next-field cue.
    // Every coherent value suffix must be protected, including numeric groups.
    for value in ["Q731284 ABCD 204", "Q731284 842395", "AB 204731 284"] {
        let input = format!("{cue}: {value}; lookup {value} again.");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap() else {
            panic!("text")
        };
        let entries = session.snapshot_entries();
        let entry = entries
            .iter()
            .find(|entry| entry.raw == value)
            .unwrap_or_else(|| {
                panic!("legacy value suffix leaked: {input:?} -> {clean:?}; {entries:?}")
            });
        assert_eq!(
            clean,
            format!("{cue}: {}; lookup {} again.", entry.token, entry.token)
        );
        assert_eq!(
            core.pipeline()
                .restore_strict_text(&session, &clean)
                .unwrap(),
            input
        );
    }
}

#[test]
fn national_identifier_keeps_its_value_and_stops_at_complete_next_cues() {
    check_legacy_cue_ownership("identity card", "national_id");
}

#[test]
fn tax_identifier_keeps_its_value_and_stops_at_complete_next_cues() {
    check_legacy_cue_ownership("tax number", "tax_number");
}

#[test]
fn licence_identifier_keeps_its_value_and_stops_at_complete_next_cues() {
    check_legacy_cue_ownership("driver license", "driver_license");
}

#[test]
fn legacy_public_metadata_cues_require_a_recognized_namespace() {
    let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
    for cue in ["identity card", "tax number", "driver license"] {
        for namespace in [
            "RELEASE", "SCHEMA", "PROTOCOL", "RUNTIME", "FORMAT", "API", "PACKAGE", "SOFTWARE",
        ] {
            for connector in ["=", ": ", " is ", " ist ", " lautet ", " "] {
                let input = format!("{cue}: Q7312845 {namespace} VERSION{connector}FRAME");
                let session = Session::new(Scope::Ephemeral).unwrap();
                let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap()
                else {
                    panic!("text")
                };
                let entry = session
                    .snapshot_entries()
                    .into_iter()
                    .find(|entry| entry.raw == "Q7312845")
                    .unwrap_or_else(|| {
                        panic!("public metadata cue swallowed: {input:?} -> {clean:?}")
                    });
                assert_eq!(
                    clean,
                    format!("{cue}: {} {namespace} VERSION{connector}FRAME", entry.token)
                );
                assert_eq!(
                    core.pipeline()
                        .restore_strict_text(&session, &clean)
                        .unwrap(),
                    input
                );
            }
        }
    }
}

#[test]
fn legacy_private_and_unknown_cues_never_expose_an_unclaimed_value() {
    let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
    for cue in ["identity card", "tax number", "driver license"] {
        for next in [
            "PASSPORT",
            "FOO",
            "BUILD ID",
            "RELEASE NUMBER",
            "SCHEMA REFERENCE",
            "RUNTIME VALUE",
            "SECRET VERSION",
            "NOTBUILD VERSION",
            "BUILD VERSIONX",
        ] {
            for connector in [": ", "="] {
                let input = format!("{cue}: Q7312845 {next}{connector}EF34 GH5678");
                let session = Session::new(Scope::Ephemeral).unwrap();
                let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap()
                else {
                    panic!("text")
                };
                let mut visible = clean.clone();
                for entry in session.snapshot_entries() {
                    visible = visible.replace(&entry.token, "");
                }
                for value in ["Q7312845", "EF34", "GH5678"] {
                    assert!(
                        !visible.contains(value),
                        "unclaimed private bytes exposed: {input:?} -> {clean:?}"
                    );
                }
                assert_eq!(
                    core.pipeline()
                        .restore_strict_text(&session, &clean)
                        .unwrap(),
                    input
                );
            }
        }
    }
}

#[test]
fn complete_licence_cues_leave_the_next_value_fully_protected() {
    let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
    for cue in ["DRIVER LICENSE", "DRIVER LICENCE", "FÜHRERSCHEIN"] {
        for connector in [": ", "=", " is ", " ist ", " lautet "] {
            let input = format!("customer ID: AB12 CD3456 {cue}{connector}EF34 GH5678");
            let session = Session::new(Scope::Ephemeral).unwrap();
            let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap()
            else {
                panic!("text")
            };
            let entries = session.snapshot_entries();
            let first = entries
                .iter()
                .find(|entry| entry.raw == "AB12 CD3456")
                .unwrap_or_else(|| panic!("complete cue swallowed: {input:?}; {entries:?}"));
            let next = entries
                .iter()
                .find(|entry| {
                    entry.raw == "EF34 GH5678"
                        && entry.class == PiiClass::custom("driver_license").unwrap()
                })
                .unwrap_or_else(|| panic!("next licence value leaked: {input:?}; {entries:?}"));
            assert_eq!(
                clean,
                format!(
                    "customer ID: {} {cue}{connector}{}",
                    first.token, next.token
                )
            );
            assert_eq!(
                core.pipeline()
                    .restore_strict_text(&session, &clean)
                    .unwrap(),
                input
            );
        }
    }
}

#[test]
fn legacy_german_licence_tails_preserve_raw_values_at_every_connector() {
    let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
    for prior in ["driver license", "tax number", "identity card"] {
        for next in ["FÜHRERSCHEIN", "FU\u{0308}HRERSCHEIN"] {
            for connector in [": ", "=", " is ", " ist ", " lautet "] {
                let input = format!("{prior}: Q7312845 {next}{connector}EF34 GH5678");
                let session = Session::new(Scope::Ephemeral).unwrap();
                let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap()
                else {
                    panic!("text")
                };
                let entries = session.snapshot_entries();
                let mut visible = clean.clone();
                for entry in &entries {
                    let start = input.find(&entry.raw).expect("exact original raw bytes");
                    assert!(input.is_char_boundary(start + entry.raw.len()));
                    visible = visible.replace(&entry.token, "");
                }
                for value in ["Q7312845", "EF34", "GH5678"] {
                    assert!(
                        !visible.contains(value),
                        "German private tail leaked: {input:?} -> {clean:?}"
                    );
                }
                assert!(
                    entries
                        .iter()
                        .any(|entry| entry.raw.contains("EF34 GH5678")),
                    "complete next value missing: {entries:?}"
                );
                assert!(
                    entries.iter().any(|entry| entry.raw.contains("Q7312845")),
                    "earlier capture missing: {entries:?}"
                );
                assert_eq!(
                    core.pipeline()
                        .restore_strict_text(&session, &clean)
                        .unwrap(),
                    input
                );
            }
        }
    }
}

#[test]
fn legacy_unicode_groups_require_numeric_or_cased_uppercase_evidence() {
    let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
    for suffix in ["ÉTAGE 204", "E\u{0301}TAGE 204", "ЖА 204"] {
        let value = format!("Q7312845 {suffix}");
        let input = format!("driver license: {value}; checked.");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap() else {
            panic!("text")
        };
        assert!(
            session
                .snapshot_entries()
                .iter()
                .any(|entry| entry.raw == value),
            "Unicode value lost original bytes: {input:?} -> {clean:?}"
        );
        assert_eq!(
            core.pipeline()
                .restore_strict_text(&session, &clean)
                .unwrap(),
            input
        );
    }
    for suffix in [
        "étiquette validée",
        "Étiquette validée",
        "東京 状態",
        "公開バージョン FRAME",
        "\u{0301}\u{0308} only",
        "release VERSION FRAME",
    ] {
        let input = format!("driver license: Q7312845 {suffix}");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap() else {
            panic!("text")
        };
        let first = session
            .snapshot_entries()
            .into_iter()
            .find(|entry| entry.raw == "Q7312845")
            .expect("prose never extends the identifier");
        assert_eq!(clean, format!("driver license: {} {suffix}", first.token));
        assert_eq!(
            core.pipeline()
                .restore_strict_text(&session, &clean)
                .unwrap(),
            input
        );
    }
}

#[derive(Clone, Default)]
struct ScanReasons(std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>);

impl gaze::RedactionLogger for ScanReasons {
    fn log(&self, entry: &gaze::RedactionEntry) -> Result<(), gaze::RedactionLogError> {
        if !entry.conflict_loser && entry.action == Action::Tokenize {
            self.0
                .lock()
                .unwrap()
                .push(serde_json::to_value(entry.labelled_value_scan_reason).unwrap());
        }
        Ok(())
    }
}

#[test]
fn nested_quotes_and_inconsistent_contexts_keep_private_content() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("fixtures/record_id_boundaries_v27.json")).unwrap();
    for family in ["customer_id", "employee_id", "record_id"] {
        let case = fixture
            .records
            .iter()
            .find(|case| {
                case.family == family
                    && case.surface == "recordids_boundary_serialized_depth9_unicode"
                    && case.id.contains("-A-")
            })
            .unwrap();
        let gold = &case.gold[0];
        let pipeline = record_pipeline();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = pipeline
            .clean_text_with_safety_net_policy_detect_context_and_protection_evidence(
                &session,
                &case.text,
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::default(),
            )
            .unwrap()
            .0
        else {
            panic!("text")
        };
        assert!(
            session
                .snapshot_entries()
                .iter()
                .any(|entry| entry.raw == case.text[gold.start..gold.end]),
            "nested scalar must exclude the structure tail"
        );
        assert_eq!(
            pipeline.restore_strict_text(&session, &clean).unwrap(),
            case.text
        );
    }
    for cue in ["customer ID", "employee ID", "member ID"] {
        let input = format!(
            "{cue}: {}\"Q731284\\\"; public_version=FRAME",
            "\\".repeat(3)
        );
        let reasons = ScanReasons::default();
        let pipeline = record_pipeline().with_redaction_logger(reasons.clone());
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = pipeline
            .clean_text_with_safety_net_policy_detect_context_and_protection_evidence(
                &session,
                &input,
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::default(),
            )
            .unwrap()
            .0
        else {
            panic!("text")
        };
        assert!(session
            .snapshot_entries()
            .iter()
            .any(|entry| entry.raw == "Q731284\\\"; public_version=FRAME"));
        assert!(!clean.contains("FRAME"));
        assert!(reasons
            .0
            .lock()
            .unwrap()
            .contains(&serde_json::json!("uncertain_quote_boundary")));
        assert_eq!(
            pipeline.restore_strict_text(&session, &clean).unwrap(),
            input
        );
    }
}

fn public_clean(pipeline: &Pipeline, session: &Session, input: &str) -> String {
    let (CleanDocument::Text(clean), _, _, _, _) = pipeline
        .clean_text_with_safety_net_policy_detect_context_and_protection_evidence(
            session,
            input,
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .unwrap()
    else {
        panic!("text")
    };
    assert_eq!(
        pipeline.restore_strict_text(session, &clean).unwrap(),
        input
    );
    clean
}

#[test]
fn field_owned_repeat_seeds_survive_later_turns_and_signed_snapshots() {
    let pipeline = record_pipeline();
    for cue in ["customer ID", "employee ID", "member ID"] {
        for (value, declaration) in [
            (
                "\nNX731284 É842395",
                format!("{cue}: \"\nNX731284 É842395\"; public_version=FRAME"),
            ),
            (
                " NX731284 É842395",
                format!("{cue}: \" NX731284 É842395\npublic_version=FRAME"),
            ),
            (
                "NX731284\tÉ842395",
                format!("{cue}: NX731284\tÉ842395\tONYX"),
            ),
        ] {
            let session =
                Session::new(Scope::Conversation("synthetic-repeat-contract".into())).unwrap();
            public_clean(&pipeline, &session, &declaration);
            let snapshot = session.export().unwrap().into_bytes();
            assert_eq!(
                snapshot[0], 8,
                "new seed metadata requires a fail-closed envelope"
            );
            let mut downgraded = snapshot.clone();
            downgraded[0] = 6;
            assert!(matches!(
                Session::import(downgraded.into()),
                Err(gaze::Error::InvalidSnapshotSignature)
            ));
            let imported = Session::import(snapshot.into()).unwrap();
            let input = format!("Lookup {value} again.");
            let clean = public_clean(&pipeline, &imported, &input);
            let entries = imported.snapshot_entries();
            let entry = entries
                .iter()
                .find(|entry| entry.raw == value)
                .expect("complete raw repeat is mapped");
            assert_eq!(clean, format!("Lookup {} again.", entry.token));
        }
    }
}

#[test]
fn uncertain_repeat_prefixes_keep_the_numeric_floor_and_public_cue_bytes() {
    let pipeline = record_pipeline();
    for declaration in [
        "customer ID: \"1234; public_version=FRAME",
        "customer ID: 1234\tONYX",
    ] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let input = format!("Lookup 1234 again.\n{declaration}");
        let clean = public_clean(&pipeline, &session, &input);
        assert!(
            clean.starts_with("Lookup 1234 again.\n"),
            "short numeric prefix must not sweep"
        );
    }
    // The apparent following label must never enter a conservative prefix seed.
    let session = Session::new(Scope::Ephemeral).unwrap();
    public_clean(
        &pipeline,
        &session,
        "customer ID: \"NX731284 É842395; public_version=FRAME",
    );
    assert_eq!(
        public_clean(&pipeline, &session, "public_version=FRAME"),
        "public_version=FRAME"
    );
}

#[test]
fn legacy_lowercase_actions_and_repeat_markers_keep_scalar_repeat_protection() {
    let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
    for cue in ["identity card", "tax number", "driver license"] {
        for action in [
            "action=lookup",
            "action: lookup",
            "verified is PUBLIC",
            "checked ist PUBLIC",
            "reviewed lautet PUBLIC",
        ] {
            let input = format!("{cue}: Q7312845 {action}; repeat: Q7312845");
            let session = Session::new(Scope::Ephemeral).unwrap();
            let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap()
            else {
                panic!("text")
            };
            let entry = session
                .snapshot_entries()
                .into_iter()
                .find(|entry| entry.raw == "Q7312845")
                .unwrap_or_else(|| {
                    panic!("lowercase action consumed scalar: {input:?} -> {clean:?}")
                });
            assert_eq!(
                clean,
                format!("{cue}: {} {action}; repeat: {}", entry.token, entry.token)
            );
            assert_eq!(
                core.pipeline()
                    .restore_strict_text(&session, &clean)
                    .unwrap(),
                input
            );
        }
    }
}

#[test]
fn legacy_complete_connector_state_expires_before_unrelated_text_or_next_record() {
    let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
    for cue in ["identity card", "tax number", "driver license"] {
        for connector in ["is", "ist", "lautet"] {
            for boundary in [";", "\n"] {
                let tail = format!(
                    "verified {connector} PUBLIC{boundary} driver license: Q8423956 action=lookup"
                );
                let input =
                    format!("{cue}: Q7312845 SECRET VERSION {connector} EF34 GH5678 {tail}");
                let session = Session::new(Scope::Ephemeral).unwrap();
                let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap()
                else {
                    panic!("text")
                };
                let entries = session.snapshot_entries();
                let private = format!("Q7312845 SECRET VERSION {connector} EF34 GH5678");
                assert!(
                    entries.iter().any(|entry| entry.raw == private),
                    "complete private cue lost: {input:?} -> {clean:?}"
                );
                assert!(
                    clean.contains(&format!("verified {connector} PUBLIC{boundary}")),
                    "expired connector consumed public text: {input:?} -> {clean:?}"
                );
                assert!(clean.ends_with(" action=lookup"));
                assert!(entries.iter().any(|entry| entry.raw == "Q8423956"));
                assert_eq!(
                    core.pipeline()
                        .restore_strict_text(&session, &clean)
                        .unwrap(),
                    input
                );
            }
        }
    }
}

#[test]
fn legacy_uncertain_complete_cues_keep_first_value_repeats_and_provenance() {
    let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
    for cue in ["identity card", "tax number", "driver license"] {
        for next in [
            "Fu\u{0308}hrerschein",
            "FU\u{0308}HRERSCHEIN",
            "Führerschein",
            "FÜHRERSCHEIN",
        ] {
            for connector in [": ", "=", " is ", " ist ", " lautet "] {
                let input =
                    format!("{cue}: Q9176243 {next}{connector}EF34 GH5678; lookup Q9176243 again.");
                let reasons = ScanReasons::default();
                let pipeline = core
                    .pipeline()
                    .clone()
                    .with_redaction_logger(reasons.clone());
                let session =
                    Session::new(Scope::Conversation("synthetic-legacy-seed".into())).unwrap();
                let clean = public_clean(&pipeline, &session, &input);
                let imported = Session::import(session.export().unwrap()).unwrap();
                assert_eq!(
                    pipeline.restore_strict_text(&imported, &clean).unwrap(),
                    input
                );
                let entries = session.snapshot_entries();
                let mut visible = clean.clone();
                for entry in &entries {
                    visible = visible.replace(&entry.token, "");
                }
                for private in ["Q9176243", "EF34", "GH5678"] {
                    assert!(
                        !visible.contains(private),
                        "legacy first-value repeat leaked: {input:?} -> {clean:?}"
                    );
                }
                if next == "Fu\u{0308}hrerschein" {
                    assert!(
                        reasons
                            .0
                            .lock()
                            .unwrap()
                            .contains(&serde_json::json!("uncertain_repeat_prefix")),
                        "uncertain repeat must retain provenance"
                    );
                }
            }
        }
    }
}

#[test]
fn legacy_uncertain_prefix_geometry_retains_wrappers_floor_and_long_values() {
    let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
    for value in [
        "Q9176243".to_owned(),
        "Q9176243 AB2345 CD6789 EF1234 GH5678 IJ9012 KL3456".to_owned(),
    ] {
        for wrapped in [false, true] {
            let declaration = if wrapped {
                format!("<{value} Fu\u{0308}hrerschein is EF34 GH5678>")
            } else {
                format!("{value} SECRET VERSION is EF34 GH5678")
            };
            let repeat = if wrapped {
                format!("<{value}")
            } else {
                value.clone()
            };
            let closing = if wrapped { ">" } else { "" };
            let input = format!("driver license: {declaration}; lookup {repeat}{closing} again.");
            let reasons = ScanReasons::default();
            let pipeline = if wrapped {
                // The adopter recognizer admits a wrapper through the public
                // seam, exercising scanner geometry independently of cue seeds.
                let detector = gaze_recognizers::RegexDetector::with_rulepack_fields(
                    r"driver license: <(Q[0-9]+)",
                    PiiClass::custom("driver_license").unwrap(),
                    "synthetic.wrapped.licence",
                    vec![LocaleTag::Global],
                    0.9,
                    100,
                    "driver_license.counter",
                    Some(vec![1]),
                    vec![],
                    None,
                    None,
                )
                .unwrap()
                .with_complete_labelled_value(true);
                Pipeline::builder()
                    .recognizer(detector)
                    .rule(gaze::DefaultRule::new(Action::Tokenize))
                    .build()
                    .unwrap()
            } else {
                core.pipeline().clone()
            }
            .with_redaction_logger(reasons.clone());
            let session =
                Session::new(Scope::Conversation("synthetic-legacy-seed".into())).unwrap();
            let clean = public_clean(&pipeline, &session, &input);
            let imported = Session::import(session.export().unwrap()).unwrap();
            assert_eq!(
                pipeline.restore_strict_text(&imported, &clean).unwrap(),
                input
            );
            let entries = session.snapshot_entries();
            assert!(
                entries.iter().any(|entry| entry.raw == declaration),
                "whole wrapped/long declaration owner lost: {input:?} -> {clean:?}"
            );
            assert!(
                entries.iter().any(|entry| entry.raw == repeat),
                "raw repeat lost: {input:?} -> {clean:?}"
            );
            assert!(reasons
                .0
                .lock()
                .unwrap()
                .contains(&serde_json::json!("uncertain_repeat_prefix")));
        }
    }
    let input = "driver license: Q1234 Fu\u{0308}hrerschein is EF34 GH5678; lookup Q1234 again.";
    let session = Session::new(Scope::Conversation("synthetic-legacy-seed".into())).unwrap();
    let clean = public_clean(core.pipeline(), &session, input);
    assert!(
        clean.ends_with("lookup Q1234 again."),
        "short prefix cannot lower the existing repeat floor"
    );
}

#[test]
fn legacy_rejected_or_public_cues_do_not_license_uncertain_prefixes() {
    let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
    for tail in [
        "verified is PUBLIC",
        "action=lookup",
        "2027-04-12",
        "RELEASE VERSION=FRAME",
        "SECRET VERSION verified is PUBLIC",
        "; SECRET VERSION is PUBLIC",
        "\nSECRET VERSION is PUBLIC",
    ] {
        let input = format!("driver license: Q9176243 {tail}; lookup Q9176243 again.");
        let reasons = ScanReasons::default();
        let pipeline = core
            .pipeline()
            .clone()
            .with_redaction_logger(reasons.clone());
        let session = Session::new(Scope::Conversation("synthetic-legacy-seed".into())).unwrap();
        public_clean(&pipeline, &session, &input);
        assert!(
            !reasons
                .0
                .lock()
                .unwrap()
                .contains(&serde_json::json!("uncertain_repeat_prefix")),
            "unconsumed/public cue licensed uncertainty: {input:?}"
        );
    }
}

#[test]
fn legacy_uncertain_seed_uses_earliest_cue_and_charges_public_copy_bytes() {
    let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
    for cue in ["identity card", "tax number", "driver license"] {
        let owned = "Q9176243 PASSPORT: EF34 GH5678 SECRET VERSION is ZX98 KL7654";
        let input = format!("{cue}: {owned}; lookup Q9176243 again.\nOperational inventory: public artifact reference Q9176243.");
        let reasons = ScanReasons::default();
        let pipeline = core
            .pipeline()
            .clone()
            .with_redaction_logger(reasons.clone());
        let session =
            Session::new(Scope::Conversation("synthetic-legacy-prefix-cost".into())).unwrap();
        let clean = public_clean(&pipeline, &session, &input);
        let entries = session.snapshot_entries();
        assert!(entries.iter().any(|entry| entry.raw == owned));
        let repeat = entries
            .iter()
            .find(|entry| entry.raw == "Q9176243")
            .expect("earliest prefix survives later complete cues");
        assert!(clean.ends_with(&format!("public artifact reference {}.", repeat.token)));
        // The independently public artifact copy is non-gold: exactly eight
        // extra protected bytes, charged as a conservative precision cost.
        assert_eq!(repeat.raw.len(), 8);
        assert!(reasons
            .0
            .lock()
            .unwrap()
            .contains(&serde_json::json!("uncertain_repeat_prefix")));
        let imported = Session::import(session.export().unwrap()).unwrap();
        assert_eq!(
            pipeline.restore_strict_text(&imported, &clean).unwrap(),
            input
        );
    }
}

fn wrapper_floor_check(
    raw_body: &str,
    normalized_body: &str,
    class: PiiClass,
    wrapped: bool,
    eligible: bool,
) {
    let pattern = format!(
        "short code: {}({})",
        if wrapped { "<" } else { "" },
        regex::escape(normalized_body)
    );
    let detector = gaze_recognizers::RegexDetector::with_rulepack_fields(
        &pattern,
        class,
        "synthetic.wrapper.floor",
        vec![LocaleTag::Global],
        0.9,
        100,
        "synthetic.counter",
        Some(vec![1]),
        vec![],
        None,
        None,
    )
    .unwrap()
    .with_complete_labelled_value(true);
    let reasons = ScanReasons::default();
    let pipeline = Pipeline::builder()
        .recognizer(detector)
        .rule(gaze::DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap()
        .with_redaction_logger(reasons.clone());
    let declaration = format!(
        "{}{raw_body} Fu\u{0308}hrerschein is EF34 GH5678{}",
        if wrapped { "<" } else { "" },
        if wrapped { ">" } else { "" }
    );
    let repeat = format!("{}{raw_body}", if wrapped { "<" } else { "" });
    let input = format!(
        "short code: {declaration}; visible copy: {repeat}{}.",
        if wrapped { ">" } else { "" }
    );
    let session = Session::new(Scope::Conversation("synthetic-wrapper-floor".into())).unwrap();
    let clean = public_clean(&pipeline, &session, &input);
    assert!(
        session
            .snapshot_entries()
            .iter()
            .any(|entry| entry.raw == declaration),
        "whole owner lost: {input:?} -> {clean:?}"
    );
    let imported = Session::import(session.export().unwrap()).unwrap();
    assert_eq!(
        pipeline.restore_strict_text(&imported, &clean).unwrap(),
        input
    );
    let protected = session
        .snapshot_entries()
        .iter()
        .any(|entry| entry.raw == repeat);
    assert_eq!(
        protected, eligible,
        "wrapper/body floor differs: {input:?} -> {clean:?}"
    );
    assert_eq!(
        reasons
            .0
            .lock()
            .unwrap()
            .contains(&serde_json::json!("uncertain_repeat_prefix")),
        eligible
    );
    if !eligible {
        assert!(clean.ends_with(&format!(
            "visible copy: {repeat}{}.",
            if wrapped { ">" } else { "" }
        )));
    }
}

#[test]
fn scanner_prepended_wrapper_cannot_lower_numeric_repeat_floor() {
    for wrapped in [false, true] {
        for (body, eligible) in [
            ("12345", false),
            ("123456", true),
            ("12345678", true),
            ("12 345", false),
            ("12 3456", true),
        ] {
            wrapper_floor_check(
                body,
                body,
                PiiClass::custom("synthetic_identifier").unwrap(),
                wrapped,
                eligible,
            );
        }
    }
}

#[test]
fn wrapper_floor_uses_pipeline_normalization_domain() {
    for (raw, normalized, eligible) in [
        ("１２３４５", "12345", false),
        ("１２３４５６", "123456", true),
        ("12\u{200d}345", "12345", false),
        ("12\u{200d}3456", "123456", true),
        ("12\u{a0}345", "12 345", false),
        ("12\u{202f}3456", "12 3456", true),
        ("ABＣ", "ABC", false),
        ("ABＣＤ", "ABCD", true),
        ("E\u{0301}TA", "E\u{0301}TA", true),
        ("東京", "東京", false),
    ] {
        wrapper_floor_check(
            raw,
            normalized,
            PiiClass::custom("synthetic_identifier").unwrap(),
            true,
            eligible,
        );
    }
    wrapper_floor_check(
        "12345",
        "12345",
        PiiClass::family("synthetic_identifier"),
        true,
        true,
    );
}

#[test]
fn content_wrapper_is_not_a_scanner_prepended_wrapper() {
    let detector = gaze_recognizers::RegexDetector::with_rulepack_fields(
        r"short code: (<12345)",
        PiiClass::custom("synthetic_identifier").unwrap(),
        "synthetic.content.wrapper",
        vec![LocaleTag::Global],
        0.9,
        100,
        "synthetic.counter",
        Some(vec![1]),
        vec![],
        None,
        None,
    )
    .unwrap()
    .with_complete_labelled_value(true);
    let pipeline = Pipeline::builder()
        .recognizer(detector)
        .rule(gaze::DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    let input = "short code: <12345>; visible copy: <12345>.";
    let session = Session::new(Scope::Conversation("synthetic-content-wrapper".into())).unwrap();
    let clean = public_clean(&pipeline, &session, input);
    // An original capture includes the wrapper; its existing whole-value
    // pattern is outside the scanner-prepended seed eligibility guard.
    assert!(!clean.contains("12345"));
    let imported = Session::import(session.export().unwrap()).unwrap();
    assert_eq!(
        pipeline.restore_strict_text(&imported, &clean).unwrap(),
        input
    );
}

boundary_test!(customer_unterminated_corpus, "customer_id", "unterminated_");
boundary_test!(employee_unterminated_corpus, "employee_id", "unterminated_");
boundary_test!(member_unterminated_corpus, "record_id", "unterminated_");
boundary_test!(customer_private_key_corpus, "customer_id", "private_keys_");
boundary_test!(employee_private_key_corpus, "employee_id", "private_keys_");
boundary_test!(member_private_key_corpus, "record_id", "private_keys_");
boundary_test!(
    customer_quote_dialect_corpus,
    "customer_id",
    "quote_dialects_"
);
boundary_test!(
    employee_quote_dialect_corpus,
    "employee_id",
    "quote_dialects_"
);
boundary_test!(member_quote_dialect_corpus, "record_id", "quote_dialects_");

#[test]
fn unterminated_values_own_key_looking_private_suffixes() {
    let pipeline = record_pipeline();
    for cue in ["customer ID", "employee ID", "member ID"] {
        for tail in [
            ";É842395",
            ",É842395",
            "]É842395",
            "}É842395",
            ";private_code=É842395",
            ";ID: 123",
            ", name=Anna",
            ";\u{a0}private_code=É842395",
            "; public_version=FRAME",
            "; \"public_version\": 7",
            "\\\nprivate_code=É842395",
        ] {
            let value = format!("Q731284{tail}");
            let input = format!("{cue}: \"{value}");
            let session =
                Session::new(Scope::Conversation("synthetic-unterminated-value".into())).unwrap();
            let clean = public_clean(&pipeline, &session, &input);
            assert!(
                session
                    .snapshot_entries()
                    .iter()
                    .any(|entry| entry.raw == value),
                "entire private value must be owned: {input:?} -> {clean:?}"
            );
            let imported = Session::import(session.export().unwrap()).unwrap();
            assert_eq!(
                pipeline.restore_strict_text(&imported, &clean).unwrap(),
                input
            );
        }
    }
}

fn check_quote_dialect(pipeline: &Pipeline, input: String, value: String) {
    check_quote_dialect_depths(pipeline, input, value, 9);
}

fn check_quote_dialect_depths(
    pipeline: &Pipeline,
    mut input: String,
    mut value: String,
    max_depth: usize,
) {
    let class = if input.contains("customerId") {
        "customer_id"
    } else if input.contains("employeeId") {
        "employee_id"
    } else {
        "record_id"
    };
    let class = PiiClass::custom(class).unwrap();
    for depth in 0..=max_depth {
        let session = Session::new(Scope::Conversation("synthetic-quote-dialect".into())).unwrap();
        let clean = public_clean(pipeline, &session, &input);
        let entries = session.snapshot_entries();
        let mut decoded = value.clone();
        while let Some((text, _)) = gaze_types::json_regions::unescape_json_fragments(&decoded) {
            decoded = text;
        }
        if !decoded.chars().any(char::is_numeric) {
            // The digit floor deliberately excludes quoted alphabetic placeholders.
            assert_eq!(clean, input, "digit-free quoted data stays public");
            assert!(entries.is_empty());
            input = serde_json::to_string(&serde_json::json!({
                "note": input, "public_version": "'", "tags": ["'", "FRAME"]
            }))
            .unwrap();
            let encoded = serde_json::to_string(&value).unwrap();
            value = encoded[1..encoded.len() - 1].to_owned();
            continue;
        }
        let entry = entries
            .iter()
            .find(|entry| entry.raw == value && entry.class == class)
            .unwrap_or_else(|| {
                panic!("whole private scalar at depth {depth}: {input:?} -> {clean:?}; {entries:?}")
            });
        assert_eq!(
            clean,
            input.replacen(&value, &entry.token, 1),
            "public bytes changed"
        );
        let imported = Session::import(session.export().unwrap()).unwrap();
        assert_eq!(
            pipeline.restore_strict_text(&imported, &clean).unwrap(),
            input
        );
        let public = input
            .replace("customerId", "inventory_reference")
            .replace("employeeId", "inventory_reference")
            .replace("member_id", "inventory_reference");
        let public_session = Session::new(Scope::Ephemeral).unwrap();
        assert_eq!(public_clean(pipeline, &public_session, &public), public);
        assert!(public_session.snapshot_entries().is_empty());
        if depth > 0 {
            let mut parsed: serde_json::Value = serde_json::from_str(&clean).unwrap();
            for level in 1..=depth {
                assert_eq!(parsed["public_version"], "'");
                assert_eq!(parsed["tags"], serde_json::json!(["'", "FRAME"]));
                if level < depth {
                    parsed = serde_json::from_str(parsed["note"].as_str().unwrap()).unwrap();
                }
            }
        }
        input = serde_json::to_string(&serde_json::json!({
            "note": input, "public_version": "'", "tags": ["'", "FRAME"]
        }))
        .unwrap();
        value = serde_json::to_string(&value).unwrap();
        value = value[1..value.len() - 1].to_owned();
    }
}

fn ascii_json(text: &str) -> String {
    let json = serde_json::to_string(text).unwrap();
    let mut encoded = String::new();
    for ch in json.chars() {
        if ch.is_ascii() {
            encoded.push(ch);
        } else {
            for unit in ch.encode_utf16(&mut [0; 2]) {
                use std::fmt::Write;
                write!(&mut encoded, "\\u{unit:04x}").unwrap();
            }
        }
    }
    encoded
}

#[test]
fn ascii_encoded_wrappers_keep_embedded_quotes_and_opaque_values() {
    let pipeline = record_pipeline();
    for (open, close) in [('“', '”'), ('🔐', '🔐'), ('「', '」')] {
        for value in ["ZX-48\"2619", "opaque private value", "ZX-48\n2619"] {
            let note = format!("customerId={open}{value}{close}");
            let input = format!(
                "{{\"note\":{},\"public_version\":\"FRAME\"}}",
                ascii_json(&note)
            );
            let value = ascii_json(value);
            check_quote_dialect_depths(&pipeline, input, value[1..value.len() - 1].to_owned(), 8);
        }
    }
}

#[test]
fn streamed_json_wrappers_keep_complete_private_values_and_escape_tails() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        let value = "ZX-48\"2619\nprivate suffix";
        for (open, close) in [('“', '”'), ('🔐', '🔐'), ('「', '」')] {
            let note = ascii_json(&format!("{cue}={open}{value}{close}"));
            let encoded = ascii_json(value);
            check_quote_dialect_depths(
                &pipeline,
                format!("{{\"note\":{}", &note[..note.len() - 1]),
                encoded[1..encoded.len() - 1].to_owned(),
                8,
            );
            for tail in [
                "\\",
                "\\u",
                "\\u2",
                "\\u20",
                "\\u201",
                "\\ud83d",
                "\\ud83d\\",
                "\\ud83d\\u",
                "\\ud83d\\ud",
                "\\ud83d\\udc",
                "\\ud83d\\udc0",
            ] {
                let note = ascii_json(&format!("{cue}={open}{value}"));
                let raw = format!("{}{tail}", &encoded[1..encoded.len() - 1]);
                check_quote_dialect_depths(
                    &pipeline,
                    format!("{{\"note\":{}{tail}", &note[..note.len() - 1]),
                    raw,
                    8,
                );
            }
        }
    }
}

#[test]
fn escaped_quotes_in_earlier_repeats_cannot_invent_json_enclosures() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        let class = PiiClass::custom(if cue == "customerId" {
            "customer_id"
        } else if cue == "employeeId" {
            "employee_id"
        } else {
            "record_id"
        })
        .unwrap();
        for (open, close) in [
            ('“', '”'),
            ('「', '」'),
            ('🔐', '🔐'),
            ('`', '`'),
            ('\'', '\''),
            ('"', '"'),
        ] {
            for streamed in [false, true] {
                let value = "NX512\"345";
                let value = if open == close {
                    value.replace(open, &open.to_string().repeat(2))
                } else {
                    value.to_owned()
                };
                let note = ascii_json(&format!("{cue}={open}{value}{close}"));
                let mut input = if streamed {
                    format!(
                        "{{\"public_version\":\"FRAME\",\"note\":{}",
                        &note[..note.len() - 1]
                    )
                } else {
                    format!("{{\"public_version\":\"FRAME\",\"note\":{note}}}")
                };
                let encoded = ascii_json(&value);
                let mut raw = encoded[1..encoded.len() - 1].to_owned();
                for depth in 1..=9 {
                    let text = format!("Lookup {raw} again.\n{input}");
                    let session =
                        Session::new(Scope::Conversation("synthetic-repeat-json".into())).unwrap();
                    let clean = public_clean(&pipeline, &session, &text);
                    let entries = session.snapshot_entries();
                    let entry = entries.iter().find(|entry| entry.raw == raw && entry.class == class).unwrap_or_else(|| panic!("whole private scalar at repeat depth{depth}: {text:?} -> {clean:?}; {entries:?}"));
                    assert_eq!(
                        clean,
                        text.replace(&raw, &entry.token),
                        "public bytes changed"
                    );
                    let imported = Session::import(session.export().unwrap()).unwrap();
                    assert_eq!(
                        pipeline.restore_strict_text(&imported, &clean).unwrap(),
                        text
                    );
                    let public = text.replace(cue, "inventory_reference");
                    let public_session = Session::new(Scope::Ephemeral).unwrap();
                    assert_eq!(public_clean(&pipeline, &public_session, &public), public);
                    assert!(public_session.snapshot_entries().is_empty());
                    input = serde_json::to_string(
                        &serde_json::json!({"note":input,"public_version":"FRAME"}),
                    )
                    .unwrap();
                    let encoded = serde_json::to_string(&raw).unwrap();
                    raw = encoded[1..encoded.len() - 1].to_owned();
                }
            }
        }
    }
}

#[test]
fn doubled_quote_dialects_keep_private_suffixes() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        // YAML and SQL use doubled apostrophes; CSV uses doubled double quotes.
        for (quote, connector) in [('\'', ": "), ('\'', "="), ('"', ": ")] {
            let value = format!("Q731284{quote}{quote}É842395");
            check_quote_dialect(
                &pipeline,
                format!("{cue}{connector}{quote}{value}{quote}\npublic_version=FRAME"),
                value,
            );
        }
    }
}

#[test]
fn triple_quoted_dialects_preserve_complete_multiline_values() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        for quote in ['\'', '"'] {
            let delimiter = quote.to_string().repeat(3);
            for newline in ["\n", "\r\n"] {
                for value in [
                    format!("Q731284{newline}É842395"),
                    format!("{newline}Q731284{quote}É842395{quote}{quote}"),
                ] {
                    check_quote_dialect(
                        &pipeline,
                        format!("{cue} = {delimiter}{value}{delimiter}\npublic_version=FRAME"),
                        value,
                    );
                }
            }
        }
    }
}

#[test]
fn all_quote_closes_stop_before_their_enclosing_serialized_string() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        for quote in ["'", "'''", "\"\"\""] {
            let value = "Q731284;É842395";
            let input = serde_json::to_string(&serde_json::json!({
                "note": format!("{cue}: {quote}{value}"),
                "public_version": "'", "tags": ["'", "FRAME"]
            }))
            .unwrap();
            check_quote_dialect(&pipeline, input, value.into());
        }
    }
}

#[test]
fn ambiguous_quote_openings_keep_nonempty_private_ownership() {
    let pipeline = record_pipeline();
    for quote in ['\'', '"'] {
        let value = format!("{quote}Q731284{quote};private_code=É842395");
        let input = serde_json::to_string(&serde_json::json!({
            "note": format!("customerId: {quote}{value}"),
            "public_version": "'", "tags": ["'", "FRAME"]
        }))
        .unwrap();
        let encoded = serde_json::to_string(&value).unwrap();
        check_quote_dialect(&pipeline, input, encoded[1..encoded.len() - 1].into());
    }
}

#[test]
fn prior_quote_dialects_do_not_supply_false_enclosing_strings() {
    let pipeline = record_pipeline();
    for prefix in [
        "format_hint='''literal '\" character'''; ",
        "format_hint=\"\"\"literal '\" character\"\"\"; ",
        "format_hint='literal ''\" character'; ",
    ] {
        let value = "Q731284; \"private_code\": É842395";
        check_quote_dialect(
            &pipeline,
            format!("{prefix}customerId: '{value}'\npublic_version=FRAME"),
            value.into(),
        );
    }
}

#[test]
fn serialized_record_separators_keep_person_label_evidence() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        for separator in ["\n", "\r\n", "\t", "\\u000a", "\\u000D"] {
            let value = "Q731284;É842395";
            check_quote_dialect(
                &pipeline,
                format!("format_hint='FRAME'{separator}{cue}='{value}'\npublic_version=FRAME"),
                value.into(),
            );
        }
    }
}

#[test]
fn classified_empty_quoted_fields_preserve_public_structure() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        for delimiter in ["'", "\"", "'''", "\"\"\""] {
            let mut input = format!("{cue}: {delimiter}{delimiter}\npublic_version=FRAME");
            for _ in 0..=9 {
                let session = Session::new(Scope::Ephemeral).unwrap();
                assert_eq!(public_clean(&pipeline, &session, &input), input);
                assert!(session.snapshot_entries().is_empty());
                input = serde_json::to_string(
                    &serde_json::json!({"note":input,"public_version":"FRAME"}),
                )
                .unwrap();
            }
        }
    }
}

#[test]
fn enclosing_string_close_cannot_open_an_absent_private_value() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        let mut input = serde_json::to_string(&serde_json::json!({
            "note": format!("{cue}: "), "public_version": "'", "tags": ["'", "FRAME"]
        }))
        .unwrap();
        for _ in 0..9 {
            let session = Session::new(Scope::Ephemeral).unwrap();
            assert_eq!(public_clean(&pipeline, &session, &input), input);
            assert!(session.snapshot_entries().is_empty());
            input =
                serde_json::to_string(&serde_json::json!({"note":input,"public_version":"FRAME"}))
                    .unwrap();
        }
    }
}

#[test]
fn opaque_nested_text_cannot_prove_a_private_value_absent() {
    let pipeline = record_pipeline();
    let value = "Q731284;private_code=É842395";
    for cue in ["customerId", "employeeId", "member_id"] {
        let input = serde_json::to_string(&serde_json::json!({
            "note": format!("format_hint: \"marker; {cue}: \"{value}"),
            "public_version": "'", "tags": ["'", "FRAME"]
        }))
        .unwrap();
        check_quote_dialect(&pipeline, input, value.into());
    }
}

#[test]
fn prior_single_quoted_fields_do_not_supply_an_enclosing_string() {
    let pipeline = record_pipeline();
    let value = "Q731284; \"public_version\": 7";
    for prefix in ["note: 'literal \"'; ", "note: 'literal \\' \"'; "] {
        for cue in ["customer ID", "employee ID", "member ID"] {
            let input = format!("{prefix}{cue}: \"{value}");
            let session = Session::new(Scope::Ephemeral).unwrap();
            let clean = public_clean(&pipeline, &session, &input);
            assert!(session
                .snapshot_entries()
                .iter()
                .any(|entry| entry.raw == value));
            assert!(clean.starts_with(prefix));
            assert_eq!(
                pipeline.restore_strict_text(&session, &clean).unwrap(),
                input
            );
        }
    }
}

#[test]
fn prior_unterminated_records_do_not_supply_an_enclosing_string() {
    let pipeline = record_pipeline();
    let value = "Q731284; \"public_version\": 7";
    let input = format!("note: \"unfinished\ncustomer ID: \"{value}");
    let session = Session::new(Scope::Ephemeral).unwrap();
    let clean = public_clean(&pipeline, &session, &input);
    assert!(session
        .snapshot_entries()
        .iter()
        .any(|entry| entry.raw == value));
    assert_eq!(
        pipeline.restore_strict_text(&session, &clean).unwrap(),
        input
    );
}

#[test]
fn unterminated_values_end_at_unescaped_record_line_breaks() {
    let pipeline = record_pipeline();
    for newline in ["\n", "\r\n", "\r"] {
        for public in [
            "public_version=FRAME",
            "public_version: FRAME",
            "\"public_version\": 7",
        ] {
            let value = "Q731284;private_code=É842395";
            let input = format!("customer ID: \"{value}{newline}{public}");
            let session =
                Session::new(Scope::Conversation("synthetic-unterminated-value".into())).unwrap();
            let clean = public_clean(&pipeline, &session, &input);
            assert!(session
                .snapshot_entries()
                .iter()
                .any(|entry| entry.raw == value));
            assert!(
                clean.ends_with(&format!("{newline}{public}")),
                "record tail consumed"
            );
            assert_eq!(
                pipeline.restore_strict_text(&session, &clean).unwrap(),
                input
            );
        }
    }
}

#[test]
fn unterminated_values_preserve_enclosing_serialized_string_closes() {
    let pipeline = record_pipeline();
    for cue in ["customer ID", "employee ID", "member ID"] {
        for tail in [
            ";É842395",
            ";private_code=É842395",
            ";ID: 123",
            ", name=Anna",
            ";\u{a0}private_code=É842395",
            "\r\nprivate_code=É842395",
        ] {
            for inner_json in [false, true] {
                let value = format!("Q731284{tail}");
                let content = if inner_json {
                    let key = match cue {
                        "customer ID" => "customerId",
                        "employee ID" => "employeeId",
                        _ => "member_id",
                    };
                    format!("{{\"{key}\":\"{value}")
                } else {
                    format!("{cue}: \"{value}")
                };
                let mut input = content;
                let mut raw = value;
                for depth in 0..=9 {
                    input = serde_json::to_string(
                        &serde_json::json!({"note": input, "public_version": "FRAME"}),
                    )
                    .unwrap();
                    raw = serde_json::to_string(&raw).unwrap();
                    raw = raw[1..raw.len() - 1].to_owned();
                    let session =
                        Session::new(Scope::Conversation("synthetic-unterminated-value".into()))
                            .unwrap();
                    let clean = public_clean(&pipeline, &session, &input);
                    assert!(
                        session
                            .snapshot_entries()
                            .iter()
                            .any(|entry| entry.raw == raw),
                        "private scalar bytes differ at depth {depth}: {input:?} -> {clean:?}"
                    );
                    let mut parsed: serde_json::Value =
                        serde_json::from_str(&clean).expect("outer JSON stays valid");
                    for _ in 0..depth {
                        assert_eq!(parsed["public_version"], "FRAME");
                        parsed = serde_json::from_str(parsed["note"].as_str().unwrap())
                            .expect("nested JSON stays valid");
                    }
                    assert_eq!(parsed["public_version"], "FRAME");
                    let imported = Session::import(session.export().unwrap()).unwrap();
                    assert_eq!(
                        pipeline.restore_strict_text(&imported, &clean).unwrap(),
                        input
                    );
                }
            }
        }
    }
}

#[test]
fn closed_private_values_preserve_separate_real_public_fields() {
    let pipeline = record_pipeline();
    for delimiter in ["; ", ", ", "\n"] {
        let value = "Q731284;private_code=É842395";
        let public = "public_version=FRAME";
        let input = format!("customer ID: \"{value}\"{delimiter}{public}");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let clean = public_clean(&pipeline, &session, &input);
        assert!(session
            .snapshot_entries()
            .iter()
            .any(|entry| entry.raw == value));
        assert!(clean.ends_with(&format!("\"{delimiter}{public}")));
        assert_eq!(
            pipeline.restore_strict_text(&session, &clean).unwrap(),
            input
        );
    }
}

#[test]
fn cross_class_trim_keeps_repeat_seeds_inside_the_owner() {
    let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
    for cue in ["SECRET VERSION", "FÜHRERSCHEIN"] {
        let input = format!("driver license: Q7312845 4111111111111111 {cue} is EF34 GH5678");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = core
            .pseudonymize_text(&session, &input)
            .expect("valid mixed-class ownership")
        else {
            panic!("text")
        };
        for private in ["Q7312845", "4111111111111111"] {
            assert!(!clean.contains(private), "recognized private value exposed");
        }
        assert_eq!(
            core.pipeline()
                .restore_strict_text(&session, &clean)
                .unwrap(),
            input
        );
    }
}

#[test]
fn wrapper_and_enclosing_dialects_cover_every_private_byte() {
    let pipeline = record_pipeline();
    let wrappers: &[(char, char)] = &[
        ('\u{28}', '\u{29}'),
        ('\u{5b}', '\u{5d}'),
        ('\u{7b}', '\u{7d}'),
        ('\u{ab}', '\u{bb}'),
        ('\u{f3a}', '\u{f3b}'),
        ('\u{f3c}', '\u{f3d}'),
        ('\u{169b}', '\u{169c}'),
        ('\u{2018}', '\u{2019}'),
        ('\u{201a}', '\u{2019}'),
        ('\u{201b}', '\u{2019}'),
        ('\u{201c}', '\u{201d}'),
        ('\u{201e}', '\u{201d}'),
        ('\u{201f}', '\u{201d}'),
        ('\u{2039}', '\u{203a}'),
        ('\u{2045}', '\u{2046}'),
        ('\u{207d}', '\u{207e}'),
        ('\u{208d}', '\u{208e}'),
        ('\u{2308}', '\u{2309}'),
        ('\u{230a}', '\u{230b}'),
        ('\u{2329}', '\u{232a}'),
        ('\u{2768}', '\u{2769}'),
        ('\u{276a}', '\u{276b}'),
        ('\u{276c}', '\u{276d}'),
        ('\u{276e}', '\u{276f}'),
        ('\u{2770}', '\u{2771}'),
        ('\u{2772}', '\u{2773}'),
        ('\u{2774}', '\u{2775}'),
        ('\u{27c5}', '\u{27c6}'),
        ('\u{27e6}', '\u{27e7}'),
        ('\u{27e8}', '\u{27e9}'),
        ('\u{27ea}', '\u{27eb}'),
        ('\u{27ec}', '\u{27ed}'),
        ('\u{27ee}', '\u{27ef}'),
        ('\u{2983}', '\u{2984}'),
        ('\u{2985}', '\u{2986}'),
        ('\u{2987}', '\u{2988}'),
        ('\u{2989}', '\u{298a}'),
        ('\u{298b}', '\u{298c}'),
        ('\u{298d}', '\u{2990}'),
        ('\u{298f}', '\u{298e}'),
        ('\u{2991}', '\u{2992}'),
        ('\u{2993}', '\u{2994}'),
        ('\u{2995}', '\u{2996}'),
        ('\u{2997}', '\u{2998}'),
        ('\u{29d8}', '\u{29d9}'),
        ('\u{29da}', '\u{29db}'),
        ('\u{29fc}', '\u{29fd}'),
        ('\u{2e02}', '\u{2e03}'),
        ('\u{2e04}', '\u{2e05}'),
        ('\u{2e09}', '\u{2e0a}'),
        ('\u{2e0c}', '\u{2e0d}'),
        ('\u{2e1c}', '\u{2e1d}'),
        ('\u{2e20}', '\u{2e21}'),
        ('\u{2e22}', '\u{2e23}'),
        ('\u{2e24}', '\u{2e25}'),
        ('\u{2e26}', '\u{2e27}'),
        ('\u{2e28}', '\u{2e29}'),
        ('\u{2e42}', '\u{201d}'),
        ('\u{3008}', '\u{3009}'),
        ('\u{300a}', '\u{300b}'),
        ('\u{300c}', '\u{300d}'),
        ('\u{300e}', '\u{300f}'),
        ('\u{3010}', '\u{3011}'),
        ('\u{3014}', '\u{3015}'),
        ('\u{3016}', '\u{3017}'),
        ('\u{3018}', '\u{3019}'),
        ('\u{301a}', '\u{301b}'),
        ('\u{301d}', '\u{301e}'),
        ('\u{fd3f}', '\u{fd3e}'),
        ('\u{fe17}', '\u{fe18}'),
        ('\u{fe35}', '\u{fe36}'),
        ('\u{fe37}', '\u{fe38}'),
        ('\u{fe39}', '\u{fe3a}'),
        ('\u{fe3b}', '\u{fe3c}'),
        ('\u{fe3d}', '\u{fe3e}'),
        ('\u{fe3f}', '\u{fe40}'),
        ('\u{fe41}', '\u{fe42}'),
        ('\u{fe43}', '\u{fe44}'),
        ('\u{fe47}', '\u{fe48}'),
        ('\u{fe59}', '\u{fe5a}'),
        ('\u{fe5b}', '\u{fe5c}'),
        ('\u{fe5d}', '\u{fe5e}'),
        ('\u{ff08}', '\u{ff09}'),
        ('\u{ff3b}', '\u{ff3d}'),
        ('\u{ff5b}', '\u{ff5d}'),
        ('\u{ff5f}', '\u{ff60}'),
        ('\u{ff62}', '\u{ff63}'),
        ('\u{22}', '\u{22}'),
        ('\u{27}', '\u{27}'),
        ('\u{60}', '\u{60}'),
        ('\u{3c}', '\u{3e}'),
        ('\u{201e}', '\u{201c}'),
        ('\u{201a}', '\u{2018}'),
        ('\u{301d}', '\u{301f}'),
        ('\u{2e55}', '\u{2e56}'),
        ('\u{2e57}', '\u{2e58}'),
        ('\u{2e59}', '\u{2e5a}'),
        ('\u{2e5b}', '\u{2e5c}'),
        ('§', '§'),
        ('🔐', '🔐'),
        ('⏣', '⏣'),
    ];
    let category = regex::Regex::new(r"\A[\p{Ps}\p{Pi}]\z").unwrap();
    for ch in (0..=0x10ffff).filter_map(char::from_u32) {
        if category.is_match(ch.encode_utf8(&mut [0; 4])) {
            assert!(
                wrappers.iter().any(|(open, _)| *open == ch),
                "missing Unicode wrapper {ch:?}"
            );
        }
    }
    let mut checked = 0;
    for &(open, close) in wrappers {
        eprintln!("wrapper {open:?} -> {close:?}");
        for cue in ["customerId", "employeeId", "member_id"] {
            for shape in ["ZX-48\"2619", "opaque private value", "ZX-48\n2619"] {
                let value = if open == close {
                    shape.replace(open, &open.to_string().repeat(2))
                } else {
                    shape.to_owned()
                };
                let input = format!(
                    "{{\"note\":{},\"public_version\":\"FRAME\"}}",
                    ascii_json(&format!("{cue}={open}{value}{close}"))
                );
                let encoded = ascii_json(&value);
                check_quote_dialect_depths(
                    &pipeline,
                    input,
                    encoded[1..encoded.len() - 1].to_owned(),
                    8,
                );
                checked += 9;
                let note = ascii_json(&format!("{cue}={open}{value}{close}"));
                check_quote_dialect_depths(
                    &pipeline,
                    format!("{{\"note\":{}", &note[..note.len() - 1]),
                    encoded[1..encoded.len() - 1].to_owned(),
                    8,
                );
                checked += 9;
            }
            for shape in ["ZX-482619", "ZX-48\"2619", "ZX-48'2619"] {
                let value = if open == close {
                    shape.replace(open, &open.to_string().repeat(2))
                } else {
                    shape.to_owned()
                };
                let declaration = format!("{cue}={open}{value}{close}");
                // JSON depths 0..9; every public sibling is independently checked.
                check_quote_dialect(
                    &pipeline,
                    format!("{declaration}\npublic_version=FRAME"),
                    value.clone(),
                );
                checked += 10;
                for quoted in [false, true] {
                    let cell = if quoted || declaration.contains([',', '"', '\n', '\r']) {
                        format!("\"{}\"", declaration.replace('"', "\"\""))
                    } else {
                        declaration.clone()
                    };
                    let raw_value = if cell.starts_with('"') {
                        value.replace('"', "\"\"")
                    } else {
                        value.clone()
                    };
                    let input =
                        format!("{cell},{}\r\n", if quoted { "\"FRAME\"" } else { "FRAME" });
                    check_quote_dialect_depths(&pipeline, input, raw_value, 0);
                    checked += 1;
                }
                for input in [
                    (format!("| {declaration} | FRAME |"), value.clone()),
                    (
                        format!("{cue}: {open}{value}{close}\npublic_version: FRAME"),
                        value.clone(),
                    ),
                    (
                        format!("SELECT '{}', 'FRAME';", declaration.replace('\'', "''")),
                        value.replace('\'', "''"),
                    ),
                ] {
                    check_quote_dialect_depths(&pipeline, input.0, input.1, 0);
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, wrappers.len() * 3 * 3 * 33);
    eprintln!("wrapper matrix: {checked} private cases plus {checked} public counterweights");
}

#[test]
fn unpaired_wrappers_own_the_value_to_a_record_boundary() {
    let pipeline = record_pipeline();
    for (open, _) in [('“', '”'), ('「', '」'), ('`', '`'), ('⟨', '⟩')] {
        for cue in ["customerId", "employeeId", "member_id"] {
            check_quote_dialect(
                &pipeline,
                format!("{cue}={open}ZX-482619"),
                "ZX-482619".into(),
            );
        }
    }
}

#[test]
fn complete_csv_and_sql_cells_keep_embedded_quote_suffixes() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        for (input, value) in [
            (
                format!("\"{cue}=\"\"ZX-482619\"\"\",public\r\n"),
                "ZX-482619",
            ),
            (
                format!("\"{cue}='ZX-48\"\"2619'\",public\r\n"),
                "ZX-48\"\"2619",
            ),
            (
                format!("SELECT '{cue}=''ZX-482619''', 'FRAME';"),
                "ZX-482619",
            ),
            (
                format!("SELECT '{cue}=“ZX-48''''2619”', 'FRAME';"),
                "ZX-48''''2619",
            ),
        ] {
            check_quote_dialect(&pipeline, input, value.into());
        }
    }
}

#[test]
fn backtick_and_typographic_wrappers_keep_complete_values() {
    let pipeline = record_pipeline();
    for (open, close) in [('`', '`'), ('“', '”'), ('‘', '’'), ('«', '»')] {
        for value in [
            format!("ZX-48{close}{close}2619"),
            format!("ZX-48{close}2619"),
        ] {
            check_quote_dialect(
                &pipeline,
                format!("customerId={open}{value}{close}\npublic_version=FRAME"),
                value,
            );
        }
        for cue in ["customerId", "employeeId", "member_id"] {
            check_quote_dialect(
                &pipeline,
                format!("{cue}={open}ZX-482619{close}\npublic_version=FRAME"),
                "ZX-482619".into(),
            );
        }
    }
}

#[test]
fn unknown_wrappers_and_escaped_markdown_never_reject_owned_values() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        for (open, close) in [("§", "§"), ("🔐", "🔐"), ("⏣", "⏣"), ("\\`", "\\`")] {
            for value in ["ZX-482619", "PALACE", "ÉCHELON"] {
                check_quote_dialect(
                    &pipeline,
                    format!("{cue}={open}{value}{close}\npublic_version=FRAME"),
                    value.into(),
                );
            }
        }
    }
}

#[test]
fn multiline_enclosing_cells_keep_unpaired_private_suffixes() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        for opener in ["“", "'", "\"", "§", "`"] {
            for value in ["ZX-48\n2619", "\nZX-482619"] {
                let declaration = format!("{cue}={opener}{value}");
                for quoted in [false, true] {
                    let input = format!(
                        "\"{}\",{}\r\n",
                        declaration.replace('"', "\"\""),
                        if quoted { "\"FRAME\"" } else { "FRAME" }
                    );
                    check_quote_dialect(&pipeline, input, value.into());
                }
                let input = format!("SELECT '{}', 'FRAME';", declaration.replace('\'', "''"));
                check_quote_dialect(&pipeline, input, value.into());
                let input = format!("\"{}", declaration.replace('"', "\"\""));
                check_quote_dialect(&pipeline, input, value.into());
            }
        }
    }
}

#[test]
fn multiline_sql_after_prior_records_keeps_entire_private_values_and_repeats() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        let class = PiiClass::custom(if cue == "customerId" {
            "customer_id"
        } else if cue == "employeeId" {
            "employee_id"
        } else {
            "record_id"
        })
        .unwrap();
        for open in ["“", "'", "\"", "§", "`"] {
            for value in ["NX512\n345", "\nNX512345", "NX512\"\n345"] {
                let value = if matches!(open, "'" | "\"") {
                    value.replace(open, &open.repeat(2))
                } else {
                    value.to_owned()
                };
                let raw = value.replace('\'', "''");
                let declaration = format!("metadata: FRAME\n{cue}={open}{value}");
                let sql = format!("SELECT '{}', 'FRAME';", declaration.replace('\'', "''"));
                for record_end in ["\n", "\r\n", "\r"] {
                    let mut input = format!(
                        "public_version='VALUES'{record_end}Lookup {raw} again.{record_end}{sql}"
                    );
                    let mut raw = raw.clone();
                    for depth in 0..=9 {
                        let session =
                            Session::new(Scope::Conversation("synthetic-prior-sql-record".into()))
                                .unwrap();
                        let clean = public_clean(&pipeline, &session, &input);
                        let entries = session.snapshot_entries();
                        let entry = entries.iter().find(|entry| entry.raw == raw && entry.class == class).unwrap_or_else(|| panic!("whole private scalar at SQL depth{depth}: {input:?} -> {clean:?}; {entries:?}"));
                        assert_eq!(
                            clean,
                            input.replace(&raw, &entry.token),
                            "public bytes changed"
                        );
                        let imported = Session::import(session.export().unwrap()).unwrap();
                        assert_eq!(
                            pipeline.restore_strict_text(&imported, &clean).unwrap(),
                            input
                        );
                        let public = input.replace(cue, "inventory_reference");
                        let public_session = Session::new(Scope::Ephemeral).unwrap();
                        assert_eq!(public_clean(&pipeline, &public_session, &public), public);
                        assert!(public_session.snapshot_entries().is_empty());
                        input = serde_json::to_string(
                            &serde_json::json!({"note":input,"public_version":"FRAME"}),
                        )
                        .unwrap();
                        let encoded = serde_json::to_string(&raw).unwrap();
                        raw = encoded[1..encoded.len() - 1].to_owned();
                    }
                }
            }
        }
    }
}

#[test]
fn encoded_triple_scalars_and_empty_cells_preserve_public_syntax() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        for quote in ['\'', '"'] {
            for width in [1, 3] {
                let delimiter = quote.to_string().repeat(width);
                for value in [
                    String::new(),
                    format!("ZX-482619{quote}\nÉ731284{quote}{quote}"),
                ] {
                    if width == 1 && !value.is_empty() {
                        continue;
                    }
                    let declaration = format!("{cue}={delimiter}{value}{delimiter}");
                    for (input, raw) in [
                        (
                            format!("\"{}\",FRAME\r\n", declaration.replace('"', "\"\"")),
                            value.replace('"', "\"\""),
                        ),
                        (
                            format!("\"{}\",\"FRAME\"\r\n", declaration.replace('"', "\"\"")),
                            value.replace('"', "\"\""),
                        ),
                        (
                            format!("SELECT '{}', 'FRAME';", declaration.replace('\'', "''")),
                            value.replace('\'', "''"),
                        ),
                    ] {
                        if value.is_empty() {
                            let mut input = input;
                            for _ in 0..=9 {
                                let session = Session::new(Scope::Ephemeral).unwrap();
                                assert_eq!(public_clean(&pipeline, &session, &input), input);
                                assert!(session.snapshot_entries().is_empty());
                                input = serde_json::to_string(
                                    &serde_json::json!({"note":input,"public_version":"FRAME"}),
                                )
                                .unwrap();
                            }
                        } else {
                            check_quote_dialect(&pipeline, input, raw);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn csv_parents_ignore_literal_backslashes_and_keep_prior_cell_lines() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        for (opener, closer) in [("'", "'"), ("“", "”"), ("`", "`")] {
            let value = "ZX-48\\\"2619";
            let declaration = format!("metadata: FRAME\n{cue}={opener}{value}{closer}");
            let input = format!("\"{}\",FRAME\r\n", declaration.replace('"', "\"\""));
            check_quote_dialect(&pipeline, input, value.replace('"', "\"\""));
            let value = "ZX-48\n2619";
            let declaration = format!("metadata: FRAME\n{cue}={opener}{value}");
            let input = format!("FRAME,\"{}\",FRAME\r\n", declaration.replace('"', "\"\""));
            check_quote_dialect(&pipeline, input, value.into());
        }
    }
}

#[test]
fn json_escapes_cannot_fabricate_a_csv_cell_close() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        let value = "ZX-48\\\",2619";
        let input = serde_json::to_string(&format!("{cue}='{value}'")).unwrap();
        let raw = serde_json::to_string(value).unwrap();
        check_quote_dialect(&pipeline, input, raw[1..raw.len() - 1].into());
        let value = "ZX-482619\"";
        let input = serde_json::to_string(&format!("{cue}='{value}")).unwrap();
        let raw = serde_json::to_string(value).unwrap();
        check_quote_dialect(&pipeline, input, raw[1..raw.len() - 1].into());
    }
}

#[test]
fn verified_cells_with_no_private_value_preserve_public_columns() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        for mut input in [
            format!("\"{cue}=\",FRAME\r\n"),
            format!("SELECT '{cue}=', 'FRAME';"),
        ] {
            for _ in 0..=9 {
                let session = Session::new(Scope::Ephemeral).unwrap();
                assert_eq!(public_clean(&pipeline, &session, &input), input);
                assert!(session.snapshot_entries().is_empty());
                input = serde_json::to_string(
                    &serde_json::json!({"note":input,"public_version":"FRAME"}),
                )
                .unwrap();
            }
        }
    }
}

#[test]
fn sql_literal_dialects_keep_backslash_escaped_private_quotes() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        let value = "ZX-48\\'2619";
        check_quote_dialect(
            &pipeline,
            format!("SELECT '{cue}=“{value}”', 'FRAME';"),
            value.into(),
        );
    }
}

#[test]
fn sql_quoted_identifiers_cannot_fabricate_an_absent_private_value() {
    let pipeline = record_pipeline();
    for cue in ["customerId", "employeeId", "member_id"] {
        for identifier in ["\"quote ' hint\"", "`quote ' hint`", "[quote ' hint]"] {
            let input = format!("SELECT {identifier}, {cue}='ZX-482619' FROM (SELECT 'FRAME' AS {identifier}, 'FRAME' AS {cue});");
            check_quote_dialect(&pipeline, input, "ZX-482619".into());
        }
    }
}

#[test]
fn record_ids_keep_label_suffixes_and_quotes_inside_wrappers() {
    let pipeline = record_pipeline();
    for (label, value) in [
        ("Customer ID number", "2024 10 15"),
        ("Employee ID number", "2024 10 15"),
        ("Member ID number", "2024 10 15"),
        ("customerId", "(ZX-48\"2619)"),
        ("employeeId", "§ZX-48'2619§"),
    ] {
        let input = format!("{label} {value}\npublic_version=FRAME");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let clean = pipeline.clean_text(&session, &input, &[LocaleTag::Global]).unwrap();
        let CleanDocument::Text(clean) = clean else { panic!("text") };
        assert!(!session.snapshot_entries().is_empty(), "{input:?}");
        assert!(clean.ends_with("\npublic_version=FRAME"), "{clean:?}");
        assert_eq!(pipeline.restore_strict_text(&session, &clean).unwrap(), input);
    }
}
