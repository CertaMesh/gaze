use super::*;
use serde_json::{json, Value};

fn daemon() -> Daemon {
    use clap::Parser;
    #[derive(Parser)]
    struct TestArgs {
        #[command(flatten)]
        daemon: Args,
    }
    let dir = tempfile::tempdir().unwrap();
    let policy = dir.path().join("policy.toml");
    std::fs::write(
        &policy,
        r#"
[session]
scope = "conversation"
[policy.rulepacks]
bundled = ["core"]
[[policy.custom_recognizers]]
kind = "regex"
name = "synthetic_email"
pattern = '[A-Za-z0-9._%+-]+@example[.]invalid'
class = "email"
[[rule]]
kind = "class"
class = "email"
action = "tokenize"
[[rule]]
kind = "class"
class = "name"
action = "tokenize"
[[rule]]
kind = "default"
action = "preserve"
"#,
    )
    .unwrap();
    Daemon::new(TestArgs::parse_from(["daemon", "--policy", policy.to_str().unwrap()]).daemon)
        .unwrap()
}

fn raw(d: &mut Daemon, frame: &str) -> Value {
    serde_json::to_value(d.handle_line(frame)).unwrap()
}
fn send(d: &mut Daemon, request: Value) -> Value {
    raw(d, &request.to_string())
}
fn opening(id: &str) -> Value {
    json!({"version":2,"op":"open","request_id":"opening-a","session_id":id,"cache":"blocks","framing_profile":PROFILE,"projection_id":"projection-v1"})
}
fn open(d: &mut Daemon, id: &str) -> Value {
    let result = send(d, opening(id));
    assert_eq!(result["op"], "open", "{result}");
    result
}
fn operation(state: &Value, op: &str) -> Value {
    let mut value = json!({"version":2,"op":op,"request_id":"attempt-a","request_seq":state["next_request_seq"],"session_id":state["session_id"],
        "daemon_epoch":state["daemon_epoch"],"session_epoch":state["session_epoch"],"pipeline_id":state["pipeline_id"],"framing_profile":PROFILE,"projection_id":state["projection_id"]});
    if op == "clean_blocks" {
        value["expected_conversation_revision"] = state["conversation_revision"].clone();
        value["expected_manifest_generation"] = state["manifest_generation"].clone();
    }
    value
}
fn block(id: &str, text: &str) -> Value {
    json!({"id":id,"sha256":hash(text),"text":text})
}
fn clean_request(state: &Value, blocks: Vec<Value>) -> Value {
    let mut value = operation(state, "clean_blocks");
    value["blocks"] = json!(blocks);
    value
}
fn clean(d: &mut Daemon, state: &Value, blocks: Vec<Value>) -> Value {
    let result = send(d, clean_request(state, blocks));
    assert_eq!(result["op"], "clean_blocks", "{result}");
    result
}
fn assert_error(value: &Value, variant: &str) {
    assert_eq!(value["error"], variant, "{value}");
    assert!(value.get("blocks").is_none());
    assert!(value.get("text").is_none());
    assert!(value.get("tokens").is_none());
    assert!(value["session_id"].is_null());
}

