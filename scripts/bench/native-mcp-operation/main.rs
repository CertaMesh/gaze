//! Synthetic benchmark host. Every target goes through the runtime's actual dispatch path.
use async_trait::async_trait;
use gaze_mcp_core::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    io::{self, BufRead},
    path::PathBuf,
    sync::{Arc, Mutex},
};

struct Auth;
#[async_trait]
impl AuthHook for Auth {
    async fn authorize_agent(&self, _: &Principal, _: &str) -> Result<(), AuthError> {
        Ok(())
    }
    async fn authorize_operator(&self, _: &Principal, _: &str) -> Result<(), AuthError> {
        Err(AuthError::MissingHook)
    }
}
#[derive(Default)]
struct Audit {
    begin: usize,
    finish: usize,
    fail: usize,
    args: Value,
    call: String,
    snapshot: Option<SnapshotRef>,
}
#[derive(Default)]
struct Store(Mutex<Audit>);
#[async_trait]
impl ManifestStore for Store {
    async fn begin_call(&self, context: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
        let mut audit = self.0.lock().unwrap();
        audit.begin += 1;
        audit.args = context.redacted_args.clone();
        audit.call = context.call_id.to_string();
        Ok(CallHandle::new(context.call_id))
    }
    async fn finish_call(&self, _: CallHandle, snapshot: SnapshotRef) -> Result<(), ManifestError> {
        let mut audit = self.0.lock().unwrap();
        audit.finish += 1;
        audit.snapshot = Some(snapshot);
        Ok(())
    }
    async fn fail_call(&self, _: CallHandle, _: FailureReason) -> Result<(), ManifestError> {
        self.0.lock().unwrap().fail += 1;
        Ok(())
    }
}
struct Capture {
    descriptor: ToolDescriptor,
    seen: Arc<Mutex<Value>>,
    output: Arc<Mutex<Value>>,
}
#[async_trait]
impl Tool for Capture {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }
    async fn invoke(&self, context: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        *self.seen.lock().unwrap() = context.redacted_args().clone();
        Ok(ToolResponse::json(self.output.lock().unwrap().clone()))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    direction: Direction,
    payload: Value,
    seed_names: Vec<String>,
}
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Direction {
    Arguments,
    Response,
}
fn replace_owned(value: &mut Value, tokens: &[String]) {
    match value {
        Value::String(text) => {
            for (index, token) in tokens.iter().enumerate() {
                *text = text.replace(&format!("{{{{owned_{index}}}}}"), token);
            }
        }
        Value::Array(values) => {
            for value in values {
                replace_owned(value, tokens);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                replace_owned(value, tokens);
            }
        }
        _ => {}
    }
}
fn restore_json(session: &gaze::Session, value: &Value) -> Result<Value, gaze::RestoreError> {
    match value {
        Value::String(text) => session
            .assess_restore_text(text)
            .map(|assessment| Value::String(assessment.into_restored().text)),
        Value::Array(values) => values
            .iter()
            .map(|value| restore_json(session, value))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| Ok((key.clone(), restore_json(session, value)?)))
            .collect::<Result<serde_json::Map<_, _>, _>>()
            .map(Value::Object),
        other => Ok(other.clone()),
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
async fn run_case(
    case: Case,
    pipeline: &gaze::Pipeline,
    locales: &[gaze::LocaleTag],
    dictionaries: &gaze::DictionaryBundle,
) -> Result<Value, Box<dyn std::error::Error>> {
    let session = gaze::Session::new(gaze::Scope::Conversation("synthetic-mcp-benchmark".into()))?;
    let store = Store::default();
    let seen = Arc::new(Mutex::new(Value::Null));
    let output = Arc::new(Mutex::new(json!([])));
    // Fixed producer authority; fixtures cannot declare arbitrary keys or private numbers public.
    let declaration = CarrierDeclaration::new(
        vec![
            vec![CarrierSegment::Member("items".into())],
            vec![CarrierSegment::Member("count".into())],
        ],
        vec![
            vec![CarrierSegment::Member("count".into())],
            vec![
                CarrierSegment::Member("items".into()),
                CarrierSegment::AnyIndex,
            ],
        ],
    );
    let mut registry = ToolRegistry::new();
    registry.register(Capture {
        descriptor: ToolDescriptor::agent("capture", json!({}))
            .with_carriers(declaration.clone(), declaration),
        seen: seen.clone(),
        output: output.clone(),
    })?;
    let policy = SessionIdPolicy::default_strict();
    let envelope = PiiEnvelope::new(
        &registry, &Auth, &store, pipeline, &session, locales, &policy,
    )
    .with_dictionaries(dictionaries);
    let principal = Principal::new("synthetic-agent");
    for name in &case.seed_names {
        if name.is_empty() {
            session.tokenize(&gaze::PiiClass::Name, "")?;
            continue;
        }
        let args = json!({"items": [format!("From: \"{name}\" <alice@example.invalid>")], "count": 0});
        if let Err(error) = envelope.dispatch(&principal, "capture", args, None).await {
            return Ok(json!({"id":case.id,"phase":"seed","error":error.to_string()}));
        }
    }
    let entries = session.snapshot_entries();
    let mut tokens = Vec::new();
    for name in &case.seed_names {
        let matches = entries
            .iter()
            .filter(|entry| entry.raw == *name && entry.class == gaze::PiiClass::Name)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Ok(
                json!({"id":case.id,"phase":"seed","error":"seed did not produce one exact owned Name"}),
            );
        }
        tokens.push(matches[0].token.clone());
    }
    *store.0.lock().unwrap() = Audit::default();
    *seen.lock().unwrap() = Value::Null;
    let mut payload = case.payload;
    replace_owned(&mut payload, &tokens);
    let args = match case.direction {
        Direction::Arguments => payload,
        Direction::Response => {
            *output.lock().unwrap() = payload;
            json!([])
        }
    };
    let result = envelope.dispatch(&principal, "capture", args, None).await;
    let (response, error) = match result {
        Ok(response) => (response.payload, Value::Null),
        Err(error) => (Value::Null, json!(error.to_string())),
    };
    let snapshot = session.export()?.into_bytes();
    // Also exercise the runtime's signature/import validation and restore after import.
    let imported = gaze::Session::import(snapshot.clone().into())?;
    if imported.snapshot_entries() != session.snapshot_entries() {
        return Err("export/import changed mappings".into());
    }
    let target = match case.direction {
        Direction::Arguments => seen.lock().unwrap().clone(),
        Direction::Response => response.clone(),
    };
    let restored = restore_json(&imported, &target).ok();
    let audit = store.0.lock().unwrap();
    Ok(
        json!({"id":case.id,"phase":"target","error":error,"tool_args":seen.lock().unwrap().clone(),"response":response,"response_wire":serde_json::to_string(&response)?,
        "snapshot_hex":hex(&snapshot),"restored":restored,"audit":{"begin":audit.begin,"finish":audit.finish,"fail":audit.fail,"args":audit.args,"call":audit.call,"snapshot":audit.snapshot}}),
    )
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("expected setup policy path")?;
    let policy = gaze::Policy::load_for_cli(&PathBuf::from(path))?;
    if policy.safety_net.backend != gaze::SafetyNetPolicyBackend::Nym {
        return Err("MCP arm requires own-setup Nym policy".into());
    }
    let inputs = gaze_assembly::resolve_policy_inputs(&policy, None, None, None)?;
    let context =
        gaze::Context::from_json_str(r#"{"dictionaries":{},"class_map":{},"fields":{}}"#)?;
    let pipeline = gaze_assembly::build_pipeline_builder(
        &policy,
        &context,
        &inputs.rulepacks,
        &inputs.locale_chain,
        Some(inputs.ner_threshold),
    )?
    .build()?;
    if pipeline.safety_net_count() != 0 {
        return Err("builder unexpectedly attached a net".into());
    }
    let model = std::env::var_os("GAZE_NYM_MODEL_DIR").map(PathBuf::from);
    let pipeline =
        gaze_assembly::attach_nym_safety_net(pipeline, &policy, model.as_deref(), None, None)?;
    if pipeline.safety_net_count() != 1 {
        return Err("MCP arm requires exactly one Nym net".into());
    }
    for line in io::stdin().lock().lines() {
        let case: Case = serde_json::from_str(&line?)?;
        println!(
            "{}",
            run_case(
                case,
                &pipeline,
                inputs.locale_chain.as_slice(),
                &inputs.dictionaries
            )
            .await?
        );
    }
    Ok(())
}
