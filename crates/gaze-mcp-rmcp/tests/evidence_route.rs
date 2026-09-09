fn controlled_pipeline() -> gaze::Pipeline {
    const PACK: &str = r#"schema_version = "0.1.0"
rulepack_id = "evidence-controlled"
rulepack_version = "1.0.0"
default_locales = ["global"]
[[recognizers]]
id = "email.controlled.v1"
safety_tier = "safe_default"
class = "Email"
enabled = true
locales = ["global"]
locale_basis = "format"
[recognizers.match]
kind = "regex"
pattern = '([a-z]+@example\.invalid)'
capture_groups = [1]
"#;
    let pack = must(gaze::Rulepack::load(gaze::RulepackSource::Embedded(PACK)));
    assert!(
        pack.recognizers.len() == 1 && pack.recognizers[0].class == gaze::PiiClass::Email,
        "controlled-class"
    );
    let mut policy = gaze::Policy::default();
    policy.rules = vec![gaze::RuleSpec::Class {
        class: gaze::PiiClass::Email,
        action: gaze::Action::Tokenize,
    }];
    let context = must(gaze::Context::from_json_str(
        r#"{"dictionaries":{},"class_map":{},"fields":{}}"#,
    ));
    let locales = gaze::LocaleChain::merge_cli_policy_rulepack_default(
        None,
        None,
        Some(&[gaze::LocaleTag::Global]),
    );
    must(gaze_assembly::build_pipeline(
        &policy,
        &context,
        &[pack],
        &locales,
        None,
    ))
}

use async_trait::async_trait;
use gaze::{PiiClass, Scope, Session};
use gaze_mcp_core::*;
use gaze_mcp_rmcp::{FixedPrincipalResolver, RmcpFrontend};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
const MIN_FRAGMENT_BYTES: usize = 8;
const EMAIL: &str = "alice@example.invalid";
const FRESH: &str = "bob@example.invalid";
const PHONE: &str = "+49 1555 0112233";
const GOLDEN: &str =
    include_str!("../../../scripts/bench/fixtures/evidence/mcp_route_v1.golden.json");

// Static errors prevent private Debug payloads from appearing in failed tests.
fn must<T, E>(r: Result<T, E>) -> T {
    match r {
        Ok(v) => v,
        Err(_) => panic!("fixture-operation-failed"),
    }
}
fn text(v: &Value) -> &str {
    v.as_str().expect("string-carrier")
}