#[test]
fn opening_receipt_sequences_and_exact_frame_retries() {
    let mut d = daemon();
    let opened = open(&mut d, "conversation-a");
    assert_eq!(opened["limits"]["transport_frame_bytes"], 167772160);
    assert_eq!(opened["limits"]["v2_request_frame_bytes"], 67108864);
    assert_eq!(send(&mut d, opening("conversation-a")), opened);
    let request = clean_request(&opened, vec![block("leaf-a", "alice@example.invalid")]);
    let frame = request.to_string();
    let first = raw(&mut d, &frame);
    assert_eq!(first["conversation_revision"], 1);
    assert_eq!(first["manifest_generation"], 1);
    assert_eq!(raw(&mut d, &frame), first);
    assert_error(&raw(&mut d, &format!(" {frame}")), "RequestConflict");
    assert_eq!(
        send(&mut d, opening("conversation-a")),
        opened,
        "lost open receipt does not reset counters"
    );
    let mut altered = opening("conversation-a");
    altered["request_id"] = json!("opening-b");
    assert_error(&send(&mut d, altered), "SessionExists");
    let second = clean(
        &mut d,
        &first,
        vec![json!({"id":"leaf-a","sha256":hash("alice@example.invalid")})],
    );
    assert_eq!(second["manifest_generation"], 1);
    assert_eq!(second["conversation_revision"], 2);
    assert_eq!(second["blocks"][0]["cache_hit"], false);
    assert_error(&raw(&mut d, &frame), "CacheConflict");
    let stale = raw(&mut d, &frame);
    assert_eq!(stale["current"]["next_request_seq"], 3);
    // Receipt eviction leaves the sequence high-water mark intact.
    d.v2.sessions.get_mut("conversation-a").unwrap().receipt = None;
    let mut resync = operation(&second, "resync");
    resync["blocks"] = json!([block("leaf-a", "alice@example.invalid")]);
    let synced = send(&mut d, resync.clone());
    assert_eq!(synced["conversation_revision"], 3);
    assert_eq!(send(&mut d, resync.clone()), synced);
    d.v2.sessions.get_mut("conversation-a").unwrap().receipt = None;
    assert_error(&send(&mut d, resync), "CacheConflict");
}

#[test]
fn strict_restore_deletion_compaction_and_close_tombstone() {
    let mut d = daemon();
    let opened = open(&mut d, "conversation-a");
    let first = clean(
        &mut d,
        &opened,
        vec![block("leaf-a", "alice@example.invalid")],
    );
    let token = first["blocks"][0]["clean_text"]
        .as_str()
        .unwrap()
        .to_string();
    let empty = clean(&mut d, &first, vec![]);
    let mut restore = operation(&empty, "restore");
    restore["minimum_manifest_generation"] = json!(1);
    restore["text"] = json!(token);
    let restored = send(&mut d, restore.clone());
    assert_eq!(restored["text"], "alice@example.invalid");
    assert_eq!(restored["conversation_revision"], 2);
    assert_eq!(restored["manifest_generation"], 1);
    assert_eq!(send(&mut d, restore), restored);
    let close = operation(&restored, "close");
    let closed = send(&mut d, close.clone());
    assert_eq!(closed["op"], "close");
    assert!(!d.v2.contains("conversation-a"));
    assert_eq!(send(&mut d, close.clone()), closed);
    let mut conflicting = close.clone();
    conflicting["request_id"] = json!("different");
    assert_error(&send(&mut d, conflicting), "RequestConflict");
    let reopened = open(&mut d, "conversation-a");
    assert_ne!(reopened["session_epoch"], opened["session_epoch"]);
    assert_eq!(send(&mut d, close.clone()), closed);
    assert!(d.v2.contains("conversation-a"));
    d.v2.tombstones.front_mut().unwrap().created = Instant::now() - Duration::from_secs(61);
    assert_error(&send(&mut d, close), "CacheConflict");
    assert!(d.v2.contains("conversation-a"));
}

