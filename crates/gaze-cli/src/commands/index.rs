//! `gaze index` subcommand — owner-side local text/Markdown ingest + search.
//!
//! Available only when the binary is built with `--features index`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use gaze::{Action, ClassRule, DefaultRule, Detection, Detector, LocaleTag, PiiClass, Pipeline};
use gaze_recognizers::{LocaleAwareModelRegistry, RegexDetector};
use gaze_token_bridge::adapter::CorpusIndexStore;
use gaze_token_bridge::bridge::TokenBridge;
use gaze_token_bridge::ingest::CorpusIngestor;
use gaze_token_bridge::model::{BridgeRequest, BridgeSearchOutcome, Principal, RequestedScope};
use gaze_token_bridge::persistent::{
    FileCorpusIndexStore, DEFAULT_DOMAIN_ID, DEFAULT_INDEX_DIR, LOCAL_ACTION, LOCAL_PRINCIPAL_ID,
    LOCAL_PURPOSE, LOCAL_ROLE, LOCAL_TOOL_NAME, LOCAL_WORKSPACE_ID,
};
use gaze_token_bridge::projection::HmacDomainProjector;
use gaze_token_bridge::registry::IndexDomainRegistry;
use gaze_token_bridge::util::sha256_hex;
use gaze_token_bridge::{BridgeError, DenyReason, RedactionSession};

use crate::error::CliError;

const KIJI_COMMAND_ENV: &str = "GAZE_KIJI_DISTILBERT_COMMAND";
const KIJI_MODEL_DIR_ENV: &str = "GAZE_KIJI_DISTILBERT_MODEL_DIR";

pub(crate) struct IngestArgs {
    pub(crate) dir: PathBuf,
    pub(crate) domain: String,
    pub(crate) index_path: Option<PathBuf>,
}

pub(crate) struct SearchArgs {
    pub(crate) entity: String,
    pub(crate) domain: String,
    pub(crate) class: Option<PiiClass>,
    pub(crate) index_path: Option<PathBuf>,
}

pub(crate) fn parse_index_class(value: &str) -> Result<PiiClass, String> {
    match value {
        "org" => Ok(PiiClass::Organization),
        other => PiiClass::from_canonical_str(other)
            .or_else(|| PiiClass::from_policy_name(other))
            .ok_or_else(|| {
                "class must be one of name, email, org, organization, or custom:<name>".to_string()
            }),
    }
}

pub(crate) fn default_domain() -> String {
    DEFAULT_DOMAIN_ID.to_string()
}

pub(crate) fn ingest(args: IngestArgs) -> Result<(), CliError> {
    let index_path = resolve_index_path(args.index_path);
    let docs = collect_docs(&args.dir)?;
    let classes = classes_for_docs(&docs);
    let pipeline = build_index_pipeline(&classes)?;

    let mut store = FileCorpusIndexStore::load_or_create(&index_path, &args.domain, &classes)
        .map_err(map_bridge_error)?;
    let policy_json = store.policy_json().map_err(map_bridge_error)?;
    let registry = IndexDomainRegistry::from_json(&policy_json).map_err(map_bridge_error)?;
    let domain = registry
        .domain(&args.domain)
        .cloned()
        .ok_or_else(index_failed)?;
    let projector = HmacDomainProjector::new(&registry);
    let ingestor = CorpusIngestor::new(&pipeline, &domain, &projector).with_safety_net_resolution();

    store.clear_domain(&args.domain);
    let mut ingested_files = 0_usize;
    for doc in &docs {
        let hit = ingestor
            .ingest_text(doc.doc_id(), &doc.text)
            .map_err(map_bridge_error)?;
        store.insert_hit(args.domain.clone(), hit);
        ingested_files += 1;
    }
    store.save().map_err(map_bridge_error)?;

    println!("files: {ingested_files}");
    println!("entities: {}", store.entity_count_for_domain(&args.domain));
    println!("domain: {}", args.domain);
    println!("index: {}", store.index_file_path().display());
    println!("owner-side index is sensitive; raw values live only in this local store");

    Ok(())
}

