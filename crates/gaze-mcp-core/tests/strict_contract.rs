//! Synthetic producer-to-envelope contract, not direct Tool::invoke tests.
use async_trait::async_trait;
use gaze::{Action, ClassRule, DefaultRule, Detection, Detector, PiiClass, Scope, Session};
use gaze_mcp_core::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
const EMAIL: &str = "alice@example.invalid";
const FRESH: &str = "bob@example.invalid";
#[derive(Clone)]
struct Primary;
impl Detector for Primary {
    fn detect(&self, input: &str) -> Vec<Detection> {
        [EMAIL, FRESH]
            .into_iter()
            .flat_map(|raw| {
                input.match_indices(raw).map(move |(start, _)| {
                    Detection::new(start..start + raw.len(), PiiClass::Email, "synthetic")
                })
            })
            .collect()
    }
}
fn pipeline() -> gaze::Pipeline {
    gaze::Pipeline::builder()
        .detector(Primary)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .unwrap()
}
struct Auth;
#[async_trait]
impl AuthHook for Auth {
    async fn authorize_agent(&self, _: &Principal, _: &str) -> Result<(), AuthError> {
        Ok(())
    }
    async fn authorize_operator(&self, principal: &Principal, _: &str) -> Result<(), AuthError> {
        if principal.id == "operator" {
            Ok(())
        } else {
            Err(AuthError::Denied("synthetic".into()))
        }
    }
}
#[derive(Default)]
struct Store {
    events: Arc<Mutex<Vec<&'static str>>>,
    fail: Option<&'static str>,
    mutate_begin: Option<Arc<Session>>,
}
#[async_trait]
impl ManifestStore for Store {
    async fn begin_call(&self, ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
        self.events.lock().unwrap().push("begin");
        if let Some(session) = &self.mutate_begin {
            session
                .tokenize(&PiiClass::Name, "synthetic concurrent")
                .unwrap();
        }
        if self.fail == Some("begin") {
            return Err(ManifestError::Validation("synthetic".into()));
        }
        Ok(CallHandle::new(ctx.call_id))
    }
    async fn finish_call(&self, _: CallHandle, _: SnapshotRef) -> Result<(), ManifestError> {
        self.events.lock().unwrap().push("finish");
        if self.fail == Some("finish") {
            return Err(ManifestError::Validation("synthetic".into()));
        }
        Ok(())
    }
    async fn fail_call(&self, _: CallHandle, _: FailureReason) -> Result<(), ManifestError> {
        self.events.lock().unwrap().push("fail");
        if self.fail == Some("fail") {
            return Err(ManifestError::Validation("synthetic".into()));
        }
        Ok(())
    }
}
struct Producer {
    descriptor: ToolDescriptor,
    events: Arc<Mutex<Vec<&'static str>>>,
    output: Option<Value>,
    error: bool,
    mutate: bool,
}
#[async_trait]
impl Tool for Producer {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }
    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        self.events.lock().unwrap().push("invoke");
        if self.mutate {
            ctx.resources()
                .session()
                .tokenize(
                    &PiiClass::Custom("class_alpha".into()),
                    "synthetic tool value",
                )
                .unwrap();
        }
        if self.error {
            return Err(ToolError::BackendFailure(EMAIL.into()));
        }
        Ok(ToolResponse::json(
            self.output
                .clone()
                .unwrap_or_else(|| ctx.redacted_args().clone()),
        ))
    }
}
fn registry(
    store: &Store,
    declaration: CarrierDeclaration,
    output: Option<Value>,
    error: bool,
    mutate: bool,
) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry
        .register(Producer {
            descriptor: ToolDescriptor::agent("test", json!({}))
                .with_carriers(declaration.clone(), declaration),
            events: store.events.clone(),
            output,
            error,
            mutate,
        })
        .unwrap();
    registry
}
async fn dispatch(
    registry: &ToolRegistry,
    store: &Store,
    pipeline: &gaze::Pipeline,
    session: &Session,
    args: Value,
) -> Result<ToolResponse, DispatchError> {
    PiiEnvelope::new(
        registry,
        &Auth,
        store,
        pipeline,
        session,
        &[gaze::LocaleTag::Global],
        &SessionIdPolicy::default_strict(),
    )
    .dispatch(&Principal::new("agent"), "test", args, None)
    .await
}
#[tokio::test]
async fn whole_operation_collision_is_rejected_without_begin_or_cache_state() {
    let store = Store::default();
    let registry = registry(&store, Default::default(), None, false, false);
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut prediction = session.begin_transaction();
    let token = prediction.tokenize(&PiiClass::Email, EMAIL).unwrap();
    drop(prediction);
    for args in [json!([EMAIL, token]), json!([token, EMAIL])] {
        assert!(matches!(
            dispatch(&registry, &store, &pipeline(), &session, args).await,
            Err(DispatchError::Protection(gaze::ProtectionError::Provenance))
        ));
        assert!(store.events.lock().unwrap().is_empty());
        assert!(session.tokens().is_empty());
        assert_eq!(session.prefix_cache_entry_count(), 0);
    }
}
#[tokio::test]
async fn terminal_attempts_are_once_only_and_response_commit_precedes_failed_finish() {
    for (failure, tool_error, expected) in [
        ("begin", false, vec!["begin"]),
        ("finish", false, vec!["begin", "invoke", "finish"]),
        ("fail", true, vec!["begin", "invoke", "fail"]),
    ] {
        let store = Store {
            fail: Some(failure),
            ..Default::default()
        };
        let registry = registry(
            &store,
            Default::default(),
            Some(json!(FRESH)),
            tool_error,
            false,
        );
        let session = Session::new(Scope::Ephemeral).unwrap();
        assert!(matches!(
            dispatch(&registry, &store, &pipeline(), &session, json!(EMAIL)).await,
            Err(DispatchError::Manifest(_))
        ));
        assert_eq!(*store.events.lock().unwrap(), expected);
        let entries = session.snapshot_entries();
        assert_eq!(
            entries.iter().any(|entry| entry.raw == EMAIL),
            failure != "begin"
        );
        assert_eq!(
            entries.iter().any(|entry| entry.raw == FRESH),
            failure == "finish"
        );
    }
}
#[tokio::test]
async fn begin_commit_conflict_prevents_invoke_and_keeps_only_winning_state() {
    let session = Arc::new(Session::new(Scope::Ephemeral).unwrap());
    let store = Store {
        mutate_begin: Some(session.clone()),
        ..Default::default()
    };
    let registry = registry(&store, Default::default(), None, false, false);
    assert!(matches!(
        dispatch(&registry, &store, &pipeline(), &session, json!(EMAIL)).await,
        Err(DispatchError::Transaction(_))
    ));
    assert_eq!(*store.events.lock().unwrap(), ["begin", "fail"]);
    assert!(session
        .snapshot_entries()
        .iter()
        .all(|entry| entry.raw != EMAIL));
}
#[tokio::test]
async fn response_later_leaf_failure_rolls_back_response_only() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut prediction = session.begin_transaction();
    prediction.tokenize(&PiiClass::Email, EMAIL).unwrap();
    let token = prediction.tokenize(&PiiClass::Email, FRESH).unwrap();
    drop(prediction);
    let store = Store::default();
    let registry = registry(
        &store,
        Default::default(),
        Some(json!([FRESH, token])),
        false,
        true,
    );
    assert!(matches!(
        dispatch(&registry, &store, &pipeline(), &session, json!(EMAIL)).await,
        Err(DispatchError::Protection(gaze::ProtectionError::Provenance))
    ));
    assert_eq!(*store.events.lock().unwrap(), ["begin", "invoke", "fail"]);
    let entries = session.snapshot_entries();
    assert!(entries.iter().any(|entry| entry.raw == EMAIL));
    assert!(entries
        .iter()
        .any(|entry| entry.raw == "synthetic tool value"));
    assert!(!entries.iter().any(|entry| entry.raw == FRESH));
}
#[tokio::test]
async fn exact_typed_paths_and_numbers_preserve_json_without_schema_authority() {
    use CarrierSegment::{AnyIndex as I, Member as M};
    let keys = ["*", "/", "~", ".", "123"];
    let mut members = vec![vec![M("items".into())]];
    let mut numbers = vec![];
    let mut object = serde_json::Map::new();
    for (key, number) in keys.into_iter().zip([
        json!(u64::MAX),
        json!(i64::MIN),
        json!(1.25),
        json!(-0.0),
        json!(0),
    ]) {
        let path = vec![M("items".into()), I, M(key.into())];
        members.push(path.clone());
        numbers.push(path);
        object.insert(key.into(), number);
    }
    let store = Store::default();
    let registry = registry(
        &store,
        CarrierDeclaration::new(members, numbers),
        None,
        false,
        false,
    );
    let input = json!({"items":[Value::Object(object)]});
    let session = Session::new(Scope::Ephemeral).unwrap();
    let result = dispatch(&registry, &store, &pipeline(), &session, input.clone())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_string(&result.payload).unwrap(),
        serde_json::to_string(&input).unwrap()
    );
    for rejected in [
        json!({"items":{"*":42}}),
        json!({"items":[{"*":{"nested":EMAIL}}]}),
        json!({"alice@example.invalid":EMAIL}),
        json!(42),
    ] {
        assert!(matches!(
            dispatch(&registry, &store, &pipeline(), &session, rejected).await,
            Err(DispatchError::Carrier(_))
        ));
    }
    let descriptor =
        ToolDescriptor::agent("schema", json!({"properties":{"text":{"type":"string"}}}))
            .with_carriers(
                CarrierDeclaration::text_fields(&["text"]),
                Default::default(),
            );
    let decoded: ToolDescriptor =
        serde_json::from_value(serde_json::to_value(descriptor).unwrap()).unwrap();
    let mut registry = ToolRegistry::new();
    registry
        .register(Producer {
            descriptor: decoded,
            events: store.events.clone(),
            output: None,
            error: false,
            mutate: false,
        })
        .unwrap();
    let result = PiiEnvelope::new(
        &registry,
        &Auth,
        &store,
        &pipeline(),
        &session,
        &[gaze::LocaleTag::Global],
        &SessionIdPolicy::default_strict(),
    )
    .dispatch(
        &Principal::new("agent"),
        "schema",
        json!({"text":EMAIL}),
        None,
    )
    .await;
    assert!(matches!(result, Err(DispatchError::Carrier(_))));
}
#[test]
fn invalid_declaration_paths_and_duplicates_fail_registration() {
    for declaration in [
        CarrierDeclaration::new(vec![vec![]], vec![]),
        CarrierDeclaration::new(vec![vec![CarrierSegment::AnyIndex]], vec![]),
        CarrierDeclaration::text_fields(&["text", "text"]),
        CarrierDeclaration::new(vec![], vec![vec![], vec![]]),
    ] {
        let mut registry = ToolRegistry::new();
        assert!(registry
            .register(Producer {
                descriptor: ToolDescriptor::agent("test", json!({}))
                    .with_carriers(declaration, Default::default()),
                events: Default::default(),
                output: None,
                error: false,
                mutate: false
            })
            .is_err());
    }
}