#[derive(Default)]
struct Store {
    events: Mutex<Vec<&'static str>>,
    fail_finish: bool,
}
#[async_trait]
impl ManifestStore for Store {
    async fn begin_call(&self, c: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
        self.events.lock().unwrap().push("begin");
        Ok(CallHandle::new(c.call_id))
    }
    async fn finish_call(&self, _: CallHandle, _: SnapshotRef) -> Result<(), ManifestError> {
        self.events.lock().unwrap().push("finish");
        if self.fail_finish {
            Err(ManifestError::Backend(Box::new(std::io::Error::other(
                "synthetic",
            ))))
        } else {
            Ok(())
        }
    }
    async fn fail_call(&self, _: CallHandle, _: FailureReason) -> Result<(), ManifestError> {
        self.events.lock().unwrap().push("fail");
        Ok(())
    }
}
struct Auth;
#[async_trait]
impl AuthHook for Auth {
    async fn authorize_agent(&self, _: &Principal, _: &str) -> Result<(), AuthError> {
        Ok(())
    }
    async fn authorize_operator(&self, _: &Principal, _: &str) -> Result<(), AuthError> {
        Err(AuthError::Denied("synthetic".into()))
    }
}
#[derive(Clone)]
struct Observer {
    spans: Arc<Mutex<Vec<usize>>>,
    mode: u8,
    session: Arc<Session>,
}
impl gaze::SafetyNet for Observer {
    fn id(&self) -> &str {
        "evidence.observer.v1"
    }
    fn supported_locales(&self) -> &[gaze::LocaleTag] {
        &[gaze::LocaleTag::Global]
    }
    fn check(
        &self,
        clean: &str,
        ctx: gaze::SafetyNetContext<'_>,
    ) -> Result<Vec<gaze::LeakSuspect>, gaze::SafetyNetError> {
        self.spans.lock().unwrap().push(ctx.manifest.spans.len());
        assert!(ctx.field_path.is_none(), "ordinal-only-observation");
        if self.mode == 3 && !ctx.manifest.spans.is_empty() {
            must(
                self.session
                    .tokenize(&PiiClass::Name, "Synthetic concurrent fixture"),
            );
        }
        if self.mode == 1 || self.mode == 2 {
            let span = if self.mode == 1 {
                0..clean.len()
            } else {
                ctx.manifest
                    .spans
                    .first()
                    .map(|s| s.clean_span.clone())
                    .unwrap_or(0..0)
            };
            if !span.is_empty() {
                return Ok(vec![gaze::LeakSuspect::new(
                    span,
                    PiiClass::Name,
                    "evidence.observer.v1",
                    None,
                    gaze::LeakKind::Uncovered,
                    "fixture",
                    None,
                )]);
            }
        }
        Ok(vec![])
    }
}
#[derive(Clone, Default)]
struct Logger(Arc<AtomicUsize>);
impl gaze::RedactionLogger for Logger {
    fn log(&self, _: &gaze::RedactionEntry) -> Result<(), gaze::RedactionLogError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
enum Output {
    Echo,
    Fixed(Value),
    Wait,
    Error,
}
struct Producer {
    descriptor: ToolDescriptor,
    output: Output,
    invokes: Arc<AtomicUsize>,
    argument: Arc<Mutex<Value>>,
}
#[async_trait]
impl Tool for Producer {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }
    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        self.invokes.fetch_add(1, Ordering::SeqCst);
        *self.argument.lock().unwrap() = ctx.redacted_args().clone();
        match &self.output {
            Output::Echo => Ok(ToolResponse::json(
                json!({"result":ctx.redacted_args()["text"]}),
            )),
            Output::Fixed(v) => Ok(ToolResponse::json(v.clone())),
            Output::Wait => {
                std::future::pending::<()>().await;
                unreachable!()
            }
            Output::Error => Err(ToolError::BackendFailure(EMAIL.into())),
        }
    }
}
struct Host {
    registry: ToolRegistry,
    pipeline: gaze::Pipeline,
    session: Arc<Session>,
    store: Store,
    spans: Arc<Mutex<Vec<usize>>>,
    logger: Logger,
    invokes: Arc<AtomicUsize>,
    argument: Arc<Mutex<Value>>,
}
#[async_trait]
impl DispatchHost for Host {
    async fn dispatch(
        &self,
        p: &Principal,
        n: &str,
        a: Value,
        s: Option<&str>,
    ) -> Result<ToolResponse, DispatchError> {
        PiiEnvelope::new(
            &self.registry,
            &Auth,
            &self.store,
            &self.pipeline,
            &self.session,
            &[gaze::LocaleTag::Global],
            &SessionIdPolicy::default_strict(),
        )
        .dispatch(p, n, a, s)
        .await
    }
    fn list_tools(&self) -> Vec<ToolDescriptor> {
        self.registry.list().into_iter().cloned().collect()
    }
}
fn host(
    controlled: bool,
    output: Output,
    response_fields: &[&str],
    mode: u8,
    fail_finish: bool,
    session: Arc<Session>,
) -> Arc<Host> {
    let spans = Arc::new(Mutex::new(Vec::new()));
    let logger = Logger::default();
    let invokes = Arc::new(AtomicUsize::new(0));
    let pipeline = if controlled {
        controlled_pipeline()
    } else {
        must(gaze_assembly::CorePipelineConfig::new().build()).into_pipeline()
    };
    let pipeline = pipeline
        .with_safety_net(Observer {
            spans: spans.clone(),
            mode,
            session: session.clone(),
        })
        .with_redaction_logger(logger.clone());
    let argument = Arc::new(Mutex::new(Value::Null));
    let mut registry = ToolRegistry::new();
    must(
        registry.register(Producer {
            descriptor: ToolDescriptor::agent("test", json!({"type":"object"})).with_carriers(
                CarrierDeclaration::text_fields(&["text"]),
                CarrierDeclaration::new(
                    response_fields
                        .iter()
                        .map(|path| {
                            path.split('.')
                                .map(|s| CarrierSegment::Member(s.into()))
                                .collect()
                        })
                        .collect(),
                    vec![],
                ),
            ),
            output,
            invokes: invokes.clone(),
            argument: argument.clone(),
        }),
    );
    Arc::new(Host {
        registry,
        pipeline,
        session,
        store: Store {
            fail_finish,
            ..Default::default()
        },
        spans,
        logger,
        invokes,
        argument,
    })
}
fn string_leaves(value: &Value) -> usize {
    match value {
        Value::String(_) => 1,
        Value::Array(items) => items.iter().map(string_leaves).sum(),
        Value::Object(items) => items.values().map(string_leaves).sum(),
        _ => 0,
    }
}
fn fresh_session() -> Arc<Session> {
    Arc::new(must(Session::new(Scope::Ephemeral)))
}
async fn call(h: Arc<Host>, args: Value, timeout: bool) -> Option<CallToolResult> {
    let handler = RmcpFrontend::stdio(Arc::new(FixedPrincipalResolver::agent("synthetic")))
        .into_server_handler(h);
    let (client_stream, server_stream) = tokio::io::duplex(16384);
    let server = tokio::spawn(async move {
        if let Ok(s) = rmcp::serve_server(handler, server_stream).await {
            let _ = s.waiting().await;
        }
    });
    let client = must(().serve(client_stream).await);
    let request = CallToolRequestParams::new("test")
        .with_arguments(args.as_object().expect("argument-object").clone());
    let response = if timeout {
        tokio::time::timeout(Duration::from_millis(30), client.call_tool(request))
            .await
            .ok()
            .and_then(Result::ok)
    } else {
        Some(must(client.call_tool(request).await))
    };
    let _ = client.cancel().await;
    server.abort();
    let _ = server.await;
    response
}
fn no_payload_surfaces(r: &CallToolResult) -> bool {
    let Some(t) = r.content.first().and_then(|c| c.raw.as_text()) else {
        return false;
    };
    // Equality closes every serialized data-bearing surface, including future fields.
    must(serde_json::to_value(r))
        == json!({"isError":true,"content":[{"type":"text","text":t.text}]})
}
fn outcome(r: Option<&CallToolResult>) -> &'static str {
    match r {
        None => "UNKNOWN_EGRESS",
        Some(r) if r.is_error != Some(true) => "COMPLETED",
        Some(r)
            if no_payload_surfaces(r)
                && r.content.len() == 1
                && r.content[0]
                    .raw
                    .as_text()
                    .is_some_and(|t| CONTROL_REFUSAL_CODES.contains(&t.text.as_str())) =>
        {
            "FAILED_CLOSED_NO_EGRESS"
        }
        Some(_) => "UNKNOWN_EGRESS",
    }
}
fn decode(r: &CallToolResult) -> Value {
    assert!(
        outcome(Some(r)) == "COMPLETED" && r.content.len() == 1,
        "completed-single-carrier"
    );
    must(serde_json::from_str(
        &r.content[0].raw.as_text().expect("text-frame").text,
    ))
}
#[derive(PartialEq, Eq)]
enum Verdict {
    Full,
    Partial(usize),
    Protected,
    Unknown,
}
fn prefix(s: &str, n: usize) -> &str {
    let mut n = n.min(s.len());
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    &s[..n]
}
fn occurrence(session: &Session, observed: &str, expected: &str, anchor: bool) -> Verdict {
    let window = if anchor {
        if observed.matches("[[g]]").count() != 1 || observed.matches("[[/g]]").count() != 1 {
            return Verdict::Unknown;
        }
        let a = observed.find("[[g]]").unwrap() + 5;
        let b = observed.find("[[/g]]").unwrap();
        if a > b {
            return Verdict::Unknown;
        }
        &observed[a..b]
    } else {
        observed
    };
    let restored = session.restore_strict_text_with_provenance(window);
    if restored
        .as_ref()
        .is_ok_and(|r| r.authorized_output_ranges.len() > 1)
    {
        return Verdict::Unknown;
    }
    if window.contains(expected) {
        return Verdict::Full;
    }
    // Only authored contiguous prefixes are candidates; never infer a trace by diff.
    for n in (MIN_FRAGMENT_BYTES..expected.len()).rev() {
        let fragment = prefix(expected, n);
        if fragment.len() >= MIN_FRAGMENT_BYTES && window.contains(fragment) {
            return Verdict::Partial(fragment.len());
        }
    }
    if let Ok(r) = restored
        && r.authorized_output_ranges.len() == 1
        && r.text == expected
        && r.authorized_output_ranges[0] == (0..expected.len())
    {
        return Verdict::Protected;
    }
    Verdict::Unknown
}
#[derive(Default)]
struct Counts(BTreeMap<&'static str, u64>, BTreeMap<&'static str, usize>);
impl Counts {
    fn add(&mut self, key: &'static str, n: usize) {
        *self.0.entry(key).or_default() += n as u64;
    }
    fn get(&self, key: &str) -> u64 {
        self.0.get(key).copied().unwrap_or(0)
    }
    fn score(
        &mut self,
        session: &Session,
        observed: &str,
        expected: &str,
        anchor: bool,
    ) -> Verdict {
        *self.1.entry("gold").or_default() += 1;
        for m in [
            "gold_occurrences_surviving_egress",
            "gold_occurrences_partially_surviving_egress",
            "gold_occurrences_attribution_not_measured",
            "gold_bytes_surviving_egress",
        ] {
            self.add(m, 0);
        }
        self.add("gold_occurrences_planned", 1);
        self.add("gold_bytes_planned", expected.len());
        let v = occurrence(session, observed, expected, anchor);
        match v {
            Verdict::Full => {
                self.add("gold_occurrences_surviving_egress", 1);
                self.add("gold_bytes_surviving_egress", expected.len());
            }
            Verdict::Partial(n) => {
                self.add("gold_occurrences_partially_surviving_egress", 1);
                self.add("gold_bytes_surviving_egress", n);
            }
            Verdict::Unknown => self.add("gold_occurrences_attribution_not_measured", 1),
            Verdict::Protected => {}
        }
        v
    }
    fn restore(
        &mut self,
        session: &Session,
        observed: &str,
        expected: &str,
        expected_occurrence: &str,
    ) {
        *self.1.entry("restore").or_default() += 1;
        for m in [
            "leaf_restore_exact",
            "leaf_restore_decision_failures",
            "egress_token_restore_failures",
        ] {
            self.add(m, 0);
        }
        match session.restore_strict_text_with_provenance(observed) {
            Err(_) => {
                self.add("egress_token_restore_failures", 1);
                self.add("leaf_restore_decision_failures", 1);
            }
            Ok(r) => {
                if r.text == expected {
                    self.add("leaf_restore_exact", 1);
                }
                if r.authorized_output_ranges.len() == 1 {
                    self.add("egress_raw_value_mismatches", 0);
                    if &r.text[r.authorized_output_ranges[0].clone()] != expected_occurrence {
                        self.add("egress_raw_value_mismatches", 1);
                    }
                }
            }
        }
    }
    fn negative(&mut self, session: &Session, observed: &str, expected: &str) {
        *self.1.entry("negative").or_default() += 1;
        self.add("false_positive_occurrences", 0);
        self.add("false_positive_bytes", 0);
        if occurrence(session, observed, expected, false) == Verdict::Protected {
            self.add("false_positive_occurrences", 1);
            self.add("false_positive_bytes", expected.len());
        }
    }
    fn observe_host(&mut self, h: &Host, planned_leaves: usize) {
        let spans = h.spans.lock().unwrap();
        assert!(spans.len() <= planned_leaves, "observer-ordinal-overflow");
        self.add("observer_leaves_observed", spans.len());
        self.add("observer_leaves_unobserved", planned_leaves - spans.len());
        self.add("observer_manifest_spans_observed", spans.iter().sum());
        self.add(
            "observer_recognizer_source_events",
            h.logger.0.load(Ordering::SeqCst),
        );
        self.add("tool_invocations", h.invokes.load(Ordering::SeqCst));
        self.add(
            "manifest_terminal_events",
            h.store
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| **e == "finish" || **e == "fail")
                .count(),
        );
    }
}