pub(crate) fn search(args: SearchArgs) -> Result<(), CliError> {
    let index_path = resolve_index_path(args.index_path);
    let store = FileCorpusIndexStore::load(&index_path).map_err(map_bridge_error)?;
    let domain = store
        .domain(&args.domain)
        .cloned()
        .ok_or_else(index_failed)?;
    let policy_json = store.policy_json().map_err(map_bridge_error)?;
    let output_safety_net = build_index_output_safety_net_pipeline()?;
    let mut bridge = TokenBridge::from_policy_json_and_store(&policy_json, store)
        .map_err(map_bridge_error)?
        .with_output_safety_net(output_safety_net, vec![LocaleTag::Global]);

    let principal = local_principal(&domain);
    let session = RedactionSession::ephemeral_for(&principal.id).map_err(map_bridge_error)?;
    let class = args.class.unwrap_or_else(|| infer_class(&args.entity));
    let source_token = session
        .tokenize(&class, &args.entity)
        .map_err(map_bridge_error)?;
    let request = BridgeRequest {
        principal: principal.clone(),
        tenant_id: principal.tenant_id.clone(),
        workspace_id: principal.workspace_id.clone(),
        agent_run_id: "gaze-index-cli".to_string(),
        conversation_session_id: session.session_id().to_string(),
        tool_name: LOCAL_TOOL_NAME.to_string(),
        action: LOCAL_ACTION.to_string(),
        purpose: LOCAL_PURPOSE.to_string(),
        source_token,
        target_domain: args.domain,
        requested_scope: RequestedScope::SameDomain,
    };

    match bridge.search(&session, &request) {
        BridgeSearchOutcome::Allowed(response) if !response.results.is_empty() => {
            for hit in response.results {
                println!("doc: {}", hit.doc_id);
                println!("snippet: {}", hit.snippet);
            }
        }
        BridgeSearchOutcome::Allowed(_) => {
            println!("no hits");
        }
        BridgeSearchOutcome::Denied(reason) => return Err(index_denied(reason)),
    }
    println!("raw PII never shown (owner-side only)");

    Ok(())
}

fn resolve_index_path(index_path: Option<PathBuf>) -> PathBuf {
    index_path
        .or_else(|| std::env::var_os("GAZE_INDEX_PATH").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_INDEX_DIR))
}

fn collect_docs(root: &Path) -> Result<Vec<SourceDoc>, CliError> {
    let mut docs = Vec::new();
    collect_docs_inner(root, root, &mut docs)?;
    docs.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(docs)
}

fn collect_docs_inner(root: &Path, path: &Path, docs: &mut Vec<SourceDoc>) -> Result<(), CliError> {
    let entries = fs::read_dir(path).map_err(|_| CliError::Io)?;
    for entry in entries {
        let entry = entry.map_err(|_| CliError::Io)?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|_| CliError::Io)?;
        if file_type.is_dir() {
            collect_docs_inner(root, &path, docs)?;
            continue;
        }
        if !file_type.is_file() || !is_text_markdown(&path) {
            continue;
        }

        let text = fs::read_to_string(&path).map_err(|_| CliError::InvalidEncoding)?;
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        docs.push(SourceDoc { relative, text });
    }
    Ok(())
}

fn is_text_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            let extension = extension.to_ascii_lowercase();
            extension == "txt" || extension == "md"
        })
        .unwrap_or(false)
}

fn classes_for_docs(docs: &[SourceDoc]) -> Vec<PiiClass> {
    let mut classes = BTreeSet::from([PiiClass::Email, PiiClass::Name, PiiClass::Organization]);
    for doc in docs {
        classes.extend(field_classes(&doc.text));
    }
    classes.into_iter().collect()
}

fn build_index_pipeline(classes: &[PiiClass]) -> Result<Pipeline, CliError> {
    let mut builder = Pipeline::builder()
        .detector(RegexDetector::emails().map_err(|_| index_failed())?)
        .detector(FieldEntityDetector);
    builder = builder.register_safety_net_registry(index_kiji_registry()?);

    let mut rule_classes =
        BTreeSet::from([PiiClass::Email, PiiClass::Name, PiiClass::Organization]);
    rule_classes.extend(classes.iter().cloned());
    for class in rule_classes {
        builder = builder.rule(ClassRule::new(class, Action::Tokenize));
    }

    builder
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .map_err(|_| index_failed())
}

fn build_index_output_safety_net_pipeline() -> Result<Pipeline, CliError> {
    Pipeline::builder()
        .register_safety_net_registry(index_kiji_registry()?)
        .build()
        .map_err(|_| index_failed())
}

fn index_kiji_registry() -> Result<LocaleAwareModelRegistry, CliError> {
    let mut registry = LocaleAwareModelRegistry::new();
    registry.register(index_kiji_safety_net()?);
    Ok(registry)
}

