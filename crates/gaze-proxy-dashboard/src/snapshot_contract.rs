//! Browser/encoder contract tests.
//!
//! The shipped `assets/app.js` reads the live snapshot through `snap.*`, per-event
//! `ev.*`, and per-stage `stage.*` property chains. These tests extract every such
//! chain from the real asset and require each one to resolve in a fixture produced by
//! the production encoder: real `InspectionEventV1` values, the IPC encoder and decoder,
//! the event store, and the snapshot/follow body encoder. No hand-written JSON stands in
//! for the encoder.

use std::collections::BTreeSet;
use std::time::Instant;

use gaze_types::inspection::{CaptureDomainsV1, InspectionStageDomainV1};
use serde_json::Value;

use crate::test_support::{
    admit_all, emit_logical, fixture_store, StageShape, ALL_REASONS, ALL_STAGES,
    OWNER_RAW_SENTINEL, OWNER_RESTORED_SENTINEL, PROVIDER_SENTINEL,
};

const APP_JS: &str = include_str!("../assets/app.js");

/// Strips `//` and `/* */` comments so prose never counts as a read.
fn strip_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"/*") {
            match source[index + 2..].find("*/") {
                Some(end) => index += end + 4,
                None => break,
            }
        } else if bytes[index..].starts_with(b"//") {
            match source[index..].find('\n') {
                Some(end) => index += end,
                None => break,
            }
        } else {
            let ch = source[index..].chars().next().unwrap();
            out.push(ch);
            index += ch.len_utf8();
        }
    }
    out
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

/// Extracts every `<namespace>.a.b.c` property chain. A trailing method call segment is
/// dropped, so `ev.x.map(...)` yields `["x"]`.
pub(crate) fn property_reads(source: &str, namespace: &str) -> BTreeSet<Vec<String>> {
    let code = strip_comments(source);
    let bytes = code.as_bytes();
    let needle = format!("{namespace}.");
    let mut reads = BTreeSet::new();
    let mut search = 0;
    while let Some(found) = code[search..].find(&needle) {
        let start = search + found;
        search = start + needle.len();
        if start > 0 && (is_ident(bytes[start - 1]) || bytes[start - 1] == b'.') {
            continue;
        }
        let mut chain = Vec::new();
        let mut cursor = start + needle.len();
        loop {
            let ident_start = cursor;
            while cursor < bytes.len() && is_ident(bytes[cursor]) {
                cursor += 1;
            }
            if cursor == ident_start {
                break;
            }
            chain.push(code[ident_start..cursor].to_owned());
            if cursor + 1 < bytes.len() && bytes[cursor] == b'.' && is_ident(bytes[cursor + 1]) {
                cursor += 1;
                continue;
            }
            if cursor < bytes.len() && bytes[cursor] == b'(' {
                chain.pop();
            }
            break;
        }
        if !chain.is_empty() {
            reads.insert(chain);
        }
    }
    reads
}

/// Keys of one `const NAME = { Key: "...", ... };` table in app.js.
pub(crate) fn js_table_keys(source: &str, table: &str) -> BTreeSet<String> {
    let start = source
        .find(&format!("const {table} = {{"))
        .unwrap_or_else(|| panic!("app.js must define {table}"));
    let body = &source[start..];
    let end = body.find("};").expect("table closes");
    body[..end]
        .lines()
        .skip(1)
        .filter_map(|line| {
            let line = line.trim();
            let colon = line.find(':')?;
            let key = line[..colon].trim().trim_matches('"');
            (!key.is_empty() && key.bytes().all(is_ident)).then(|| key.to_owned())
        })
        .collect()
}

fn resolves(root: &Value, path: &[String]) -> bool {
    let mut current = root;
    for segment in path {
        match current.get(segment) {
            Some(next) => current = next,
            None => return false,
        }
    }
    true
}

fn events_of(doc: &Value) -> Vec<&Value> {
    doc.get("events")
        .and_then(Value::as_array)
        .map(|events| events.iter().collect())
        .unwrap_or_default()
}

fn stages_of(doc: &Value) -> Vec<&Value> {
    events_of(doc)
        .into_iter()
        .filter_map(|event| event.get("stages").and_then(Value::as_object))
        .flat_map(|stages| stages.values())
        .collect()
}

/// Rich fixture documents produced only by the production encoder path.
fn encoder_documents() -> Vec<Value> {
    let now = Instant::now();
    let mut store = fixture_store();
    use InspectionStageDomainV1::{
        OwnerRequest, OwnerRestoredResponse, ProviderRequest, ProviderResponse,
    };
    use StageShape::{PayloadOmitted, Present, ProjectionOmitted};
    let all_present = ALL_STAGES.map(|stage| (stage, Present));
    admit_all(
        &mut store,
        &emit_logical(CaptureDomainsV1::All, &all_present),
        now,
    );
    admit_all(
        &mut store,
        &emit_logical(
            CaptureDomainsV1::All,
            &[
                (OwnerRequest, PayloadOmitted(ALL_REASONS[5])),
                (ProviderRequest, ProjectionOmitted(ALL_REASONS[4])),
                (ProviderResponse, Present),
                (OwnerRestoredResponse, ProjectionOmitted(ALL_REASONS[9])),
            ],
        ),
        now,
    );
    admit_all(
        &mut store,
        &emit_logical(
            CaptureDomainsV1::ProviderVisible,
            &[(OwnerRequest, Present), (ProviderRequest, Present)],
        ),
        now,
    );
    crate::child::contract_documents(store, now)
}

