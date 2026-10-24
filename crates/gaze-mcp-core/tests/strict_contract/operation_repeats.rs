use super::*;
use gaze::{Candidate, ConflictTier, DetectContext, EvidenceKind, Recognizer};

struct Header(EvidenceKind);
impl Recognizer for Header {
    fn id(&self) -> &str {
        "header"
    }
    fn evidence(&self) -> EvidenceKind {
        self.0
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(
        &self,
        input: &str,
        _: &DetectContext<'_>,
    ) -> Result<Vec<Candidate>, gaze_types::DetectError> {
        Ok(regex::Regex::new(r"(?m)^From: ([^<\n]+?) <")
            .unwrap()
            .captures_iter(input)
            .map(|caps| {
                Candidate::new(
                    caps.get(1).unwrap().range(),
                    PiiClass::Name,
                    "header",
                    0.9,
                    0,
                    None,
                    "counter",
                    "header",
                    ConflictTier::None,
                    vec![],
                )
            })
            .collect())
    }
}
fn repeat_pipeline(evidence: EvidenceKind) -> gaze::Pipeline {
    gaze::Pipeline::builder()
        .recognizer(Header(evidence))
        .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .unwrap()
}
struct Capture {
    descriptor: ToolDescriptor,
    seen: Arc<Mutex<Value>>,
    output: Value,
}
#[async_trait]
impl Tool for Capture {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }
    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        *self.seen.lock().unwrap() = ctx.redacted_args().clone();
        Ok(ToolResponse::json(self.output.clone()))
    }
}
fn capture(output: Value) -> (ToolRegistry, Arc<Mutex<Value>>) {
    let seen = Arc::new(Mutex::new(Value::Null));
    let mut registry = ToolRegistry::new();
    registry
        .register(Capture {
            descriptor: ToolDescriptor::agent("test", json!({})).with_carriers(
                CarrierDeclaration::new(
                    vec![vec![
                        CarrierSegment::AnyIndex,
                        CarrierSegment::Member("later".into()),
                    ]],
                    vec![],
                ),
                CarrierDeclaration::new(
                    vec![vec![
                        CarrierSegment::AnyIndex,
                        CarrierSegment::Member("later".into()),
                    ]],
                    vec![],
                ),
            ),
            seen: seen.clone(),
            output,
        })
        .unwrap();
    (registry, seen)
}
fn source(name: &str) -> Value {
    json!(format!("From: {name} <alice@example.invalid>"))
}
fn restore(session: &Session, value: &Value) -> Value {
    match value {
        Value::String(text) => json!(session.restore_strict_text(text).unwrap()),
        Value::Array(values) => Value::Array(values.iter().map(|v| restore(session, v)).collect()),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(k, v)| (k.clone(), restore(session, v)))
                .collect(),
        ),
        _ => value.clone(),
    }
}
#[tokio::test]
async fn earlier_argument_repeat_is_protected_before_tool_invocation() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let store = Store::default();
    let (registry, seen) = capture(json!("public response"));
    let args = json!(["hello maria schneider", {"later": source("Maria Schneider")}]);
    dispatch(
        &registry,
        &store,
        &repeat_pipeline(EvidenceKind::Rule),
        &session,
        args.clone(),
    )
    .await
    .unwrap();
    let protected = seen.lock().unwrap().clone();
    assert_eq!(restore(&session, &protected), args);
    assert!(
        !protected[0].as_str().unwrap().contains("maria schneider"),
        "raw earlier argument reached tool"
    );
}
#[tokio::test]
async fn earlier_response_repeat_is_protected_before_host_release() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let store = Store::default();
    let output = json!(["hello maria schneider", {"later": source("Maria Schneider")}]);
    let (registry, _) = capture(output.clone());
    let result = dispatch(
        &registry,
        &store,
        &repeat_pipeline(EvidenceKind::Rule),
        &session,
        json!("public request"),
    )
    .await
    .unwrap();
    assert_eq!(restore(&session, &result.payload), output);
    assert!(
        !result.payload[0]
            .as_str()
            .unwrap()
            .contains("maria schneider"),
        "raw earlier response reached host"
    );
}

