//! Atomic, complete-inventory Text protection. There are deliberately no output hits.
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fmt;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};

use super::*;

const PROFILE: &str = "text-blocks-v1";
const MAX_COUNTER: u64 = 9_007_199_254_740_991;
const TOMBSTONE_TTL: Duration = Duration::from_secs(60);
const TOMBSTONE_CAP: usize = 1_000;

type Result<T> = std::result::Result<T, Failure>;

#[derive(Clone, Copy, Serialize)]
struct Limits {
    transport_frame_bytes: usize,
    v2_request_frame_bytes: usize,
    v2_response_frame_bytes: usize,
    block_raw_bytes: usize,
    active_raw_bytes: usize,
    active_blocks: usize,
    session_cache_bytes: usize,
    session_manifest_bytes: usize,
    global_manifest_bytes: usize,
    global_v2_state_bytes: usize,
    stabilization_passes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            transport_frame_bytes: 160 * 1024 * 1024,
            v2_request_frame_bytes: 64 * 1024 * 1024,
            v2_response_frame_bytes: 64 * 1024 * 1024,
            block_raw_bytes: 10 * 1024 * 1024,
            active_raw_bytes: 32 * 1024 * 1024,
            active_blocks: 4096,
            session_cache_bytes: 32 * 1024 * 1024,
            session_manifest_bytes: 32 * 1024 * 1024,
            global_manifest_bytes: 128 * 1024 * 1024,
            global_v2_state_bytes: 256 * 1024 * 1024,
            stabilization_passes: 8,
        }
    }
}

pub(super) struct Registry {
    daemon_epoch: String,
    // A random assembly identity binds this immutable loaded pipeline instance.
    // No hot reload, persisted certificates, or cross-process reuse is supported.
    pipeline_id: String,
    digest_key: [u8; 32],
    sessions: BTreeMap<String, Live>,
    tombstones: VecDeque<Tombstone>,
    limits: Limits,
}

impl Registry {
    pub(super) fn new() -> std::result::Result<Self, getrandom::Error> {
        let mut digest_key = [0; 32];
        getrandom::fill(&mut digest_key)?;
        Ok(Self {
            daemon_epoch: nonce()?,
            pipeline_id: nonce()?,
            digest_key,
            sessions: BTreeMap::new(),
            tombstones: VecDeque::new(),
            limits: Limits::default(),
        })
    }

    pub(super) fn contains(&self, id: &str) -> bool {
        self.sessions.contains_key(id)
    }

    pub(super) fn evict_idle(&mut self, timeout: Duration) -> Vec<Session> {
        let expired: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, entry)| entry.last_seen.elapsed() >= timeout)
            .map(|(id, _)| id.clone())
            .collect();
        let sessions = expired
            .into_iter()
            .map(|id| self.sessions.remove(&id).expect("expired session").session)
            .collect();
        self.tombstones
            .retain(|entry| entry.created.elapsed() < TOMBSTONE_TTL);
        sessions
    }

    fn digest(&self, frame: &str) -> [u8; 32] {
        // HMAC-SHA256: a retained request digest is not a public PII oracle.
        let mut inner_pad = [0x36; 64];
        let mut outer_pad = [0x5c; 64];
        for (index, byte) in self.digest_key.iter().enumerate() {
            inner_pad[index] ^= byte;
            outer_pad[index] ^= byte;
        }
        let mut inner = Sha256::new();
        inner.update(inner_pad);
        inner.update(frame.as_bytes());
        let mut outer = Sha256::new();
        outer.update(outer_pad);
        outer.update(inner.finalize());
        outer.finalize().into()
    }

    fn replay(&self, response: &RawValue) -> Result<Box<RawValue>> {
        if self.bytes_except("").0.saturating_add(response.get().len())
            > self.limits.global_v2_state_bytes
        {
            return Err(Failure::CacheCapacity);
        }
        Ok(response.to_owned())
    }

    fn bytes_except(&self, active: &str) -> (usize, usize) {
        let mut state = std::mem::size_of::<Self>()
            .saturating_add(4096)
            .saturating_add(
                self.tombstones
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Tombstone>()),
            )
            .saturating_add(self.tombstones.iter().map(Tombstone::bytes).sum::<usize>());
        let mut manifests = 0usize;
        for (id, live) in &self.sessions {
            if id != active {
                state = state.saturating_add(live.bytes());
                manifests = manifests.saturating_add(live.session.manifest_allocation_bytes());
            }
        }
        (state, manifests)
    }
}

struct Live {
    session: Session,
    session_epoch: String,
    projection_id: String,
    revision: u64,
    generation: u64,
    next_seq: u64,
    blocks: Vec<RawBlock>,
    opening_digest: [u8; 32],
    opening: Box<RawValue>,
    receipt: Option<Receipt>,
    last_seen: Instant,
}

impl Live {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + 1024
            + self.session_epoch.capacity()
            + self.projection_id.capacity()
            + self.session.manifest_allocation_bytes().saturating_mul(2)
            + raw_bytes(&self.blocks)
            + self.opening.get().len()
            + self.receipt.as_ref().map_or(0, Receipt::bytes)
    }
}

struct RawBlock {
    id: String,
    sha256: String,
    text: String,
}

