use super::*;

const KNOWN: &str = "Synthetic Qorvex";
const UNUSED: &str = "Synthetic Zunvel";

fn host_policy() -> gaze::Policy {
    let mut detector = gaze::DetectorSpec::default();
    detector.name = "synthetic-email".into();
    detector.pattern = Some(r"\b[a-z]+@example\.invalid\b".into());
    let mut policy = gaze::Policy::default();
    policy.detectors.push(detector);
    policy
}

fn raw_context(name: &str) -> HostContextSelection {
    HostContextSelection::present(format!(
        r#"{{"record":{{"name":"{name}","unused_private_key":"{UNUSED}"}},"field_map":{{"/unused_private_key":"Name"}}}}"#,
    ))
}

async fn call(
    session: &Session,
    args: Value,
    output: Option<Value>,
    selection: HostContextSelection,
) -> Result<ToolResponse, DispatchError> {
    let store = Store::default();
    let registry = registry(
        &store,
        CarrierDeclaration::new(
            vec![
                vec![CarrierSegment::Member("guess".into())],
                vec![CarrierSegment::Member("nested".into())],
            ],
            vec![
                vec![],
                vec![
                    CarrierSegment::Member("nested".into()),
                    CarrierSegment::AnyIndex,
                ],
            ],
        ),
        output,
        false,
        false,
    );
    let mut policy = host_policy();
    policy.rules = vec![gaze::RuleSpec::Default {
        action: Action::Tokenize,
    }];
    let locales = gaze::LocaleChain::from(&[gaze::LocaleTag::Global][..]);
    let context = gaze::Context::from_json_str("{}").unwrap();
    let pipeline = gaze_assembly::build_pipeline(&policy, &context, &[], &locales, None).unwrap();
    let result = PiiEnvelope::new(
        &registry,
        &Auth,
        &store,
        &pipeline,
        session,
        locales.as_slice(),
        &SessionIdPolicy::default_strict(),
    )
    .with_context_policy(&policy, &[])
    .dispatch(&Principal::new("agent"), "test", args, None, selection)
    .await;
    if matches!(result, Err(DispatchError::HostContext(_))) {
        assert!(store.events.lock().unwrap().is_empty());
    }
    result
}

#[tokio::test]
async fn host_context_response_persists_only_matched_rule_sources_and_restores_exactly() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    call(
        &session,
        json!({}),
        Some(json!("plain first turn")),
        HostContextSelection::Absent,
    )
    .await
    .unwrap();
    let text = format!("returned {KNOWN}");
    let second = call(&session, json!({}), Some(json!(text)), raw_context(KNOWN))
        .await
        .unwrap();
    assert!(!second.payload.to_string().contains(KNOWN));
    assert_eq!(
        session
            .restore_strict_text(second.payload.as_str().unwrap())
            .unwrap(),
        text
    );
    assert!(!format!("{:?}", session.snapshot_entries()).contains(UNUSED));
    let third = call(
        &session,
        json!({}),
        Some(json!(text)),
        HostContextSelection::Absent,
    )
    .await
    .unwrap();
    assert_eq!(third.payload, second.payload);
    let fresh = Session::new(Scope::Ephemeral).unwrap();
    assert_eq!(
        call(
            &fresh,
            json!({}),
            Some(json!(KNOWN)),
            HostContextSelection::Absent
        )
        .await
        .unwrap()
        .payload,
        json!(KNOWN)
    );
}

#[tokio::test]
async fn host_context_echo_has_no_membership_oracle_or_argument_promotion() {
    for guess in [KNOWN, "synthetic qorvex", "Synthetic   Qorvex"] {
        let mut outcomes = Vec::new();
        for name in [KNOWN, UNUSED] {
            let session = Session::new(Scope::Ephemeral).unwrap();
            let args = json!({"guess":guess});
            let result = call(&session, args.clone(), None, raw_context(name))
                .await
                .unwrap();
            assert!(session.tokens().is_empty());
            assert_eq!(result.payload, args);
            outcomes.push(result.payload);
        }
        assert_eq!(outcomes[0], outcomes[1]);
    }
}