#[cfg(feature = "safety-net-kiji")]
fn index_kiji_safety_net(
) -> Result<gaze_recognizers::safety_net::kiji_distilbert::KijiDistilbertSafetyNet, CliError> {
    use gaze_recognizers::safety_net::kiji_distilbert::{
        KijiDistilbertConfig, KijiDistilbertSafetyNet, OrtKijiConfig, SubprocessKijiConfig,
    };

    if let Some(model_dir) = std::env::var_os(KIJI_MODEL_DIR_ENV) {
        return Ok(KijiDistilbertSafetyNet::new(KijiDistilbertConfig::from(
            OrtKijiConfig::new(model_dir),
        )));
    }

    if let Some(command) = std::env::var_os(KIJI_COMMAND_ENV) {
        return Ok(KijiDistilbertSafetyNet::new(KijiDistilbertConfig::from(
            SubprocessKijiConfig::new(command),
        )));
    }

    Err(CliError::SafetyNetConfigDetail(format!(
        "gaze index requires {KIJI_MODEL_DIR_ENV} or {KIJI_COMMAND_ENV}; install the pinned model with scripts/fetch-kiji-safetynet-model.sh"
    )))
}

#[cfg(not(feature = "safety-net-kiji"))]
fn index_kiji_safety_net() -> Result<impl gaze_recognizers::LocaleAwareModel, CliError> {
    Err(CliError::SafetyNetConfigDetail(
        "gaze index requires gaze-cli feature safety-net-kiji".to_string(),
    ))
}

fn infer_class(entity: &str) -> PiiClass {
    if entity.contains('@') {
        PiiClass::Email
    } else {
        PiiClass::Name
    }
}

fn local_principal(domain: &gaze_token_bridge::model::IndexDomain) -> Principal {
    Principal {
        id: LOCAL_PRINCIPAL_ID.to_string(),
        roles: vec![LOCAL_ROLE.to_string()],
        tenant_id: domain.tenant_id.clone(),
        workspace_id: LOCAL_WORKSPACE_ID.to_string(),
    }
}

fn map_bridge_error(err: BridgeError) -> CliError {
    match err {
        BridgeError::Session(message)
            if message.contains("safety net") || message.contains("SafetyNet") =>
        {
            CliError::SafetyNetFailure {
                variant: "IndexSafetyNet",
            }
        }
        _ => index_failed(),
    }
}

fn index_denied(reason: DenyReason) -> CliError {
    CliError::PolicyConfigDetail(format!("index search failed closed: {reason:?}"))
}

fn index_failed() -> CliError {
    CliError::PolicyConfigDetail("index operation failed closed".to_string())
}

struct SourceDoc {
    relative: String,
    text: String,
}

impl SourceDoc {
    fn doc_id(&self) -> String {
        let digest = sha256_hex(&self.relative);
        format!("doc:{}", &digest[..16])
    }
}

#[derive(Debug)]
struct FieldEntityDetector;

impl Detector for FieldEntityDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        field_detections(input)
    }
}

fn field_classes(input: &str) -> Vec<PiiClass> {
    field_detections(input)
        .into_iter()
        .map(|detection| detection.class)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn field_detections(input: &str) -> Vec<Detection> {
    let mut detections = Vec::new();
    let mut offset = 0_usize;

    for segment in input.split_inclusive('\n') {
        let line = segment.trim_end_matches(['\r', '\n']);
        if let Some(detection) = field_detection_for_line(line, offset) {
            detections.push(detection);
        }
        offset += segment.len();
    }

    detections
}

fn field_detection_for_line(line: &str, line_offset: usize) -> Option<Detection> {
    let colon = line.find(':')?;
    let label = normalize_label(&line[..colon]);
    let class = class_for_label(&label)?;
    let value_part = &line[colon + 1..];
    let trimmed_start = value_part.trim_start();
    let leading = value_part.len() - trimmed_start.len();
    let trimmed = trimmed_start
        .trim_end_matches(|ch: char| ch.is_ascii_whitespace() || matches!(ch, ';' | ','));
    if trimmed.is_empty() {
        return None;
    }

    let start = line_offset + colon + 1 + leading;
    let end = start + trimmed.len();
    Some(Detection::new(start..end, class, "gaze.index.field"))
}

fn normalize_label(label: &str) -> String {
    let label = label
        .trim()
        .trim_start_matches(['-', '*', '#', '>', '|'])
        .trim()
        .trim_matches('*')
        .trim();
    let mut out = String::new();
    let mut pending_underscore = false;
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_underscore && !out.is_empty() {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            pending_underscore = false;
        } else {
            pending_underscore = true;
        }
    }
    out
}

fn class_for_label(label: &str) -> Option<PiiClass> {
    match label {
        "name" | "full_name" | "customer_name" | "contact_name" | "person_name" => {
            Some(PiiClass::Name)
        }
        "organization" | "organisation" | "org" | "company" | "employer" => {
            Some(PiiClass::Organization)
        }
        "email" | "email_address" => None,
        custom
            if custom.ends_with("_id")
                || custom.ends_with("_ref")
                || custom.ends_with("_number")
                || custom.ends_with("_code") =>
        {
            Some(PiiClass::custom(custom))
        }
        _ => None,
    }
}