#[tokio::test]
async fn empty_primary_graph_rejects_even_operations_without_string_leaves() {
    let store = Store::default();
    let registry = registry(&store, Default::default(), None, false, false);
    let session = Session::new(Scope::Ephemeral).unwrap();
    let empty = gaze::Pipeline::builder().build().unwrap();
    for args in [json!({}), json!(true), json!([]), json!([null, false])] {
        assert!(matches!(
            dispatch(&registry, &store, &empty, &session, args).await,
            Err(DispatchError::Protection(
                gaze::ProtectionError::EmptyPrimary
            ))
        ));
    }
    assert!(store.events.lock().unwrap().is_empty());
}

#[cfg(feature = "core-tools")]
#[tokio::test]
async fn real_primary_builtins_succeed_and_operator_bypass_requires_authorization() {
    let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
    let store = Store::default();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut registry = ToolRegistry::new();
    registry.register(core_tools::CleanTool::new()).unwrap();
    registry
        .register(core_tools::TokenizeFieldTool::new())
        .unwrap();
    registry
        .register(core_tools::SafetyNetCheckTool::new())
        .unwrap();
    let policy = SessionIdPolicy::default_strict();
    let env = PiiEnvelope::new(
        &registry,
        &Auth,
        &store,
        core.pipeline(),
        &session,
        core.locale_chain().as_slice(),
        &policy,
    );
    for (name, args, field) in [
        ("clean", json!({"text":EMAIL}), "text"),
        ("tokenize_field", json!({"value":FRESH}), "token"),
    ] {
        let result = env
            .dispatch(&Principal::new("agent"), name, args, None)
            .await
            .unwrap();
        let text = result.payload[field].as_str().unwrap();
        assert!(!text.contains("@example.invalid"));
        assert!(session.contains_token(text));
    }
    let observation = env
        .dispatch(
            &Principal::new("agent"),
            "safety_net_check",
            json!({"text":"benign"}),
            None,
        )
        .await
        .unwrap();
    assert_eq!(observation.payload["ok"], "unconfigured");
    assert_eq!(observation.payload["nets_run"], 0);
    assert_eq!(store.events.lock().unwrap().len(), 6);
    let raw = json!({"alice@example.invalid":{"raw":EMAIL,"number":123456789}});
    let descriptor = ToolDescriptor::operator("raw", json!({}))
        .with_response_redaction(ResponseRedaction::BypassByOperator);
    registry
        .register(Producer {
            descriptor,
            events: store.events.clone(),
            output: Some(raw.clone()),
            error: false,
            mutate: false,
        })
        .unwrap();
    let env = PiiEnvelope::new(
        &registry,
        &Auth,
        &store,
        core.pipeline(),
        &session,
        core.locale_chain().as_slice(),
        &policy,
    );
    let before = store.events.lock().unwrap().len();
    assert!(matches!(
        env.dispatch(&Principal::new("agent"), "raw", json!({}), None)
            .await,
        Err(DispatchError::Auth(_))
    ));
    assert_eq!(store.events.lock().unwrap().len(), before);
    let result = env
        .dispatch(&Principal::new("operator"), "raw", json!({}), None)
        .await
        .unwrap();
    assert_eq!(result.payload, raw);
    assert!(registry
        .register(Producer {
            descriptor: ToolDescriptor::agent("bad", json!({}))
                .with_response_redaction(ResponseRedaction::BypassByOperator),
            events: Default::default(),
            output: None,
            error: false,
            mutate: false
        })
        .is_err());
}