fn raw_bytes(blocks: &[RawBlock]) -> usize {
    std::mem::size_of_val(blocks)
        .saturating_mul(4)
        .saturating_add(
            blocks
                .iter()
                .map(|b| b.id.capacity() + b.sha256.capacity() + b.text.capacity() + 64)
                .sum::<usize>(),
        )
}

struct Receipt {
    seq: u64,
    request_id: String,
    digest: [u8; 32],
    response: Box<RawValue>,
    clean: bool,
}

impl Receipt {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.request_id.capacity() + self.response.get().len() + 64
    }
}

struct Tombstone {
    session_id: String,
    session_epoch: String,
    projection_id: String,
    receipt: Receipt,
    created: Instant,
}

impl Tombstone {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.session_id.capacity()
            + self.session_epoch.capacity()
            + self.projection_id.capacity()
            + self.receipt.bytes()
            + 64
    }
}

fn nonce() -> std::result::Result<String, getrandom::Error> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Protocol objects reject duplicates without ever deserializing context here.
#[derive(Default)]
struct Object(BTreeMap<String, Box<RawValue>>);

impl<'de> Deserialize<'de> for Object {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct ObjectVisitor;
        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = Object;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("unique protocol fields")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Object, M::Error> {
                let mut fields = BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, Box<RawValue>>()? {
                    if fields.insert(key, value).is_some() {
                        return Err(serde::de::Error::custom("duplicate protocol field"));
                    }
                }
                Ok(Object(fields))
            }
        }
        deserializer.deserialize_map(ObjectVisitor)
    }
}

impl Object {
    fn field<T: serde::de::DeserializeOwned>(&self, name: &str) -> Result<T> {
        self.0
            .get(name)
            .ok_or(Failure::ProtocolInvalid)
            .and_then(|raw| serde_json::from_str(raw.get()).map_err(|_| Failure::ProtocolInvalid))
    }
    fn string(&self, name: &str) -> Result<String> {
        self.field(name)
    }
    fn counter(&self, name: &str) -> Result<u64> {
        let value = self.field::<u64>(name)?;
        if value > MAX_COUNTER {
            return Err(Failure::ProtocolInvalid);
        }
        Ok(value)
    }
    fn id(&self, name: &str) -> Result<String> {
        let id = self.string(name)?;
        valid_id(&id).then_some(id).ok_or(Failure::ProtocolInvalid)
    }
}

fn valid_id(id: &str) -> bool {
    (1..=128).contains(&id.len()) && id.bytes().all(|b| (0x20..=0x7e).contains(&b))
}

/// Recognize any top-level v2 declaration, even a conflicting duplicate version.
pub(super) fn is_v2(line: &str) -> bool {
    struct Probe;
    impl<'de> Visitor<'de> for Probe {
        type Value = bool;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("request object")
        }
        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> std::result::Result<bool, M::Error> {
            let mut found = false;
            while let Some((key, value)) = map.next_entry::<String, Box<RawValue>>()? {
                found |= key == "version" && value.get() == "2";
            }
            Ok(found)
        }
    }
    let mut deserializer = serde_json::Deserializer::from_str(line);
    deserializer.deserialize_map(Probe).unwrap_or(false)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    ProtocolInvalid,
    SessionExists,
    SessionMissing,
    BlockMissing,
    HashMismatch,
    CacheConflict,
    RequestConflict,
    CacheCapacity,
    CacheConvergence,
    Pipeline,
    UnknownToken,
    RestoreFailed,
    ContextInvalid,
    Protection(&'static str),
    /// Confirmed sensitive value with no safe rendering; carries the closed reason.
    Untransformable(&'static str),
}

impl Failure {
    fn variant(self) -> &'static str {
        match self {
            Self::ProtocolInvalid => "ProtocolInvalid",
            Self::SessionExists => "SessionExists",
            Self::SessionMissing => "SessionMissing",
            Self::BlockMissing => "BlockMissing",
            Self::HashMismatch => "HashMismatch",
            Self::CacheConflict => "CacheConflict",
            Self::RequestConflict => "RequestConflict",
            Self::CacheCapacity => "CacheCapacity",
            Self::CacheConvergence => "CacheConvergence",
            Self::Pipeline => "Pipeline",
            Self::UnknownToken => "UnknownToken",
            Self::RestoreFailed => "RestoreFailed",
            Self::ContextInvalid => "PolicyConfig",
            Self::Protection(variant) => variant,
            Self::Untransformable(_) => "Untransformable",
        }
    }
    fn detail(self) -> &'static str {
        match self {
            Self::CacheConflict => "cache state conflict",
            Self::CacheCapacity => "complete request exceeds capacity",
            Self::CacheConvergence => "complete request did not stabilize",
            Self::Untransformable(_) => "confirmed sensitive field cannot be transformed",
            _ => "daemon request failed closed",
        }
    }
    fn reason(self) -> Option<&'static str> {
        match self {
            Self::Untransformable(reason) => Some(reason),
            _ => None,
        }
    }
}

#[derive(Serialize)]
struct Current<'a> {
    daemon_epoch: &'a str,
    session_epoch: &'a str,
    pipeline_id: &'a str,
    framing_profile: &'static str,
    projection_id: &'a str,
    conversation_revision: u64,
    manifest_generation: u64,
    next_request_seq: u64,
}

