pub(crate) fn controlled_pipeline() -> gaze::Pipeline {
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

pub(crate) use async_trait::async_trait;
pub(crate) use gaze::{PiiClass, Scope, Session};
pub(crate) use gaze_mcp_core::*;
pub(crate) use gaze_mcp_rmcp::{FixedPrincipalResolver, RmcpFrontend};
pub(crate) use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult},
};
pub(crate) use serde_json::{Value, json};
pub(crate) use std::collections::{BTreeMap, BTreeSet};
pub(crate) use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
pub(crate) use std::time::Duration;
pub(crate) const MIN_FRAGMENT_BYTES: usize = 8;
pub(crate) const EMAIL: &str = "alice@example.invalid";
pub(crate) const FRESH: &str = "bob@example.invalid";
pub(crate) const PHONE: &str = "+49 1555 0112233";
pub(crate) const GOLDEN: &str =
    include_str!("../../../../scripts/bench/fixtures/evidence/mcp_route_v1.golden.json");

// Static errors prevent private Debug payloads from appearing in failed tests.
pub(crate) fn must<T, E>(r: Result<T, E>) -> T {
    match r {
        Ok(v) => v,
        Err(_) => panic!("fixture-operation-failed"),
    }
}
pub(crate) fn text(v: &Value) -> &str {
    v.as_str().expect("string-carrier")
}

