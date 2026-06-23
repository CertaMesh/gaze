//! Owner-side persistent corpus index.
//!
//! This module deliberately uses private serde structs instead of deriving
//! serialization for [`crate::model::IndexSearchHit`]. Raw values are persisted
//! only inside the owner-side index file; the frozen agent-visible contract stays
//! non-serializable for owner hits.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use gaze::PiiClass;
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::adapter::CorpusIndexStore;
use crate::error::BridgeError;
use crate::model::{
    DomainId, IndexDomain, IndexEntity, IndexSearchHit, IndexedEntityRef, PolicyRule,
};
use crate::util::hex;

pub const DEFAULT_INDEX_DIR: &str = ".gaze-index";
pub const INDEX_FILE_NAME: &str = "index.json";
pub const INDEX_KEY_ENV: &str = "GAZE_INDEX_KEY";
pub const DEFAULT_DOMAIN_ID: &str = "local_owner/docs/v1";
pub const LOCAL_PRINCIPAL_ID: &str = "local_owner";
pub const LOCAL_ROLE: &str = "owner";
pub const LOCAL_TENANT_ID: &str = "local_owner";
pub const LOCAL_WORKSPACE_ID: &str = "owner_workspace";
pub const LOCAL_TOOL_NAME: &str = "gaze_index_search";
pub const LOCAL_ACTION: &str = "search_documents";
pub const LOCAL_PURPOSE: &str = "owner_lookup";

const SCHEMA_VERSION: u32 = 1;
const DEFAULT_MAX_RESULTS: usize = 20;
const INDEX_MAGIC: &[u8] = b"GAZEIDX1";
const INDEX_NONCE_LEN: usize = 12;
const INDEX_AAD: &[u8] = b"gaze-token-bridge:index:v1";

/// File-backed owner-side corpus index store.
#[derive(Debug, Clone)]
pub struct FileCorpusIndexStore {
    dir: PathBuf,
    file: PersistentIndexFile,
}