impl Registry {
    fn current<'a>(&'a self, live: &'a Live) -> Current<'a> {
        Current {
            daemon_epoch: &self.daemon_epoch,
            session_epoch: &live.session_epoch,
            pipeline_id: &self.pipeline_id,
            framing_profile: PROFILE,
            projection_id: &live.projection_id,
            conversation_revision: live.revision,
            manifest_generation: live.generation,
            next_request_seq: live.next_seq,
        }
    }
    fn epochs_match(&self, request: &Object, live: &Live) -> bool {
        request.string("daemon_epoch").ok().as_deref() == Some(&self.daemon_epoch)
            && request.string("session_epoch").ok().as_deref() == Some(&live.session_epoch)
    }
    fn error(&self, failure: Failure, request: Option<&Object>) -> Box<RawValue> {
        #[derive(Serialize)]
        struct ErrorResponse<'a> {
            version: u32,
            session_id: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            request_id: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            request_seq: Option<u64>,
            error: &'static str,
            detail: &'static str,
            #[serde(skip_serializing_if = "Option::is_none")]
            reason: Option<&'static str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            current: Option<Current<'a>>,
        }
        let current = request.and_then(|r| {
            if r.string("op").ok().as_deref() == Some("open") {
                return None;
            }
            let live = self.sessions.get(&r.id("session_id").ok()?)?;
            self.epochs_match(r, live).then(|| self.current(live))
        });
        serde_json::value::to_raw_value(&ErrorResponse {
            version: 2,
            session_id: None,
            request_id: request.and_then(|r| r.id("request_id").ok()),
            request_seq: request.and_then(|r| r.counter("request_seq").ok()),
            error: failure.variant(),
            detail: failure.detail(),
            reason: failure.reason(),
            current,
        })
        .expect("value-free error serializes")
    }
}

#[derive(Serialize)]
struct Envelope<'a> {
    version: u32,
    op: &'a str,
    session_id: &'a str,
    request_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_seq: Option<u64>,
    #[serde(flatten)]
    current: Current<'a>,
}

#[derive(Serialize)]
struct BlockResult {
    id: String,
    sha256: String,
    clean_text: String,
    manifest: Vec<EmittedTokenSpan>,
    tokens: Vec<TokenJson>,
    cache_hit: bool,
    validated_generation: u64,
}

struct LimitedWriter {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("response capacity"));
        }
        self.bytes
            .try_reserve_exact(bytes.len())
            .map_err(|_| io::Error::other("response capacity"))?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn encode(value: &impl Serialize, limit: usize) -> Result<Box<RawValue>> {
    let mut writer = LimitedWriter {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut writer, value).map_err(|_| Failure::CacheCapacity)?;
    RawValue::from_string(String::from_utf8(writer.bytes).map_err(|_| Failure::Pipeline)?)
        .map_err(|_| Failure::Pipeline)
}

impl Daemon {
    pub(super) fn handle_v2(&mut self, line: &str) -> Box<RawValue> {
        if line.len() > self.v2.limits.v2_request_frame_bytes {
            return self.v2.error(Failure::CacheCapacity, None);
        }
        let request = match serde_json::from_str::<Object>(line) {
            Ok(request) => request,
            Err(_) => return self.v2.error(Failure::ProtocolInvalid, None),
        };
        self.evict_idle_sessions();
        match self.v2_operation(&request, line) {
            Ok(response) => response,
            Err(failure) => self.v2.error(failure, Some(&request)),
        }
    }