#[tokio::test]
async fn host_context_parser_is_shared_and_errors_are_value_free_before_invoke() {
    for raw in [
        "null".to_owned(),
        "[]".to_owned(),
        r#"{"private_canary_key":"private_canary_value"}"#.to_owned(),
        r#"{"record":{"name":"Synthetic Qorvex","name":"Synthetic Zunvel"}}"#.to_owned(),
        " ".repeat(4 * 1024 * 1024 + 1),
    ] {
        assert!(gaze::Context::from_json_str(&raw).is_err());
        let session = Session::new(Scope::Ephemeral).unwrap();
        let error = call(
            &session,
            json!({}),
            Some(json!("unused")),
            HostContextSelection::present(raw),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, DispatchError::HostContext(_)));
        let diagnostic = format!("{error:?} {error}");
        assert!(!diagnostic.contains("private_canary"));
        assert!(session.tokens().is_empty());
    }
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<String>>>);
impl gaze::RedactionLogger for Capture {
    fn log(&self, entry: &gaze::RedactionEntry) -> Result<(), gaze::RedactionLogError> {
        self.0.lock().unwrap().push(format!("{entry:?}"));
        Ok(())
    }
}
struct TraceCapture(Capture);
impl tracing::field::Visit for Capture {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0
            .lock()
            .unwrap()
            .push(format!("{}={value:?}", field.name()));
    }
}
impl tracing::Subscriber for TraceCapture {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attrs: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        attrs.record(&mut self.0.clone());
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        values.record(&mut self.0.clone());
    }
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        event.record(&mut self.0.clone());
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}
struct PrivateStore(Capture);
#[async_trait]
impl ManifestStore for PrivateStore {
    async fn begin_call(&self, ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
        self.0 .0.lock().unwrap().push(format!("{ctx:?}"));
        Ok(CallHandle::new(ctx.call_id))
    }
    async fn finish_call(&self, _: CallHandle, snapshot: SnapshotRef) -> Result<(), ManifestError> {
        self.0 .0.lock().unwrap().push(format!("{snapshot:?}"));
        Ok(())
    }
    async fn fail_call(&self, _: CallHandle, reason: FailureReason) -> Result<(), ManifestError> {
        self.0 .0.lock().unwrap().push(format!("{reason:?}"));
        Ok(())
    }
}
struct PrivateTool {
    descriptor: ToolDescriptor,
    capture: Capture,
    output: Value,
}
#[async_trait]
impl Tool for PrivateTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }
    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        self.capture
            .0
            .lock()
            .unwrap()
            .push(format!("{ctx:?} {:?}", ctx.resources().dictionaries()));
        Ok(ToolResponse::json(self.output.clone()))
    }
}