impl FileCorpusIndexStore {
    /// Load an existing index directory.
    pub fn load(dir: impl AsRef<Path>) -> Result<Self, BridgeError> {
        let dir = dir.as_ref().to_path_buf();
        let index_path = dir.join(INDEX_FILE_NAME);
        let encrypted = fs::read(&index_path).map_err(|err| {
            BridgeError::Policy(format!(
                "failed to read owner-side index {}: {err}",
                index_path.display()
            ))
        })?;
        let contents = decrypt_index_file(&encrypted)?;
        let file: PersistentIndexFile = serde_json::from_slice(&contents).map_err(|err| {
            BridgeError::Policy(format!(
                "failed to parse owner-side index {}: {err}",
                index_path.display()
            ))
        })?;
        if file.schema_version != SCHEMA_VERSION {
            return Err(BridgeError::Policy(format!(
                "unsupported owner-side index schema {}; supported {}",
                file.schema_version, SCHEMA_VERSION
            )));
        }
        Ok(Self { dir, file })
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

        let index_path = self.index_file_path();
        let tmp_path = self.dir.join(format!("{INDEX_FILE_NAME}.tmp"));
        let contents = serde_json::to_vec_pretty(&self.file)
            .map_err(|err| BridgeError::Policy(format!("failed to encode index: {err}")))?;
        let encrypted = encrypt_index_file(&contents)?;
        fs::write(&tmp_path, encrypted).map_err(|err| {
            BridgeError::Policy(format!(
                "failed to write owner-side index {}: {err}",
                tmp_path.display()
            ))
        })?;
        restrict_file_permissions(&tmp_path)?;
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

    pub fn clear_domain(&mut self, domain_id: &str) {
        self.file
            .entries
            .retain(|entry| entry.domain_id != domain_id);
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

    pub fn hit_count_for_domain(&self, domain_id: &str) -> usize {
        self.file
            .entries
            .iter()
            .filter(|entry| entry.domain_id == domain_id)
            .map(|entry| entry.hit.doc_id.as_str())
            .collect::<HashSet<_>>()
            .len()
    }

    pub fn entity_count_for_domain(&self, domain_id: &str) -> usize {
        self.file
            .entries
            .iter()
            .filter(|entry| entry.domain_id == domain_id)
            .count()
    }
}

impl CorpusIndexStore for FileCorpusIndexStore {
    fn insert_hit(&mut self, domain_id: DomainId, hit: IndexSearchHit) {
        let stored_hit = StoredHit::from(&hit);
        let mut fingerprints = HashSet::new();
        for entity in &hit.entities {
            if entity.index_ref.domain_id == domain_id {
                fingerprints.insert(entity.index_ref.fingerprint_hex.clone());
            }
        }
        for fingerprint_hex in fingerprints {
            self.file.entries.push(StoredIndexEntry {
                domain_id: domain_id.clone(),
                fingerprint_hex,
                hit: stored_hit.clone(),
            });
        }
    }

    fn hits_for_entity(&self, domain_id: &DomainId, fingerprint_hex: &str) -> Vec<IndexSearchHit> {
        let mut seen_doc_ids = HashSet::new();
        self.file
            .entries
            .iter()
            .filter(|entry| {
                &entry.domain_id == domain_id && entry.fingerprint_hex == fingerprint_hex
            })
            .filter(|entry| seen_doc_ids.insert(entry.hit.doc_id.clone()))
            .map(|entry| entry.hit.to_index_hit())
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistentIndexFile {
    schema_version: u32,
    domains: Vec<IndexDomain>,
    projection_keys: Vec<StoredProjectionKey>,
    rules: Vec<PolicyRule>,
    entries: Vec<StoredIndexEntry>,
}

impl Default for PersistentIndexFile {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            domains: Vec::new(),
            projection_keys: Vec::new(),
            rules: Vec::new(),
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredProjectionKey {
    key_id: String,
    material: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredIndexEntry {
    domain_id: DomainId,
    fingerprint_hex: String,
    hit: StoredHit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        PiiClass::custom("order_id"),
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
    hex(&bytes)
}

fn encrypt_index_file(plaintext: &[u8]) -> Result<Vec<u8>, BridgeError> {
    let key = index_key_from_env()?;
    encrypt_index_bytes(&key, plaintext)
}

fn decrypt_index_file(bytes: &[u8]) -> Result<Vec<u8>, BridgeError> {
    let key = index_key_from_env()?;
    decrypt_index_bytes(&key, bytes)
}

fn encrypt_index_bytes(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, BridgeError> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let mut nonce_bytes = [0_u8; INDEX_NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad: INDEX_AAD,
            },
        )
        .map_err(|err| BridgeError::Policy(format!("failed to encrypt owner-side index: {err}")))?;

    let mut out = Vec::with_capacity(INDEX_MAGIC.len() + INDEX_NONCE_LEN + ciphertext.len());
    out.extend_from_slice(INDEX_MAGIC);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

fn decrypt_index_bytes(key: &[u8; 32], bytes: &[u8]) -> Result<Vec<u8>, BridgeError> {
    if bytes.len() < INDEX_MAGIC.len() + INDEX_NONCE_LEN
        || &bytes[..INDEX_MAGIC.len()] != INDEX_MAGIC
    {
        return Err(BridgeError::Policy(
            "owner-side index has invalid encrypted header; re-ingest with GAZE_INDEX_KEY"
                .to_string(),
        ));
    }

    let nonce_start = INDEX_MAGIC.len();
    let nonce_end = nonce_start + INDEX_NONCE_LEN;
    let nonce = Nonce::from_slice(&bytes[nonce_start..nonce_end]);
    let cipher = ChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(
            nonce,
            Payload {
                msg: &bytes[nonce_end..],
                aad: INDEX_AAD,
            },
        )
        .map_err(|err| BridgeError::Policy(format!("failed to decrypt owner-side index: {err}")))
}

fn index_key_from_env() -> Result<[u8; 32], BridgeError> {
    let raw = std::env::var(INDEX_KEY_ENV).map_err(|_| {
        BridgeError::Policy(format!(
            "{INDEX_KEY_ENV} is required for encrypted owner-side index"
        ))
    })?;
    parse_index_key(&raw)
}

fn parse_index_key(raw: &str) -> Result<[u8; 32], BridgeError> {
    let trimmed = raw.trim();
    if trimmed.len() == 64 && trimmed.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return decode_hex_key(trimmed);
    }
    if trimmed.len() == 32 {
        let mut key = [0_u8; 32];
        key.copy_from_slice(trimmed.as_bytes());
        return Ok(key);
    }
    Err(BridgeError::Policy(format!(
        "{INDEX_KEY_ENV} must be 32 bytes, provided as 64 hex chars or 32 ASCII chars"
    )))
}

fn decode_hex_key(input: &str) -> Result<[u8; 32], BridgeError> {
    let mut key = [0_u8; 32];
    for (index, chunk) in input.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble(chunk[0]).ok_or_else(invalid_index_key)?;
        let low = hex_nibble(chunk[1]).ok_or_else(invalid_index_key)?;
        key[index] = (high << 4) | low;
    }
    Ok(key)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn invalid_index_key() -> BridgeError {
    BridgeError::Policy(format!("{INDEX_KEY_ENV} contains invalid hex"))
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
    use super::*;

    #[test]
    fn encrypted_index_envelope_round_trips_with_magic() {
        let key = [0x11; 32];
        let plaintext = br#"{"schema_version":1}"#;
        let encrypted = encrypt_index_bytes(&key, plaintext).expect("encrypt");

        assert!(encrypted.starts_with(INDEX_MAGIC));
        assert_ne!(&encrypted[INDEX_MAGIC.len()..], plaintext);
        let decrypted = decrypt_index_bytes(&key, &encrypted).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn parse_index_key_accepts_openssl_hex() {
        let key = parse_index_key(&"22".repeat(32)).expect("parse key");
        assert_eq!(key, [0x22; 32]);
    }

    #[test]
    fn decrypt_index_rejects_plain_json() {
        let key = [0x33; 32];
        let error = decrypt_index_bytes(&key, br#"{"schema_version":1}"#).expect_err("decrypt");

        assert!(
            matches!(error, BridgeError::Policy(message) if message.contains("encrypted header"))
        );
    }
}