    fn v2_operation(&mut self, r: &Object, line: &str) -> Result<Box<RawValue>> {
        if r.field::<u32>("version")? != 2 {
            return Err(Failure::ProtocolInvalid);
        }
        let op = r.string("op")?;
        let session_id = r.id("session_id")?;
        let request_id = r.id("request_id")?;
        let specific: &[&str] = match op.as_str() {
            "open" => &["cache"],
            "clean_blocks" => &[
                "expected_conversation_revision",
                "expected_manifest_generation",
                "blocks",
                "context",
            ],
            "resync" => &["blocks", "context"],
            "restore" => &["minimum_manifest_generation", "text"],
            "close" => &[],
            _ => return Err(Failure::ProtocolInvalid),
        };
        let common = [
            "version",
            "op",
            "request_id",
            "session_id",
            "framing_profile",
            "projection_id",
        ];
        let operation_common = [
            "request_seq",
            "daemon_epoch",
            "session_epoch",
            "pipeline_id",
        ];
        if r.0.keys().any(|key| {
            !common.contains(&key.as_str())
                && !specific.contains(&key.as_str())
                && (op == "open" || !operation_common.contains(&key.as_str()))
        }) {
            return Err(Failure::ProtocolInvalid);
        }
        let projection_id = r.id("projection_id")?;
        if r.string("framing_profile")? != PROFILE {
            return Err(Failure::ProtocolInvalid);
        }
        let digest = self.v2.digest(line);
        if op == "open" {
            if r.string("cache")? != "blocks" {
                return Err(Failure::ProtocolInvalid);
            }
            return self.v2_open(session_id, request_id, projection_id, digest);
        }
        let seq = r.counter("request_seq")?;
        r.string("daemon_epoch")?;
        r.string("session_epoch")?;
        r.string("pipeline_id")?;
        if op == "close"
            && self.v2.tombstones.iter().any(|t| {
                t.session_id == session_id
                    && r.string("session_epoch").ok().as_deref() == Some(&t.session_epoch)
            })
        {
            return self.v2_closed_retry(r, &session_id, &request_id, &projection_id, seq, digest);
        }
        if self.sessions.contains_key(&session_id) {
            return Err(Failure::ProtocolInvalid);
        }
        let Some(live) = self.v2.sessions.get(&session_id) else {
            return self.v2_closed_retry(r, &session_id, &request_id, &projection_id, seq, digest);
        };
        if !self.v2.epochs_match(r, live) {
            return Err(Failure::CacheConflict);
        }
        if r.string("pipeline_id")? != self.v2.pipeline_id || projection_id != live.projection_id {
            return Err(Failure::ProtocolInvalid);
        }
        if let Some(receipt) = &live.receipt {
            if seq == receipt.seq {
                if request_id != receipt.request_id || digest != receipt.digest {
                    return Err(Failure::RequestConflict);
                }
                // Required model health is not exposed as a durable identity. A
                // clean receipt cannot bypass fresh admission in that configuration.
                if receipt.clean
                    && (self.pipeline.safety_net_count() > 0
                        || self.policy.ner.is_some()
                        || self.policy.dob_judge.is_some())
                {
                    return Err(Failure::CacheConflict);
                }
                let response = self.v2.replay(&receipt.response)?;
                self.v2
                    .sessions
                    .get_mut(&session_id)
                    .expect("live receipt")
                    .last_seen = Instant::now();
                return Ok(response);
            }
        }
        if seq != live.next_seq || seq == MAX_COUNTER {
            return Err(Failure::CacheConflict);
        }
        let response = match op.as_str() {
            "clean_blocks" | "resync" => self.v2_clean(r, &session_id, &request_id, &op, seq)?,
            "restore" => self.v2_restore(r, &session_id, &request_id, seq)?,
            "close" => self.v2_close(&session_id, &request_id, seq, digest)?,
            _ => return Err(Failure::ProtocolInvalid),
        };
        if op != "close" {
            let live = self
                .v2
                .sessions
                .get_mut(&session_id)
                .expect("validated live session");
            live.next_seq = seq + 1;
            live.last_seen = Instant::now();
            live.receipt = Some(Receipt {
                seq,
                request_id,
                digest,
                response: response.clone(),
                clean: op != "restore",
            });
        }
        Ok(response)
    }

    fn v2_open(
        &mut self,
        session_id: String,
        request_id: String,
        projection_id: String,
        digest: [u8; 32],
    ) -> Result<Box<RawValue>> {
        if self.sessions.contains_key(&session_id) {
            return Err(Failure::ProtocolInvalid);
        }
        if let Some(live) = self.v2.sessions.get(&session_id) {
            if live.opening_digest != digest {
                return Err(Failure::SessionExists);
            }
            let response = self.v2.replay(&live.opening)?;
            self.v2
                .sessions
                .get_mut(&session_id)
                .expect("live opening")
                .last_seen = Instant::now();
            return Ok(response);
        }
        let session = Session::from_policy(&self.policy).map_err(|_| Failure::Pipeline)?;
        let epoch = nonce().map_err(|_| Failure::Pipeline)?;
        #[derive(Serialize)]
        struct Opened<'a> {
            #[serde(flatten)]
            envelope: Envelope<'a>,
            capabilities: [&'static str; 1],
            limits: Limits,
        }
        let current = Current {
            daemon_epoch: &self.v2.daemon_epoch,
            session_epoch: &epoch,
            pipeline_id: &self.v2.pipeline_id,
            framing_profile: PROFILE,
            projection_id: &projection_id,
            conversation_revision: 0,
            manifest_generation: 0,
            next_request_seq: 1,
        };
        let response = encode(
            &Opened {
                envelope: Envelope {
                    version: 2,
                    op: "open",
                    session_id: &session_id,
                    request_id: &request_id,
                    request_seq: None,
                    current,
                },
                capabilities: ["block-cache-v1"],
                limits: self.v2.limits,
            },
            self.v2.limits.v2_response_frame_bytes,
        )?;
        let live = Live {
            session,
            session_epoch: epoch,
            projection_id,
            revision: 0,
            generation: 0,
            next_seq: 1,
            blocks: Vec::new(),
            opening_digest: digest,
            opening: response.clone(),
            receipt: None,
            last_seen: Instant::now(),
        };
        if self
            .v2
            .bytes_except(&session_id)
            .0
            .saturating_add(live.bytes())
            .saturating_add(response.get().len())
            > self.v2.limits.global_v2_state_bytes
        {
            return Err(Failure::CacheCapacity);
        }
        self.make_session_room();
        self.v2.sessions.insert(session_id, live);
        Ok(response)
    }