#[test]
fn tombstone_expiry_eviction_restart_and_session_isolation() {
    let mut d = daemon();
    let a = open(&mut d, "conversation-a");
    let a = clean(&mut d, &a, vec![block("same-id", "alice@example.invalid")]);
    let b = open(&mut d, "conversation-b");
    let mut missing = clean_request(
        &b,
        vec![json!({"id":"same-id","sha256":hash("alice@example.invalid")})],
    );
    assert_error(&send(&mut d, missing.clone()), "BlockMissing");
    missing["session_epoch"] = a["session_epoch"].clone();
    let conflict = send(&mut d, missing);
    assert_error(&conflict, "CacheConflict");
    assert!(conflict.get("current").is_none());
    let b = clean(&mut d, &b, vec![block("same-id", "alice@example.invalid")]);
    assert_ne!(a["blocks"][0]["clean_text"], b["blocks"][0]["clean_text"]);
    let close = operation(&a, "close");
    send(&mut d, close.clone());
    d.v2.tombstones.front_mut().unwrap().created = Instant::now() - Duration::from_secs(61);
    assert_error(&send(&mut d, close), "SessionMissing");
    let old_request = clean_request(&b, vec![]);
    d.v2.sessions.get_mut("conversation-b").unwrap().last_seen =
        Instant::now() - Duration::from_secs(4000);
    assert_error(&send(&mut d, old_request.clone()), "SessionMissing");
    let fresh = open(&mut d, "conversation-b");
    assert_ne!(b["session_epoch"], fresh["session_epoch"]);
    assert_error(&send(&mut d, old_request.clone()), "CacheConflict");
    let mut restarted = daemon();
    open(&mut restarted, "conversation-b");
    let conflict = send(&mut restarted, old_request);
    assert_error(&conflict, "CacheConflict");
    assert!(conflict.get("current").is_none());
}

#[test]
fn invalid_protocol_objects_never_fall_back_to_v1() {
    let mut d = daemon();
    let opened = open(&mut d, "conversation-a");
    let base = clean_request(&opened, vec![block("leaf-a", "")]);
    for (field, value, variant) in [
        (
            "projection_id",
            json!("other-projection"),
            "ProtocolInvalid",
        ),
        ("framing_profile", json!("json-fields"), "ProtocolInvalid"),
        ("pipeline_id", json!("other-pipeline"), "ProtocolInvalid"),
        ("session_epoch", json!("other-epoch"), "CacheConflict"),
        ("daemon_epoch", json!("other-epoch"), "CacheConflict"),
        ("request_seq", json!(0), "CacheConflict"),
        ("request_seq", json!(2), "CacheConflict"),
        ("request_seq", json!(-1), "ProtocolInvalid"),
        ("request_seq", json!(MAX_COUNTER + 1), "ProtocolInvalid"),
        ("expected_manifest_generation", json!(1), "CacheConflict"),
        ("expected_conversation_revision", json!(1), "CacheConflict"),
        ("request_id", json!(""), "ProtocolInvalid"),
        ("request_id", json!("x".repeat(129)), "ProtocolInvalid"),
        ("request_id", json!("private\nvalue"), "ProtocolInvalid"),
        ("unexpected", json!(true), "ProtocolInvalid"),
        ("context", Value::Null, "PolicyConfig"),
    ] {
        let mut request = base.clone();
        request[field] = value;
        assert_error(&send(&mut d, request), variant);
        assert_eq!(d.v2.sessions["conversation-a"].next_seq, 1);
        assert!(d.v2.sessions["conversation-a"]
            .session
            .snapshot_entries()
            .is_empty());
    }
    let mut duplicate = base.to_string();
    duplicate.insert_str(1, "\"request_id\":\"duplicate\",");
    assert_error(&raw(&mut d, &duplicate), "ProtocolInvalid");
    let nested_duplicate = base
        .to_string()
        .replace("\"id\":\"leaf-a\"", "\"id\":\"leaf-a\",\"id\":\"leaf-b\"");
    assert_error(&raw(&mut d, &nested_duplicate), "ProtocolInvalid");
    let mut duplicate_version = base.to_string();
    duplicate_version.insert_str(1, "\"version\":1,");
    assert_error(&raw(&mut d, &duplicate_version), "ProtocolInvalid");
    let mut request = base.clone();
    request["blocks"] = json!([block("leaf-a", ""), block("leaf-a", "")]);
    assert_error(&send(&mut d, request), "ProtocolInvalid");
    let mut request = base.clone();
    request["blocks"][0]["sha256"] = json!("0".repeat(64));
    assert_error(&send(&mut d, request), "HashMismatch");
    let mut request = base.clone();
    request["blocks"][0]["unknown"] = json!(true);
    assert_error(&send(&mut d, request), "ProtocolInvalid");
    for op in ["clean", "restore", "close", "open"] {
        assert_error(
            &send(
                &mut d,
                json!({"version":1,"op":op,"session_id":"conversation-a","text":"owner text"}),
            ),
            "ProtocolInvalid",
        );
    }
    assert_error(
        &send(
            &mut d,
            json!({"session_id":"conversation-a","text":"owner text"}),
        ),
        "ProtocolInvalid",
    );
    assert_eq!(d.v2.sessions["conversation-a"].next_seq, 1);
    clean(&mut d, &opened, vec![block("leaf-a", "")]);
}

