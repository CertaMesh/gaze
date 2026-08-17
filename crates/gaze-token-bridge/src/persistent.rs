//! Owner-side persistent corpus index.
//!
//! This module deliberately uses private serde structs instead of deriving
//! serialization for [`crate::model::IndexSearchHit`]. Raw values are sealed
//! inside the owner-side index file; the frozen agent-visible contract stays
//! non-serializable for owner hits.
//!
//! Layout (schema v2): each document is persisted exactly once, keyed by
//! `(domain_id, doc_id)`. The fingerprint -> document postings map is derived
//! on load and never written to disk, so raw PII exists in one place per
//! document and re-ingesting a `doc_id` replaces (never shadows) its bytes.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use gaze::PiiClass;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::adapter::CorpusIndexStore;
use crate::error::BridgeError;
use crate::model::{
    DomainId, IndexDomain, IndexEntity, IndexSearchHit, IndexedEntityRef, PolicyRule,
};
use crate::util::hex;

pub const DEFAULT_INDEX_DIR: &str = ".gaze-index";
pub const INDEX_FILE_NAME: &str = "index.json";
pub const DEFAULT_DOMAIN_ID: &str = "local_owner/docs/v1";
pub const LOCAL_PRINCIPAL_ID: &str = "local_owner";
pub const LOCAL_ROLE: &str = "owner";
pub const LOCAL_TENANT_ID: &str = "local_owner";
pub const LOCAL_WORKSPACE_ID: &str = "owner_workspace";
pub const LOCAL_TOOL_NAME: &str = "gaze_index_search";
pub const LOCAL_ACTION: &str = "search_documents";
pub const LOCAL_PURPOSE: &str = "owner_lookup";

const SCHEMA_VERSION: u32 = 2;
const DEFAULT_MAX_RESULTS: usize = 20;
// follow-up: extract shared gaze-crypto seal/open with gaze-mcp-bridge session storage.
// follow-up: add explicit owner-side index key rotation without plaintext migration fallback.
const INDEX_MAGIC: &[u8] = b"GAZEIDX1";
const INDEX_FORMAT_VERSION: u8 = 1;
const NONCE_LEN: usize = 12;
const AEAD_TAG_LEN: usize = 16;
const INDEX_ID_LEN: usize = 16;
const KEY_SOURCE_ENV: u8 = 1;
const KEY_SOURCE_KEYCHAIN: u8 = 2;
const KEY_ID_LEN_BYTES: usize = 2;
const INDEX_KEY_ENV: &str = "GAZE_INDEX_KEY";
#[cfg(feature = "os-keychain")]
const KEYCHAIN_SERVICE: &str = "dev.empiretwo.gaze.index";
const INDEX_AAD_CONTEXT: &[u8] = b"gaze-token-bridge-owner-index-v1";

#[cfg(all(test, feature = "os-keychain"))]
static KEYCHAIN_DISABLED_FOR_TESTS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Derived inverted index: domain -> fingerprint -> positions into
/// `PersistentIndexFile::documents`. Rebuilt on load, never persisted.
type Postings = HashMap<DomainId, HashMap<String, Vec<usize>>>;

/// File-backed owner-side corpus index store.
#[derive(Debug, Clone)]
pub struct FileCorpusIndexStore {
    dir: PathBuf,
    file: PersistentIndexFile,
    index_id: [u8; INDEX_ID_LEN],
    postings: Postings,
}

impl FileCorpusIndexStore {
    /// Load an existing index directory.
    pub fn load(dir: impl AsRef<Path>) -> Result<Self, BridgeError> {
        let dir = dir.as_ref().to_path_buf();
        let index_path = dir.join(INDEX_FILE_NAME);
        let contents = fs::read(&index_path).map_err(|err| {
            BridgeError::Policy(format!(
                "failed to read owner-side index {}: {err}",
                index_path.display()
            ))
        })?;
        let envelope = parse_index_envelope(&contents)?;
        let key = resolve_index_key_for_load(&dir, &envelope)?;
        let index_id = parsed_index_id(&envelope)?;
        let plaintext = decrypt_index_file(&key.material, envelope.index_id, &envelope)?;
        // Probe the schema version before the full parse so an older layout
        // yields the typed schema-mismatch error, not a field-shape parse error.
        let probe: SchemaProbe = serde_json::from_slice(&plaintext).map_err(|err| {
            BridgeError::Policy(format!(
                "failed to parse owner-side index {}: {err}",
                index_path.display()
            ))
        })?;
        if probe.schema_version != SCHEMA_VERSION {
            return Err(BridgeError::Policy(format!(
                "unsupported owner-side index schema {}; supported {}; rebuild with `gaze index ingest ...`",
                probe.schema_version, SCHEMA_VERSION
            )));
        }
        let file: PersistentIndexFile = serde_json::from_slice(&plaintext).map_err(|err| {
            BridgeError::Policy(format!(
                "failed to parse owner-side index {}: {err}",
                index_path.display()
            ))
        })?;
        let mut store = Self {
            dir,
            file,
            index_id,
            postings: Postings::new(),
        };
        store.rebuild_postings()?;
        Ok(store)
    }

    /// Load an existing index, or create a new one with `domain_id` registered.
    pub fn load_or_create(
        dir: impl AsRef<Path>,
        domain_id: &str,
        classes: &[PiiClass],
    ) -> Result<Self, BridgeError> {
        let dir = dir.as_ref().to_path_buf();
        let index_path = dir.join(INDEX_FILE_NAME);
        let mut store = if index_path.exists() {
            Self::load(&dir)?
        } else {
            Self {
                dir,
                file: PersistentIndexFile::default(),
                index_id: random_index_id(),
                postings: Postings::new(),
            }
        };
        store.ensure_domain(domain_id, classes)?;
        Ok(store)
    }

    /// Persist the owner-side index file atomically within its directory.
    pub fn save(&self) -> Result<(), BridgeError> {
        fs::create_dir_all(&self.dir).map_err(|err| {
            BridgeError::Policy(format!(
                "failed to create owner-side index dir {}: {err}",
                self.dir.display()
            ))
        })?;
        restrict_dir_permissions(&self.dir)?;

        let key = resolve_index_key_for_save(&self.dir)?;
        let index_path = self.index_file_path();
        let tmp_path = self.dir.join(format!("{INDEX_FILE_NAME}.tmp"));
        let plaintext = Zeroizing::new(
            serde_json::to_vec_pretty(&self.file)
                .map_err(|err| BridgeError::Policy(format!("failed to encode index: {err}")))?,
        );
        let contents = encrypt_index_file(&key, &self.index_id, plaintext.as_slice())?;
        let mut file = fs::File::create(&tmp_path).map_err(|err| {
            BridgeError::Policy(format!(
                "failed to create owner-side index {}: {err}",
                tmp_path.display()
            ))
        })?;
        restrict_file_permissions(&tmp_path)?;
        file.write_all(&contents).map_err(|err| {
            BridgeError::Policy(format!(
                "failed to write owner-side index {}: {err}",
                tmp_path.display()
            ))
        })?;
        file.sync_all().map_err(|err| {
            BridgeError::Policy(format!(
                "failed to sync owner-side index {}: {err}",
                tmp_path.display()
            ))
        })?;
        drop(file);
        fs::rename(&tmp_path, &index_path).map_err(|err| {
            BridgeError::Policy(format!(
                "failed to replace owner-side index {}: {err}",
                index_path.display()
            ))
        })?;
        Ok(())
    }

    pub fn index_file_path(&self) -> PathBuf {
        self.dir.join(INDEX_FILE_NAME)
    }