    fn v2_closed_retry(
        &self,
        r: &Object,
        id: &str,
        request_id: &str,
        projection_id: &str,
        seq: u64,
        digest: [u8; 32],
    ) -> Result<Box<RawValue>> {
        if r.string("daemon_epoch")? != self.v2.daemon_epoch {
            return Err(Failure::CacheConflict);
        }
        let epoch = r.string("session_epoch")?;
        let pipeline = r.string("pipeline_id")?;
        if let Some(t) = self
            .v2
            .tombstones
            .iter()
            .find(|t| t.session_id == id && t.session_epoch == epoch)
        {
            if pipeline != self.v2.pipeline_id || projection_id != t.projection_id {
                return Err(Failure::ProtocolInvalid);
            }
            return if t.receipt.seq == seq
                && t.receipt.request_id == request_id
                && t.receipt.digest == digest
            {
                self.v2.replay(&t.receipt.response)
            } else {
                Err(Failure::RequestConflict)
            };
        }
        Err(Failure::SessionMissing)
    }

    fn v2_clean(
        &mut self,
        r: &Object,
        id: &str,
        request_id: &str,
        op: &str,
        seq: u64,
    ) -> Result<Box<RawValue>> {
        let live = &self.v2.sessions[id];
        let limits = self.v2.limits;
        if op == "clean_blocks"
            && (r.counter("expected_conversation_revision")? != live.revision
                || r.counter("expected_manifest_generation")? != live.generation)
        {
            return Err(Failure::CacheConflict);
        }
        if live.revision == MAX_COUNTER || live.generation == MAX_COUNTER {
            return Err(Failure::CacheCapacity);
        }
        let inputs =
            r.0.get("context")
                .map(|raw| {
                    let context = TypedContext::from_json_str(raw.get())
                        .map_err(|_| Failure::ContextInvalid)?;
                    if let Some(reason) = context.untransformable_reason() {
                        return Err(Failure::Untransformable(reason.as_str()));
                    }
                    let pipeline = gaze_assembly::pipeline_with_context(
                        &self.pipeline,
                        &self.policy,
                        &context,
                        &self.rulepacks,
                        &self.locale_chain,
                    )
                    .map_err(|_| Failure::ContextInvalid)?;
                    let dictionaries = DictionaryBundle::merge(
                        self.dictionaries.clone(),
                        gaze::dictionary_bundle_from_context(&context),
                    );
                    Ok::<_, Failure>((pipeline, dictionaries))
                })
                .transpose()?;
        let inventory = inventory(r, live, op == "resync", limits)?;
        let (pipeline, dictionaries) = inputs
            .as_ref()
            .map(|(p, d)| (p, d))
            .unwrap_or((&self.pipeline, &self.dictionaries));
        let policy = safety_net_policy(self.safety_net_mode, self.safety_net_fallback);
        let (other_bytes, other_manifests) = self.v2.bytes_except(id);
        let base_bytes = check_stage(
            limits,
            live,
            &inventory,
            &[],
            live.session.manifest_allocation_bytes(),
            other_bytes,
            other_manifests,
            0,
        )?;
        let mut transaction = live.session.begin_transaction();
        transaction
            .set_manifest_byte_limit(transaction_manifest_limit(
                limits,
                other_manifests,
                live.session.manifest_allocation_bytes(),
            ))
            .map_err(|_| Failure::CacheCapacity)?;
        transaction.enable_deterministic_sweep();
        let initial = transaction.semantic_revision();
        let audit = self
            .logger
            .defer_redactions(limits.global_v2_state_bytes.saturating_sub(base_bytes))
            .map_err(|_| Failure::Pipeline)?;
        let mut results = None;
        for _ in 0..limits.stabilization_passes {
            audit.reset_pass().map_err(|_| Failure::Pipeline)?;
            let before = transaction.semantic_revision();
            let mut pass = Vec::new();
            let mut reports = Vec::new();
            for block in &inventory {
                let (clean, manifest, report) = pipeline
                    .clean_transaction_with_safety_net_policy_detect_context(
                        &mut transaction,
                        RawDocument::Text(block.text.clone()),
                        self.locale_chain.as_slice(),
                        dictionaries,
                        policy,
                    )
                    .map_err(|error| {
                        if audit.capacity_exceeded() {
                            Failure::CacheCapacity
                        } else {
                            protection_failure(pipeline, error)
                        }
                    })?;
                enforce_safety_net_mode(&report, policy)
                    .map_err(|error| Failure::Protection(DaemonError::Cli(error).variant()))?;
                let gaze::CleanDocument::Text(clean_text) = clean else {
                    return Err(Failure::Pipeline);
                };
                reports.push(report);
                pass.push(BlockResult {
                    id: block.id.clone(),
                    sha256: block.sha256.clone(),
                    clean_text,
                    manifest,
                    tokens: Vec::new(),
                    cache_hit: false,
                    validated_generation: 0,
                });
                check_stage(
                    limits,
                    live,
                    &inventory,
                    &pass,
                    transaction.manifest_allocation_bytes(),
                    other_bytes.saturating_add(buffered_audit_bytes(&audit, &reports)?),
                    other_manifests,
                    0,
                )?;
            }
            let snapshot = transaction.snapshot_entries();
            for index in 0..pass.len() {
                let result = &mut pass[index];
                let entries = snapshot
                    .iter()
                    .filter(|entry| result.clean_text.contains(&entry.token))
                    .cloned()
                    .collect();
                result.tokens = wire_ordered_entries(entries, &result.clean_text, &result.manifest)
                    .into_iter()
                    .map(TokenJson::from)
                    .collect();
                check_stage(
                    limits,
                    live,
                    &inventory,
                    &pass,
                    transaction.manifest_allocation_bytes(),
                    other_bytes.saturating_add(buffered_audit_bytes(&audit, &reports)?),
                    other_manifests,
                    0,
                )?;
            }
            if before == transaction.semantic_revision() {
                results = Some((pass, reports));
                break;
            }
        }
        let (mut results, reports) = results.ok_or(Failure::CacheConvergence)?;
        let generation = live.generation + u64::from(initial != transaction.semantic_revision());
        for (result, raw) in results.iter_mut().zip(&inventory) {
            result.validated_generation = generation;
            validate_result(&transaction, result, raw)?;
        }
        #[derive(Serialize)]
        struct Cleaned<'a> {
            #[serde(flatten)]
            envelope: Envelope<'a>,
            blocks: &'a [BlockResult],
        }
        let manifest_bytes = transaction.manifest_allocation_bytes();
        let staged_bytes = check_stage(
            limits,
            live,
            &inventory,
            &results,
            manifest_bytes,
            other_bytes.saturating_add(buffered_audit_bytes(&audit, &reports)?),
            other_manifests,
            0,
        )?;
        let response_limit = limits
            .v2_response_frame_bytes
            .min(limits.global_v2_state_bytes.saturating_sub(staged_bytes) / 2);
        let mut current = self.v2.current(live);
        current.conversation_revision += 1;
        current.manifest_generation = generation;
        current.next_request_seq = seq + 1;
        let response = encode(
            &Cleaned {
                envelope: Envelope {
                    version: 2,
                    op,
                    session_id: id,
                    request_id,
                    request_seq: Some(seq),
                    current,
                },
                blocks: &results,
            },
            response_limit,
        )?;
        check_stage(
            limits,
            live,
            &inventory,
            &results,
            manifest_bytes,
            other_bytes.saturating_add(buffered_audit_bytes(&audit, &reports)?),
            other_manifests,
            response.get().len(),
        )?;
        let audit_session_id = live.session.audit_session_id().to_string();
        let prepared = transaction.prepare_commit().map_err(|error| match error {
            gaze::SessionTransactionError::CapacityExceeded => Failure::CacheCapacity,
            _ => Failure::CacheConflict,
        })?;
        audit
            .flush(&reports, audit_session_id)
            .map_err(|_| Failure::Pipeline)?;
        prepared.commit();
        let live = self.v2.sessions.get_mut(id).expect("validated session");
        live.blocks = inventory;
        live.revision += 1;
        live.generation = generation;
        Ok(response)
    }

    fn v2_restore(
        &self,
        r: &Object,
        id: &str,
        request_id: &str,
        seq: u64,
    ) -> Result<Box<RawValue>> {
        let live = &self.v2.sessions[id];
        if r.counter("minimum_manifest_generation")? > live.generation {
            return Err(Failure::CacheConflict);
        }
        let text = r.string("text")?;
        let transaction = live.session.begin_transaction();
        let available = self
            .v2
            .limits
            .global_v2_state_bytes
            .saturating_sub(self.v2.bytes_except(id).0)
            .saturating_sub(live.bytes())
            .saturating_sub(transaction.manifest_allocation_bytes().saturating_mul(2));
        let assessment = transaction
            .assess_restore_text_bounded(
                &text,
                self.v2.limits.v2_response_frame_bytes.min(available / 4),
            )
            .map_err(|error| match error {
                gaze::BoundedRestoreError::UnknownToken
                | gaze::BoundedRestoreError::MalformedToken => Failure::UnknownToken,
                gaze::BoundedRestoreError::CapExceeded => Failure::CacheCapacity,
                _ => Failure::RestoreFailed,
            })?;
        restore_pass2_validate(&assessment, RestoreMode::Strict)
            .map_err(|_| Failure::UnknownToken)?;
        let text = assessment.into_restored().text;
        #[derive(Serialize)]
        struct Restored<'a> {
            #[serde(flatten)]
            envelope: Envelope<'a>,
            text: &'a str,
        }
        let staged_bytes = self
            .v2
            .bytes_except(id)
            .0
            .saturating_add(live.bytes())
            .saturating_add(transaction.manifest_allocation_bytes().saturating_mul(2))
            .saturating_add(text.capacity())
            .saturating_add(512);
        let response_limit = self.v2.limits.v2_response_frame_bytes.min(
            self.v2
                .limits
                .global_v2_state_bytes
                .saturating_sub(staged_bytes)
                / 2,
        );
        let mut current = self.v2.current(live);
        current.next_request_seq = seq + 1;
        let response = encode(
            &Restored {
                envelope: Envelope {
                    version: 2,
                    op: "restore",
                    session_id: id,
                    request_id,
                    request_seq: Some(seq),
                    current,
                },
                text: &text,
            },
            response_limit,
        )?;
        if self
            .v2
            .bytes_except(id)
            .0
            .saturating_add(live.bytes())
            .saturating_add(transaction.manifest_allocation_bytes().saturating_mul(2))
            .saturating_add(text.capacity())
            .saturating_add(response.get().len().saturating_mul(2))
            > self.v2.limits.global_v2_state_bytes
        {
            return Err(Failure::CacheCapacity);
        }
        Ok(response)
    }

    fn v2_close(
        &mut self,
        id: &str,
        request_id: &str,
        seq: u64,
        digest: [u8; 32],
    ) -> Result<Box<RawValue>> {
        let live = &self.v2.sessions[id];
        let mut current = self.v2.current(live);
        current.next_request_seq = seq + 1;
        let response = encode(
            &Envelope {
                version: 2,
                op: "close",
                session_id: id,
                request_id,
                request_seq: Some(seq),
                current,
            },
            self.v2.limits.v2_response_frame_bytes,
        )?;
        let tombstone = Tombstone {
            session_id: id.into(),
            session_epoch: live.session_epoch.clone(),
            projection_id: live.projection_id.clone(),
            receipt: Receipt {
                seq,
                request_id: request_id.into(),
                digest,
                response: response.clone(),
                clean: false,
            },
            created: Instant::now(),
        };
        if self
            .v2
            .bytes_except(id)
            .0
            .saturating_add(live.bytes())
            .saturating_add(tombstone.bytes())
            .saturating_add(response.get().len())
            > self.v2.limits.global_v2_state_bytes
        {
            return Err(Failure::CacheCapacity);
        }
        self.v2.sessions.remove(id);
        self.v2.tombstones.push_back(tombstone);
        while self.v2.tombstones.len() > TOMBSTONE_CAP {
            self.v2.tombstones.pop_front();
        }
        Ok(response)
    }

    pub(super) fn make_session_room(&mut self) {
        while self.sessions.len() + self.v2.sessions.len() >= self.session_cap {
            let v1 = self.lru.front().and_then(|id| {
                self.sessions
                    .get(id)
                    .map(|entry| (id.clone(), entry.last_seen))
            });
            let v2 = self
                .v2
                .sessions
                .iter()
                .min_by_key(|(_, e)| e.last_seen)
                .map(|(id, e)| (id.clone(), e.last_seen));
            match (v1, v2) {
                (Some((_, time)), Some((other, other_time))) if other_time < time => {
                    if let Some(entry) = self.v2.sessions.remove(&other) {
                        self.log_eviction(&entry.session, "lru");
                    }
                }
                (Some((id, _)), _) => {
                    if let Some(entry) = self.sessions.remove(&id) {
                        self.log_eviction(&entry.session, "lru");
                    }
                    self.lru.retain(|entry| entry != &id);
                }
                (None, Some((id, _))) => {
                    if let Some(entry) = self.v2.sessions.remove(&id) {
                        self.log_eviction(&entry.session, "lru");
                    }
                }
                (None, None) => break,
            }
        }
    }
}