#[tokio::test]
async fn supplied_locale_and_dictionary_reach_both_boundaries_and_tool_observers() {
    use gaze::{
        DictionaryBundle, DictionaryEntry, DictionarySource, LeakSuspect, LocaleTag,
        ProtectionContext, SafetyNet, SafetyNetContext, SafetyNetError,
    };
    use gaze_types::{Candidate, ConflictTier, DetectContext, DetectError, Recognizer};
    struct DictPrimary;
    impl Recognizer for DictPrimary {
        fn token_family(&self) -> &str {
            "name"
        }
        fn id(&self) -> &str {
            "dict-spy"
        }
        fn supported_class(&self) -> &PiiClass {
            &PiiClass::Name
        }
        fn detect(
            &self,
            input: &str,
            ctx: &DetectContext<'_>,
        ) -> Result<Vec<Candidate>, DetectError> {
            if ctx.locale_chain.first() != Some(&LocaleTag::DeDe) {
                return Ok(vec![]);
            }
            let Some(dictionary) = ctx.dictionaries.get("dict_alpha") else {
                return Ok(vec![]);
            };
            Ok(dictionary
                .terms()
                .iter()
                .flat_map(|term| {
                    input.match_indices(term).map(move |(start, _)| {
                        Candidate::new(
                            start..start + term.len(),
                            PiiClass::Name,
                            "dict-spy",
                            1.0,
                            0,
                            None,
                            "name",
                            "dict-spy",
                            ConflictTier::None,
                            vec![],
                        )
                    })
                })
                .collect())
        }
    }
    struct DictNet(Arc<Mutex<Vec<Option<String>>>>);
    impl SafetyNet for DictNet {
        fn id(&self) -> &str {
            "dict-net"
        }
        fn supported_locales(&self) -> &[LocaleTag] {
            &[LocaleTag::DeDe]
        }
        fn check(
            &self,
            _: &str,
            context: SafetyNetContext<'_>,
        ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
            assert_eq!(context.locale_chain.first(), Some(&LocaleTag::DeDe));
            assert_eq!(
                context
                    .dictionaries
                    .unwrap()
                    .get("dict_alpha")
                    .unwrap()
                    .terms(),
                ["Synthetic Alpha", "Synthetic Beta"]
            );
            self.0
                .lock()
                .unwrap()
                .push(context.field_path.map(str::to_string));
            Ok(vec![])
        }
    }
    struct Observer {
        descriptor: ToolDescriptor,
    }
    #[async_trait]
    impl Tool for Observer {
        fn descriptor(&self) -> &ToolDescriptor {
            &self.descriptor
        }
        async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
            assert!(!ctx
                .redacted_args()
                .as_str()
                .unwrap()
                .contains("Synthetic Alpha"));
            let resources = ctx.resources();
            let context: ProtectionContext<'_> = resources.protection_context();
            resources
                .pipeline()
                .scan_safety_nets_with_dictionaries(
                    resources.session(),
                    "benign",
                    context.locale_chain(),
                    context.dictionaries(),
                )
                .unwrap();
            resources
                .pipeline()
                .scan_safety_nets_structured_with_dictionaries(
                    resources.session(),
                    &std::collections::BTreeMap::from([(
                        "field".into(),
                        gaze::Value::String("benign".into()),
                    )]),
                    context.locale_chain(),
                    context.dictionaries(),
                )
                .unwrap();
            Ok(ToolResponse::text("Synthetic Beta"))
        }
    }
    let seen = Arc::new(Mutex::new(vec![]));
    let pipeline = gaze::Pipeline::builder()
        .recognizer(DictPrimary)
        .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
        .register_safety_net(DictNet(seen.clone()))
        .build()
        .unwrap();
    let dictionary = DictionaryBundle::from_entries([(
        "dict_alpha".into(),
        DictionaryEntry::new(
            "dict_alpha",
            vec!["Synthetic Alpha".into(), "Synthetic Beta".into()],
            true,
            DictionarySource::Cli,
        )
        .unwrap(),
    )]);
    let session = Session::new(Scope::Ephemeral).unwrap();
    let store = Store::default();
    let mut registry = ToolRegistry::new();
    registry
        .register(Observer {
            descriptor: ToolDescriptor::agent("observe", json!({})),
        })
        .unwrap();
    let policy = SessionIdPolicy::default_strict();
    let env = PiiEnvelope::new(
        &registry,
        &Auth,
        &store,
        &pipeline,
        &session,
        &[LocaleTag::DeDe],
        &policy,
    )
    .with_dictionaries(&dictionary);
    let response = env
        .dispatch(
            &Principal::new("agent"),
            "observe",
            json!("Synthetic Alpha"),
            None,
        )
        .await
        .unwrap();
    assert!(!response
        .payload
        .as_str()
        .unwrap()
        .contains("Synthetic Beta"));
    assert_eq!(
        session
            .restore(response.payload.as_str().unwrap())
            .as_deref(),
        Some("Synthetic Beta")
    );
    assert_eq!(session.tokens().len(), 2);
    assert_eq!(*store.events.lock().unwrap(), ["begin", "finish"]);
    assert_eq!(
        *seen.lock().unwrap(),
        [None, None, Some("field".into()), None]
    );
}