#[tokio::test]
async fn host_context_private_identifiers_never_reach_output_tools_or_audit() {
    let logs = Capture::default();
    let _trace = tracing::subscriber::set_default(TraceCapture(logs.clone()));
    let store = PrivateStore(logs.clone());
    let mut registry = ToolRegistry::new();
    registry
        .register(PrivateTool {
            descriptor: ToolDescriptor::agent("private_test", json!({})),
            capture: logs.clone(),
            output: json!("private_canary_value"),
        })
        .unwrap();
    let mut policy = host_policy();
    policy.rules = vec![
        gaze::RuleSpec::Class {
            class: PiiClass::Custom("private_canary_class".into()),
            action: Action::Tokenize,
        },
        gaze::RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    let context = gaze::Context::from_json_str("{}").unwrap();
    let locales = gaze::LocaleChain::from(&[gaze::LocaleTag::Global][..]);
    let pipeline = gaze_assembly::build_pipeline(&policy, &context, &[], &locales, None)
        .unwrap()
        .with_redaction_logger(logs.clone());
    let session = Session::new(Scope::Ephemeral).unwrap();
    let id_policy = SessionIdPolicy::default_strict();
    let envelope = PiiEnvelope::new(
        &registry,
        &Auth,
        &store,
        &pipeline,
        &session,
        locales.as_slice(),
        &id_policy,
    )
    .with_context_policy(&policy, &[]);
    let selection = HostContextSelection::present(r#"{"dictionaries":{"private_canary_key":{"terms":["private_canary_value","private_canary_unused"],"case_sensitive":true}},"class_map":{"private_canary_key":"custom:private_canary_class"},"fields":{"private_canary_field":"private_canary_field_value"}}"#.into());
    assert!(!format!("{selection:?}").contains("private_canary"));
    let output = envelope
        .dispatch(
            &Principal::new("agent"),
            "private_test",
            json!({}),
            None,
            selection,
        )
        .await
        .unwrap();
    assert!(!output.payload.to_string().contains("private_canary"));
    assert_eq!(
        session
            .restore_strict_text(output.payload.as_str().unwrap())
            .unwrap(),
        "private_canary_value"
    );
    let later = envelope
        .dispatch(
            &Principal::new("agent"),
            "private_test",
            json!({}),
            None,
            HostContextSelection::Absent,
        )
        .await
        .unwrap();
    assert_eq!(later.payload, output.payload);
    let entries = session.snapshot_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].raw, "private_canary_value");
    let audit = logs.0.lock().unwrap().join("\n");
    assert!(!audit.contains("private_canary"), "{audit}");
}

#[tokio::test]
async fn host_context_requires_explicit_host_support_before_begin() {
    let store = Store::default();
    let registry = registry(&store, Default::default(), Some(json!(KNOWN)), false, false);
    let pipeline = pipeline();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let error = PiiEnvelope::new(
        &registry,
        &Auth,
        &store,
        &pipeline,
        &session,
        &[gaze::LocaleTag::Global],
        &SessionIdPolicy::default_strict(),
    )
    .dispatch(
        &Principal::new("agent"),
        "test",
        json!({}),
        None,
        raw_context(KNOWN),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        DispatchError::HostContext(HostContextError::Unsupported)
    ));
    assert!(store.events.lock().unwrap().is_empty());
    assert!(session.tokens().is_empty());
}

#[tokio::test]
async fn host_context_guesses_in_json_escapes_and_new_cues_cannot_create_an_oracle() {
    for guess in [
        json!(r#"{"guess":"\u0053ynthetic Qorvex"}"#),
        json!(r#""\u0053ynthetic Qorvex""#),
        json!("prefixSynthetic Qorvexsuffix"),
    ] {
        for name in [KNOWN, UNUSED] {
            let session = Session::new(Scope::Ephemeral).unwrap();
            let output = call(
                &session,
                guess.clone(),
                Some(json!(KNOWN)),
                raw_context(name),
            )
            .await
            .unwrap();
            assert_eq!(output.payload, json!(KNOWN));
            assert!(session.tokens().is_empty());
        }
    }
    for name in ["Will", "Synthetic"] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let output = call(
            &session,
            json!("Will"),
            Some(json!("Name: Will")),
            raw_context(name),
        )
        .await
        .unwrap();
        assert_eq!(output.payload, json!("Name: Will"));
        assert!(session.tokens().is_empty());
    }
}

#[tokio::test]
async fn host_context_rejects_operator_bypass_before_begin_or_invoke() {
    let store = Store::default();
    let mut registry = ToolRegistry::new();
    registry
        .register(Producer {
            descriptor: ToolDescriptor::operator("raw", json!({}))
                .with_response_redaction(ResponseRedaction::BypassByOperator),
            events: store.events.clone(),
            output: Some(json!(KNOWN)),
            error: false,
            mutate: false,
        })
        .unwrap();
    let pipeline = pipeline();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let policy = host_policy();
    let error = PiiEnvelope::new(
        &registry,
        &Auth,
        &store,
        &pipeline,
        &session,
        &[gaze::LocaleTag::Global],
        &SessionIdPolicy::default_strict(),
    )
    .with_context_policy(&policy, &[])
    .dispatch(
        &Principal::new("operator"),
        "raw",
        json!({}),
        None,
        raw_context(KNOWN),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        DispatchError::HostContext(HostContextError::Unsupported)
    ));
    assert!(store.events.lock().unwrap().is_empty());
    assert!(session.tokens().is_empty());
}