fn buffered_audit_bytes(audit: &DeferredRedactions<'_>, reports: &[LeakReport]) -> Result<usize> {
    let bytes = audit.bytes().map_err(|_| Failure::Pipeline)?;
    reports.iter().try_fold(bytes, |bytes, report| {
        Ok(bytes.saturating_add(report_allocation_bytes(report)?))
    })
}

fn class_allocation_bytes(class: &PiiClass) -> usize {
    match class {
        PiiClass::Custom(value) => value.capacity(),
        PiiClass::Email | PiiClass::Name | PiiClass::Location | PiiClass::Organization => 0,
    }
}

fn report_allocation_bytes(report: &LeakReport) -> Result<usize> {
    // Vec growth can retain up to four slots for one report. Charge four
    // headers per element to cover the outer pass Vec conservatively too.
    let mut bytes = std::mem::size_of::<LeakReport>()
        .saturating_mul(4)
        .saturating_add(
            report
                .suspects
                .capacity()
                .saturating_mul(std::mem::size_of::<gaze::LeakSuspect>()),
        )
        .saturating_add(
            report
                .telemetry
                .capacity()
                .saturating_mul(std::mem::size_of::<gaze_types::LeakReportTelemetry>()),
        )
        .saturating_add(report.replay_hash.as_ref().map_or(0, String::capacity));
    for suspect in &report.suspects {
        bytes = bytes
            .saturating_add(suspect.safety_net_id.capacity())
            .saturating_add(suspect.raw_label.capacity())
            .saturating_add(suspect.field_path.as_ref().map_or(0, String::capacity))
            .saturating_add(class_allocation_bytes(&suspect.class));
        match &suspect.kind {
            gaze::LeakKind::ClassMismatch {
                pipeline_class,
                safety_net_class,
            } => {
                bytes = bytes
                    .saturating_add(class_allocation_bytes(pipeline_class))
                    .saturating_add(class_allocation_bytes(safety_net_class));
            }
            gaze::LeakKind::Uncovered | gaze::LeakKind::PartialBleed { .. } => {}
            _ => return Err(Failure::CacheCapacity),
        }
    }
    for event in &report.telemetry {
        use gaze_types::LeakReportTelemetry;
        let (id, path, class) = match event {
            LeakReportTelemetry::LocaleSkipped {
                safety_net_id,
                field_path,
                ..
            }
            | LeakReportTelemetry::ModelSpanRefused {
                safety_net_id,
                field_path,
                ..
            } => (safety_net_id, field_path, 0),
            LeakReportTelemetry::UnactionableSubword {
                safety_net_id,
                field_path,
                class,
                ..
            } => (safety_net_id, field_path, class_allocation_bytes(class)),
            _ => return Err(Failure::CacheCapacity),
        };
        bytes = bytes
            .saturating_add(id.capacity())
            .saturating_add(path.as_ref().map_or(0, String::capacity))
            .saturating_add(class);
    }
    Ok(bytes)
}