#[tokio::test]
async fn response_commit_conflict_returns_no_payload_and_no_losing_mappings() {
    struct ConflictingNet {
        session: Arc<Session>,
    }
    impl gaze::SafetyNet for ConflictingNet {
        fn id(&self) -> &str {
            "conflict"
        }
        fn supported_locales(&self) -> &[gaze::LocaleTag] {
            &[gaze::LocaleTag::Global]
        }
        fn check(
            &self,
            _: &str,
            context: gaze::SafetyNetContext<'_>,
        ) -> Result<Vec<gaze::LeakSuspect>, gaze::SafetyNetError> {
            if !context.manifest.spans.is_empty() {
                self.session
                    .tokenize(&PiiClass::Name, "synthetic concurrent response")
                    .unwrap();
            }
            Ok(vec![])
        }
    }
    let session = Arc::new(Session::new(Scope::Ephemeral).unwrap());
    let pipeline = gaze::Pipeline::builder()
        .detector(Primary)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .register_safety_net(ConflictingNet {
            session: session.clone(),
        })
        .build()
        .unwrap();
    let store = Store::default();
    let registry = registry(&store, Default::default(), Some(json!(FRESH)), false, false);
    assert!(matches!(
        dispatch(&registry, &store, &pipeline, &session, json!("benign")).await,
        Err(DispatchError::Transaction(_))
    ));
    assert_eq!(*store.events.lock().unwrap(), ["begin", "invoke", "fail"]);
    assert!(session
        .snapshot_entries()
        .iter()
        .all(|entry| entry.raw != FRESH));
    assert_eq!(session.snapshot_entries().len(), 1);
}