#[tokio::test]
async fn host_context_nonreversible_policy_admission_does_not_depend_on_guesses() {
    for action in [Action::Preserve, Action::Redact, Action::Generalize] {
        for args in [json!({}), json!({"guess": KNOWN})] {
            let store = Store::default();
            let registry = registry(&store, Default::default(), Some(json!(KNOWN)), false, false);
            let mut policy = host_policy();
            policy.rules = vec![gaze::RuleSpec::Default { action }];
            let context = gaze::Context::from_json_str("{}").unwrap();
            let locales = gaze::LocaleChain::from(&[gaze::LocaleTag::Global][..]);
            let pipeline =
                gaze_assembly::build_pipeline(&policy, &context, &[], &locales, None).unwrap();
            let session = Session::new(Scope::Ephemeral).unwrap();
            let error = PiiEnvelope::new(
                &registry,
                &Auth,
                &store,
                &pipeline,
                &session,
                locales.as_slice(),
                &SessionIdPolicy::default_strict(),
            )
            .with_context_policy(&policy, &[])
            .dispatch(
                &Principal::new("agent"),
                "test",
                args,
                None,
                raw_context(KNOWN),
            )
            .await
            .unwrap_err();
            assert!(matches!(
                error,
                DispatchError::HostContext(HostContextError::Policy)
            ));
            assert!(store.events.lock().unwrap().is_empty());
            assert!(session.tokens().is_empty());
        }
    }
}

#[tokio::test]
async fn host_context_numeric_guesses_cannot_create_an_oracle() {
    for guess in [json!(731946), json!({"nested":[731946]}), json!("731946")] {
        let mut outcomes = Vec::new();
        for value in ["731946", "829157"] {
            let raw = json!({"dictionaries":{"order_ref":{"terms":[value]}}}).to_string();
            let session = Session::new(Scope::Ephemeral).unwrap();
            let output = call(
                &session,
                guess.clone(),
                Some(json!("order 731946")),
                HostContextSelection::present(raw),
            )
            .await
            .unwrap();
            assert!(session.tokens().is_empty());
            outcomes.push(output.payload);
        }
        assert_eq!(outcomes[0], json!("order 731946"));
        assert_eq!(outcomes[0], outcomes[1]);
    }
}

struct UntrustedProducer {
    descriptor: ToolDescriptor,
    args: Value,
    output: Value,
}
#[async_trait]
impl Tool for UntrustedProducer {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }
    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        assert_eq!(ctx.invocation_args().unwrap().as_value(), &self.args);
        Ok(ToolResponse::json(self.output.clone()))
    }
}

#[tokio::test]
async fn host_context_untrusted_invocation_protects_responses_without_a_guess_oracle() {
    for guessed in [false, true] {
        let mut outcomes = Vec::new();
        for name in [KNOWN, UNUSED] {
            let args = if guessed {
                json!({"guess":KNOWN})
            } else {
                json!({})
            };
            let mut registry = ToolRegistry::new();
            registry
                .register(UntrustedProducer {
                    descriptor: ToolDescriptor::operator("test", json!({}))
                        .with_request_mode(RequestMode::UntrustedInvocation),
                    args: args.clone(),
                    output: json!(KNOWN),
                })
                .unwrap();
            let store = PrivateStore(Capture::default());
            let session = Session::new(Scope::Ephemeral).unwrap();
            let mut policy = host_policy();
            policy.rules = vec![gaze::RuleSpec::Default {
                action: Action::Tokenize,
            }];
            let context = gaze::Context::from_json_str("{}").unwrap();
            let locales = gaze::LocaleChain::from(&[gaze::LocaleTag::Global][..]);
            let pipeline =
                gaze_assembly::build_pipeline(&policy, &context, &[], &locales, None).unwrap();
            let output = PiiEnvelope::new(
                &registry,
                &Auth,
                &store,
                &pipeline,
                &session,
                locales.as_slice(),
                &SessionIdPolicy::default_strict(),
            )
            .with_context_policy(&policy, &[])
            .dispatch_request(
                &Principal::new("operator"),
                "test",
                UntrustedInvocationArgs::new(args),
                None,
                raw_context(name),
            )
            .await
            .unwrap();
            if !guessed && name == KNOWN {
                assert_ne!(output.payload, json!(KNOWN));
                assert_eq!(
                    session
                        .restore_strict_text(output.payload.as_str().unwrap())
                        .unwrap(),
                    KNOWN
                );
            } else {
                assert_eq!(output.payload, json!(KNOWN));
                assert!(session.tokens().is_empty());
            }
            assert!(!store.0 .0.lock().unwrap().join("\n").contains(KNOWN));
            outcomes.push(output.payload);
        }
        if guessed {
            assert_eq!(outcomes[0], outcomes[1]);
        }
    }
}

