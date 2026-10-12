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
                snapshot[0], 7,
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