#[test]
fn resync_requires_every_body_and_forbids_revision_preconditions() {
    let mut d = daemon();
    let opened = open(&mut d, "conversation-a");
    let first = clean(
        &mut d,
        &opened,
        vec![block("leaf-a", "alice@example.invalid")],
    );
    let mut request = operation(&first, "resync");
    request["blocks"] = json!([{"id":"leaf-a","sha256":hash("alice@example.invalid")}]);
    assert_error(&send(&mut d, request.clone()), "BlockMissing");
    request["blocks"] = json!([block("leaf-a", "alice@example.invalid")]);
    request["expected_conversation_revision"] = json!(1);
    assert_error(&send(&mut d, request.clone()), "ProtocolInvalid");
    request
        .as_object_mut()
        .unwrap()
        .remove("expected_conversation_revision");
    let response = send(&mut d, request);
    assert_eq!(response["op"], "resync");
    assert_eq!(response["blocks"], first["blocks"]);
}

#[test]
fn capacity_failures_are_atomic_and_do_not_consume_sequences() {
    for limit in [
        "block",
        "raw",
        "count",
        "cache",
        "manifest",
        "global-manifest",
        "global-state",
        "response",
        "passes",
        "request",
    ] {
        let mut d = daemon();
        let opened = open(&mut d, "conversation-a");
        let first = clean(
            &mut d,
            &opened,
            vec![block("leaf-a", "alice@example.invalid")],
        );
        let original = d.v2.sessions["conversation-a"].session.snapshot_entries();
        match limit {
            "block" => d.v2.limits.block_raw_bytes = 1,
            "raw" => d.v2.limits.active_raw_bytes = 1,
            "count" => d.v2.limits.active_blocks = 1,
            "cache" => d.v2.limits.session_cache_bytes = 1,
            "manifest" => d.v2.limits.session_manifest_bytes = 1,
            "global-manifest" => d.v2.limits.global_manifest_bytes = 1,
            "global-state" => d.v2.limits.global_v2_state_bytes = 1,
            "response" => d.v2.limits.v2_response_frame_bytes = 32,
            "passes" => d.v2.limits.stabilization_passes = 0,
            "request" => d.v2.limits.v2_request_frame_bytes = 1,
            _ => unreachable!(),
        }
        let result = send(
            &mut d,
            clean_request(
                &first,
                vec![
                    json!({"id":"leaf-a","sha256":hash("alice@example.invalid")}),
                    block("leaf-b", "bob@example.invalid"),
                ],
            ),
        );
        assert_error(
            &result,
            if limit == "passes" {
                "CacheConvergence"
            } else {
                "CacheCapacity"
            },
        );
        let live = &d.v2.sessions["conversation-a"];
        assert_eq!(live.session.snapshot_entries(), original, "{limit}");
        assert_eq!(live.blocks.len(), 1, "{limit}");
        assert_eq!(live.next_seq, 2, "{limit}");
        assert_eq!(live.revision, 1, "{limit}");
        assert_eq!(live.generation, 1, "{limit}");
    }
}