async fn private_identifier_call(
    session: &Session,
    dictionary: &str,
    class: Option<&str>,
    selection: bool,
) -> Result<ToolResponse, DispatchError> {
    let mut raw = json!({"dictionaries":{dictionary:{"terms":[KNOWN]}}});
    if let Some(name) = class {
        raw["class_map"] = json!({dictionary:format!("custom:{name}")});
    }
    let selection = if selection {
        HostContextSelection::present(raw.to_string())
    } else {
        HostContextSelection::Absent
    };
    private_identifier_dispatch(session, class, selection, Capture::default()).await
}

async fn private_identifier_dispatch(
    session: &Session,
    class: Option<&str>,
    selection: HostContextSelection,
    logs: Capture,
) -> Result<ToolResponse, DispatchError> {
    let store = PrivateStore(logs.clone());
    let registry = registry(
        &Store::default(),
        Default::default(),
        Some(json!(KNOWN)),
        false,
        false,
    );
    let mut policy = host_policy();
    policy.rules = match class {
        Some(name) => vec![
            gaze::RuleSpec::Class {
                class: PiiClass::Custom(name.into()),
                action: Action::Tokenize,
            },
            gaze::RuleSpec::Default {
                action: Action::Preserve,
            },
        ],
        None => vec![gaze::RuleSpec::Default {
            action: Action::Tokenize,
        }],
    };
    let context = gaze::Context::from_json_str("{}").unwrap();
    let locales = gaze::LocaleChain::from(&[gaze::LocaleTag::Global][..]);
    let pipeline = gaze_assembly::build_pipeline(&policy, &context, &[], &locales, None)
        .unwrap()
        .with_redaction_logger(logs);
    PiiEnvelope::new(
        &registry,
        &Auth,
        &store,
        &pipeline,
        session,
        locales.as_slice(),
        &SessionIdPolicy::default_strict(),
    )
    .with_context_policy(&policy, &[])
    .dispatch(&Principal::new("agent"), "test", json!({}), None, selection)
    .await
}

#[tokio::test]
async fn host_context_private_labels_are_session_scoped_and_defeat_public_hash_guesses() {
    use sha2::{Digest, Sha256};
    for class in [Some("private_canary_class"), None] {
        let dictionary = "private_canary_dictionary";
        let mut classes = Vec::new();
        for _ in 0..2 {
            let session = Session::new(Scope::Ephemeral).unwrap();
            let first = private_identifier_call(&session, dictionary, class, true)
                .await
                .unwrap();
            assert_ne!(first.payload, json!(KNOWN));
            assert_eq!(
                session
                    .restore_strict_text(first.payload.as_str().unwrap())
                    .unwrap(),
                KNOWN
            );
            for guess in [dictionary, class.unwrap_or(dictionary)] {
                let public_digest = format!("{:x}", Sha256::digest(guess.as_bytes()));
                assert!(
                    !first.payload.to_string().contains(&public_digest[..32]),
                    "private identifier was recovered by a public hash guess"
                );
            }
            let later = private_identifier_call(&session, dictionary, class, false)
                .await
                .unwrap();
            assert_eq!(later.payload, first.payload);
            classes.push(session.snapshot_entries()[0].class.clone());
        }
        assert_ne!(
            classes[0], classes[1],
            "private labels must not link independent sessions"
        );
    }
}