// Independently compiled mirrors, reconciled against the committed artifact.
const ARM_IDS: &[&str] = &["base", "candidate"];
const ATTESTATION_SOURCES: &[&str] = &["live_repository", "test_seam"];
const BASIS_IDS: &[&str] = &["full_cell", "paired_completed"];
const BLOCKED_GATES: &[&str] = &[
    "class_commitment_completeness",
    "manifest_integrity_six_counter_schema_v4",
    "per_token_protection_trace",
    "producer_membership_order_proof",
    "unknown_egress_route_fragment_observation",
];
const CELL_IDS: &[&str] = &[
    "synthetic.evaluator.v1",
    "synthetic.mcp.controlled.v1",
    "synthetic.mcp.core.v1",
];
const CLAIM_SCOPES: &[&str] = &["synthetic_harness_capability_only"];
const CONTROL_REFUSAL_CODES: &[&str] = &[
    "auth-denied",
    "backend-failure",
    "backend-unavailable",
    "internal",
    "invalid-args",
    "invalid-session-id",
    "limit-exceeded",
    "manifest-persistence-failed",
    "not-found",
    "redaction-failed",
    "response-serialization-failed",
];
const COUNTING_GRADES: &[&str] = &[
    "egress_reconstructed",
    "observed_subset_lower_bound",
    "observer_native",
    "planned_inventory",
    "private_authored_records",
    "route_native",
];
const DECLARATION_FIELDS: &[&str] = &[
    "acceptance_limit",
    "confidence_level",
    "coverage_target",
    "multiplicity_treatment",
    "resample_count",
    "seed",
    "strata",
    "weighting",
];
const DERIVATIONS: &[&str] = &[
    "egress_reconstructed",
    "invariant_enforced_not_counted",
    "not_applicable_by_construction",
    "not_measured",
    "observed_subset_lower_bound",
    "observer_native",
    "planned_inventory",
    "private_authored_records",
    "route_native",
];
const ERROR_CODES: &[&str] = &[
    "auth-denied",
    "backend-failure",
    "backend-unavailable",
    "internal",
    "invalid-args",
    "invalid-session-id",
    "limit-exceeded",
    "manifest-persistence-failed",
    "not-found",
    "redaction-failed",
    "response-serialization-failed",
];
const FEATURE_GRAPH_IDS: &[&str] = &["workspace.all_features", "workspace.default"];
const GATE_IDS: &[&str] = &[
    "build_attestation_clean_source",
    "claim_scope_present",
    "class_commitment_completeness",
    "class_commitment_schema_load",
    "cross_language_vocabulary_equality",
    "declaration_gating",
    "egress_integrity_analogues",
    "false_positive_negative_control",
    "gold_survival_oracle",
    "local_membership_order_proof",
    "manifest_integrity_six_counter_schema_v4",
    "no_payload_positive_observation",
    "outcome_identities",
    "paired_grouped_interval_arithmetic",
    "per_token_protection_trace",
    "planned_inventory_reconciliation",
    "producer_membership_order_proof",
    "receipt_path_allowlist",
    "rejection_is_not_protection",
    "source_attribution_events",
    "stamped_field_separation",
    "string_byte_reversibility",
    "unknown_egress_lower_bound",
    "unknown_egress_route_fragment_observation",
    "vocabulary_closure",
];
const GATE_RESULTS: &[&str] = &["BLOCKED", "FAIL", "NOT_EVALUABLE", "PASS"];
const LEAK_FAMILY_METRIC_IDS: &[&str] = &[
    "gold_bytes_surviving_egress",
    "gold_occurrences_attribution_not_measured",
    "gold_occurrences_partially_surviving_egress",
    "gold_occurrences_surviving_egress",
];
const METHOD_IDS: &[&str] = &["grouped_paired_percentile_v1"];
const METRIC_IDS: &[&str] = &[
    "egress_authorized_range_bounds_invalid",
    "egress_authorized_range_non_monotonic",
    "egress_clean_bounds_invalid",
    "egress_overlapping_clean_spans",
    "egress_raw_value_mismatches",
    "egress_token_restore_failures",
    "false_positive_bytes",
    "false_positive_occurrences",
    "gold_bytes_planned",
    "gold_bytes_surviving_egress",
    "gold_occurrences_attribution_not_measured",
    "gold_occurrences_partially_surviving_egress",
    "gold_occurrences_planned",
    "gold_occurrences_surviving_egress",
    "leaf_restore_decision_failures",
    "leaf_restore_exact",
    "manifest_raw_entry_agreement_enforced",
    "manifest_span_monotonicity_enforced",
    "manifest_terminal_events",
    "observer_leaves_observed",
    "observer_leaves_unobserved",
    "observer_manifest_spans_observed",
    "observer_recognizer_source_events",
    "protected_leaves",
    "protection_trace_items",
    "tool_invocations",
    "unknown_egress_lower_bound_cases",
];
const MULTIPLICITY_IDS: &[&str] = &["synthetic_none"];
const OUTCOME_STATES: &[&str] = &[
    "COMPLETED",
    "ERROR_PROTOCOL",
    "FAILED_CLOSED_NO_EGRESS",
    "NOT_STARTED",
    "UNKNOWN_EGRESS",
];
const POLICY_IDENTITIES: &[&str] = &[
    "authored.records.v1",
    "controlled.email_only.v1",
    "core.rule_floor.v1",
];
const RECEIPT_PATHS: &[&str] = &[
    "$",
    "$.analysis_declaration",
    "$.analysis_declaration.acceptance_limit",
    "$.analysis_declaration.confidence_level",
    "$.analysis_declaration.coverage_target",
    "$.analysis_declaration.multiplicity_treatment",
    "$.analysis_declaration.resample_count",
    "$.analysis_declaration.seed",
    "$.analysis_declaration.strata",
    "$.analysis_declaration.strata.[]",
    "$.analysis_declaration.weighting",
    "$.arm_id",
    "$.asymmetric_outcome_table",
    "$.asymmetric_outcome_table.*",
    "$.asymmetric_outcome_table.*.*",
    "$.cell_id",
    "$.claim_scope",
    "$.class_commitment_table",
    "$.class_commitment_table.id",
    "$.class_commitment_table.version",
    "$.counts",
    "$.counts.*",
    "$.derivations",
    "$.derivations.*",
    "$.error_codes",
    "$.error_codes.*",
    "$.gate_results",
    "$.gate_results.*",
    "$.intervals",
    "$.intervals.*",
    "$.intervals.*.basis",
    "$.intervals.*.conditional",
    "$.intervals.*.high",
    "$.intervals.*.low",
    "$.intervals.*.method_id",
    "$.intervals.*.point",
    "$.not_measured",
    "$.not_measured.blocked_gates",
    "$.not_measured.blocked_gates.[]",
    "$.not_measured.metrics",
    "$.not_measured.metrics.[]",
    "$.outcomes",
    "$.outcomes.*",
    "$.planned_case_count",
    "$.policy_identity",
    "$.population_handle",
    "$.protocol_id",
    "$.protocol_version",
    "$.route_id",
    "$.route_status",
    "$.route_status.*",
];
const REFUSAL_CODES: &[&str] = &[
    "attestation_shape_invalid",
    "class_commitment_invalid",
    "conditional_reported_as_full_cell",
    "counted_non_measurement",
    "declaration_invalid",
    "derivation_conflict",
    "derivation_coverage_incomplete",
    "duplicate_json_key",
    "gate_coverage_incomplete",
    "handle_shape_invalid",
    "input_limit",
    "interval_without_declaration",
    "inventory_conflict",
    "lower_bound_reported_as_exact",
    "malformed_json",
    "membership_order_proof_failed",
    "missing_mandatory_key",
    "non_finite_number",
    "observer_coverage_incomplete",
    "outcome_identity_violation",
    "protocol_identity_mismatch",
    "stamped_key_in_emitted_receipt",
    "uncounted_measurable_metric",
    "unknown_path",
    "value_out_of_vocabulary",
    "wrong_type",
];
const ROUTE_IDS: &[&str] = &[
    "daemon.jsonl.v1",
    "evaluator.private.v1",
    "mcp.rmcp.duplex.v1",
    "ocr.document.v1",
    "proxy.http.v1",
    "session.episode.v1",
    "stream.v1",
    "structured.core.v1",
    "text.clean_for_bench.v1",
];
const ROUTE_STATUSES: &[&str] = &["IMPLEMENTED", "NOT_IMPLEMENTED"];
const STAMPED_KEYS: &[&str] = &[
    "build_attestation",
    "local_membership_order_proof_method",
    "local_membership_order_verified",
];
const STRATUM_IDS: &[&str] = &["synthetic_de", "synthetic_en"];
const TABLE_IDS: &[&str] = &["class-commitments-v1"];
const TOOLCHAIN_IDS: &[&str] = &["rust.workspace_pinned"];
const WEIGHTING_IDS: &[&str] = &["inventory_group"];
fn mirrored_vocabularies() -> BTreeMap<&'static str, &'static [&'static str]> {
    BTreeMap::from([
        ("ARM_IDS", ARM_IDS),
        ("ATTESTATION_SOURCES", ATTESTATION_SOURCES),
        ("BASIS_IDS", BASIS_IDS),
        ("BLOCKED_GATES", BLOCKED_GATES),
        ("CELL_IDS", CELL_IDS),
        ("CLAIM_SCOPES", CLAIM_SCOPES),
        ("CONTROL_REFUSAL_CODES", CONTROL_REFUSAL_CODES),
        ("COUNTING_GRADES", COUNTING_GRADES),
        ("DECLARATION_FIELDS", DECLARATION_FIELDS),
        ("DERIVATIONS", DERIVATIONS),
        ("ERROR_CODES", ERROR_CODES),
        ("FEATURE_GRAPH_IDS", FEATURE_GRAPH_IDS),
        ("GATE_IDS", GATE_IDS),
        ("GATE_RESULTS", GATE_RESULTS),
        ("LEAK_FAMILY_METRIC_IDS", LEAK_FAMILY_METRIC_IDS),
        ("METHOD_IDS", METHOD_IDS),
        ("METRIC_IDS", METRIC_IDS),
        ("MULTIPLICITY_IDS", MULTIPLICITY_IDS),
        ("OUTCOME_STATES", OUTCOME_STATES),
        ("POLICY_IDENTITIES", POLICY_IDENTITIES),
        ("RECEIPT_PATHS", RECEIPT_PATHS),
        ("REFUSAL_CODES", REFUSAL_CODES),
        ("ROUTE_IDS", ROUTE_IDS),
        ("ROUTE_STATUSES", ROUTE_STATUSES),
        ("STAMPED_KEYS", STAMPED_KEYS),
        ("STRATUM_IDS", STRATUM_IDS),
        ("TABLE_IDS", TABLE_IDS),
        ("TOOLCHAIN_IDS", TOOLCHAIN_IDS),
        ("WEIGHTING_IDS", WEIGHTING_IDS),
    ])
}
const PATH_RULES: &str = r#"{"$":["object",null],"$.analysis_declaration":["nullable_object",["acceptance_limit","confidence_level","coverage_target","multiplicity_treatment","resample_count","seed","strata","weighting"]],"$.analysis_declaration.acceptance_limit":["number",null],"$.analysis_declaration.confidence_level":["number",null],"$.analysis_declaration.coverage_target":["number",null],"$.analysis_declaration.multiplicity_treatment":["enum",["synthetic_none"]],"$.analysis_declaration.resample_count":["positive",null],"$.analysis_declaration.seed":["int",null],"$.analysis_declaration.strata":["array",null],"$.analysis_declaration.strata.[]":["enum",["synthetic_de","synthetic_en"]],"$.analysis_declaration.weighting":["enum",["inventory_group"]],"$.arm_id":["enum",["base","candidate"]],"$.asymmetric_outcome_table":["object",["COMPLETED","ERROR_PROTOCOL","FAILED_CLOSED_NO_EGRESS","NOT_STARTED","UNKNOWN_EGRESS"]],"$.asymmetric_outcome_table.*":["object",["COMPLETED","ERROR_PROTOCOL","FAILED_CLOSED_NO_EGRESS","NOT_STARTED","UNKNOWN_EGRESS"]],"$.asymmetric_outcome_table.*.*":["int",null],"$.cell_id":["enum",["synthetic.evaluator.v1","synthetic.mcp.controlled.v1","synthetic.mcp.core.v1"]],"$.claim_scope":["enum",["synthetic_harness_capability_only"]],"$.class_commitment_table":["object",["id","version"]],"$.class_commitment_table.id":["enum",["class-commitments-v1"]],"$.class_commitment_table.version":["positive",null],"$.counts":["object",["egress_authorized_range_bounds_invalid","egress_authorized_range_non_monotonic","egress_clean_bounds_invalid","egress_overlapping_clean_spans","egress_raw_value_mismatches","egress_token_restore_failures","false_positive_bytes","false_positive_occurrences","gold_bytes_planned","gold_bytes_surviving_egress","gold_occurrences_attribution_not_measured","gold_occurrences_partially_surviving_egress","gold_occurrences_planned","gold_occurrences_surviving_egress","leaf_restore_decision_failures","leaf_restore_exact","manifest_raw_entry_agreement_enforced","manifest_span_monotonicity_enforced","manifest_terminal_events","observer_leaves_observed","observer_leaves_unobserved","observer_manifest_spans_observed","observer_recognizer_source_events","protected_leaves","protection_trace_items","tool_invocations","unknown_egress_lower_bound_cases"]],"$.counts.*":["int",null],"$.derivations":["object",["egress_authorized_range_bounds_invalid","egress_authorized_range_non_monotonic","egress_clean_bounds_invalid","egress_overlapping_clean_spans","egress_raw_value_mismatches","egress_token_restore_failures","false_positive_bytes","false_positive_occurrences","gold_bytes_planned","gold_bytes_surviving_egress","gold_occurrences_attribution_not_measured","gold_occurrences_partially_surviving_egress","gold_occurrences_planned","gold_occurrences_surviving_egress","leaf_restore_decision_failures","leaf_restore_exact","manifest_raw_entry_agreement_enforced","manifest_span_monotonicity_enforced","manifest_terminal_events","observer_leaves_observed","observer_leaves_unobserved","observer_manifest_spans_observed","observer_recognizer_source_events","protected_leaves","protection_trace_items","tool_invocations","unknown_egress_lower_bound_cases"]],"$.derivations.*":["enum",["egress_reconstructed","invariant_enforced_not_counted","not_applicable_by_construction","not_measured","observed_subset_lower_bound","observer_native","planned_inventory","private_authored_records","route_native"]],"$.error_codes":["object",["auth-denied","backend-failure","backend-unavailable","internal","invalid-args","invalid-session-id","limit-exceeded","manifest-persistence-failed","not-found","redaction-failed","response-serialization-failed"]],"$.error_codes.*":["int",null],"$.gate_results":["object",["build_attestation_clean_source","claim_scope_present","class_commitment_completeness","class_commitment_schema_load","cross_language_vocabulary_equality","declaration_gating","egress_integrity_analogues","false_positive_negative_control","gold_survival_oracle","local_membership_order_proof","manifest_integrity_six_counter_schema_v4","no_payload_positive_observation","outcome_identities","paired_grouped_interval_arithmetic","per_token_protection_trace","planned_inventory_reconciliation","producer_membership_order_proof","receipt_path_allowlist","rejection_is_not_protection","source_attribution_events","stamped_field_separation","string_byte_reversibility","unknown_egress_lower_bound","unknown_egress_route_fragment_observation","vocabulary_closure"]],"$.gate_results.*":["enum",["BLOCKED","FAIL","NOT_EVALUABLE","PASS"]],"$.intervals":["object",["egress_authorized_range_bounds_invalid","egress_authorized_range_non_monotonic","egress_clean_bounds_invalid","egress_overlapping_clean_spans","egress_raw_value_mismatches","egress_token_restore_failures","false_positive_bytes","false_positive_occurrences","gold_bytes_planned","gold_bytes_surviving_egress","gold_occurrences_attribution_not_measured","gold_occurrences_partially_surviving_egress","gold_occurrences_planned","gold_occurrences_surviving_egress","leaf_restore_decision_failures","leaf_restore_exact","manifest_raw_entry_agreement_enforced","manifest_span_monotonicity_enforced","manifest_terminal_events","observer_leaves_observed","observer_leaves_unobserved","observer_manifest_spans_observed","observer_recognizer_source_events","protected_leaves","protection_trace_items","tool_invocations","unknown_egress_lower_bound_cases"]],"$.intervals.*":["interval",["basis","conditional","high","low","method_id","point"]],"$.intervals.*.basis":["enum",["full_cell","paired_completed"]],"$.intervals.*.conditional":["bool",null],"$.intervals.*.high":["number",null],"$.intervals.*.low":["number",null],"$.intervals.*.method_id":["enum",["grouped_paired_percentile_v1"]],"$.intervals.*.point":["number",null],"$.not_measured":["object",["blocked_gates","metrics"]],"$.not_measured.blocked_gates":["array",null],"$.not_measured.blocked_gates.[]":["enum",["build_attestation_clean_source","claim_scope_present","class_commitment_completeness","class_commitment_schema_load","cross_language_vocabulary_equality","declaration_gating","egress_integrity_analogues","false_positive_negative_control","gold_survival_oracle","local_membership_order_proof","manifest_integrity_six_counter_schema_v4","no_payload_positive_observation","outcome_identities","paired_grouped_interval_arithmetic","per_token_protection_trace","planned_inventory_reconciliation","producer_membership_order_proof","receipt_path_allowlist","rejection_is_not_protection","source_attribution_events","stamped_field_separation","string_byte_reversibility","unknown_egress_lower_bound","unknown_egress_route_fragment_observation","vocabulary_closure"]],"$.not_measured.metrics":["array",null],"$.not_measured.metrics.[]":["enum",["egress_authorized_range_bounds_invalid","egress_authorized_range_non_monotonic","egress_clean_bounds_invalid","egress_overlapping_clean_spans","egress_raw_value_mismatches","egress_token_restore_failures","false_positive_bytes","false_positive_occurrences","gold_bytes_planned","gold_bytes_surviving_egress","gold_occurrences_attribution_not_measured","gold_occurrences_partially_surviving_egress","gold_occurrences_planned","gold_occurrences_surviving_egress","leaf_restore_decision_failures","leaf_restore_exact","manifest_raw_entry_agreement_enforced","manifest_span_monotonicity_enforced","manifest_terminal_events","observer_leaves_observed","observer_leaves_unobserved","observer_manifest_spans_observed","observer_recognizer_source_events","protected_leaves","protection_trace_items","tool_invocations","unknown_egress_lower_bound_cases"]],"$.outcomes":["object",["COMPLETED","ERROR_PROTOCOL","FAILED_CLOSED_NO_EGRESS","NOT_STARTED","UNKNOWN_EGRESS"]],"$.outcomes.*":["int",null],"$.planned_case_count":["int",null],"$.policy_identity":["enum",["authored.records.v1","controlled.email_only.v1","core.rule_floor.v1"]],"$.population_handle":["handle",null],"$.protocol_id":["enum",["gaze-evidence"]],"$.protocol_version":["version",null],"$.route_id":["enum",["daemon.jsonl.v1","evaluator.private.v1","mcp.rmcp.duplex.v1","ocr.document.v1","proxy.http.v1","session.episode.v1","stream.v1","structured.core.v1","text.clean_for_bench.v1"]],"$.route_status":["object",["daemon.jsonl.v1","evaluator.private.v1","mcp.rmcp.duplex.v1","ocr.document.v1","proxy.http.v1","session.episode.v1","stream.v1","structured.core.v1","text.clean_for_bench.v1"]],"$.route_status.*":["enum",["IMPLEMENTED","NOT_IMPLEMENTED"]]}"#;
fn walk(node: &Value, path: &str, rules: &Value) -> bool {
    if !RECEIPT_PATHS.contains(&path) {
        return false;
    }
    let rule = &rules[path];
    let kind = text(&rule[0]);
    if kind == "nullable_object" && node.is_null() {
        return true;
    }
    if kind == "interval" && node == "NOT_EVALUABLE" {
        return true;
    }
    match kind {
        "object" | "nullable_object" | "interval" => {
            let Some(map) = node.as_object() else {
                return false;
            };
            map.iter().all(|(key, value)| {
                if rule[1]
                    .as_array()
                    .is_some_and(|a| !a.iter().any(|v| v == key))
                {
                    return false;
                }
                let exact = format!("{path}.{key}");
                let wildcard = format!("{path}.*");
                walk(
                    value,
                    if RECEIPT_PATHS.contains(&exact.as_str()) {
                        &exact
                    } else {
                        &wildcard
                    },
                    rules,
                )
            })
        }
        "array" => node.as_array().is_some_and(|a| {
            a.iter()
                .enumerate()
                .all(|(i, v)| !a[..i].contains(v) && walk(v, &format!("{path}.[]"), rules))
        }),
        "enum" => node.is_string() && rule[1].as_array().is_some_and(|a| a.contains(node)),
        "int" => node.as_u64().is_some(),
        "positive" => node.as_u64().is_some_and(|n| n > 0),
        "version" => node.as_u64() == Some(1),
        "bool" => node.is_boolean(),
        "number" => node.as_f64().is_some_and(f64::is_finite),
        "handle" => node.as_str().is_some_and(|s| {
            (32..=64).contains(&s.len())
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }),
        _ => false,
    }
}
fn receipt_allowlisted(r: &Value) -> bool {
    let rules: Value = must(serde_json::from_str(PATH_RULES));
    if !walk(r, "$", &rules) {
        return false;
    }
    if RECEIPT_PATHS
        .iter()
        .filter(|p| p.matches('.').count() == 1 && **p != "$.asymmetric_outcome_table")
        .any(|p| r.get(&p[2..]).is_none())
    {
        return false;
    }
    let counts = r["counts"].as_object().unwrap();
    let derivations = r["derivations"].as_object().unwrap();
    let gates = r["gate_results"].as_object().unwrap();
    let outcomes = r["outcomes"].as_object().unwrap();
    let exact_keys = |map: &serde_json::Map<String, Value>, keys: &[&str]| {
        map.len() == keys.len() && keys.iter().all(|k| map.contains_key(*k))
    };
    if !exact_keys(derivations, METRIC_IDS)
        || !exact_keys(gates, GATE_IDS)
        || !exact_keys(outcomes, OUTCOME_STATES)
        || !exact_keys(r["route_status"].as_object().unwrap(), ROUTE_IDS)
        || !exact_keys(
            r["not_measured"].as_object().unwrap(),
            &["metrics", "blocked_gates"],
        )
        || !exact_keys(
            r["class_commitment_table"].as_object().unwrap(),
            &["id", "version"],
        )
        || outcomes.values().map(|v| v.as_u64().unwrap()).sum::<u64>()
            != r["planned_case_count"].as_u64().unwrap()
    {
        return false;
    }
    let candidate = r["arm_id"] == "candidate";
    if candidate != r.get("asymmetric_outcome_table").is_some() {
        return false;
    }
    if candidate {
        let table = r["asymmetric_outcome_table"].as_object().unwrap();
        if !exact_keys(table, OUTCOME_STATES)
            || table
                .values()
                .any(|v| !exact_keys(v.as_object().unwrap(), OUTCOME_STATES))
            || OUTCOME_STATES.iter().any(|b| {
                table
                    .values()
                    .map(|row| row[*b].as_u64().unwrap())
                    .sum::<u64>()
                    != outcomes[*b].as_u64().unwrap()
            })
        {
            return false;
        }
    }
    let expected_missing: BTreeSet<_> = derivations
        .iter()
        .filter_map(|(m, g)| (g == "not_measured").then_some(m.as_str()))
        .collect();
    let expected_blocked: BTreeSet<_> = gates
        .iter()
        .filter_map(|(g, v)| (v == "BLOCKED").then_some(g.as_str()))
        .collect();
    let actual_set = |key| {
        r["not_measured"][key]
            .as_array()
            .unwrap()
            .iter()
            .map(text)
            .collect::<BTreeSet<_>>()
    };
    expected_missing == actual_set("metrics")
        && expected_blocked == actual_set("blocked_gates")
        && BLOCKED_GATES.iter().all(|g| gates[*g] == "BLOCKED")
        && !(counts
            .get("observer_leaves_unobserved")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
            && gates["source_attribution_events"] == "PASS")
        && METRIC_IDS
            .iter()
            .all(|m| counts.contains_key(*m) == COUNTING_GRADES.contains(&text(&derivations[*m])))
        && STAMPED_KEYS.iter().all(|k| r.get(*k).is_none())
}
fn receipt(c: &Counts, state: &str, controlled: bool) -> Value {
    let mut derivations = BTreeMap::new();
    let mut counts = BTreeMap::new();
    for &m in METRIC_IDS {
        let grade = match m {
            "protection_trace_items" | "unknown_egress_lower_bound_cases" => "not_measured",
            "egress_clean_bounds_invalid"
            | "egress_authorized_range_bounds_invalid"
            | "egress_authorized_range_non_monotonic"
            | "egress_overlapping_clean_spans" => "not_applicable_by_construction",
            "manifest_span_monotonicity_enforced" | "manifest_raw_entry_agreement_enforced" => {
                "invariant_enforced_not_counted"
            }
            "observer_leaves_observed"
            | "observer_leaves_unobserved"
            | "observer_manifest_spans_observed"
            | "observer_recognizer_source_events" => "observer_native",
            "gold_occurrences_surviving_egress"
            | "gold_occurrences_partially_surviving_egress"
            | "gold_occurrences_attribution_not_measured"
            | "gold_bytes_surviving_egress"
            | "false_positive_occurrences"
            | "false_positive_bytes"
            | "egress_token_restore_failures"
            | "egress_raw_value_mismatches" => "egress_reconstructed",
            _ => "route_native",
        };
        let grade = if COUNTING_GRADES.contains(&grade) && !c.0.contains_key(m) {
            "not_measured"
        } else {
            grade
        };
        derivations.insert(m, grade);
        if COUNTING_GRADES.contains(&grade) {
            counts.insert(m, c.get(m));
        }
    }
    let gates: BTreeMap<_, _> = GATE_IDS
        .iter()
        .map(|&g| {
            let status = if BLOCKED_GATES.contains(&g) {
                "BLOCKED"
            } else if [
                "receipt_path_allowlist",
                "vocabulary_closure",
                "stamped_field_separation",
                "outcome_identities",
                "declaration_gating",
                "claim_scope_present",
            ]
            .contains(&g)
            {
                "PASS"
            } else {
                "NOT_EVALUABLE"
            };
            (g, status)
        })
        .collect();
    let mut r = json!({
        "protocol_id":"gaze-evidence","protocol_version":1,"arm_id":"base","route_id":"mcp.rmcp.duplex.v1",
        "route_status":ROUTE_IDS.iter().map(|&r|(r,if r=="mcp.rmcp.duplex.v1"{"IMPLEMENTED"}else{"NOT_IMPLEMENTED"})).collect::<BTreeMap<_,_>>(),
        "cell_id":if controlled{"synthetic.mcp.controlled.v1"}else{"synthetic.mcp.core.v1"},
        "policy_identity":if controlled{"controlled.email_only.v1"}else{"core.rule_floor.v1"},
        "population_handle":"0123456789abcdef0123456789abcdef","class_commitment_table":{"id":"class-commitments-v1","version":1},
        "claim_scope":"synthetic_harness_capability_only","planned_case_count":1,
        "outcomes":OUTCOME_STATES.iter().map(|&s|(s,u64::from(s==state))).collect::<BTreeMap<_,_>>(),
        "counts":counts,"derivations":derivations,"gate_results":gates,"not_measured":{"metrics":["protection_trace_items","unknown_egress_lower_bound_cases"],"blocked_gates":BLOCKED_GATES},
        "analysis_declaration":null,"intervals":{},"error_codes":{}
    });
    r["not_measured"]["metrics"] = json!(
        derivations
            .iter()
            .filter_map(|(m, g)| (*g == "not_measured").then_some(m))
            .collect::<Vec<_>>()
    );
    let gate = |performed: bool, fail: bool, incomplete: bool| {
        if !performed {
            "NOT_EVALUABLE"
        } else if fail {
            "FAIL"
        } else if incomplete {
            "NOT_EVALUABLE"
        } else {
            "PASS"
        }
    };
    let restores = c.1.get("restore").copied().unwrap_or(0);
    r["gate_results"]["string_byte_reversibility"] = json!(gate(
        restores > 0,
        c.get("leaf_restore_exact") != restores as u64,
        false
    ));
    r["gate_results"]["egress_integrity_analogues"] = json!(gate(
        restores > 0,
        c.get("egress_token_restore_failures") + c.get("egress_raw_value_mismatches") > 0,
        !c.0.contains_key("egress_raw_value_mismatches")
    ));
    r["gate_results"]["gold_survival_oracle"] = json!(gate(
        c.1.contains_key("gold"),
        c.get("gold_occurrences_surviving_egress")
            + c.get("gold_occurrences_partially_surviving_egress")
            > 0,
        c.get("gold_occurrences_attribution_not_measured") > 0
    ));
    r["gate_results"]["false_positive_negative_control"] = json!(gate(
        c.1.contains_key("negative"),
        c.get("false_positive_occurrences") > 0,
        false
    ));
    r["gate_results"]["source_attribution_events"] = json!(gate(
        c.0.contains_key("observer_leaves_unobserved"),
        false,
        c.get("observer_leaves_unobserved") > 0
    ));
    r["gate_results"]["no_payload_positive_observation"] =
        json!(if state == "FAILED_CLOSED_NO_EGRESS" {
            "PASS"
        } else {
            "NOT_EVALUABLE"
        });
    r["gate_results"]["rejection_is_not_protection"] = json!(if state == "COMPLETED" {
        "NOT_EVALUABLE"
    } else {
        "PASS"
    });
    assert!(receipt_allowlisted(&r), "emitter-conformance");
    r
}