#[tokio::test]
async fn later_residual_leaf_rejects_all_staging_and_undeclared_response_preflights_before_detection(
) {
    struct ResidualNet;
    impl gaze::SafetyNet for ResidualNet {
        fn id(&self) -> &str {
            "residual"
        }
        fn supported_locales(&self) -> &[gaze::LocaleTag] {
            &[gaze::LocaleTag::Global]
        }
        fn check(
            &self,
            text: &str,
            _: gaze::SafetyNetContext<'_>,
        ) -> Result<Vec<gaze::LeakSuspect>, gaze::SafetyNetError> {
            Ok(text
                .find("residual")
                .map(|start| {
                    gaze::LeakSuspect::new(
                        start..start + 8,
                        PiiClass::Name,
                        "residual",
                        None,
                        gaze::LeakKind::Uncovered,
                        "synthetic",
                        None,
                    )
                })
                .into_iter()
                .collect())
        }
    }
    let pipeline = gaze::Pipeline::builder()
        .detector(Primary)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .register_safety_net(ResidualNet)
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let pipeline = pipeline.with_pipeline_optimizations(
        gaze::PipelineOptimizationConfig::new().with_prefix_cache(true),
    );
    pipeline
        .redact(&session, gaze::RawDocument::Text("cache seed".into()))
        .unwrap();
    let cache_before = session.prefix_cache_entry_count();
    assert!(cache_before > 0);
    let store = Store::default();
    let registry_a = registry(&store, Default::default(), None, false, false);
    assert!(matches!(
        dispatch(
            &registry_a,
            &store,
            &pipeline,
            &session,
            json!([EMAIL, "residual"])
        )
        .await,
        Err(DispatchError::Protection(gaze::ProtectionError::Residual))
    ));
    assert!(session.tokens().is_empty());
    assert_eq!(session.prefix_cache_entry_count(), cache_before);
    assert!(store.events.lock().unwrap().is_empty());
    let registry_b = registry(
        &store,
        Default::default(),
        Some(json!([FRESH, "residual"])),
        false,
        false,
    );
    assert!(matches!(
        dispatch(&registry_b, &store, &pipeline, &session, json!("benign")).await,
        Err(DispatchError::Protection(gaze::ProtectionError::Residual))
    ));
    assert!(session.tokens().is_empty());
    assert_eq!(session.prefix_cache_entry_count(), cache_before);
    assert_eq!(*store.events.lock().unwrap(), ["begin", "invoke", "fail"]);
    let registry_c = registry(
        &store,
        Default::default(),
        Some(json!([FRESH,{"undeclared":false}])),
        false,
        false,
    );
    assert!(matches!(
        dispatch(&registry_c, &store, &pipeline, &session, json!("benign")).await,
        Err(DispatchError::Carrier(_))
    ));
    assert!(session.tokens().is_empty());
    assert_eq!(session.prefix_cache_entry_count(), cache_before);
}