#[tokio::test]
async fn host_context_private_labels_survive_trusted_snapshot_import() {
    for class in [Some("private_canary_class"), None] {
        let session = Session::new(Scope::Conversation("synthetic-conversation".into())).unwrap();
        let first = private_identifier_call(&session, "private_canary_dictionary", class, true)
            .await
            .unwrap();
        let blob = session.export().unwrap().into_bytes();
        assert_eq!(blob[0], 8);
        let restored = Session::import(blob.into()).unwrap();
        assert_eq!(
            restored
                .restore_strict_text(first.payload.as_str().unwrap())
                .unwrap(),
            KNOWN
        );
        let later = private_identifier_call(&restored, "private_canary_dictionary", class, false)
            .await
            .unwrap();
        assert_eq!(first.payload, later.payload);
        assert_eq!(
            session.snapshot_entries()[0].class,
            restored.snapshot_entries()[0].class
        );
    }
}

#[tokio::test]
async fn host_context_record_dictionary_identifiers_defeat_public_hash_guesses() {
    use sha2::{Digest, Sha256};
    let session = Session::new(Scope::Ephemeral).unwrap();
    let logs = Capture::default();
    let class = "private_canary_class";
    let selection = HostContextSelection::present(
        json!({
            "record":{"private_canary_field":KNOWN},
            "field_map":{"/private_canary_field":format!("custom:{class}")},
        "record_match_kinds":{format!("custom:{class}"):["exact"]},
        })
        .to_string(),
    );
    let output = private_identifier_dispatch(&session, Some(class), selection, logs.clone())
        .await
        .unwrap();
    assert_ne!(output.payload, json!(KNOWN));
    assert_eq!(
        session
            .restore_strict_text(output.payload.as_str().unwrap())
            .unwrap(),
        KNOWN
    );
    let observed = format!("{} {}", output.payload, logs.0.lock().unwrap().join("\n"));
    assert!(!observed.contains("private_canary"));
    for guess in [class.to_owned(), format!("custom:{class}")] {
        let digest = format!("{:x}", Sha256::digest(guess.as_bytes()));
        assert!(
            !observed.contains(&digest[..32]),
            "record metadata disclosed a public identifier digest"
        );
    }
}

#[tokio::test]
async fn host_context_legacy_opaque_snapshot_restores_but_dispatch_fails_closed() {
    use sha2::{Digest, Sha256};
    let session = Session::new(Scope::Conversation("synthetic-legacy".into())).unwrap();
    let digest = format!("{:x}", Sha256::digest(b"private_canary_class"));
    let class = PiiClass::Custom(format!("host_{}", &digest[..32]));
    let token = session.tokenize(&class, KNOWN).unwrap();
    let blob = session.export().unwrap().into_bytes();
    assert_eq!(blob[0], 6);
    let restored = Session::import(blob.into()).unwrap();
    assert_eq!(restored.restore_strict_text(&token).unwrap(), KNOWN);
    for context in [false, true] {
        let error = private_identifier_call(
            &restored,
            "private_canary_dictionary",
            Some("private_canary_class"),
            context,
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            DispatchError::HostContext(HostContextError::Policy)
        ));
        assert!(!restored.has_opaque_identifier_key());
        assert_eq!(restored.tokens(), vec![token.clone()]);
    }
    restored
        .opaque_identifier("unrelated-host-use", "synthetic-label")
        .unwrap();
    let error = private_identifier_call(
        &restored,
        "private_canary_dictionary",
        Some("private_canary_class"),
        false,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        DispatchError::HostContext(HostContextError::Policy)
    ));
}