    pub fn domain(&self, domain_id: &str) -> Option<&IndexDomain> {
        self.file
            .domains
            .iter()
            .find(|domain| domain.domain_id == domain_id)
    }

    /// Remove every stored document for `domain_id`. After `save`, none of
    /// their bytes remain in the sealed payload.
    pub fn clear_domain(&mut self, domain_id: &str) {
        self.file
            .documents
            .retain(|document| document.domain_id != domain_id);
        self.rebuild_postings_unchecked();
    }

    pub fn ensure_domain(
        &mut self,
        domain_id: &str,
        classes: &[PiiClass],
    ) -> Result<(), BridgeError> {
        let mut classes = normalized_classes(classes);
        if classes.is_empty() {
            classes = default_classes();
        }

        let existing = self
            .file
            .domains
            .iter()
            .position(|domain| domain.domain_id == domain_id);

        match existing {
            Some(index) => {
                merge_classes(
                    &mut self.file.domains[index].allowed_entity_classes,
                    &classes,
                );
                for rule in &mut self.file.rules {
                    if rule.target_domain == domain_id {
                        merge_classes(&mut rule.allowed_entity_classes, &classes);
                    }
                }
            }
            None => {
                let key_id = projection_key_id(domain_id, &self.file.projection_keys);
                self.file.projection_keys.push(StoredProjectionKey {
                    key_id: key_id.clone(),
                    material: generated_projection_key(),
                });
                self.file
                    .domains
                    .push(local_domain(domain_id, &key_id, &classes));
                self.file.rules.push(local_rule(domain_id, &classes));
            }
        }

        Ok(())
    }

    pub fn policy_json(&self) -> Result<String, BridgeError> {
        let policy = PolicyJson {
            domains: self.file.domains.iter().map(RawIndexDomain::from).collect(),
            projection_keys: self.file.projection_keys.clone(),
            rules: self.file.rules.iter().map(RawPolicyRule::from).collect(),
        };
        serde_json::to_string(&policy)
            .map_err(|err| BridgeError::Policy(format!("failed to encode policy: {err}")))
    }

    /// Number of `(document, fingerprint)` pairs indexed for `domain_id`
    /// (the `entities: N` metric printed by `gaze index ingest`).
    pub fn entity_count_for_domain(&self, domain_id: &str) -> usize {
        self.postings
            .get(domain_id)
            .map(|by_fingerprint| by_fingerprint.values().map(Vec::len).sum())
            .unwrap_or(0)
    }

    fn document_position(&self, domain_id: &str, doc_id: &str) -> Option<usize> {
        self.file
            .documents
            .iter()
            .position(|document| document.domain_id == domain_id && document.hit.doc_id == doc_id)
    }

    /// Rebuild the derived postings from the persisted documents, failing
    /// closed if the payload violates the one-record-per-`(domain, doc_id)`
    /// invariant (a hand-edited or corrupted file must not resurrect shadowed
    /// document copies).
    fn rebuild_postings(&mut self) -> Result<(), BridgeError> {
        let mut seen = HashSet::new();
        for document in &self.file.documents {
            if !seen.insert((document.domain_id.as_str(), document.hit.doc_id.as_str())) {
                return Err(BridgeError::Policy(
                    "owner-side index has duplicate document keys; rebuild with `gaze index ingest ...`"
                        .to_string(),
                ));
            }
        }
        self.rebuild_postings_unchecked();
        Ok(())
    }

    fn rebuild_postings_unchecked(&mut self) {
        self.postings.clear();
        for position in 0..self.file.documents.len() {
            self.add_postings_for(position);
        }
    }

    /// Append `position` to every fingerprint posting list of that document.
    /// Positions are only ever added in ascending order, so each posting list
    /// stays sorted by document order and `hits_for_entity` is order-stable
    /// across save + load.
    fn add_postings_for(&mut self, position: usize) {
        let document = &self.file.documents[position];
        let domain_id = document.domain_id.clone();
        let fingerprints = document_fingerprints(&domain_id, &document.hit);
        let by_fingerprint = self.postings.entry(domain_id).or_default();
        for fingerprint_hex in fingerprints {
            by_fingerprint
                .entry(fingerprint_hex)
                .or_default()
                .push(position);
        }
    }
}

/// Distinct fingerprints of `hit` that live in `domain_id`, in first-seen order.
fn document_fingerprints(domain_id: &str, hit: &StoredHit) -> Vec<String> {
    let mut seen = HashSet::new();
    hit.entities
        .iter()
        .filter(|entity| entity.index_ref.domain_id == domain_id)
        .filter(|entity| seen.insert(entity.index_ref.fingerprint_hex.as_str()))
        .map(|entity| entity.index_ref.fingerprint_hex.clone())
        .collect()
}

impl CorpusIndexStore for FileCorpusIndexStore {
    /// Upsert on `(domain_id, doc_id)`: a re-ingested document replaces its
    /// previous copy outright, so stale snippets and raw values never survive.
    /// A document with no fingerprint in `domain_id` is unreachable through
    /// `hits_for_entity`, so it is not stored (and any previous copy is erased).
    fn insert_hit(&mut self, domain_id: DomainId, hit: IndexSearchHit) {
        let stored_hit = StoredHit::from(&hit);
        let reachable = !document_fingerprints(&domain_id, &stored_hit).is_empty();
        let existing = self.document_position(&domain_id, &hit.doc_id);

        match (existing, reachable) {
            (Some(position), true) => {
                self.file.documents[position].hit = stored_hit;
                self.rebuild_postings_unchecked();
            }
            (Some(position), false) => {
                self.file.documents.remove(position);
                self.rebuild_postings_unchecked();
            }
            (None, true) => {
                self.file.documents.push(StoredDocument {
                    domain_id,
                    hit: stored_hit,
                });
                self.add_postings_for(self.file.documents.len() - 1);
            }
            (None, false) => {}
        }
    }