async fn seed(pipeline: &gaze::Pipeline, session: &Session, names: &[&str]) -> Vec<String> {
    let store = Store::default();
    let (registry, seen) = capture(json!("public response"));
    dispatch(
        &registry,
        &store,
        pipeline,
        session,
        json!(names.iter().map(|name| source(name)).collect::<Vec<_>>()),
    )
    .await
    .unwrap();
    let value = seen.lock().unwrap().clone();
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            let text = item.as_str().unwrap();
            text.strip_prefix("From: ")
                .unwrap()
                .split(" <alice")
                .next()
                .unwrap()
                .to_owned()
        })
        .collect()
}

#[tokio::test]
async fn all_overlapping_rule_obligations_cross_owned_tokens_in_both_directions() {
    let pipeline = repeat_pipeline(EvidenceKind::Rule);
    for response in [false, true] {
        for source_first in [false, true] {
            for (seed_names, input, names) in [
                (
                    vec!["Maria Schneider"],
                    "hello $0 johanna",
                    vec!["Schneider Johanna"],
                ),
                (vec!["Maria Schneider"], "hello eva $0", vec!["Eva Maria"]),
                (
                    vec!["Maria Schneider", "Johanna Fischer"],
                    "hello $0 und $1",
                    vec!["Schneider Und Johanna"],
                ),
                (
                    vec!["Maria Schneider", "Julia Weber"],
                    "hello $0 johanna $1 paula",
                    vec!["Schneider Johanna", "Johanna Julia", "Weber Paula"],
                ),
                (
                    vec!["Mária Schneider"],
                    "hello $0 johanna",
                    vec!["Schneider Johanna"],
                ),
            ] {
                let session = Session::new(Scope::Ephemeral).unwrap();
                let tokens = seed(&pipeline, &session, &seed_names).await;
                let mut text = input.to_owned();
                for (index, token) in tokens.iter().enumerate() {
                    text = text.replace(&format!("${index}"), token);
                }
                let sources = json!(names.iter().map(|name| source(name)).collect::<Vec<_>>());
                let original = if source_first {
                    json!([sources, text])
                } else {
                    json!([text, sources])
                };
                let expected = restore(&session, &original);
                let (registry, seen) = capture(if response {
                    original.clone()
                } else {
                    json!("public response")
                });
                let result = dispatch(
                    &registry,
                    &Store::default(),
                    &pipeline,
                    &session,
                    if response {
                        json!("public request")
                    } else {
                        original.clone()
                    },
                )
                .await
                .unwrap();
                let protected = if response {
                    result.payload
                } else {
                    seen.lock().unwrap().clone()
                };
                assert_eq!(restore(&session, &protected), expected);
                let text = protected[if source_first { 1 } else { 0 }]
                    .as_str()
                    .unwrap();
                for token in tokens {
                    assert!(text.contains(&token), "owned token changed");
                }
                for raw in ["johanna", "eva", "und", "paula"] {
                    if input.contains(raw) {
                        assert!(!text.contains(raw), "raw overlap reached output");
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn learned_sources_do_not_propagate_and_unrelated_siblings_stay_public() {
    for evidence in [EvidenceKind::Rule, EvidenceKind::Learned] {
        let pipeline = repeat_pipeline(evidence);
        let session = Session::new(Scope::Ephemeral).unwrap();
        let args = json!([
            "hello maria schneider",
            "public counter",
            "let item_count = 17;",
            source("Maria Schneider")
        ]);
        let (registry, seen) = capture(args.clone());
        let result = dispatch(
            &registry,
            &Store::default(),
            &pipeline,
            &session,
            args.clone(),
        )
        .await
        .unwrap();
        for protected in [seen.lock().unwrap().clone(), result.payload] {
            assert_eq!(restore(&session, &protected), args);
            assert_eq!(
                protected[0].as_str().unwrap().contains("maria schneider"),
                evidence == EvidenceKind::Learned
            );
            assert_eq!(protected[1], args[1]);
            assert_eq!(protected[2], args[2]);
        }
    }
}

#[tokio::test]
async fn other_namespaces_stay_literal_while_current_session_tokens_expand() {
    let pipeline = repeat_pipeline(EvidenceKind::Rule);
    let owner = Session::new(Scope::Ephemeral).unwrap();
    let foreign = seed(&pipeline, &owner, &["Maria Schneider"])
        .await
        .remove(0);
    for token in [
        "<deadbeef:Name_1>".to_owned(),
        "<deadbeef:Name_>".to_owned(),
        foreign,
    ] {
        for response in [false, true] {
            let session = Session::new(Scope::Ephemeral).unwrap();
            let owned = seed(&pipeline, &session, &["Julia Weber"]).await;
            let store = Store::default();
            let payload = json!([
                token,
                "hello maria schneider",
                format!("hello {} brenner", owned[0]),
                source("Julia Weber Brenner")
            ]);
            let (registry, seen) = capture(if response {
                payload.clone()
            } else {
                json!("public")
            });
            let out = dispatch(
                &registry,
                &store,
                &pipeline,
                &session,
                if response {
                    json!("public")
                } else {
                    payload.clone()
                },
            )
            .await
            .unwrap();
            let protected = if response {
                out.payload
            } else {
                seen.lock().unwrap().clone()
            };
            assert_eq!(protected[0], payload[0]);
            assert_eq!(
                protected[1], payload[1],
                "other namespace seeded a Rule source"
            );
            assert!(protected[2].as_str().unwrap().contains(&owned[0]));
            assert!(!protected[2].as_str().unwrap().contains("brenner"));
            assert_eq!(
                session
                    .restore_strict_text(protected[2].as_str().unwrap())
                    .unwrap(),
                "hello Julia Weber brenner"
            );
            assert_eq!(*store.events.lock().unwrap(), vec!["begin", "finish"]);
        }
    }
}

#[tokio::test]
async fn expanded_operation_budget_is_aggregate_and_rolls_back() {
    let pipeline = repeat_pipeline(EvidenceKind::Rule);
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (registry, seen) = capture(json!("public"));
    let store = Store::default();
    let limit = 2 * 1024 * 1024;
    dispatch(
        &registry,
        &store,
        &pipeline,
        &session,
        json!(["x".repeat(limit / 2), "y".repeat(limit / 2)]),
    )
    .await
    .unwrap();
    let before = session.snapshot_entries();
    let payload = json!([
        source("Maria Schneider"),
        "x".repeat(limit / 2),
        "y".repeat(limit / 2)
    ]);
    *seen.lock().unwrap() = Value::Null;
    assert!(dispatch(&registry, &store, &pipeline, &session, payload)
        .await
        .is_err());
    assert_eq!(*seen.lock().unwrap(), Value::Null);
    assert_eq!(session.snapshot_entries(), before);
}

struct HeaderNet;

impl gaze::SafetyNet for HeaderNet {
    fn id(&self) -> &str {
        "synthetic-header-net"
    }
    fn supported_locales(&self) -> &[gaze::LocaleTag] {
        &[gaze::LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        _: gaze::SafetyNetContext<'_>,
    ) -> Result<Vec<gaze::LeakSuspect>, gaze::SafetyNetError> {
        let pattern = regex::Regex::new(r"(?m)^From: ([^<\n]+?) <").unwrap();
        Ok(pattern
            .captures_iter(text)
            .map(|caps| {
                gaze::LeakSuspect::new(
                    caps.get(1).unwrap().range(),
                    PiiClass::Name,
                    self.id(),
                    Some(0.95),
                    gaze::LeakKind::Uncovered,
                    "synthetic header",
                    None,
                )
            })
            .collect())
    }
}

#[tokio::test]
async fn safety_net_only_entries_never_seed_operation_rule_discovery() {
    let pipeline = gaze::Pipeline::builder()
        .detector(Primary)
        .register_safety_net(HeaderNet)
        .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let dictionaries = gaze::DictionaryBundle::default();
    let owned = pipeline
        .resolve_boundary_text(
            &session,
            "From: Maria Schneider <alice@example.invalid>",
            &[gaze::LocaleTag::Global],
            &dictionaries,
        )
        .unwrap();
    let original = json!(["hello maria schneider", owned]);
    let (registry, seen) = capture(original.clone());
    let result = dispatch(
        &registry,
        &Store::default(),
        &pipeline,
        &session,
        original.clone(),
    )
    .await
    .unwrap();
    assert_eq!(*seen.lock().unwrap(), original);
    assert_eq!(result.payload, original);
}

#[tokio::test]
async fn owned_token_expansion_counts_against_the_operation_budget() {
    let pipeline = repeat_pipeline(EvidenceKind::Rule);
    let session = Session::new(Scope::Ephemeral).unwrap();
    let token = session
        .tokenize(&PiiClass::Name, &"x".repeat(256 * 1024))
        .unwrap();
    let before = session.snapshot_entries();
    let (registry, seen) = capture(json!("public"));
    assert!(dispatch(
        &registry,
        &Store::default(),
        &pipeline,
        &session,
        json!([token.repeat(9)])
    )
    .await
    .is_err());
    assert_eq!(*seen.lock().unwrap(), Value::Null);
    assert_eq!(session.snapshot_entries(), before);
}

struct FaultHeader {
    calls: Arc<std::sync::atomic::AtomicUsize>,
    fail_at: Option<usize>,
}
impl Recognizer for FaultHeader {
    fn id(&self) -> &str {
        "header"
    }
    fn evidence(&self) -> EvidenceKind {
        EvidenceKind::Rule
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(
        &self,
        input: &str,
        context: &DetectContext<'_>,
    ) -> Result<Vec<Candidate>, gaze_types::DetectError> {
        let call = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.fail_at == Some(call) {
            return Err(gaze_types::DetectError::backend(
                self.id(),
                "synthetic private backend detail",
            ));
        }
        Header(EvidenceKind::Rule).detect(input, context)
    }
}
#[tokio::test]
async fn every_observed_rule_failure_refuses_without_partial_operation_state() {
    for response in [false, true] {
        let mut observed_calls = 0;
        for fail_at in std::iter::once(None).chain((0..32).map(Some)) {
            if fail_at.is_some_and(|index| index >= observed_calls) {
                break;
            }
            let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let pipeline = gaze::Pipeline::builder()
                .recognizer(FaultHeader {
                    calls: calls.clone(),
                    fail_at,
                })
                .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
                .rule(DefaultRule::new(Action::Preserve))
                .build()
                .unwrap();
            let session = Session::new(Scope::Ephemeral).unwrap();
            session.tokenize(&PiiClass::Name, "synthetic seed").unwrap();
            let before = session.snapshot_entries();
            let cache = session.prefix_cache_entry_count();
            let payload = json!(["hello maria schneider", source("Maria Schneider")]);
            let (registry, seen) = capture(if response { payload.clone() } else { json!([]) });
            let store = Store::default();
            let result = dispatch(
                &registry,
                &store,
                &pipeline,
                &session,
                if response { json!([]) } else { payload },
            )
            .await;
            if fail_at.is_none() {
                result.unwrap();
                observed_calls = calls.load(std::sync::atomic::Ordering::SeqCst);
                assert!((1..=32).contains(&observed_calls));
            } else {
                let error = result.unwrap_err();
                assert!(!error
                    .to_string()
                    .contains("synthetic private backend detail"));
                assert_eq!(session.snapshot_entries(), before);
                assert_eq!(session.prefix_cache_entry_count(), cache);
                if !response {
                    assert_eq!(*seen.lock().unwrap(), Value::Null);
                }
                assert_eq!(
                    *store.events.lock().unwrap(),
                    if response {
                        vec!["begin", "fail"]
                    } else {
                        vec![]
                    }
                );
            }
        }
    }
}

#[tokio::test]
async fn shrinking_owned_values_do_not_bypass_the_input_text_budget() {
    let pipeline = repeat_pipeline(EvidenceKind::Rule);
    let session = Session::new(Scope::Ephemeral).unwrap();
    let token = session.tokenize(&PiiClass::Name, "").unwrap();
    let before = session.snapshot_entries();
    let (registry, seen) = capture(json!([]));
    let payload = json!([token.repeat((2 * 1024 * 1024) / token.len() + 1)]);
    assert!(
        dispatch(&registry, &Store::default(), &pipeline, &session, payload)
            .await
            .is_err()
    );
    assert_eq!(*seen.lock().unwrap(), Value::Null);
    assert_eq!(session.snapshot_entries(), before);
}

#[tokio::test]
async fn overlapping_rule_values_preserve_owned_tokens_from_different_classes() {
    let pipeline = repeat_pipeline(EvidenceKind::Rule);
    for response in [false, true] {
        for source_first in [false, true] {
            for email_value in ["Johanna", "m@example.invalid"] {
                let session = Session::new(Scope::Ephemeral).unwrap();
                let name = session.tokenize(&PiiClass::Name, "Maria").unwrap();
                let email = session.tokenize(&PiiClass::Email, email_value).unwrap();
                let copy = format!("hello {name} schneider {email} brenner");
                let declaration = source(&format!("Maria Schneider {email_value} Brenner"));
                let payload = if source_first {
                    json!([declaration, copy])
                } else {
                    json!([copy, declaration])
                };
                let expected = restore(&session, &payload);
                let (registry, seen) = capture(if response { payload.clone() } else { json!([]) });
                let result = dispatch(
                    &registry,
                    &Store::default(),
                    &pipeline,
                    &session,
                    if response { json!([]) } else { payload },
                )
                .await
                .unwrap();
                let protected = if response {
                    result.payload
                } else {
                    seen.lock().unwrap().clone()
                };
                assert_eq!(restore(&session, &protected), expected);
                let copy = protected[if source_first { 1 } else { 0 }]
                    .as_str()
                    .unwrap();
                assert!(copy.contains(&name));
                assert!(copy.contains(&email));
                assert!(!copy.contains("schneider"));
                assert!(!copy.contains("brenner"));
            }
        }
    }
}

#[tokio::test]
async fn bare_filenames_survive_operation_discovery_and_rule_proof() {
    for response in [false, true] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let pipeline = repeat_pipeline(EvidenceKind::Rule);
        let payload = json!([
            "scan_1.png Scan_1.png invoice_20250111.pdf doc_v2.png",
            "hello maria schneider",
            {"later": source("Maria Schneider")}
        ]);
        let (registry, seen) = capture(if response {
            payload.clone()
        } else {
            json!("public")
        });
        let output = dispatch(
            &registry,
            &Store::default(),
            &pipeline,
            &session,
            if response {
                json!("public")
            } else {
                payload.clone()
            },
        )
        .await
        .unwrap();
        let protected = if response {
            output.payload
        } else {
            seen.lock().unwrap().clone()
        };
        assert_eq!(protected[0], payload[0]);
        assert!(!protected[1].as_str().unwrap().contains("maria schneider"));
        assert_eq!(restore(&session, &protected), payload);
    }
}

#[tokio::test]
async fn rule_coverage_crosses_literal_bare_identifier_boundaries() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let payload =
        json!(["hello maria scan_1 schneider", {"later": source("Maria scan_1 Schneider")}]);
    let (registry, seen) = capture(json!("public"));
    dispatch(
        &registry,
        &Store::default(),
        &repeat_pipeline(EvidenceKind::Rule),
        &session,
        payload.clone(),
    )
    .await
    .unwrap();
    let protected = seen.lock().unwrap().clone();
    assert!(!protected[0]
        .as_str()
        .unwrap()
        .contains("maria scan_1 schneider"));
    assert_eq!(restore(&session, &protected), payload);
}