#[tokio::test]
async fn protected_success_and_golden_receipt() {
    let authored = format!("literal [[g]]{EMAIL}[[/g]]");
    let h = host(false, Output::Echo, &["result"], 0, false, fresh_session());
    let r = call(h.clone(), json!({"text":authored}), false)
        .await
        .expect("response");
    let v = decode(&r);
    let mut c = Counts::default();
    assert!(
        c.score(&h.session, text(&v["result"]), EMAIL, true) == Verdict::Protected,
        "protected-oracle"
    );
    c.restore(&h.session, text(&v["result"]), &authored, EMAIL);
    // Independently observe the protected argument at ToolCtx and the client response.
    c.restore(
        &h.session,
        text(&h.argument.lock().unwrap()["text"]),
        &authored,
        EMAIL,
    );
    c.add(
        "protected_leaves",
        string_leaves(&h.argument.lock().unwrap()) + string_leaves(&v),
    );
    c.observe_host(&h, 2);
    assert!(
        h.session
            .snapshot_entries()
            .iter()
            .any(|e| e.class == PiiClass::Email),
        "typed-email"
    );
    assert!(
        *h.store.events.lock().unwrap() == ["begin", "finish"],
        "terminal-order"
    );
    let emitted = receipt(&c, outcome(Some(&r)), false);
    let fixture: Value = must(serde_json::from_str(GOLDEN));
    assert!(
        must(serde_json::to_vec(&emitted)) == must(serde_json::to_vec(&fixture["receipt"])),
        "golden-receipt"
    );
}
#[tokio::test]
async fn controlled_four_slot_occurrence_oracle() {
    let session = fresh_session();
    let token = must(session.tokenize(&PiiClass::Custom("phone".into()), PHONE));
    let output = json!({"a":PHONE,"b":token,"c":&PHONE[..8],"d":format!("[[g]]{PHONE}[[/g]][[g]]{PHONE}[[/g]]"),"email":EMAIL});
    let h = host(
        true,
        Output::Fixed(output),
        &["a", "b", "c", "d", "email"],
        0,
        false,
        session,
    );
    let r = call(h.clone(), json!({"text":"[[g]][[/g]]"}), false)
        .await
        .expect("response");
    let v = decode(&r);
    let mut c = Counts::default();
    let mut protected = 0;
    for key in ["a", "b", "c", "d"] {
        if c.score(&h.session, text(&v[key]), PHONE, key == "d") == Verdict::Protected {
            protected += 1;
        }
    }
    assert!(
        c.get("gold_occurrences_surviving_egress") == 1
            && c.get("gold_occurrences_partially_surviving_egress") == 1
            && c.get("gold_occurrences_attribution_not_measured") == 1
            && protected == 1,
        "four-exact-verdicts"
    );
    assert!(
        occurrence(&h.session, text(&v["email"]), EMAIL, false) == Verdict::Protected,
        "email-floor"
    );
    assert!(h.session.snapshot_entries().len() == 2, "anchor-benignity");
    c.observe_host(&h, 6);
    let r = receipt(&c, "COMPLETED", true);
    assert!(
        r["counts"]["gold_occurrences_surviving_egress"] == 1,
        "computed-leak"
    );
}
#[tokio::test]
async fn benign_negative_and_positive_false_positive_control() {
    let h = host(false, Output::Echo, &["result"], 0, false, fresh_session());
    let r = call(h.clone(), json!({"text":"ordinary fixture text"}), false)
        .await
        .expect("response");
    let v = decode(&r);
    let mut c = Counts::default();
    c.negative(&h.session, text(&v["result"]), "ordinary fixture text");
    assert!(
        c.get("false_positive_occurrences") == 0
            && c.get("false_positive_bytes") == 0
            && h.session.snapshot_entries().is_empty(),
        "benign-negative"
    );
    // Deliberately authored non-PII fixture identifier with Email spelling.
    let h = host(true, Output::Echo, &["result"], 0, false, fresh_session());
    let r = call(h.clone(), json!({"text":EMAIL}), false)
        .await
        .expect("response");
    let v = decode(&r);
    c.negative(&h.session, text(&v["result"]), EMAIL);
    assert!(
        c.get("false_positive_occurrences") == 1
            && c.get("false_positive_bytes") == EMAIL.len() as u64,
        "positive-fp-control"
    );
    assert!(
        receipt(&c, outcome(Some(&r)), false)["gate_results"]["false_positive_negative_control"]
            == "FAIL",
        "fp-gate-fail"
    );
}
#[tokio::test]
async fn undeclared_carrier_has_positive_no_payload_and_unobserved_leaves() {
    let h = host(false, Output::Echo, &["result"], 0, false, fresh_session());
    let r = call(h.clone(), json!({"text":EMAIL,"extra":EMAIL}), false)
        .await
        .expect("response");
    assert!(
        outcome(Some(&r)) == "FAILED_CLOSED_NO_EGRESS",
        "positive-no-payload"
    );
    let mut c = Counts::default();
    c.observe_host(&h, 3);
    assert!(
        c.get("tool_invocations") == 0
            && c.get("observer_leaves_observed") == 0
            && c.get("observer_leaves_unobserved") == 3,
        "early-leaf-accounting"
    );
    let r = receipt(&c, outcome(Some(&r)), false);
    assert!(
        r["counts"].get("protected_leaves").is_none(),
        "no-rejection-credit"
    );
}
#[tokio::test]
async fn no_payload_classifier_rejects_extra_surfaces() {
    let h = host(false, Output::Echo, &["result"], 0, false, fresh_session());
    let received = call(h, json!({"text":EMAIL,"extra":EMAIL}), false)
        .await
        .expect("response");
    assert!(
        outcome(Some(&received)) == "FAILED_CLOSED_NO_EGRESS",
        "safe-error-control"
    );
    // In-memory classifier falsifiers cloned from a received frame, not production leaks.
    for path in [
        "/structuredContent",
        "/_meta",
        "/content/0/_meta",
        "/content/0/annotations",
    ] {
        let mut value = must(serde_json::to_value(&received));
        let payload = if path.ends_with("annotations") {
            json!({"audience":["user"]})
        } else {
            json!({"synthetic":EMAIL})
        };
        if path.starts_with("/content") {
            value["content"][0][path.rsplit('/').next().unwrap()] = payload;
        } else {
            value[&path[1..]] = payload;
        }
        let adversarial: CallToolResult = must(serde_json::from_value(value));
        assert!(
            outcome(Some(&adversarial)) == "UNKNOWN_EGRESS",
            "extra-surface-unknown"
        );
    }
}
#[test]
fn missing_measurements_and_ambiguous_only_gates() {
    let c = Counts::default();
    for state in ["COMPLETED", "UNKNOWN_EGRESS", "FAILED_CLOSED_NO_EGRESS"] {
        let r = receipt(&c, state, true);
        assert!(
            r["counts"].as_object().unwrap().is_empty(),
            "absent-measurements"
        );
        for gate in [
            "string_byte_reversibility",
            "egress_integrity_analogues",
            "false_positive_negative_control",
            "gold_survival_oracle",
        ] {
            assert!(
                r["gate_results"][gate] == "NOT_EVALUABLE",
                "unperformed-gate"
            );
        }
    }
    let mut c = Counts::default();
    c.score(&fresh_session(), "[[g]]missing", PHONE, true);
    let r = receipt(&c, "COMPLETED", true);
    assert!(
        r["gate_results"]["gold_survival_oracle"] == "NOT_EVALUABLE",
        "ambiguous-gate"
    );
    assert!(
        r["counts"]["gold_bytes_surviving_egress"] == 0
            && r["counts"].get("leaf_restore_exact").is_none(),
        "observed-zero-no-restore"
    );
}
#[tokio::test]
async fn known_token_continuity() {
    let h = host(false, Output::Echo, &["result"], 0, false, fresh_session());
    let a = decode(
        &call(h.clone(), json!({"text":EMAIL}), false)
            .await
            .expect("response"),
    );
    let b = decode(
        &call(h.clone(), json!({"text":a["result"]}), false)
            .await
            .expect("response"),
    );
    assert!(
        a == b && h.session.snapshot_entries().len() == 1,
        "known-token-continuity"
    );
    assert!(
        must(h.session.restore_strict_text(text(&b["result"]))) == EMAIL,
        "exact-string-bytes"
    );
}
#[tokio::test]
async fn response_conflict_rolls_back_but_failed_finish_retains_mappings() {
    let h = host(
        false,
        Output::Fixed(json!({"result":FRESH})),
        &["result"],
        3,
        false,
        fresh_session(),
    );
    let r = call(h.clone(), json!({"text":"benign"}), false)
        .await
        .expect("response");
    assert!(
        outcome(Some(&r)) == "FAILED_CLOSED_NO_EGRESS",
        "conflict-no-payload"
    );
    assert!(
        *h.store.events.lock().unwrap() == ["begin", "fail"]
            && h.session.snapshot_entries().iter().all(|e| e.raw != FRESH),
        "rollback-no-losing-mappings"
    );
    let h = host(
        false,
        Output::Fixed(json!({"result":FRESH})),
        &["result"],
        0,
        true,
        fresh_session(),
    );
    let r = call(h.clone(), json!({"text":"benign"}), false)
        .await
        .expect("response");
    assert!(
        outcome(Some(&r)) == "FAILED_CLOSED_NO_EGRESS",
        "finish-no-payload"
    );
    assert!(
        *h.store.events.lock().unwrap() == ["begin", "finish"]
            && h.session.snapshot_entries().iter().any(|e| e.raw == FRESH),
        "failed-finish-retains-committed"
    );
}
#[tokio::test]
async fn timeout_is_unknown_without_partial_wire_observation() {
    let h = host(false, Output::Wait, &["result"], 0, false, fresh_session());
    let r = call(h.clone(), json!({"text":EMAIL}), true).await;
    assert!(outcome(r.as_ref()) == "UNKNOWN_EGRESS", "timeout-unknown");
    let mut c = Counts::default();
    c.observe_host(&h, 2);
    c.add("gold_occurrences_planned", 1);
    c.add("gold_bytes_planned", EMAIL.len());
    let r = receipt(&c, "UNKNOWN_EGRESS", false);
    assert!(
        r["counts"]
            .get("unknown_egress_lower_bound_cases")
            .is_none()
            && r["counts"].get("protected_leaves").is_none(),
        "unknown-not-zero-or-credit"
    );
}
#[tokio::test]
async fn escaped_unicode_and_below_floor_are_occurrence_scoped() {
    const V: &str = "synthetic \"äöü\\value";
    let h = host(
        true,
        Output::Fixed(json!({"result":{"leaf":V}})),
        &["result", "result.leaf"],
        0,
        false,
        fresh_session(),
    );
    let r = call(h.clone(), json!({"text":"benign"}), false)
        .await
        .expect("response");
    let v = decode(&r);
    assert!(
        occurrence(&h.session, text(&v["result"]["leaf"]), V, false) == Verdict::Full,
        "schema-directed-nested-decode"
    );
    assert!(prefix("äöüäö", 9).len() == 8, "whole-codepoint-prefix");
    assert!(
        occurrence(&h.session, "äöüä", "äöüäö", false) == Verdict::Partial(8),
        "unicode-fragment"
    );
    assert!(
        occurrence(&h.session, &PHONE[..7], PHONE, false) == Verdict::Unknown,
        "below-floor-no-credit"
    );
    assert!(
        occurrence(&h.session, "[[g]]missing", PHONE, true) == Verdict::Unknown,
        "missing-anchor-no-credit"
    );
}
#[tokio::test]
async fn observer_passive_token_covered_and_raw_gap_are_distinct() {
    for mode in [0, 1, 2] {
        let h = host(
            false,
            Output::Echo,
            &["result"],
            mode,
            false,
            fresh_session(),
        );
        let r = call(h, json!({"text":format!("literal {EMAIL}")}), false)
            .await
            .expect("response");
        assert!(
            outcome(Some(&r))
                == if mode == 1 {
                    "FAILED_CLOSED_NO_EGRESS"
                } else {
                    "COMPLETED"
                },
            "observer-raw-gap-discriminates"
        );
    }
}
#[tokio::test]
async fn integrity_analogues_have_independent_nonzero_falsifiers() {
    let s = fresh_session();
    let first = must(s.tokenize(&PiiClass::Email, EMAIL));
    let second = must(s.tokenize(&PiiClass::Email, FRESH));
    let h = host(
        true,
        Output::Fixed(json!({"a":second,"b":first})),
        &["a", "b"],
        0,
        false,
        s,
    );
    let r = call(h.clone(), json!({"text":"benign"}), false)
        .await
        .expect("response");
    let v = decode(&r);
    let mut c = Counts::default();
    c.restore(&h.session, text(&v["a"]), EMAIL, EMAIL);
    c.restore(&h.session, text(&v["b"]), FRESH, FRESH);
    assert!(
        c.get("egress_raw_value_mismatches") == 2 && c.get("egress_token_restore_failures") == 0,
        "independent-slot-swap"
    );
    assert!(
        receipt(&c, outcome(Some(&r)), true)["gate_results"]["egress_integrity_analogues"]
            == "FAIL",
        "swap-gate-fail"
    );
    let bad = first.replace("Email_1", "Email_999999");
    assert!(
        h.session.restore_strict_text(&bad).is_err(),
        "reserved-unknown-token"
    );
    c.restore(&h.session, &bad, EMAIL, EMAIL);
    assert!(
        c.get("egress_token_restore_failures") == 1 && c.get("leaf_restore_decision_failures") == 1,
        "restore-error-counted"
    );
    // This second receipt measures a deliberately mutated restore operand, not route egress.
    assert!(
        receipt(&c, "COMPLETED", true)["gate_results"]["egress_integrity_analogues"] == "FAIL",
        "restore-gate-fail"
    );
}
#[tokio::test]
async fn json_text_string_bytes_are_stricter_than_semantic_equality() {
    const ORIGINAL: &str = "{ \"n\": 1 }";
    const CHANGED: &str = "{\"n\":1}";
    let h = host(
        true,
        Output::Fixed(json!({"result":CHANGED})),
        &["result"],
        0,
        false,
        fresh_session(),
    );
    let r = call(h.clone(), json!({"text":ORIGINAL}), false)
        .await
        .expect("response");
    let v = decode(&r);
    let restored = must(h.session.restore_strict_text(text(&v["result"])));
    assert!(
        must(serde_json::from_str::<Value>(&restored))
            == must(serde_json::from_str::<Value>(ORIGINAL)),
        "semantic-equality-control"
    );
    let mut c = Counts::default();
    c.restore(&h.session, text(&v["result"]), ORIGINAL, ORIGINAL);
    assert!(
        c.get("leaf_restore_exact") == 0 && restored != ORIGINAL,
        "string-bytes-discriminate"
    );
    assert!(
        receipt(&c, outcome(Some(&r)), true)["counts"]
            .get("egress_raw_value_mismatches")
            .is_none(),
        "no-authorized-range-no-raw-check"
    );
    assert!(
        receipt(&c, outcome(Some(&r)), true)["gate_results"]["string_byte_reversibility"] == "FAIL",
        "string-gate-fail"
    );
}
#[test]
fn vocabularies_match_committed_artifact() {
    let golden: Value = must(serde_json::from_str(GOLDEN));
    let mirrors = mirrored_vocabularies();
    assert!(
        golden["vocabularies"]
            .as_object()
            .expect("vocabularies")
            .len()
            == mirrors.len(),
        "vocabulary-set-count"
    );
    for (name, values) in mirrors {
        let a: BTreeSet<_> = values.iter().copied().collect();
        let b: BTreeSet<_> = golden["vocabularies"][name]
            .as_array()
            .expect("vocabulary")
            .iter()
            .map(text)
            .collect();
        assert!(a == b, "vocabulary-mirror");
    }
    let rules: Value = must(serde_json::from_str(PATH_RULES));
    assert!(
        rules
            .as_object()
            .expect("rules")
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            == RECEIPT_PATHS.iter().copied().collect(),
        "rule-path-equality"
    );
}
#[test]
fn rust_walker_rejects_nested_paths_types_and_forbidden_counts() {
    let fixture: Value = must(serde_json::from_str(GOLDEN));
    let r = &fixture["receipt"];
    assert!(receipt_allowlisted(r), "positive-receipt");
    for ptr in [
        "/clean_text",
        "/not_measured/extra",
        "/analysis_declaration",
    ] {
        let mut bad = r.clone();
        if ptr == "/clean_text" {
            bad["clean_text"] = json!("synthetic-private-canary");
        } else if ptr == "/not_measured/extra" {
            bad["not_measured"]["extra"] = json!({});
        } else {
            bad["analysis_declaration"] = json!([]);
        }
        assert!(!receipt_allowlisted(&bad), "nested-path-type-refusal");
    }
    for key in ["outcomes", "claim_scope", "gate_results"] {
        let mut bad = r.clone();
        bad.as_object_mut().unwrap().remove(key);
        assert!(!receipt_allowlisted(&bad), "mandatory-receipt-key");
    }
    let mut bad = r.clone();
    bad["outcomes"]["COMPLETED"] = json!(0);
    assert!(!receipt_allowlisted(&bad), "receipt-outcome-identity");
    let mut bad = r.clone();
    bad["not_measured"]["metrics"] = json!([]);
    assert!(!receipt_allowlisted(&bad), "receipt-not-measured-coherence");
    let mut bad = r.clone();
    bad["gate_results"]
        .as_object_mut()
        .unwrap()
        .remove("gold_survival_oracle");
    assert!(!receipt_allowlisted(&bad), "receipt-gate-coverage");
    let mut bad = r.clone();
    bad["counts"]["protection_trace_items"] = json!(0);
    assert!(!receipt_allowlisted(&bad), "forbidden-count-refusal");
}
#[tokio::test]
async fn private_failure_canary_child() {
    if std::env::var_os("GAZE_EVIDENCE_CANARY_CHILD").is_none() {
        return;
    }
    let h = host(false, Output::Error, &["result"], 0, false, fresh_session());
    let r = call(h, json!({"text":EMAIL}), false)
        .await
        .expect("response");
    assert!(
        outcome(Some(&r)) == "FAILED_CLOSED_NO_EGRESS",
        "error-carrier"
    );
    let wire = must(serde_json::to_string(&r));
    assert!(!wire.contains(EMAIL), "error-payload-canary");
    let _ = std::panic::catch_unwind(|| {
        panic!("static-assertion-canary");
    });
    for size in [0, 300000] {
        let payload = format!("{{{}", " ".repeat(size));
        assert!(
            serde_json::from_str::<Value>(&payload).is_err(),
            "malformed-carrier"
        );
    }
    let h = host(false, Output::Wait, &["result"], 0, false, fresh_session());
    assert!(
        call(h, json!({"text":EMAIL}), true).await.is_none(),
        "canary-timeout"
    );
}
#[test]
fn private_failure_canary_captures_stdout_stderr_and_files() {
    let root = std::env::temp_dir().join(format!("gaze-evidence-canary-{}", std::process::id()));
    must(std::fs::create_dir(&root));
    let result = std::process::Command::new(must(std::env::current_exe()))
        .args(["--exact", "private_failure_canary_child", "--nocapture"])
        .env("GAZE_EVIDENCE_CANARY_CHILD", "1")
        .current_dir(&root)
        .output();
    let files = must(std::fs::read_dir(&root)).count();
    must(std::fs::remove_dir_all(&root));
    let result = must(result);
    let mut output = result.stdout;
    output.extend(result.stderr);
    assert!(result.status.success(), "canary-child-success");
    assert!(
        !output.windows(EMAIL.len()).any(|w| w == EMAIL.as_bytes())
            && !output.windows(PHONE.len()).any(|w| w == PHONE.as_bytes())
            && !output.windows(7).any(|w| w == b":Email_"),
        "private-output-canary"
    );
    assert!(files == 0, "private-file-canary");
}

#[tokio::test]
async fn plain_string_carrier_is_decoded_without_json_reparse() {
    let h = host(
        true,
        Output::Fixed(json!(EMAIL)),
        &[],
        0,
        false,
        fresh_session(),
    );
    let r = call(h.clone(), json!({"text":"benign"}), false)
        .await
        .expect("response");
    assert!(
        outcome(Some(&r)) == "COMPLETED" && r.content.len() == 1,
        "plain-carrier"
    );
    let observed = &r.content[0].raw.as_text().expect("text-frame").text;
    assert!(
        occurrence(&h.session, observed, EMAIL, false) == Verdict::Protected,
        "plain-string-oracle"
    );
}
