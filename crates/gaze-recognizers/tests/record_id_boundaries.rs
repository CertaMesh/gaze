//! Independent application-owned byte ranges from the v19 boundary corpus.
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
        serde_json::from_str(include_str!("fixtures/record_id_boundaries_v19.json")).unwrap();
    assert_eq!(fixture.generator_version, 19);
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
        // The producer still marks only private bytes as gold. These three
        // ambiguous surfaces intentionally permit conservative public coverage;
        // the byte scorer measures its cost separately from protection/restore.
        let conservative = !case.gold.is_empty()
            && (case
                .surface
                .starts_with("recordids_boundary_serialized_depth9_")
                || case.surface.ends_with("_malformed")
                || case.surface.ends_with("tab_group_ambiguous_column"));
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
fn over_depth_and_inconsistent_quotes_own_the_tail_with_audit_reasons() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("fixtures/record_id_boundaries_v19.json")).unwrap();
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
        let reasons = ScanReasons::default();
        let pipeline = record_pipeline().with_redaction_logger(reasons.clone());
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
                .any(|entry| entry.raw == case.text[gold.start..]),
            "over-depth scalar must own the tail"
        );
        assert!(reasons
            .0
            .lock()
            .unwrap()
            .contains(&serde_json::json!("serialization_depth_exceeded")));
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
                "\nNX731284 É842395",
                format!("{cue}: \"\nNX731284 É842395; public_version=FRAME"),
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
