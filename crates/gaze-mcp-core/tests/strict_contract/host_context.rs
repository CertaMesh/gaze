use super::*;

const KNOWN: &str = "Synthetic Qorvex";
const UNUSED: &str = "Synthetic Zunvel";

fn raw_context(name: &str) -> HostContextSelection {
    HostContextSelection::present(format!(
        r#"{{"record":{{"name":"{name}","unused_private_key":"{UNUSED}"}},"field_map":{{"/unused_private_key":"Name"}}}}"#,
    ))
}

async fn call(session: &Session, args: Value, output: Option<Value>, selection: HostContextSelection) -> Result<ToolResponse, DispatchError> {
    let store = Store::default();
    let registry = registry(&store, CarrierDeclaration::text_fields(&["guess"]), output, false, false);
    let mut policy = gaze::Policy::default();
    policy.rules = vec![gaze::RuleSpec::Default { action: Action::Tokenize }];
    let locales = gaze::LocaleChain::from(&[gaze::LocaleTag::Global][..]);
    let context = gaze::Context::from_json_str("{}").unwrap();
    let pipeline = gaze_assembly::build_pipeline(&policy, &context, &[], &locales, None).unwrap();
    PiiEnvelope::new(&registry, &Auth, &store, &pipeline, session, locales.as_slice(), &SessionIdPolicy::default_strict())
        .with_context_policy(&policy, &[])
        .dispatch(&Principal::new("agent"), "test", args, None, selection).await
}

#[tokio::test]
async fn host_context_response_persists_only_matched_rule_sources_and_restores_exactly() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    call(&session, json!({}), Some(json!("plain first turn")), HostContextSelection::Absent).await.unwrap();
    let text = format!("returned {KNOWN}");
    let second = call(&session, json!({}), Some(json!(text)), raw_context(KNOWN)).await.unwrap();
    assert!(!second.payload.to_string().contains(KNOWN));
    assert_eq!(session.restore_strict_text(second.payload.as_str().unwrap()).unwrap(), text);
    assert!(!format!("{:?}", session.snapshot_entries()).contains(UNUSED));
    let third = call(&session, json!({}), Some(json!(text)), HostContextSelection::Absent).await.unwrap();
    assert_eq!(third.payload, second.payload);
    let fresh = Session::new(Scope::Ephemeral).unwrap();
    assert_eq!(call(&fresh, json!({}), Some(json!(KNOWN)), HostContextSelection::Absent).await.unwrap().payload, json!(KNOWN));
}

#[tokio::test]
async fn host_context_echo_has_no_membership_oracle_or_argument_promotion() {
    for guess in [KNOWN, "synthetic qorvex", "Synthetic   Qorvex"] {
        let mut outcomes = Vec::new();
        for name in [KNOWN, UNUSED] {
            let session = Session::new(Scope::Ephemeral).unwrap();
            let args = json!({"guess":guess});
            let result = call(&session, args.clone(), None, raw_context(name)).await.unwrap();
            assert!(session.tokens().is_empty());
            assert_eq!(result.payload, args);
            outcomes.push(result.payload);
        }
        assert_eq!(outcomes[0], outcomes[1]);
    }
}

#[tokio::test]
async fn host_context_parser_is_shared_and_errors_are_value_free_before_invoke() {
    for raw in ["null".to_owned(), "[]".to_owned(), r#"{"private_canary_key":"private_canary_value"}"#.to_owned(), r#"{"record":{"name":"Synthetic Qorvex","name":"Synthetic Zunvel"}}"#.to_owned(), " ".repeat(4*1024*1024+1)] {
        assert!(gaze::Context::from_json_str(&raw).is_err());
        let session = Session::new(Scope::Ephemeral).unwrap();
        let error = call(&session, json!({}), Some(json!("unused")), HostContextSelection::present(raw)).await.unwrap_err();
        assert!(matches!(error, DispatchError::HostContext(_)));
        let diagnostic = format!("{error:?} {error}");
        assert!(!diagnostic.contains("private_canary"));
        assert!(session.tokens().is_empty());
    }
}