#[test]
fn restore_limits_generation_and_unknown_tokens_fail_closed() {
    let mut d = daemon();
    let opened = open(&mut d, "conversation-a");
    let first = clean(
        &mut d,
        &opened,
        vec![block("leaf-a", "alice@example.invalid")],
    );
    let mut request = operation(&first, "restore");
    request["minimum_manifest_generation"] = json!(2);
    request["text"] = first["blocks"][0]["clean_text"].clone();
    assert_error(&send(&mut d, request.clone()), "CacheConflict");
    request["minimum_manifest_generation"] = json!(0);
    request["text"] = json!("<deadbeef:Email_999>");
    assert_error(&send(&mut d, request.clone()), "UnknownToken");
    request["text"] = first["blocks"][0]["clean_text"].clone();
    d.v2.limits.v2_response_frame_bytes = 8;
    assert_error(&send(&mut d, request), "CacheCapacity");
    assert_eq!(d.v2.sessions["conversation-a"].next_seq, 2);
}

/// A cue-only rule teaches a synthetic name after its first bare occurrence.
struct LateName;
impl gaze::Recognizer for LateName {
    fn id(&self) -> &str {
        "synthetic.cue"
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn evidence(&self) -> gaze::EvidenceKind {
        gaze::EvidenceKind::Rule
    }
    fn detect(
        &self,
        text: &str,
        _: &gaze::DetectContext<'_>,
    ) -> std::result::Result<Vec<gaze::Candidate>, gaze_types::DetectError> {
        let Some(start) = text
            .find("Record name: ")
            .map(|i| i + "Record name: ".len())
        else {
            return Ok(vec![]);
        };
        let end = text[start..].find('.').map_or(text.len(), |i| start + i);
        Ok(vec![gaze::Candidate::new(
            start..end,
            PiiClass::Name,
            self.id(),
            1.0,
            100,
            None,
            "counter",
            self.id(),
            ConflictTier::None,
            vec![],
        )
        .with_evidence(gaze::EvidenceKind::Rule)])
    }
}
fn late_pipeline() -> gaze::Pipeline {
    gaze::Pipeline::builder()
        .recognizer(LateName)
        .rule(gaze::ClassRule::new(PiiClass::Name, Action::Tokenize))
        .rule(gaze::DefaultRule::new(Action::Preserve))
        .build()
        .unwrap()
}

/// Reference helper forks the SAME pre-turn namespace into an uncommitted
/// transaction. It neither imports a fresh prefix nor uses daemon batch code.
fn oracle(
    d: &Daemon,
    id: &str,
    texts: &[String],
) -> (
    Vec<Value>,
    Vec<SessionSnapshotEntry>,
    Vec<Option<gaze::SourceSpelling>>,
) {
    let live = &d.v2.sessions[id];
    let mut reference = live.session.begin_transaction();
    reference.enable_deterministic_sweep();
    let mut output;
    loop {
        let before = reference.semantic_revision();
        output = Vec::new();
        for text in texts {
            let (clean, manifest, _) = d
                .pipeline
                .clean_transaction_with_safety_net_policy_detect_context(
                    &mut reference,
                    RawDocument::Text(text.clone()),
                    d.locale_chain.as_slice(),
                    &d.dictionaries,
                    safety_net_policy(d.safety_net_mode, d.safety_net_fallback),
                )
                .unwrap();
            let gaze::CleanDocument::Text(clean) = clean else {
                panic!("Text");
            };
            let tokens: Vec<_> =
                wire_ordered_entries(reference.snapshot_entries(), &clean, &manifest)
                    .into_iter()
                    .filter(|e| clean.contains(&e.token))
                    .map(TokenJson::from)
                    .collect();
            output.push(json!({"clean_text":clean,"manifest":manifest,"tokens":tokens}));
        }
        if reference.semantic_revision() == before {
            break;
        }
    }
    let entries = reference.snapshot_entries();
    let spellings = entries
        .iter()
        .map(|e| reference.source_spelling(&e.token))
        .collect();
    (output, entries, spellings)
}

#[test]
fn twenty_turn_same_namespace_oracle_covers_late_values_reorder_edit_and_old_restore() {
    let mut d = daemon();
    d.pipeline = late_pipeline();
    let mut state = open(&mut d, "conversation-a");
    let mut texts = vec!["Earlier Selene Exampleton.".to_string(), "".to_string()];
    let mut oldest = None;
    for turn in 0..20 {
        texts.push(if turn % 2 == 0 {
            format!("Record name: Selene Exampleton{turn}.")
        } else {
            format!("Earlier Selene Exampleton{prev}.", prev = turn - 1)
        });
        if turn == 4 {
            texts.push("Record name: Selene Exampleton.".into());
        }
        if turn == 8 {
            texts.reverse();
        }
        if turn == 12 {
            texts.remove(0);
        }
        if turn == 16 {
            texts[0] = "Edited complete leaf.".into();
        }
        let (expected, entries, spellings) = oracle(&d, "conversation-a", &texts);
        state = clean(
            &mut d,
            &state,
            texts
                .iter()
                .enumerate()
                .map(|(i, t)| block(&format!("leaf-{i}"), t))
                .collect(),
        );
        for (actual, expected) in state["blocks"].as_array().unwrap().iter().zip(expected) {
            for key in ["clean_text", "manifest", "tokens"] {
                assert_eq!(actual[key], expected[key], "turn {turn}: {key}");
            }
            assert_eq!(actual["cache_hit"], false);
        }
        let session = &d.v2.sessions["conversation-a"].session;
        assert_eq!(session.snapshot_entries(), entries);
        for (entry, spelling) in entries.iter().zip(spellings) {
            assert_eq!(session.source_spelling(&entry.token), spelling);
            assert_eq!(session.restore_strict(&entry.token).unwrap(), entry.raw);
        }
        if oldest.is_none() {
            oldest = entries.first().cloned();
        }
        if let Some(old) = &oldest {
            assert_eq!(session.restore_strict(&old.token).unwrap(), old.raw);
        }
    }
}

#[test]
fn late_last_block_revisits_earlier_text_and_oversize_inventory_never_partially_commits() {
    let mut d = daemon();
    d.pipeline = late_pipeline();
    let opened = open(&mut d, "conversation-a");
    let first = clean(
        &mut d,
        &opened,
        vec![block("leaf-a", "Prior Selene Exampleton.")],
    );
    assert_eq!(first["blocks"][0]["clean_text"], "Prior Selene Exampleton.");
    let blocks = vec![
        block("leaf-a", "Prior Selene Exampleton."),
        block("leaf-b", "Record name: Selene Exampleton."),
    ];
    let request = clean_request(&first, blocks.clone());
    d.v2.limits.active_raw_bytes = 40;
    assert_error(&send(&mut d, request.clone()), "CacheCapacity");
    assert!(d.v2.sessions["conversation-a"]
        .session
        .snapshot_entries()
        .is_empty());
    d.v2.limits = Limits::default();
    let result = send(&mut d, request);
    assert_eq!(result["op"], "clean_blocks", "{result}");
    assert!(!result["blocks"][0]["clean_text"]
        .as_str()
        .unwrap()
        .contains("Selene"));
    assert_eq!(result["manifest_generation"], 1);
    assert_eq!(result["blocks"][0]["validated_generation"], 1);
}

#[test]
fn v1_response_fixtures_remain_byte_exact_and_large_escaped_restore_is_accepted() {
    let mut d = daemon();
    let open = d.handle_line(r#"{"version":1,"op":"open","session_id":"legacy"}"#);
    assert_eq!(
        serde_json::to_string(&open).unwrap(),
        r#"{"version":1,"op":"open","session_id":"legacy"}"#
    );
    let clean =
        d.handle_line(r#"{"version":1,"op":"clean","session_id":"legacy","text":"benign"}"#);
    assert_eq!(
        serde_json::to_string(&clean).unwrap(),
        r#"{"version":1,"op":"clean","session_id":"legacy","clean_text":"benign","manifest":[],"tokens":[]}"#
    );
    // >64 MiB encoded, <160 MiB transport; v1 keeps its acceptance contract.
    let frame = format!(
        r#"{{"version":1,"op":"restore","session_id":"legacy","text":"{}"}}"#,
        "\\u0020".repeat(12 * 1024 * 1024)
    );
    let restored = raw(&mut d, &frame);
    assert_eq!(restored["text"].as_str().unwrap().len(), 12 * 1024 * 1024);
    let close = d.handle_line(r#"{"version":1,"op":"close","session_id":"legacy"}"#);
    assert_eq!(
        serde_json::to_string(&close).unwrap(),
        r#"{"version":1,"op":"close","session_id":"legacy"}"#
    );
}

#[test]
fn context_is_validated_on_every_request_and_does_not_carry_unmatched_values() {
    let mut d = daemon();
    let opened = open(&mut d, "conversation-a");
    let mut request = clean_request(&opened, vec![block("leaf-a", "ordinary text")]);
    request["context"] = json!({"record":{"full_name":"Zelmora Quentrix"}});
    let first = send(&mut d, request);
    assert_eq!(first["op"], "clean_blocks");
    let second = clean(&mut d, &first, vec![block("leaf-a", "Zelmora Quentrix")]);
    assert_eq!(
        second["blocks"][0]["clean_text"], "Zelmora Quentrix",
        "unmatched context is request-scoped"
    );
    let mut request = clean_request(&second, vec![block("leaf-a", "Zelmora Quentrix")]);
    request["context"] = json!({"record":{"full_name":"Zelmora Quentrix"}});
    let third = send(&mut d, request);
    assert!(!third["blocks"][0]["clean_text"]
        .as_str()
        .unwrap()
        .contains("Zelmora"));
    let fourth = clean(&mut d, &third, vec![block("leaf-a", "Zelmora Quentrix")]);
    assert_eq!(
        fourth["blocks"][0]["clean_text"], third["blocks"][0]["clean_text"],
        "matched evidence stays in the manifest"
    );
    let base = clean_request(&fourth, vec![block("leaf-a", "Zelmora Quentrix")]);
    for context in [
        r#"{"record":{"full_name":"Zelmora Quentrix","full_name":"Velmora Xantrix"}}"#.to_string(),
        format!(
            "{{{}\"record\":{{\"full_name\":\"Zelmora Quentrix\"}}}}",
            " ".repeat(4 * 1024 * 1024)
        ),
        "null".into(),
    ] {
        let mut frame = base.to_string();
        frame.insert_str(1, &format!("\"context\":{context},"));
        let response = raw(&mut d, &frame);
        assert_error(&response, "PolicyConfig");
        assert!(!response.to_string().contains("Zelmora"));
    }
}

#[test]
fn bounded_request_and_response_edges_and_nonterminal_real_v2_frame_ceiling() {
    let mut d = daemon();
    let frame = opening("conversation-a").to_string();
    d.v2.limits.v2_request_frame_bytes = frame.len();
    let opened = raw(&mut d, &frame);
    assert_eq!(opened["op"], "open");
    assert_error(&raw(&mut d, &format!("{frame} ")), "CacheCapacity");
    d.v2.limits = Limits::default();
    let frame = format!(
        "{}{}",
        opening("too-large").to_string(),
        " ".repeat(64 * 1024 * 1024)
    );
    assert_error(&raw(&mut d, &frame), "CacheCapacity");
    open(&mut d, "still-live");
    let small = json!({"value":"\u{0001}"});
    let exact = serde_json::to_string(&small).unwrap().len();
    assert!(encode(&small, exact).is_ok());
    assert_eq!(
        encode(&small, exact - 1).err(),
        Some(Failure::CacheCapacity)
    );
}

#[test]
fn manifest_growth_and_empty_inventory_refuse_even_without_new_text() {
    let mut d = daemon();
    let opened = open(&mut d, "conversation-a");
    let first = clean(
        &mut d,
        &opened,
        vec![block("leaf-a", "alice@example.invalid")],
    );
    let old = d.v2.sessions["conversation-a"].session.snapshot_entries();
    d.v2.limits.session_manifest_bytes = 512;
    assert_error(
        &send(&mut d, clean_request(&first, vec![])),
        "CacheCapacity",
    );
    assert_eq!(
        d.v2.sessions["conversation-a"].session.snapshot_entries(),
        old
    );
    assert_eq!(d.v2.sessions["conversation-a"].next_seq, 2);
}

#[test]
fn shared_session_cap_evicts_whole_namespaces_across_protocols() {
    let mut d = daemon();
    d.session_cap = 1;
    let first = open(&mut d, "conversation-a");
    assert_eq!(
        send(
            &mut d,
            json!({"version":1,"op":"open","session_id":"legacy"})
        )["op"],
        "open"
    );
    assert!(!d.v2.contains("conversation-a"));
    assert_error(
        &send(&mut d, clean_request(&first, vec![])),
        "SessionMissing",
    );
    open(&mut d, "conversation-b");
    assert!(!d.sessions.contains_key("legacy"));
}

struct BrokenNet;
impl gaze::SafetyNet for BrokenNet {
    fn id(&self) -> &str {
        "synthetic-broken"
    }
    fn supported_locales(&self) -> &[gaze::LocaleTag] {
        &[]
    }
    fn check(
        &self,
        _: &str,
        _: gaze::SafetyNetContext<'_>,
    ) -> std::result::Result<Vec<gaze::LeakSuspect>, gaze::SafetyNetError> {
        Err(gaze::SafetyNetError::Runtime {
            message: "synthetic outage".into(),
        })
    }
}

#[test]
fn model_health_uncertainty_never_replays_a_clean_receipt() {
    let mut d = daemon();
    let opened = open(&mut d, "conversation-a");
    let request = clean_request(&opened, vec![block("leaf-a", "alice@example.invalid")]);
    let cleaned = send(&mut d, request.clone());
    d.pipeline = gaze::Pipeline::builder()
        .register_safety_net(BrokenNet)
        .rule(gaze::DefaultRule::new(Action::Preserve))
        .build()
        .unwrap();
    assert_error(&send(&mut d, request), "CacheConflict");
    let mut resync = operation(&cleaned, "resync");
    resync["blocks"] = json!([block("leaf-a", "alice@example.invalid")]);
    let response = send(&mut d, resync);
    assert!(response.get("error").is_some());
    assert!(response.get("blocks").is_none());
    assert_eq!(d.v2.sessions["conversation-a"].next_seq, 2);
}

#[test]
fn individual_state_budgets_refuse_before_commit() {
    let mut d = daemon();
    open(&mut d, "conversation-a");
    let live = &d.v2.sessions["conversation-a"];
    let inventory = vec![RawBlock {
        id: "leaf-a".into(),
        sha256: hash("benign"),
        text: "benign".into(),
    }];
    for name in ["cache", "manifest", "global-manifest", "global-state"] {
        let mut limits = Limits::default();
        match name {
            "cache" => limits.session_cache_bytes = 1,
            "manifest" => limits.session_manifest_bytes = 1,
            "global-manifest" => limits.global_manifest_bytes = 1,
            "global-state" => limits.global_v2_state_bytes = 1,
            _ => unreachable!(),
        }
        assert_eq!(
            check_stage(limits, live, &inventory, &[], 1000, 0, 0, 0),
            Err(Failure::CacheCapacity),
            "{name}"
        );
    }
    let opened = serde_json::from_str::<Value>(live.opening.get()).unwrap();
    let r: Object =
        serde_json::from_str(&clean_request(&opened, vec![block("leaf-a", "benign")]).to_string())
            .unwrap();
    let mut limits = Limits::default();
    limits.session_cache_bytes = 1;
    assert_eq!(
        super::inventory(&r, live, false, limits).err(),
        Some(Failure::CacheCapacity)
    );
}