fn protection_failure(pipeline: &gaze::Pipeline, error: gaze::Error) -> Failure {
    if matches!(error, gaze::Error::SessionCapacity) {
        return Failure::CacheCapacity;
    }
    match map_clean_error(pipeline, error) {
        CliError::Untransformable { reason } => Failure::Untransformable(reason.as_str()),
        cli => Failure::Protection(DaemonError::Cli(cli).variant()),
    }
}

fn inventory(r: &Object, live: &Live, resync: bool, limits: Limits) -> Result<Vec<RawBlock>> {
    let requested = r.field::<Vec<Object>>("blocks")?;
    if requested.len() > limits.active_blocks {
        return Err(Failure::CacheCapacity);
    }
    let mut ids = HashSet::new();
    let mut blocks = Vec::new();
    let mut total = 0usize;
    for block in requested {
        if block
            .0
            .keys()
            .any(|key| !["id", "sha256", "text"].contains(&key.as_str()))
        {
            return Err(Failure::ProtocolInvalid);
        }
        let id = block.id("id")?;
        if !ids.insert(id.clone()) {
            return Err(Failure::ProtocolInvalid);
        }
        let sha256 = block.string("sha256")?;
        if sha256.len() != 64
            || !sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Failure::ProtocolInvalid);
        }
        let text = if block.0.contains_key("text") {
            let text = block.string("text")?;
            if hash(&text) != sha256 {
                return Err(Failure::HashMismatch);
            }
            text
        } else {
            if resync {
                return Err(Failure::BlockMissing);
            }
            live.blocks
                .iter()
                .find(|b| b.id == id && b.sha256 == sha256)
                .ok_or(Failure::BlockMissing)?
                .text
                .clone()
        };
        total = total
            .checked_add(text.len())
            .ok_or(Failure::CacheCapacity)?;
        if text.len() > limits.block_raw_bytes || total > limits.active_raw_bytes {
            return Err(Failure::CacheCapacity);
        }
        blocks.push(RawBlock { id, sha256, text });
    }
    if raw_bytes(&blocks) > limits.session_cache_bytes {
        return Err(Failure::CacheCapacity);
    }
    Ok(blocks)
}

