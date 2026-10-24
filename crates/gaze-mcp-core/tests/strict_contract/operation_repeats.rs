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
        "raw earlier argument reached tool: {protected}"
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
        "raw earlier response reached host: {}",
        result.payload
    );
}