#[derive(Default)]
pub(crate) struct Store {
    pub(crate) events: Mutex<Vec<&'static str>>,
    pub(crate) fail_finish: bool,
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
pub(crate) struct Auth;
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
pub(crate) struct Observer {
    pub(crate) spans: Arc<Mutex<Vec<usize>>>,
    pub(crate) mode: u8,
    pub(crate) session: Arc<Session>,
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
pub(crate) struct Logger(pub(crate) Arc<AtomicUsize>);
impl gaze::RedactionLogger for Logger {
    fn log(&self, _: &gaze::RedactionEntry) -> Result<(), gaze::RedactionLogError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
pub(crate) enum Output {
    Echo,
    Fixed(Value),
    Wait,
    Error,
}
pub(crate) struct Producer {
    pub(crate) descriptor: ToolDescriptor,
    pub(crate) output: Output,
    pub(crate) invokes: Arc<AtomicUsize>,
    pub(crate) argument: Arc<Mutex<Value>>,
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
pub(crate) struct Host {
    pub(crate) registry: ToolRegistry,
    pub(crate) pipeline: gaze::Pipeline,
    pub(crate) session: Arc<Session>,
    pub(crate) store: Store,
    pub(crate) spans: Arc<Mutex<Vec<usize>>>,
    pub(crate) logger: Logger,
    pub(crate) invokes: Arc<AtomicUsize>,
    pub(crate) argument: Arc<Mutex<Value>>,
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
pub(crate) fn host(
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
pub(crate) fn string_leaves(value: &Value) -> usize {
    match value {
        Value::String(_) => 1,
        Value::Array(items) => items.iter().map(string_leaves).sum(),
        Value::Object(items) => items.values().map(string_leaves).sum(),
        _ => 0,
    }
}
pub(crate) fn fresh_session() -> Arc<Session> {
    Arc::new(must(Session::new(Scope::Ephemeral)))
}
pub(crate) async fn call(h: Arc<Host>, args: Value, timeout: bool) -> Option<CallToolResult> {
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
pub(crate) fn no_payload_surfaces(r: &CallToolResult) -> bool {
    let Some(t) = r.content.first().and_then(|c| c.raw.as_text()) else {
        return false;
    };
    // Equality closes every serialized data-bearing surface, including future fields.
    must(serde_json::to_value(r))
        == json!({"isError":true,"content":[{"type":"text","text":t.text}]})
}
pub(crate) fn outcome(r: Option<&CallToolResult>) -> &'static str {
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
pub(crate) fn decode(r: &CallToolResult) -> Value {
    assert!(
        outcome(Some(r)) == "COMPLETED" && r.content.len() == 1,
        "completed-single-carrier"
    );
    must(serde_json::from_str(
        &r.content[0].raw.as_text().expect("text-frame").text,
    ))
}
#[derive(PartialEq, Eq)]
pub(crate) enum Verdict {
    Full,
    Partial(usize),
    Protected,
    Unknown,
}
pub(crate) fn prefix(s: &str, n: usize) -> &str {
    let mut n = n.min(s.len());
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    &s[..n]
}
pub(crate) fn occurrence(
    session: &Session,
    observed: &str,
    expected: &str,
    anchor: bool,
) -> Verdict {
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
pub(crate) struct Counts(
    pub(crate) BTreeMap<&'static str, u64>,
    pub(crate) BTreeMap<&'static str, usize>,
);
impl Counts {
    pub(crate) fn add(&mut self, key: &'static str, n: usize) {
        *self.0.entry(key).or_default() += n as u64;
    }
    pub(crate) fn get(&self, key: &str) -> u64 {
        self.0.get(key).copied().unwrap_or(0)
    }
    pub(crate) fn score(
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
    pub(crate) fn restore(
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
                    *self.1.entry("raw_compared").or_default() += 1;
                    self.add("egress_raw_value_mismatches", 0);
                    if &r.text[r.authorized_output_ranges[0].clone()] != expected_occurrence {
                        self.add("egress_raw_value_mismatches", 1);
                    }
                }
            }
        }
    }
    pub(crate) fn negative(&mut self, session: &Session, observed: &str, expected: &str) {
        *self.1.entry("negative").or_default() += 1;
        let verdict = occurrence(session, observed, expected, false);
        // Only full preservation or an exact owned token resolves this predicate.
        if !matches!(verdict, Verdict::Full | Verdict::Protected) {
            return;
        }
        *self.1.entry("negative_compared").or_default() += 1;
        self.add("false_positive_occurrences", 0);
        self.add("false_positive_bytes", 0);
        if verdict == Verdict::Protected {
            self.add("false_positive_occurrences", 1);
            self.add("false_positive_bytes", expected.len());
        }
    }
    pub(crate) fn observe_host(&mut self, h: &Host, planned_leaves: usize) {
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
pub(crate) const ARM_IDS: &[&str] = &["base", "candidate"];
pub(crate) const ATTESTATION_SOURCES: &[&str] = &["live_repository", "test_seam"];
pub(crate) const BASIS_IDS: &[&str] = &["full_cell", "paired_completed"];
pub(crate) const BLOCKED_GATES: &[&str] = &[
    "class_commitment_completeness",
    "manifest_integrity_six_counter_schema_v4",
    "per_token_protection_trace",
    "producer_membership_order_proof",
    "unknown_egress_route_fragment_observation",
];
pub(crate) const CELL_IDS: &[&str] = &[
    "synthetic.evaluator.v1",
    "synthetic.mcp.controlled.v1",
    "synthetic.mcp.core.v1",
];
pub(crate) const CLAIM_SCOPES: &[&str] = &["synthetic_harness_capability_only"];
pub(crate) const CONTROL_REFUSAL_CODES: &[&str] = &[
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
pub(crate) const COUNTING_GRADES: &[&str] = &[
    "egress_reconstructed",
    "observed_subset_lower_bound",
    "observer_native",
    "planned_inventory",
    "private_authored_records",
    "route_native",
];
pub(crate) const DECLARATION_FIELDS: &[&str] = &[
    "acceptance_limit",
    "confidence_level",
    "coverage_target",
    "multiplicity_treatment",
    "resample_count",
    "seed",
    "strata",
    "weighting",
];
pub(crate) const DERIVATIONS: &[&str] = &[
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
pub(crate) const ERROR_CODES: &[&str] = &[
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
pub(crate) const FEATURE_GRAPH_IDS: &[&str] = &["workspace.all_features", "workspace.default"];
pub(crate) const GATE_IDS: &[&str] = &[
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
pub(crate) const GATE_RESULTS: &[&str] = &["BLOCKED", "FAIL", "NOT_EVALUABLE", "PASS"];
pub(crate) const LEAK_FAMILY_METRIC_IDS: &[&str] = &[
    "gold_bytes_surviving_egress",
    "gold_occurrences_attribution_not_measured",
    "gold_occurrences_partially_surviving_egress",
    "gold_occurrences_surviving_egress",
];
pub(crate) const METHOD_IDS: &[&str] = &["grouped_paired_percentile_v1"];
pub(crate) const METRIC_IDS: &[&str] = &[
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
pub(crate) const MULTIPLICITY_IDS: &[&str] = &["synthetic_none"];
pub(crate) const OUTCOME_STATES: &[&str] = &[
    "COMPLETED",
    "ERROR_PROTOCOL",
    "FAILED_CLOSED_NO_EGRESS",
    "NOT_STARTED",
    "UNKNOWN_EGRESS",
];
pub(crate) const POLICY_IDENTITIES: &[&str] = &[
    "authored.records.v1",
    "controlled.email_only.v1",
    "core.rule_floor.v1",
];
pub(crate) const RECEIPT_PATHS: &[&str] = &[
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
pub(crate) const REFUSAL_CODES: &[&str] = &[
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
pub(crate) const ROUTE_IDS: &[&str] = &[
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
pub(crate) const ROUTE_STATUSES: &[&str] = &["IMPLEMENTED", "NOT_IMPLEMENTED"];
pub(crate) const STAMPED_KEYS: &[&str] = &[
    "build_attestation",
    "local_membership_order_proof_method",
    "local_membership_order_verified",
];
pub(crate) const STRATUM_IDS: &[&str] = &["synthetic_de", "synthetic_en"];
pub(crate) const TABLE_IDS: &[&str] = &["class-commitments-v1"];
pub(crate) const TOOLCHAIN_IDS: &[&str] = &["rust.workspace_pinned"];
pub(crate) const WEIGHTING_IDS: &[&str] = &["inventory_group"];
pub(crate) fn mirrored_vocabularies() -> BTreeMap<&'static str, &'static [&'static str]> {
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
pub(crate) const PATH_RULES: &str = r#"{"$":["object",null],"$.analysis_declaration":["nullable_object",["acceptance_limit","confidence_level","coverage_target","multiplicity_treatment","resample_count","seed","strata","weighting"]],"$.analysis_declaration.acceptance_limit":["number",null],"$.analysis_declaration.confidence_level":["number",null],"$.analysis_declaration.coverage_target":["number",null],"$.analysis_declaration.multiplicity_treatment":["enum",["synthetic_none"]],"$.analysis_declaration.resample_count":["positive",null],"$.analysis_declaration.seed":["int",null],"$.analysis_declaration.strata":["array",null],"$.analysis_declaration.strata.[]":["enum",["synthetic_de","synthetic_en"]],"$.analysis_declaration.weighting":["enum",["inventory_group"]],"$.arm_id":["enum",["base","candidate"]],"$.asymmetric_outcome_table":["object",["COMPLETED","ERROR_PROTOCOL","FAILED_CLOSED_NO_EGRESS","NOT_STARTED","UNKNOWN_EGRESS"]],"$.asymmetric_outcome_table.*":["object",["COMPLETED","ERROR_PROTOCOL","FAILED_CLOSED_NO_EGRESS","NOT_STARTED","UNKNOWN_EGRESS"]],"$.asymmetric_outcome_table.*.*":["int",null],"$.cell_id":["enum",["synthetic.evaluator.v1","synthetic.mcp.controlled.v1","synthetic.mcp.core.v1"]],"$.claim_scope":["enum",["synthetic_harness_capability_only"]],"$.class_commitment_table":["object",["id","version"]],"$.class_commitment_table.id":["enum",["class-commitments-v1"]],"$.class_commitment_table.version":["positive",null],"$.counts":["object",["egress_authorized_range_bounds_invalid","egress_authorized_range_non_monotonic","egress_clean_bounds_invalid","egress_overlapping_clean_spans","egress_raw_value_mismatches","egress_token_restore_failures","false_positive_bytes","false_positive_occurrences","gold_bytes_planned","gold_bytes_surviving_egress","gold_occurrences_attribution_not_measured","gold_occurrences_partially_surviving_egress","gold_occurrences_planned","gold_occurrences_surviving_egress","leaf_restore_decision_failures","leaf_restore_exact","manifest_raw_entry_agreement_enforced","manifest_span_monotonicity_enforced","manifest_terminal_events","observer_leaves_observed","observer_leaves_unobserved","observer_manifest_spans_observed","observer_recognizer_source_events","protected_leaves","protection_trace_items","tool_invocations","unknown_egress_lower_bound_cases"]],"$.counts.*":["int",null],"$.derivations":["object",["egress_authorized_range_bounds_invalid","egress_authorized_range_non_monotonic","egress_clean_bounds_invalid","egress_overlapping_clean_spans","egress_raw_value_mismatches","egress_token_restore_failures","false_positive_bytes","false_positive_occurrences","gold_bytes_planned","gold_bytes_surviving_egress","gold_occurrences_attribution_not_measured","gold_occurrences_partially_surviving_egress","gold_occurrences_planned","gold_occurrences_surviving_egress","leaf_restore_decision_failures","leaf_restore_exact","manifest_raw_entry_agreement_enforced","manifest_span_monotonicity_enforced","manifest_terminal_events","observer_leaves_observed","observer_leaves_unobserved","observer_manifest_spans_observed","observer_recognizer_source_events","protected_leaves","protection_trace_items","tool_invocations","unknown_egress_lower_bound_cases"]],"$.derivations.*":["enum",["egress_reconstructed","invariant_enforced_not_counted","not_applicable_by_construction","not_measured","observed_subset_lower_bound","observer_native","planned_inventory","private_authored_records","route_native"]],"$.error_codes":["object",["auth-denied","backend-failure","backend-unavailable","internal","invalid-args","invalid-session-id","limit-exceeded","manifest-persistence-failed","not-found","redaction-failed","response-serialization-failed"]],"$.error_codes.*":["int",null],"$.gate_results":["object",["build_attestation_clean_source","claim_scope_present","class_commitment_completeness","class_commitment_schema_load","cross_language_vocabulary_equality","declaration_gating","egress_integrity_analogues","false_positive_negative_control","gold_survival_oracle","local_membership_order_proof","manifest_integrity_six_counter_schema_v4","no_payload_positive_observation","outcome_identities","paired_grouped_interval_arithmetic","per_token_protection_trace","planned_inventory_reconciliation","producer_membership_order_proof","receipt_path_allowlist","rejection_is_not_protection","source_attribution_events","stamped_field_separation","string_byte_reversibility","unknown_egress_lower_bound","unknown_egress_route_fragment_observation","vocabulary_closure"]],"$.gate_results.*":["enum",["BLOCKED","FAIL","NOT_EVALUABLE","PASS"]],"$.intervals":["object",["egress_authorized_range_bounds_invalid","egress_authorized_range_non_monotonic","egress_clean_bounds_invalid","egress_overlapping_clean_spans","egress_raw_value_mismatches","egress_token_restore_failures","false_positive_bytes","false_positive_occurrences","gold_bytes_planned","gold_bytes_surviving_egress","gold_occurrences_attribution_not_measured","gold_occurrences_partially_surviving_egress","gold_occurrences_planned","gold_occurrences_surviving_egress","leaf_restore_decision_failures","leaf_restore_exact","manifest_raw_entry_agreement_enforced","manifest_span_monotonicity_enforced","manifest_terminal_events","observer_leaves_observed","observer_leaves_unobserved","observer_manifest_spans_observed","observer_recognizer_source_events","protected_leaves","protection_trace_items","tool_invocations","unknown_egress_lower_bound_cases"]],"$.intervals.*":["interval",["basis","conditional","high","low","method_id","point"]],"$.intervals.*.basis":["enum",["full_cell","paired_completed"]],"$.intervals.*.conditional":["bool",null],"$.intervals.*.high":["number",null],"$.intervals.*.low":["number",null],"$.intervals.*.method_id":["enum",["grouped_paired_percentile_v1"]],"$.intervals.*.point":["number",null],"$.not_measured":["object",["blocked_gates","metrics"]],"$.not_measured.blocked_gates":["array",null],"$.not_measured.blocked_gates.[]":["enum",["build_attestation_clean_source","claim_scope_present","class_commitment_completeness","class_commitment_schema_load","cross_language_vocabulary_equality","declaration_gating","egress_integrity_analogues","false_positive_negative_control","gold_survival_oracle","local_membership_order_proof","manifest_integrity_six_counter_schema_v4","no_payload_positive_observation","outcome_identities","paired_grouped_interval_arithmetic","per_token_protection_trace","planned_inventory_reconciliation","producer_membership_order_proof","receipt_path_allowlist","rejection_is_not_protection","source_attribution_events","stamped_field_separation","string_byte_reversibility","unknown_egress_lower_bound","unknown_egress_route_fragment_observation","vocabulary_closure"]],"$.not_measured.metrics":["array",null],"$.not_measured.metrics.[]":["enum",["egress_authorized_range_bounds_invalid","egress_authorized_range_non_monotonic","egress_clean_bounds_invalid","egress_overlapping_clean_spans","egress_raw_value_mismatches","egress_token_restore_failures","false_positive_bytes","false_positive_occurrences","gold_bytes_planned","gold_bytes_surviving_egress","gold_occurrences_attribution_not_measured","gold_occurrences_partially_surviving_egress","gold_occurrences_planned","gold_occurrences_surviving_egress","leaf_restore_decision_failures","leaf_restore_exact","manifest_raw_entry_agreement_enforced","manifest_span_monotonicity_enforced","manifest_terminal_events","observer_leaves_observed","observer_leaves_unobserved","observer_manifest_spans_observed","observer_recognizer_source_events","protected_leaves","protection_trace_items","tool_invocations","unknown_egress_lower_bound_cases"]],"$.outcomes":["object",["COMPLETED","ERROR_PROTOCOL","FAILED_CLOSED_NO_EGRESS","NOT_STARTED","UNKNOWN_EGRESS"]],"$.outcomes.*":["int",null],"$.planned_case_count":["int",null],"$.policy_identity":["enum",["authored.records.v1","controlled.email_only.v1","core.rule_floor.v1"]],"$.population_handle":["handle",null],"$.protocol_id":["enum",["gaze-evidence"]],"$.protocol_version":["version",null],"$.route_id":["enum",["daemon.jsonl.v1","evaluator.private.v1","mcp.rmcp.duplex.v1","ocr.document.v1","proxy.http.v1","session.episode.v1","stream.v1","structured.core.v1","text.clean_for_bench.v1"]],"$.route_status":["object",["daemon.jsonl.v1","evaluator.private.v1","mcp.rmcp.duplex.v1","ocr.document.v1","proxy.http.v1","session.episode.v1","stream.v1","structured.core.v1","text.clean_for_bench.v1"]],"$.route_status.*":["enum",["IMPLEMENTED","NOT_IMPLEMENTED"]]}"#;
pub(crate) fn walk(node: &Value, path: &str, rules: &Value) -> bool {
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
pub(crate) fn route_derivation(m: &str) -> &'static str {
    match m {
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
    }
}
pub(crate) fn allowed_derivation(route: &str, metric: &str, grade: &str) -> bool {
    if grade == "not_measured" {
        return true;
    }
    if route == "evaluator.private.v1" {
        match metric {
            "gold_occurrences_planned" | "gold_bytes_planned" => grade == "planned_inventory",
            "false_positive_occurrences"
            | "false_positive_bytes"
            | "unknown_egress_lower_bound_cases" => matches!(
                grade,
                "private_authored_records" | "observed_subset_lower_bound"
            ),
            m if LEAK_FAMILY_METRIC_IDS.contains(&m) => matches!(
                grade,
                "private_authored_records" | "observed_subset_lower_bound"
            ),
            _ => false,
        }
    } else {
        grade == route_derivation(metric)
    }
}
pub(crate) fn receipt_allowlisted(r: &Value) -> bool {
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
    let route = text(&r["route_id"]);
    let identity = (text(&r["cell_id"]), text(&r["policy_identity"]));
    if !match route {
        "evaluator.private.v1" => identity == ("synthetic.evaluator.v1", "authored.records.v1"),
        "mcp.rmcp.duplex.v1" => matches!(
            identity,
            ("synthetic.mcp.core.v1", "core.rule_floor.v1")
                | ("synthetic.mcp.controlled.v1", "controlled.email_only.v1")
        ),
        _ => false,
    } || ROUTE_IDS.iter().any(|id| {
        r["route_status"][*id]
            != if *id == route {
                "IMPLEMENTED"
            } else {
                "NOT_IMPLEMENTED"
            }
    }) {
        return false;
    }
    if r["intervals"].as_object().unwrap().iter().any(|(m, v)| {
        matches!(
            m.as_str(),
            "gold_occurrences_planned" | "gold_bytes_planned"
        ) && v != "NOT_EVALUABLE"
    }) {
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
    derivations
        .iter()
        .all(|(m, g)| allowed_derivation(route, m, text(g)))
        && expected_missing == actual_set("metrics")
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
pub(crate) fn receipt(c: &Counts, state: &str, controlled: bool) -> Value {
    let mut derivations = BTreeMap::new();
    let mut counts = BTreeMap::new();
    for &m in METRIC_IDS {
        let grade = route_derivation(m);
        let incomplete = match m {
            "egress_raw_value_mismatches" => c.1.get("raw_compared") != c.1.get("restore"),
            "false_positive_occurrences" | "false_positive_bytes" => {
                c.1.get("negative_compared") != c.1.get("negative")
            }
            _ => false,
        };
        let grade = if COUNTING_GRADES.contains(&grade) && (!c.0.contains_key(m) || incomplete) {
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
        c.1.get("raw_compared").copied().unwrap_or(0) != restores
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
        c.1.get("negative_compared") != c.1.get("negative")
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