fn hash(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn result_bytes(results: &[BlockResult]) -> usize {
    std::mem::size_of_val(results)
        .saturating_mul(4)
        .saturating_add(
            results
                .iter()
                .map(|b| {
                    b.id.capacity()
                        + b.sha256.capacity()
                        + b.clean_text.capacity()
                        + b.manifest.capacity() * 256
                        + b.manifest
                            .iter()
                            .map(|span| match &span.class {
                                PiiClass::Custom(name) => name.capacity(),
                                _ => 0,
                            })
                            .sum::<usize>()
                        + b.tokens.capacity() * 128
                        + b.tokens
                            .iter()
                            .map(|t| {
                                t.token.capacity()
                                    + t.class.capacity()
                                    + t.family.as_ref().map_or(0, String::capacity)
                            })
                            .sum::<usize>()
                })
                .sum::<usize>(),
        )
}

fn transaction_manifest_limit(
    limits: Limits,
    other_manifests: usize,
    live_manifest: usize,
) -> usize {
    limits.session_manifest_bytes.min(
        limits
            .global_manifest_bytes
            .saturating_sub(other_manifests)
            .saturating_sub(live_manifest),
    )
}

#[allow(clippy::too_many_arguments)]
fn check_stage(
    limits: Limits,
    live: &Live,
    inventory: &[RawBlock],
    results: &[BlockResult],
    manifest: usize,
    other: usize,
    other_manifests: usize,
    response: usize,
) -> Result<usize> {
    let cache = raw_bytes(inventory).saturating_add(result_bytes(results));
    // Old and staged manifests coexist. Reserve another manifest-sized charge
    // for matcher/index state, and two encoded responses (outgoing + receipt).
    let state = live
        .bytes()
        .saturating_add(manifest.saturating_mul(2))
        .saturating_add(cache)
        .saturating_add(response.saturating_mul(2))
        .saturating_add(512)
        .saturating_add(other);
    if cache > limits.session_cache_bytes
        || manifest > limits.session_manifest_bytes
        || other_manifests
            .saturating_add(live.session.manifest_allocation_bytes())
            .saturating_add(manifest)
            > limits.global_manifest_bytes
        || state > limits.global_v2_state_bytes
    {
        return Err(Failure::CacheCapacity);
    }
    Ok(state)
}

fn validate_result(
    transaction: &gaze::SessionTransaction<'_>,
    result: &BlockResult,
    raw: &RawBlock,
) -> Result<()> {
    let mut previous = 0;
    for span in &result.manifest {
        let Some(token) = result.clean_text.get(span.clean_span.clone()) else {
            return Err(Failure::Pipeline);
        };
        if span.clean_span.start < previous
            || raw.text.get(span.raw_span.clone()).is_none()
            || (gaze::token_shape::contains_token(token) && !transaction.contains_token(token))
        {
            return Err(Failure::Pipeline);
        }
        previous = span.clean_span.end;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