#[test]
fn every_app_js_snapshot_read_is_emitted_by_the_real_encoder() {
    let documents = encoder_documents();
    let mut missing = Vec::new();
    for (namespace, roots) in [
        ("snap", documents.iter().collect::<Vec<_>>()),
        ("ev", documents.iter().flat_map(events_of).collect()),
        ("stage", documents.iter().flat_map(stages_of).collect()),
    ] {
        let reads = property_reads(APP_JS, namespace);
        assert!(!reads.is_empty(), "app.js must read {namespace}.*");
        for path in reads {
            if !roots.iter().any(|root| resolves(root, &path)) {
                missing.push(format!("{namespace}.{}", path.join(".")));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "app.js reads keys the real encoder never emits: {missing:?}"
    );
}

#[test]
fn property_scanner_handles_chains_methods_and_comments() {
    let source =
        "/* snap.inComment */ snap.a.b; x.snap.no; ev.c.d(); // ev.gone\n(stage.e || {}).f";
    assert_eq!(
        property_reads(source, "snap"),
        BTreeSet::from([vec!["a".to_owned(), "b".to_owned()]])
    );
    assert_eq!(
        property_reads(source, "ev"),
        BTreeSet::from([vec!["c".to_owned()]])
    );
    assert_eq!(
        property_reads(source, "stage"),
        BTreeSet::from([vec!["e".to_owned()]])
    );
}

#[test]
fn browser_omission_table_matches_the_rust_enum_key_for_key() {
    let browser = js_table_keys(APP_JS, "OMISSION_LABELS");
    let rust = ALL_REASONS
        .iter()
        .map(|reason| crate::child::contract_reason_key(*reason).to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(rust.len(), ALL_REASONS.len(), "reason keys are distinct");
    assert_eq!(browser, rust);
}

fn stage_key(stage: InspectionStageDomainV1) -> &'static str {
    match stage {
        InspectionStageDomainV1::OwnerRequest => "ownerRequest",
        InspectionStageDomainV1::ProviderRequest => "providerRequest",
        InspectionStageDomainV1::ProviderResponse => "providerResponse",
        InspectionStageDomainV1::OwnerRestoredResponse => "ownerRestoredResponse",
    }
}

fn is_owner(stage: InspectionStageDomainV1) -> bool {
    matches!(
        stage,
        InspectionStageDomainV1::OwnerRequest | InspectionStageDomainV1::OwnerRestoredResponse
    )
}

fn single_stage_document(
    domains: CaptureDomainsV1,
    stage: InspectionStageDomainV1,
    shape: StageShape,
) -> Value {
    let now = Instant::now();
    let mut store = fixture_store();
    admit_all(&mut store, &emit_logical(domains, &[(stage, shape)]), now);
    crate::child::contract_documents(store, now).remove(0)
}

/// 4 stages x {present, empty present, payload omitted per reason, projection omitted per
/// reason} x owner/provider domain. Owner payloads are Concealed, provider payloads are
/// Available, and an omitted payload carries its exact reason.
#[test]
fn domain_state_matrix_is_derived_from_the_closed_projection() {
    for stage in ALL_STAGES {
        let mut shapes = vec![
            (StageShape::Present, None),
            (StageShape::PresentEmptyPayload, None),
        ];
        for reason in ALL_REASONS {
            shapes.push((StageShape::PayloadOmitted(reason), Some(reason)));
            shapes.push((StageShape::ProjectionOmitted(reason), Some(reason)));
        }
        for (shape, omitted) in shapes {
            let document = single_stage_document(CaptureDomainsV1::All, stage, shape);
            let stage_json = &events_of(&document)[0]["stages"][stage_key(stage)];
            let expected = match omitted {
                None if is_owner(stage) => serde_json::json!({ "kind": "Concealed" }),
                None => serde_json::json!({ "kind": "Available" }),
                Some(reason) => serde_json::json!({
                    "kind": "Omitted",
                    "reason": crate::child::contract_reason_key(reason),
                }),
            };
            assert_eq!(
                stage_json["domainState"], expected,
                "{stage:?} {shape:?} must map exactly"
            );
            if let Some(reason) = omitted {
                assert_eq!(
                    stage_json["measurements"],
                    serde_json::json!({
                        "state": "Omitted",
                        "reason": crate::child::contract_reason_key(reason),
                    }),
                    "{stage:?} {shape:?} omitted components carry the exact reason"
                );
            }
        }
    }
}

/// An owner stage under a provider-only startup descriptor is never presented as captured.
#[test]
fn uncaptured_owner_domains_are_omitted_not_available() {
    for stage in [
        InspectionStageDomainV1::OwnerRequest,
        InspectionStageDomainV1::OwnerRestoredResponse,
    ] {
        let document = single_stage_document(
            CaptureDomainsV1::ProviderVisible,
            stage,
            StageShape::Present,
        );
        assert_eq!(
            events_of(&document)[0]["stages"][stage_key(stage)]["domainState"],
            serde_json::json!({ "kind": "Omitted", "reason": "NotCapturedByPolicy" })
        );
    }
}

#[test]
fn metadata_streams_never_carry_payload_sentinels() {
    let documents = encoder_documents();
    assert!(
        documents.len() >= 2,
        "snapshot and follow bodies are both checked"
    );
    for document in &documents {
        let text = serde_json::to_string(document).unwrap();
        for sentinel in [
            OWNER_RAW_SENTINEL,
            PROVIDER_SENTINEL,
            OWNER_RESTORED_SENTINEL,
        ] {
            for fragment in sentinel.split(' ') {
                assert!(
                    !text.contains(fragment),
                    "metadata stream carried payload fragment {fragment:?}"
                );
            }
        }
    }
}