    fn hits_for_entity(&self, domain_id: &DomainId, fingerprint_hex: &str) -> Vec<IndexSearchHit> {
        self.postings
            .get(domain_id)
            .and_then(|by_fingerprint| by_fingerprint.get(fingerprint_hex))
            .map(|positions| {
                positions
                    .iter()
                    .filter_map(|position| self.file.documents.get(*position))
                    .map(|document| document.hit.to_index_hit())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Minimal shape read before the full parse so schema drift fails closed with
/// the typed schema-mismatch error instead of a field-shape parse error.
#[derive(Debug, Deserialize)]
struct SchemaProbe {
    schema_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PersistentIndexFile {
    schema_version: u32,
    domains: Vec<IndexDomain>,
    projection_keys: Vec<StoredProjectionKey>,
    rules: Vec<PolicyRule>,
    /// One record per `(domain_id, doc_id)`; the searchable postings are
    /// derived from these on load.
    documents: Vec<StoredDocument>,
}

impl Default for PersistentIndexFile {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            domains: Vec::new(),
            projection_keys: Vec::new(),
            rules: Vec::new(),
            documents: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredProjectionKey {
    key_id: String,
    material: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredDocument {
    domain_id: DomainId,
    hit: StoredHit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredHit {
    doc_id: String,
    snippet: String,
    entities: Vec<StoredEntity>,
}

impl StoredHit {
    fn to_index_hit(&self) -> IndexSearchHit {
        IndexSearchHit {
            doc_id: self.doc_id.clone(),
            snippet: self.snippet.clone(),
            entities: self
                .entities
                .iter()
                .map(StoredEntity::to_index_entity)
                .collect(),
        }
    }
}

impl From<&IndexSearchHit> for StoredHit {
    fn from(hit: &IndexSearchHit) -> Self {
        Self {
            doc_id: hit.doc_id.clone(),
            snippet: hit.snippet.clone(),
            entities: hit.entities.iter().map(StoredEntity::from).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredEntity {
    class: PiiClass,
    raw_value: String,
    index_ref: IndexedEntityRef,
    domain_alias: String,
}

impl StoredEntity {
    fn to_index_entity(&self) -> IndexEntity {
        IndexEntity {
            class: self.class.clone(),
            raw_value: self.raw_value.clone(),
            index_ref: self.index_ref.clone(),
            domain_alias: self.domain_alias.clone(),
        }
    }
}

impl From<&IndexEntity> for StoredEntity {
    fn from(entity: &IndexEntity) -> Self {
        Self {
            class: entity.class.clone(),
            raw_value: entity.raw_value.clone(),
            index_ref: entity.index_ref.clone(),
            domain_alias: entity.domain_alias.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
struct PolicyJson {
    domains: Vec<RawIndexDomain>,
    projection_keys: Vec<StoredProjectionKey>,
    rules: Vec<RawPolicyRule>,
}

#[derive(Debug, Serialize)]
struct RawIndexDomain {
    domain_id: DomainId,
    tenant_id: String,
    corpus_type: String,
    purpose: String,
    allowed_roles: Vec<String>,
    allowed_tools: Vec<String>,
    allowed_actions: Vec<String>,
    allowed_entity_classes: Vec<String>,
    snippets_allowed: bool,
    raw_restore_allowed: bool,
    co_searchable_with: Vec<DomainId>,
    projection_key_id: String,
}

impl From<&IndexDomain> for RawIndexDomain {
    fn from(domain: &IndexDomain) -> Self {
        Self {
            domain_id: domain.domain_id.clone(),
            tenant_id: domain.tenant_id.clone(),
            corpus_type: domain.corpus_type.clone(),
            purpose: domain.purpose.clone(),
            allowed_roles: domain.allowed_roles.clone(),
            allowed_tools: domain.allowed_tools.clone(),
            allowed_actions: domain.allowed_actions.clone(),
            allowed_entity_classes: class_names(&domain.allowed_entity_classes),
            snippets_allowed: domain.snippets_allowed,
            raw_restore_allowed: domain.raw_restore_allowed,
            co_searchable_with: domain.co_searchable_with.clone(),
            projection_key_id: domain.projection_key_id.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
struct RawPolicyRule {
    roles: Vec<String>,
    tenant_id: String,
    workspace_id: String,
    tool_name: String,
    action: String,
    purpose: String,
    target_domain: DomainId,
    allowed_entity_classes: Vec<String>,
    max_results: usize,
    max_entities_per_window: Option<usize>,
    rate_limit_window_seconds: Option<u64>,
    allow_cross_domain: bool,
    elevated_audit_required: bool,
}

impl From<&PolicyRule> for RawPolicyRule {
    fn from(rule: &PolicyRule) -> Self {
        Self {
            roles: rule.roles.clone(),
            tenant_id: rule.tenant_id.clone(),
            workspace_id: rule.workspace_id.clone(),
            tool_name: rule.tool_name.clone(),
            action: rule.action.clone(),
            purpose: rule.purpose.clone(),
            target_domain: rule.target_domain.clone(),
            allowed_entity_classes: class_names(&rule.allowed_entity_classes),
            max_results: rule.max_results,
            max_entities_per_window: Some(1_000),
            rate_limit_window_seconds: Some(60),
            allow_cross_domain: rule.allow_cross_domain,
            elevated_audit_required: rule.elevated_audit_required,
        }
    }
}

fn local_domain(domain_id: &str, projection_key_id: &str, classes: &[PiiClass]) -> IndexDomain {
    IndexDomain {
        domain_id: domain_id.to_string(),
        tenant_id: LOCAL_TENANT_ID.to_string(),
        corpus_type: "text_md".to_string(),
        purpose: LOCAL_PURPOSE.to_string(),
        allowed_roles: vec![LOCAL_ROLE.to_string()],
        allowed_tools: vec![LOCAL_TOOL_NAME.to_string()],
        allowed_actions: vec![LOCAL_ACTION.to_string()],
        allowed_entity_classes: normalized_classes(classes),
        snippets_allowed: true,
        raw_restore_allowed: false,
        co_searchable_with: Vec::new(),
        projection_key_id: projection_key_id.to_string(),
    }
}

fn local_rule(domain_id: &str, classes: &[PiiClass]) -> PolicyRule {
    PolicyRule {
        roles: vec![LOCAL_ROLE.to_string()],
        tenant_id: LOCAL_TENANT_ID.to_string(),
        workspace_id: LOCAL_WORKSPACE_ID.to_string(),
        tool_name: LOCAL_TOOL_NAME.to_string(),
        action: LOCAL_ACTION.to_string(),
        purpose: LOCAL_PURPOSE.to_string(),
        target_domain: domain_id.to_string(),
        allowed_entity_classes: normalized_classes(classes),
        max_results: DEFAULT_MAX_RESULTS,
        allow_cross_domain: false,
        elevated_audit_required: false,
    }
}

fn default_classes() -> Vec<PiiClass> {
    vec![
        PiiClass::Email,
        PiiClass::Name,
        PiiClass::Organization,
        PiiClass::custom("customer_id"),
        PiiClass::custom("account_id"),
        PiiClass::custom("case_id"),
    ]
}

fn normalized_classes(classes: &[PiiClass]) -> Vec<PiiClass> {
    let mut out = classes.to_vec();
    merge_classes(&mut out, &default_classes());
    out
}

fn merge_classes(existing: &mut Vec<PiiClass>, additional: &[PiiClass]) {
    for class in additional {
        if !existing.iter().any(|candidate| candidate == class) {
            existing.push(class.clone());
        }
    }
    existing.sort();
    existing.dedup();
}

fn class_names(classes: &[PiiClass]) -> Vec<String> {
    normalized_classes(classes)
        .iter()
        .map(PiiClass::to_canonical_str)
        .collect()
}

fn projection_key_id(domain_id: &str, keys: &[StoredProjectionKey]) -> String {
    let stem = domain_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let stem = if stem.is_empty() {
        "local-index".to_string()
    } else {
        stem
    };
    let mut suffix = 1;
    loop {
        let candidate = format!("{stem}-projection-v{suffix}");
        if !keys.iter().any(|key| key.key_id == candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

fn generated_projection_key() -> String {
    let mut bytes = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let material = hex(&bytes);
    bytes.zeroize();
    material
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexKeySource {
    Env,
    Keychain,
}

impl IndexKeySource {
    fn tag(self) -> u8 {
        match self {
            Self::Env => KEY_SOURCE_ENV,
            Self::Keychain => KEY_SOURCE_KEYCHAIN,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, BridgeError> {
        match tag {
            KEY_SOURCE_ENV => Ok(Self::Env),
            KEY_SOURCE_KEYCHAIN => Ok(Self::Keychain),
            _ => Err(index_crypto_error(
                "owner-side index has invalid encrypted key source",
            )),
        }
    }
}

struct IndexKey {
    source: IndexKeySource,
    key_id: Vec<u8>,
    material: Zeroizing<[u8; 32]>,
}

struct ParsedIndexEnvelope<'a> {
    source: IndexKeySource,
    key_id: &'a [u8],
    index_id: &'a [u8],
    header: &'a [u8],
    nonce: &'a [u8],
    ciphertext: &'a [u8],
}

fn resolve_index_key_for_save(dir: &Path) -> Result<IndexKey, BridgeError> {
    if let Some(material) = env_index_key()? {
        return Ok(IndexKey {
            source: IndexKeySource::Env,
            key_id: Vec::new(),
            material,
        });
    }

    #[cfg(feature = "os-keychain")]
    {
        let account = keychain_account_for_dir(dir)?;
        let material = match keychain_index_key(&account)? {
            Some(material) => material,
            None => generate_and_store_keychain_index_key(&account)?,
        };

        Ok(IndexKey {
            source: IndexKeySource::Keychain,
            key_id: account.into_bytes(),
            material,
        })
    }

    #[cfg(not(feature = "os-keychain"))]
    {
        let _ = dir;
        Err(index_crypto_error(
            "no owner-side index key source available; set GAZE_INDEX_KEY, or build with --features os-keychain",
        ))
    }
}

fn resolve_index_key_for_load(
    dir: &Path,
    envelope: &ParsedIndexEnvelope<'_>,
) -> Result<IndexKey, BridgeError> {
    if let Some(material) = env_index_key()? {
        return Ok(IndexKey {
            source: IndexKeySource::Env,
            key_id: Vec::new(),
            material,
        });
    }

    if envelope.source == IndexKeySource::Env {
        return Err(index_crypto_error(
            "owner-side index was sealed with GAZE_INDEX_KEY, but GAZE_INDEX_KEY is missing",
        ));
    }

    #[cfg(feature = "os-keychain")]
    {
        let key_id = std::str::from_utf8(envelope.key_id)
            .map_err(|_| index_crypto_error("owner-side index key id is not utf-8"))?;
        let expected_key_id = keychain_account_for_dir(dir)?;
        if key_id != expected_key_id {
            return Err(index_crypto_error(
                "owner-side index keychain identity mismatch",
            ));
        }

        let material = keychain_index_key(key_id)?
            .ok_or_else(|| index_crypto_error("owner-side index key missing from OS keychain"))?;

        Ok(IndexKey {
            source: IndexKeySource::Keychain,
            key_id: envelope.key_id.to_vec(),
            material,
        })
    }

    #[cfg(not(feature = "os-keychain"))]
    {
        let _ = dir;
        let _ = envelope.key_id;
        Err(index_crypto_error(
            "owner-side index was sealed with OS keychain, but this build lacks os-keychain; set GAZE_INDEX_KEY, or build with --features os-keychain",
        ))
    }
}

fn env_index_key() -> Result<Option<Zeroizing<[u8; 32]>>, BridgeError> {
    let mut raw = match env::var(INDEX_KEY_ENV) {
        Ok(raw) => raw,
        Err(env::VarError::NotPresent) => return Ok(None),
        Err(env::VarError::NotUnicode(_)) => {
            return Err(index_crypto_error(
                "GAZE_INDEX_KEY must be valid utf-8 hex for a 32-byte key",
            ));
        }
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        raw.zeroize();
        return Err(index_crypto_error(
            "GAZE_INDEX_KEY is empty; refusing plaintext owner-side index I/O",
        ));
    }

    let parsed = parse_hex_key(trimmed);
    raw.zeroize();
    parsed.map(|key| Some(Zeroizing::new(key)))
}

fn parse_hex_key(raw: &str) -> Result<[u8; 32], BridgeError> {
    if raw.len() != 64 || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(index_crypto_error(
            "GAZE_INDEX_KEY must be 64 hex characters for a 32-byte key",
        ));
    }

    let mut out = [0_u8; 32];
    for (index, chunk) in raw.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble(chunk[0]).expect("checked hex high nibble");
        let low = hex_nibble(chunk[1]).expect("checked hex low nibble");
        out[index] = (high << 4) | low;
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(feature = "os-keychain")]
fn keychain_index_key(account: &str) -> Result<Option<Zeroizing<[u8; 32]>>, BridgeError> {
    if keychain_disabled_for_tests() {
        return Ok(None);
    }

    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account)
        .map_err(|err| index_crypto_error(format!("OS keychain unavailable: {err}")))?;
    match entry.get_password() {
        Ok(mut secret) => {
            let parsed = parse_hex_key(secret.trim());
            secret.zeroize();
            parsed.map(|key| Some(Zeroizing::new(key)))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(index_crypto_error(format!(
            "failed to read owner-side index key from OS keychain: {err}"
        ))),
    }
}

#[cfg(feature = "os-keychain")]
fn generate_and_store_keychain_index_key(
    account: &str,
) -> Result<Zeroizing<[u8; 32]>, BridgeError> {
    if keychain_disabled_for_tests() {
        return Err(index_crypto_error(
            "no owner-side index key source available; set GAZE_INDEX_KEY or enable OS keychain",
        ));
    }

    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account)
        .map_err(|err| index_crypto_error(format!("OS keychain unavailable: {err}")))?;
    let mut key = Zeroizing::new([0_u8; 32]);
    rand::rngs::OsRng.fill_bytes(&mut key[..]);
    let mut secret = hex(&key[..]);
    let stored = entry.set_password(&secret);
    secret.zeroize();
    stored.map_err(|err| {
        index_crypto_error(format!(
            "failed to store owner-side index key in OS keychain: {err}"
        ))
    })?;
    Ok(key)
}

#[cfg(all(test, feature = "os-keychain"))]
fn keychain_disabled_for_tests() -> bool {
    KEYCHAIN_DISABLED_FOR_TESTS.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(all(not(test), feature = "os-keychain"))]
fn keychain_disabled_for_tests() -> bool {
    false
}

fn parse_index_envelope(bytes: &[u8]) -> Result<ParsedIndexEnvelope<'_>, BridgeError> {
    if bytes.len() < INDEX_MAGIC.len() || &bytes[..INDEX_MAGIC.len()] != INDEX_MAGIC {
        return Err(BridgeError::Policy(
            "unsupported plaintext owner-side index; rebuild with `gaze index ingest ...`"
                .to_string(),
        ));
    }

    let min_header_len = INDEX_MAGIC.len() + 1 + 1 + KEY_ID_LEN_BYTES;
    if bytes.len() < min_header_len {
        return Err(index_crypto_error(
            "owner-side index has truncated encrypted header",
        ));
    }

    let version = bytes[INDEX_MAGIC.len()];
    if version != INDEX_FORMAT_VERSION {
        return Err(index_crypto_error(
            "owner-side index has unsupported encrypted format version",
        ));
    }

    let source = IndexKeySource::from_tag(bytes[INDEX_MAGIC.len() + 1])?;
    let key_len_start = INDEX_MAGIC.len() + 2;
    let key_id_len = u16::from_be_bytes([bytes[key_len_start], bytes[key_len_start + 1]]) as usize;
    let key_id_end = min_header_len
        .checked_add(key_id_len)
        .ok_or_else(|| index_crypto_error("owner-side index encrypted header is too large"))?;
    let header_end = key_id_end
        .checked_add(INDEX_ID_LEN)
        .ok_or_else(|| index_crypto_error("owner-side index encrypted header is too large"))?;
    if bytes.len() < header_end {
        return Err(index_crypto_error(
            "owner-side index has truncated encrypted header",
        ));
    }
    let nonce_end = header_end
        .checked_add(NONCE_LEN)
        .ok_or_else(|| index_crypto_error("owner-side index encrypted body is too large"))?;
    let min_body_end = nonce_end
        .checked_add(AEAD_TAG_LEN)
        .ok_or_else(|| index_crypto_error("owner-side index encrypted body is too large"))?;
    if bytes.len() < min_body_end {
        return Err(index_crypto_error(
            "owner-side index has truncated encrypted body",
        ));
    }

    Ok(ParsedIndexEnvelope {
        source,
        key_id: &bytes[min_header_len..key_id_end],
        index_id: &bytes[key_id_end..header_end],
        header: &bytes[..header_end],
        nonce: &bytes[header_end..nonce_end],
        ciphertext: &bytes[nonce_end..],
    })
}

fn encrypt_index_file(
    key: &IndexKey,
    index_id: &[u8; INDEX_ID_LEN],
    plaintext: &[u8],
) -> Result<Vec<u8>, BridgeError> {
    let mut header =
        Vec::with_capacity(INDEX_MAGIC.len() + 1 + 1 + KEY_ID_LEN_BYTES + INDEX_ID_LEN);
    header.extend_from_slice(INDEX_MAGIC);
    header.push(INDEX_FORMAT_VERSION);
    header.push(key.source.tag());
    let key_id_len: u16 = key
        .key_id
        .len()
        .try_into()
        .map_err(|_| index_crypto_error("owner-side index key id is too long"))?;
    header.extend_from_slice(&key_id_len.to_be_bytes());
    header.extend_from_slice(&key.key_id);
    header.extend_from_slice(index_id);

    let cipher = ChaCha20Poly1305::new((&*key.material).into());
    let mut nonce_bytes = [0_u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let aad = index_aad(index_id, &header);
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|err| index_crypto_error(format!("owner-side index encrypt failed: {err}")))?;

    let mut out = Vec::with_capacity(header.len() + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&header);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

fn decrypt_index_file(
    key: &Zeroizing<[u8; 32]>,
    index_id: &[u8],
    envelope: &ParsedIndexEnvelope<'_>,
) -> Result<Zeroizing<Vec<u8>>, BridgeError> {
    let cipher = ChaCha20Poly1305::new((&**key).into());
    let nonce = Nonce::from_slice(envelope.nonce);
    let aad = index_aad(index_id, envelope.header);
    let plaintext = cipher
        .decrypt(
            nonce,
            Payload {
                msg: envelope.ciphertext,
                aad: &aad,
            },
        )
        .map_err(|err| index_crypto_error(format!("owner-side index decrypt failed: {err}")))?;
    Ok(Zeroizing::new(plaintext))
}

fn index_aad(index_id: &[u8], header: &[u8]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(
        INDEX_AAD_CONTEXT.len() + DEFAULT_DOMAIN_ID.len() + index_id.len() + header.len() + 3,
    );
    aad.extend_from_slice(INDEX_AAD_CONTEXT);
    aad.push(0);
    aad.extend_from_slice(DEFAULT_DOMAIN_ID.as_bytes());
    aad.push(0);
    aad.extend_from_slice(index_id);
    aad.push(0);
    aad.extend_from_slice(header);
    aad
}

fn random_index_id() -> [u8; INDEX_ID_LEN] {
    let mut index_id = [0_u8; INDEX_ID_LEN];
    rand::rngs::OsRng.fill_bytes(&mut index_id);
    index_id
}

fn parsed_index_id(envelope: &ParsedIndexEnvelope<'_>) -> Result<[u8; INDEX_ID_LEN], BridgeError> {
    envelope
        .index_id
        .try_into()
        .map_err(|_| index_crypto_error("owner-side index id has invalid length"))
}

#[cfg(feature = "os-keychain")]
fn keychain_account_for_dir(dir: &Path) -> Result<String, BridgeError> {
    Ok(format!(
        "owner-index-v1:{}",
        canonical_index_dir_sha256(dir)?
    ))
}

#[cfg(feature = "os-keychain")]
fn canonical_index_dir_sha256(dir: &Path) -> Result<String, BridgeError> {
    let canonical = dir.canonicalize().map_err(|err| {
        BridgeError::Policy(format!(
            "failed to resolve owner-side index dir {}: {err}",
            dir.display()
        ))
    })?;
    Ok(crate::util::sha256_hex(
        canonical.to_string_lossy().as_ref(),
    ))
}

fn index_crypto_error(message: impl Into<String>) -> BridgeError {
    BridgeError::Policy(format!(
        "owner-side encrypted index failed closed: {}",
        message.into()
    ))
}

#[cfg(unix)]
fn restrict_dir_permissions(path: &Path) -> Result<(), BridgeError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|err| {
        BridgeError::Policy(format!(
            "failed to restrict owner-side index dir {}: {err}",
            path.display()
        ))
    })
}

#[cfg(not(unix))]
fn restrict_dir_permissions(_path: &Path) -> Result<(), BridgeError> {
    Ok(())
}

#[cfg(unix)]
fn restrict_file_permissions(path: &Path) -> Result<(), BridgeError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|err| {
        BridgeError::Policy(format!(
            "failed to restrict owner-side index file {}: {err}",
            path.display()
        ))
    })
}

#[cfg(not(unix))]
fn restrict_file_permissions(_path: &Path) -> Result<(), BridgeError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::model::{IndexEntity, IndexedEntityRef};
    use crate::util::{domain_alias, sha256_hex};

    const RAW_EMAIL: &str = "alice@example.invalid";
    const RAW_NAME: &str = "Dr. Schmidt";
    const TEST_KEY_1: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const TEST_KEY_2: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn saved_index_file_is_aead_sealed_and_omits_plaintext() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _env = IndexKeyEnvGuard::set(TEST_KEY_1);
        let dir = TestDir::new("round-trip");
        let (store, projection_material) = fixture_store(dir.path());
        let expected = store.file.clone();

        store.save().expect("save sealed index");

        let bytes = fs::read(store.index_file_path()).expect("read sealed index");
        assert!(bytes.starts_with(INDEX_MAGIC));
        assert_bytes_do_not_contain(&bytes, RAW_EMAIL.as_bytes(), "raw email");
        assert_bytes_do_not_contain(&bytes, RAW_NAME.as_bytes(), "raw name");
        assert_bytes_do_not_contain(
            &bytes,
            projection_material.as_bytes(),
            "projection key material",
        );

        let loaded = FileCorpusIndexStore::load(dir.path()).expect("load sealed index");
        assert_eq!(loaded.file, expected);
    }

    #[test]
    fn wrong_key_fails_closed_without_plaintext_in_error() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _env = IndexKeyEnvGuard::set(TEST_KEY_1);
        let dir = TestDir::new("wrong-key");
        let (store, projection_material) = fixture_store(dir.path());
        store.save().expect("save sealed index");

        std::env::set_var(INDEX_KEY_ENV, TEST_KEY_2);
        let err = FileCorpusIndexStore::load(dir.path()).expect_err("wrong key must fail");
        let text = format!("{err:?}");
        assert!(!text.contains(RAW_EMAIL));
        assert!(!text.contains(RAW_NAME));
        assert!(!text.contains(&projection_material));
        assert!(text.contains("failed closed"));
    }

    #[test]
    fn tampered_header_or_ciphertext_fails_closed() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _env = IndexKeyEnvGuard::set(TEST_KEY_1);
        let dir = TestDir::new("tamper");
        let (store, _) = fixture_store(dir.path());
        store.save().expect("save sealed index");
        let path = store.index_file_path();
        let original = fs::read(&path).expect("read sealed index");

        let mut header_tampered = original.clone();
        header_tampered[INDEX_MAGIC.len() + 1] = KEY_SOURCE_KEYCHAIN;
        fs::write(&path, header_tampered).expect("write header tamper");
        let err = FileCorpusIndexStore::load(dir.path()).expect_err("header tamper must fail");
        assert!(format!("{err:?}").contains("failed closed"));

        let mut ciphertext_tampered = original;
        let last = ciphertext_tampered
            .last_mut()
            .expect("sealed index has ciphertext");
        *last ^= 0x01;
        fs::write(&path, ciphertext_tampered).expect("write ciphertext tamper");
        let err = FileCorpusIndexStore::load(dir.path()).expect_err("ciphertext tamper must fail");
        assert!(format!("{err:?}").contains("failed closed"));
    }

    #[test]
    fn env_keyed_index_loads_after_directory_move_and_index_id_tamper_fails() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _env = IndexKeyEnvGuard::set(TEST_KEY_1);
        let source_dir = TestDir::new("portable-source");
        let moved_dir = TestDir::new("portable-moved");
        fs::remove_dir_all(moved_dir.path()).expect("remove placeholder moved dir");

        let (store, _) = fixture_store(source_dir.path());
        let expected = store.file.clone();
        store.save().expect("save sealed index");
        fs::rename(source_dir.path(), moved_dir.path()).expect("move sealed index dir");

        let loaded = FileCorpusIndexStore::load(moved_dir.path())
            .expect("env-keyed index must load after directory move");
        assert_eq!(loaded.file, expected);

        let index_path = moved_dir.path().join(INDEX_FILE_NAME);
        let mut bytes = fs::read(&index_path).expect("read moved sealed index");
        let index_id_offset = {
            let envelope = parse_index_envelope(&bytes).expect("parse sealed envelope");
            assert_eq!(envelope.source, IndexKeySource::Env);
            assert_eq!(envelope.index_id.len(), INDEX_ID_LEN);
            envelope.header.len() - INDEX_ID_LEN
        };
        bytes[index_id_offset] ^= 0x80;
        fs::write(&index_path, bytes).expect("write index-id tamper");
        let err = FileCorpusIndexStore::load(moved_dir.path())
            .expect_err("tampered index id must fail closed");
        assert!(format!("{err:?}").contains("failed closed"));
    }

    #[test]
    fn magic_prefixed_truncated_index_fails_closed_for_header_and_body() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _env = IndexKeyEnvGuard::set(TEST_KEY_1);
        let dir = TestDir::new("truncated-magic");
        let index_path = dir.path().join(INDEX_FILE_NAME);

        fs::write(&index_path, INDEX_MAGIC).expect("write truncated header");
        let err = FileCorpusIndexStore::load(dir.path())
            .expect_err("magic-prefixed truncated header must fail closed");
        let text = format!("{err:?}");
        assert!(text.contains("truncated encrypted header"));
        assert!(text.contains("failed closed"));

        let index_id = [0xAB; INDEX_ID_LEN];
        let short_body = vec![0_u8; NONCE_LEN + AEAD_TAG_LEN - 1];
        fs::write(
            &index_path,
            test_envelope_bytes(KEY_SOURCE_ENV, &[], &index_id, &short_body),
        )
        .expect("write truncated body");
        let err = FileCorpusIndexStore::load(dir.path())
            .expect_err("magic-prefixed truncated body must fail closed");
        let text = format!("{err:?}");
        assert!(text.contains("truncated encrypted body"));
        assert!(text.contains("failed closed"));
    }

    #[test]
    #[cfg(feature = "os-keychain")]
    fn keychain_identity_mismatch_fails_closed_without_live_keychain() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _env = IndexKeyEnvGuard::remove();
        let _keychain = KeychainDisabledGuard::new();
        let dir = TestDir::new("keychain-identity-mismatch");
        let index_path = dir.path().join(INDEX_FILE_NAME);
        let index_id = [0xCD; INDEX_ID_LEN];
        let sealed_body = vec![0_u8; NONCE_LEN + AEAD_TAG_LEN];

        fs::write(
            &index_path,
            test_envelope_bytes(
                KEY_SOURCE_KEYCHAIN,
                b"owner-index-v1:not-this-index-dir",
                &index_id,
                &sealed_body,
            ),
        )
        .expect("write keychain-source envelope");

        let err = FileCorpusIndexStore::load(dir.path())
            .expect_err("keychain identity mismatch must fail before live keychain");
        let text = format!("{err:?}");
        assert!(text.contains("keychain identity mismatch"));
        assert!(text.contains("failed closed"));
    }

    #[test]
    fn missing_or_empty_key_source_fails_closed_for_save_and_load() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _keychain = KeychainDisabledGuard::new();

        let missing_save_dir = TestDir::new("missing-key-save");
        let _env = IndexKeyEnvGuard::remove();
        let (missing_store, _) = fixture_store(missing_save_dir.path());
        let err = missing_store
            .save()
            .expect_err("save without key source must fail");
        assert!(format!("{err:?}").contains("failed closed"));
        assert!(!missing_store.index_file_path().exists());

        let empty_save_dir = TestDir::new("empty-key-save");
        std::env::set_var(INDEX_KEY_ENV, "");
        let (empty_store, _) = fixture_store(empty_save_dir.path());
        let err = empty_store
            .save()
            .expect_err("save with empty key source must fail");
        assert!(format!("{err:?}").contains("failed closed"));
        assert!(!empty_store.index_file_path().exists());

        std::env::set_var(INDEX_KEY_ENV, TEST_KEY_1);
        let load_dir = TestDir::new("missing-key-load");
        let (sealed_store, _) = fixture_store(load_dir.path());
        sealed_store.save().expect("save sealed fixture");

        std::env::remove_var(INDEX_KEY_ENV);
        let err = FileCorpusIndexStore::load(load_dir.path())
            .expect_err("load without required env key must fail");
        assert!(format!("{err:?}").contains("failed closed"));
    }

    #[test]
    fn legacy_plaintext_index_is_refused_before_json_parse() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _env = IndexKeyEnvGuard::set(TEST_KEY_1);
        let dir = TestDir::new("legacy-plaintext");
        let index_path = dir.path().join(INDEX_FILE_NAME);
        fs::write(
            &index_path,
            format!(r#"{{"schema_version":1,"entries":[{{"raw_value":"{RAW_EMAIL}"}}]}}"#),
        )
        .expect("write legacy plaintext index");

        let err = FileCorpusIndexStore::load(dir.path())
            .expect_err("legacy plaintext index must fail closed");
        let text = format!("{err:?}");
        assert!(text.contains("unsupported plaintext owner-side index"));
        assert!(!text.contains(RAW_EMAIL));
    }

    // Synthetic re-ingest fixture: two revisions of the same doc_id, distinct raw
    // values, so stale bytes from revision one are detectable in the payload.
    const UPSERT_DOC_ID: &str = "doc:synthetic-upsert-fixture";
    const REV1_MARKER: &str = "SNIPPET-REV1-MARKER-7f3a";
    const REV2_MARKER: &str = "SNIPPET-REV2-MARKER-9c1e";
    const REV1_EMAIL: &str = "rev1.stale@example.invalid";
    const REV2_EMAIL: &str = "rev2.current@example.invalid";
    const REV2_NAME: &str = "Rev Two Person";

    #[test]
    fn reingesting_same_doc_id_replaces_document_and_leaves_no_stale_bytes() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _env = IndexKeyEnvGuard::set(TEST_KEY_1);
        let dir = TestDir::new("upsert");
        let mut store =
            FileCorpusIndexStore::load_or_create(dir.path(), DEFAULT_DOMAIN_ID, &[PiiClass::Email])
                .expect("create store");
        let key_id = store
            .file
            .projection_keys
            .first()
            .expect("projection key")
            .key_id
            .clone();

        let rev1_email = synthetic_entity(&key_id, PiiClass::Email, "email", REV1_EMAIL);
        let rev1_fp = rev1_email.index_ref.fingerprint_hex.clone();
        store.insert_hit(
            DEFAULT_DOMAIN_ID.to_string(),
            IndexSearchHit {
                doc_id: UPSERT_DOC_ID.to_string(),
                snippet: format!("{REV1_MARKER} contact {}", rev1_email.domain_alias),
                entities: vec![rev1_email],
            },
        );

        // Revision two carries two entities so per-fingerprint duplication of the
        // document copy would show up as two REV2_MARKER occurrences.
        let rev2_email = synthetic_entity(&key_id, PiiClass::Email, "email", REV2_EMAIL);
        let rev2_name = synthetic_entity(&key_id, PiiClass::Name, "name", REV2_NAME);
        let rev2_email_fp = rev2_email.index_ref.fingerprint_hex.clone();
        let rev2_name_fp = rev2_name.index_ref.fingerprint_hex.clone();
        store.insert_hit(
            DEFAULT_DOMAIN_ID.to_string(),
            IndexSearchHit {
                doc_id: UPSERT_DOC_ID.to_string(),
                snippet: format!(
                    "{REV2_MARKER} contact {} and {}",
                    rev2_email.domain_alias, rev2_name.domain_alias
                ),
                entities: vec![rev2_email, rev2_name],
            },
        );
        store.save().expect("save sealed index");

        let domain = DEFAULT_DOMAIN_ID.to_string();
        for (label, target) in [
            ("in-memory", &store),
            (
                "reloaded",
                &FileCorpusIndexStore::load(dir.path()).expect("reload"),
            ),
        ] {
            assert!(
                target.hits_for_entity(&domain, &rev1_fp).is_empty(),
                "{label}: stale revision-one entity must not be searchable after upsert"
            );
            let email_hits = target.hits_for_entity(&domain, &rev2_email_fp);
            assert_eq!(
                email_hits.len(),
                1,
                "{label}: exactly one document for rev2 email"
            );
            assert_eq!(email_hits[0].doc_id, UPSERT_DOC_ID);
            assert!(email_hits[0].snippet.starts_with(REV2_MARKER));
            let name_hits = target.hits_for_entity(&domain, &rev2_name_fp);
            assert_eq!(
                name_hits.len(),
                1,
                "{label}: exactly one document for rev2 name"
            );
            assert!(name_hits[0].snippet.starts_with(REV2_MARKER));
        }

        let plaintext = decrypted_index_plaintext(dir.path());
        assert_eq!(
            count_occurrences(&plaintext, REV1_MARKER.as_bytes()),
            0,
            "stale revision-one snippet bytes must be absent from the sealed payload"
        );
        assert_eq!(
            count_occurrences(&plaintext, REV1_EMAIL.as_bytes()),
            0,
            "stale revision-one raw value must be absent from the sealed payload"
        );
        assert_eq!(
            count_occurrences(&plaintext, REV2_MARKER.as_bytes()),
            1,
            "current document snippet must be stored exactly once (not once per fingerprint)"
        );
        assert_eq!(
            count_occurrences(&plaintext, REV2_EMAIL.as_bytes()),
            1,
            "current raw value must be stored exactly once"
        );

        // Erasure reachability: clearing the domain must leave no document bytes behind.
        store.clear_domain(DEFAULT_DOMAIN_ID);
        store.save().expect("save cleared index");
        let cleared = decrypted_index_plaintext(dir.path());
        assert_eq!(count_occurrences(&cleared, REV2_MARKER.as_bytes()), 0);
        assert_eq!(count_occurrences(&cleared, REV2_EMAIL.as_bytes()), 0);
        assert_eq!(count_occurrences(&cleared, REV2_NAME.as_bytes()), 0);
        assert!(FileCorpusIndexStore::load(dir.path())
            .expect("reload cleared")
            .hits_for_entity(&domain, &rev2_email_fp)
            .is_empty());
    }

    #[test]
    fn postings_rebuilt_on_load_match_in_memory_and_documents_persist_once() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _env = IndexKeyEnvGuard::set(TEST_KEY_1);
        let dir = TestDir::new("postings-fidelity");
        let mut store =
            FileCorpusIndexStore::load_or_create(dir.path(), DEFAULT_DOMAIN_ID, &[PiiClass::Email])
                .expect("create store");
        let key_id = store
            .file
            .projection_keys
            .first()
            .expect("projection key")
            .key_id
            .clone();

        // Overlapping fingerprints across four documents:
        //   fp_a -> docs 0,1,2   fp_b -> docs 1,3   fp_c -> doc 2 only
        let entity_a = || {
            synthetic_entity(
                &key_id,
                PiiClass::Email,
                "email",
                "shared.a@example.invalid",
            )
        };
        let entity_b = || {
            synthetic_entity(
                &key_id,
                PiiClass::Email,
                "email",
                "shared.b@example.invalid",
            )
        };
        let entity_c = || synthetic_entity(&key_id, PiiClass::Name, "name", "Only In Doc Two");
        let docs: Vec<(usize, Vec<IndexEntity>)> = vec![
            (0, vec![entity_a()]),
            (1, vec![entity_a(), entity_b()]),
            (2, vec![entity_a(), entity_c()]),
            (3, vec![entity_b()]),
        ];
        let fingerprints = [
            entity_a().index_ref.fingerprint_hex,
            entity_b().index_ref.fingerprint_hex,
            entity_c().index_ref.fingerprint_hex,
            "00".repeat(32),
        ];
        for (index, entities) in docs {
            let aliases = entities
                .iter()
                .map(|entity| entity.domain_alias.clone())
                .collect::<Vec<_>>()
                .join(" ");
            store.insert_hit(
                DEFAULT_DOMAIN_ID.to_string(),
                IndexSearchHit {
                    doc_id: format!("doc:synthetic-{index}"),
                    snippet: format!("doc {index} mentions {aliases}"),
                    entities,
                },
            );
        }
        let domain = DEFAULT_DOMAIN_ID.to_string();
        let before = fingerprints
            .iter()
            .map(|fingerprint| stored_hits(&store, &domain, fingerprint))
            .collect::<Vec<_>>();
        assert_eq!(before[0].len(), 3);
        assert_eq!(before[1].len(), 2);
        assert_eq!(before[2].len(), 1);
        assert!(before[3].is_empty());
        assert_eq!(store.entity_count_for_domain(DEFAULT_DOMAIN_ID), 6);

        store.save().expect("save sealed index");
        let loaded = FileCorpusIndexStore::load(dir.path()).expect("load sealed index");
        let after = fingerprints
            .iter()
            .map(|fingerprint| stored_hits(&loaded, &domain, fingerprint))
            .collect::<Vec<_>>();
        assert_eq!(
            after, before,
            "postings rebuilt on load must be order-stable and identical"
        );
        assert_eq!(loaded.entity_count_for_domain(DEFAULT_DOMAIN_ID), 6);

        // On-disk shape: one document record per distinct doc, no persisted postings.
        let plaintext = decrypted_index_plaintext(dir.path());
        let value: serde_json::Value =
            serde_json::from_slice(&plaintext).expect("sealed payload is json");
        assert_eq!(value["schema_version"], serde_json::json!(SCHEMA_VERSION));
        assert_eq!(
            value["documents"]
                .as_array()
                .expect("documents array persisted")
                .len(),
            4,
            "sealed payload must hold exactly one record per distinct document"
        );
        let mut keys = value
            .as_object()
            .expect("top-level object")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        keys.sort();
        assert_eq!(
            keys,
            [
                "documents",
                "domains",
                "projection_keys",
                "rules",
                "schema_version"
            ],
            "postings are derived on load and must not be persisted"
        );
    }

    #[test]
    fn schema_v1_index_fails_closed_with_typed_schema_error() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _env = IndexKeyEnvGuard::set(TEST_KEY_1);
        let dir = TestDir::new("schema-v1");
        // The v1 layout: per-fingerprint `entries`, each carrying a full hit copy.
        let v1_plaintext = format!(
            concat!(
                r#"{{"schema_version":1,"domains":[],"projection_keys":[],"rules":[],"#,
                r#""entries":[{{"domain_id":"{domain}","fingerprint_hex":"{fp}","#,
                r#""hit":{{"doc_id":"doc:v1","snippet":"legacy","entities":[]}}}}]}}"#
            ),
            domain = DEFAULT_DOMAIN_ID,
            fp = "ab".repeat(32),
        );
        let key = env_index_key().expect("env key").expect("env key set");
        let index_id = [0x5A; INDEX_ID_LEN];
        let sealed = encrypt_index_file(
            &IndexKey {
                source: IndexKeySource::Env,
                key_id: Vec::new(),
                material: key,
            },
            &index_id,
            v1_plaintext.as_bytes(),
        )
        .expect("seal v1 fixture");
        fs::write(dir.path().join(INDEX_FILE_NAME), sealed).expect("write v1 fixture");

        let err = FileCorpusIndexStore::load(dir.path())
            .expect_err("schema v1 index must fail closed on a v2 loader");
        let text = format!("{err:?}");
        assert!(
            text.contains("unsupported owner-side index schema 1; supported 2"),
            "expected typed schema-mismatch error, got: {text}"
        );
        assert!(
            !text.contains("legacy"),
            "error must not echo payload contents"
        );
    }

    #[test]
    fn schema_v2_payload_with_duplicate_document_keys_fails_closed() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _env = IndexKeyEnvGuard::set(TEST_KEY_1);
        let dir = TestDir::new("duplicate-doc-keys");
        let (mut store, _) = fixture_store(dir.path());
        // Bypass the upsert path to forge a payload that violates the
        // one-record-per-(domain, doc_id) invariant.
        let duplicate = store.file.documents[0].clone();
        store.file.documents.push(duplicate);
        store.save().expect("save forged payload");

        let err = FileCorpusIndexStore::load(dir.path())
            .expect_err("duplicate document keys must fail closed on load");
        let text = format!("{err:?}");
        assert!(text.contains("duplicate document keys"), "got: {text}");
        assert!(!text.contains(RAW_EMAIL));
    }

    fn synthetic_entity(key_id: &str, class: PiiClass, prefix: &str, raw: &str) -> IndexEntity {
        let fingerprint_hex = sha256_hex(&format!("{prefix}:{raw}"));
        IndexEntity {
            class: class.clone(),
            raw_value: raw.to_string(),
            index_ref: IndexedEntityRef {
                domain_id: DEFAULT_DOMAIN_ID.to_string(),
                key_id: key_id.to_string(),
                entity_class: class.clone(),
                fingerprint_hex: fingerprint_hex.clone(),
            },
            domain_alias: domain_alias(&class, &fingerprint_hex),
        }
    }

    fn stored_hits(
        store: &FileCorpusIndexStore,
        domain: &str,
        fingerprint: &str,
    ) -> Vec<StoredHit> {
        store
            .hits_for_entity(&domain.to_string(), fingerprint)
            .iter()
            .map(StoredHit::from)
            .collect()
    }

    fn decrypted_index_plaintext(dir: &Path) -> Vec<u8> {
        let bytes = fs::read(dir.join(INDEX_FILE_NAME)).expect("read sealed index");
        let envelope = parse_index_envelope(&bytes).expect("parse sealed envelope");
        let key = env_index_key().expect("env key").expect("env key set");
        decrypt_index_file(&key, envelope.index_id, &envelope)
            .expect("decrypt sealed index")
            .to_vec()
    }

    fn count_occurrences(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .filter(|window| *window == needle)
            .count()
    }

    fn fixture_store(dir: &Path) -> (FileCorpusIndexStore, String) {
        let mut store =
            FileCorpusIndexStore::load_or_create(dir, DEFAULT_DOMAIN_ID, &[PiiClass::Email])
                .expect("create fixture store");
        let projection_key = store
            .file
            .projection_keys
            .first()
            .expect("projection key")
            .clone();
        let fingerprint_hex = sha256_hex(&format!("email:{RAW_EMAIL}"));
        let email_entity = IndexEntity {
            class: PiiClass::Email,
            raw_value: RAW_EMAIL.to_string(),
            index_ref: IndexedEntityRef {
                domain_id: DEFAULT_DOMAIN_ID.to_string(),
                key_id: projection_key.key_id.clone(),
                entity_class: PiiClass::Email,
                fingerprint_hex: fingerprint_hex.clone(),
            },
            domain_alias: domain_alias(&PiiClass::Email, &fingerprint_hex),
        };
        let name_fingerprint_hex = sha256_hex(&format!("name:{RAW_NAME}"));
        let name_entity = IndexEntity {
            class: PiiClass::Name,
            raw_value: RAW_NAME.to_string(),
            index_ref: IndexedEntityRef {
                domain_id: DEFAULT_DOMAIN_ID.to_string(),
                key_id: projection_key.key_id.clone(),
                entity_class: PiiClass::Name,
                fingerprint_hex: name_fingerprint_hex.clone(),
            },
            domain_alias: domain_alias(&PiiClass::Name, &name_fingerprint_hex),
        };

        store.insert_hit(
            DEFAULT_DOMAIN_ID.to_string(),
            IndexSearchHit {
                doc_id: "doc:synthetic-index-fixture".to_string(),
                snippet: format!(
                    "Contact {} via {}",
                    name_entity.domain_alias, email_entity.domain_alias
                ),
                entities: vec![email_entity, name_entity],
            },
        );

        (store, projection_key.material)
    }

    fn assert_bytes_do_not_contain(bytes: &[u8], needle: &[u8], label: &str) {
        assert!(
            !bytes.windows(needle.len()).any(|window| window == needle),
            "sealed index leaked {label}"
        );
    }

    fn test_envelope_bytes(
        source: u8,
        key_id: &[u8],
        index_id: &[u8; INDEX_ID_LEN],
        body: &[u8],
    ) -> Vec<u8> {
        let key_id_len: u16 = key_id.len().try_into().expect("test key id length");
        let mut bytes = Vec::with_capacity(
            INDEX_MAGIC.len() + 1 + 1 + KEY_ID_LEN_BYTES + key_id.len() + INDEX_ID_LEN + body.len(),
        );
        bytes.extend_from_slice(INDEX_MAGIC);
        bytes.push(INDEX_FORMAT_VERSION);
        bytes.push(source);
        bytes.extend_from_slice(&key_id_len.to_be_bytes());
        bytes.extend_from_slice(key_id);
        bytes.extend_from_slice(index_id);
        bytes.extend_from_slice(body);
        bytes
    }

    struct IndexKeyEnvGuard;

    impl IndexKeyEnvGuard {
        fn set(value: &str) -> Self {
            std::env::set_var(INDEX_KEY_ENV, value);
            Self
        }

        fn remove() -> Self {
            std::env::remove_var(INDEX_KEY_ENV);
            Self
        }
    }

    impl Drop for IndexKeyEnvGuard {
        fn drop(&mut self) {
            std::env::remove_var(INDEX_KEY_ENV);
        }
    }

    struct KeychainDisabledGuard;

    impl KeychainDisabledGuard {
        fn new() -> Self {
            #[cfg(feature = "os-keychain")]
            KEYCHAIN_DISABLED_FOR_TESTS.store(true, std::sync::atomic::Ordering::SeqCst);
            Self
        }
    }

    impl Drop for KeychainDisabledGuard {
        fn drop(&mut self) {
            #[cfg(feature = "os-keychain")]
            KEYCHAIN_DISABLED_FOR_TESTS.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(label: &str) -> Self {
            let mut random = [0_u8; 8];
            rand::rngs::OsRng.fill_bytes(&mut random);
            let path =
                std::env::temp_dir().join(format!("gaze-token-bridge-{label}-{}", hex(&random)));
            fs::create_dir_all(&path).expect("create temp test dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