#[cfg(feature = "core-tools")]
#[tokio::test]
async fn structured_observer_requires_explicit_producer_document_shape() {
    use CarrierSegment::Member as M;
    let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
    let declaration = CarrierDeclaration::new(
        vec![
            vec![M("document".into())],
            vec![M("document".into()), M("text".into())],
        ],
        vec![],
    );
    let mut registry = ToolRegistry::new();
    registry
        .register(core_tools::SafetyNetCheckTool::new().with_argument_carriers(declaration))
        .unwrap();
    let store = Store::default();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let policy = SessionIdPolicy::default_strict();
    let env = PiiEnvelope::new(
        &registry,
        &Auth,
        &store,
        core.pipeline(),
        &session,
        core.locale_chain().as_slice(),
        &policy,
    );
    let result = env
        .dispatch(
            &Principal::new("agent"),
            "safety_net_check",
            json!({"document":{"text":EMAIL}}),
            None,
        )
        .await
        .unwrap();
    assert_eq!(result.payload["ok"], "unconfigured");
    assert_eq!(*store.events.lock().unwrap(), ["begin", "finish"]);
    assert!(matches!(
        env.dispatch(
            &Principal::new("agent"),
            "safety_net_check",
            json!({"document":{"other":EMAIL}}),
            None
        )
        .await,
        Err(DispatchError::Carrier(_))
    ));
    assert_eq!(*store.events.lock().unwrap(), ["begin", "finish"]);
}
