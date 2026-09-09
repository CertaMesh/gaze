#[path = "support/evidence_harness.rs"]
mod evidence_harness;
use evidence_harness::*;

#[tokio::test]
async fn protected_success_and_golden_receipt() {
    let authored = format!("literal [[g]]{EMAIL}[[/g]]");
    let h = host(false, Output::Echo, &["result"], 0, false, fresh_session());
    let r = call(h.clone(), json!({"text":authored}), false)
        .await
        .expect("response");
    let v = decode(&r);
    let mut c = Counts::default();
    assert!(
        c.score(&h.session, text(&v["result"]), EMAIL, true) == Verdict::Protected,
        "protected-oracle"
    );
    c.restore(&h.session, text(&v["result"]), &authored, EMAIL);
    // Independently observe the protected argument at ToolCtx and the client response.
    c.restore(
        &h.session,
        text(&h.argument.lock().unwrap()["text"]),
        &authored,
        EMAIL,
    );
    c.add(
        "protected_leaves",
        string_leaves(&h.argument.lock().unwrap()) + string_leaves(&v),
    );
    c.observe_host(&h, 2);
    assert!(
        h.session
            .snapshot_entries()
            .iter()
            .any(|e| e.class == PiiClass::Email),
        "typed-email"
    );
    assert!(
        *h.store.events.lock().unwrap() == ["begin", "finish"],
        "terminal-order"
    );
    let emitted = receipt(&c, outcome(Some(&r)), false);
    let fixture: Value = must(serde_json::from_str(GOLDEN));
    assert!(
        must(serde_json::to_vec(&emitted)) == must(serde_json::to_vec(&fixture["receipt"])),
        "golden-receipt"
    );
}
#[tokio::test]
async fn controlled_four_slot_occurrence_oracle() {
    let session = fresh_session();
    let token = must(session.tokenize(&PiiClass::Custom("phone".into()), PHONE));
    let output = json!({"a":PHONE,"b":token,"c":&PHONE[..8],"d":format!("[[g]]{PHONE}[[/g]][[g]]{PHONE}[[/g]]"),"email":EMAIL});
    let h = host(
        true,
        Output::Fixed(output),
        &["a", "b", "c", "d", "email"],
        0,
        false,
        session,
    );
    let r = call(h.clone(), json!({"text":"[[g]][[/g]]"}), false)
        .await
        .expect("response");
    let v = decode(&r);
    let mut c = Counts::default();
    let mut protected = 0;
    for key in ["a", "b", "c", "d"] {
        if c.score(&h.session, text(&v[key]), PHONE, key == "d") == Verdict::Protected {
            protected += 1;
        }
    }
    assert!(
        c.get("gold_occurrences_surviving_egress") == 1
            && c.get("gold_occurrences_partially_surviving_egress") == 1
            && c.get("gold_occurrences_attribution_not_measured") == 1
            && protected == 1,
        "four-exact-verdicts"
    );
    assert!(
        occurrence(&h.session, text(&v["email"]), EMAIL, false) == Verdict::Protected,
        "email-floor"
    );
    assert!(h.session.snapshot_entries().len() == 2, "anchor-benignity");
    c.observe_host(&h, 6);
    let r = receipt(&c, "COMPLETED", true);
    assert!(
        r["counts"]["gold_occurrences_surviving_egress"] == 1,
        "computed-leak"
    );
}
#[tokio::test]
async fn benign_negative_and_positive_false_positive_control() {
    let h = host(false, Output::Echo, &["result"], 0, false, fresh_session());
    let r = call(h.clone(), json!({"text":"ordinary fixture text"}), false)
        .await
        .expect("response");
    let v = decode(&r);
    let mut c = Counts::default();
    c.negative(&h.session, text(&v["result"]), "ordinary fixture text");
    assert!(
        c.get("false_positive_occurrences") == 0
            && c.get("false_positive_bytes") == 0
            && h.session.snapshot_entries().is_empty(),
        "benign-negative"
    );
    // Deliberately authored non-PII fixture identifier with Email spelling.
    let h = host(true, Output::Echo, &["result"], 0, false, fresh_session());
    let r = call(h.clone(), json!({"text":EMAIL}), false)
        .await
        .expect("response");
    let v = decode(&r);
    c.negative(&h.session, text(&v["result"]), EMAIL);
    assert!(
        c.get("false_positive_occurrences") == 1
            && c.get("false_positive_bytes") == EMAIL.len() as u64,
        "positive-fp-control"
    );
    assert!(
        receipt(&c, outcome(Some(&r)), false)["gate_results"]["false_positive_negative_control"]
            == "FAIL",
        "fp-gate-fail"
    );
}
#[tokio::test]
async fn undeclared_carrier_has_positive_no_payload_and_unobserved_leaves() {
    let h = host(false, Output::Echo, &["result"], 0, false, fresh_session());
    let r = call(h.clone(), json!({"text":EMAIL,"extra":EMAIL}), false)
        .await
        .expect("response");
    assert!(
        outcome(Some(&r)) == "FAILED_CLOSED_NO_EGRESS",
        "positive-no-payload"
    );
    let mut c = Counts::default();
    c.observe_host(&h, 3);
    assert!(
        c.get("tool_invocations") == 0
            && c.get("observer_leaves_observed") == 0
            && c.get("observer_leaves_unobserved") == 3,
        "early-leaf-accounting"
    );
    let r = receipt(&c, outcome(Some(&r)), false);
    assert!(
        r["counts"].get("protected_leaves").is_none(),
        "no-rejection-credit"
    );
}
#[tokio::test]
async fn no_payload_classifier_rejects_extra_surfaces() {
    let h = host(false, Output::Echo, &["result"], 0, false, fresh_session());
    let received = call(h, json!({"text":EMAIL,"extra":EMAIL}), false)
        .await
        .expect("response");
    assert!(
        outcome(Some(&received)) == "FAILED_CLOSED_NO_EGRESS",
        "safe-error-control"
    );
    // In-memory classifier falsifiers cloned from a received frame, not production leaks.
    for path in [
        "/structuredContent",
        "/_meta",
        "/content/0/_meta",
        "/content/0/annotations",
    ] {
        let mut value = must(serde_json::to_value(&received));
        let payload = if path.ends_with("annotations") {
            json!({"audience":["user"]})
        } else {
            json!({"synthetic":EMAIL})
        };
        if path.starts_with("/content") {
            value["content"][0][path.rsplit('/').next().unwrap()] = payload;
        } else {
            value[&path[1..]] = payload;
        }
        let adversarial: CallToolResult = must(serde_json::from_value(value));
        assert!(
            outcome(Some(&adversarial)) == "UNKNOWN_EGRESS",
            "extra-surface-unknown"
        );
    }
}
#[test]
fn missing_measurements_and_ambiguous_only_gates() {
    let c = Counts::default();
    for state in ["COMPLETED", "UNKNOWN_EGRESS", "FAILED_CLOSED_NO_EGRESS"] {
        let r = receipt(&c, state, true);
        assert!(
            r["counts"].as_object().unwrap().is_empty(),
            "absent-measurements"
        );
        for gate in [
            "string_byte_reversibility",
            "egress_integrity_analogues",
            "false_positive_negative_control",
            "gold_survival_oracle",
        ] {
            assert!(
                r["gate_results"][gate] == "NOT_EVALUABLE",
                "unperformed-gate"
            );
        }
    }
    let mut c = Counts::default();
    c.score(&fresh_session(), "[[g]]missing", PHONE, true);
    let r = receipt(&c, "COMPLETED", true);
    assert!(
        r["gate_results"]["gold_survival_oracle"] == "NOT_EVALUABLE",
        "ambiguous-gate"
    );
    assert!(
        r["counts"]["gold_bytes_surviving_egress"] == 0
            && r["counts"].get("leaf_restore_exact").is_none(),
        "observed-zero-no-restore"
    );
}
#[tokio::test]
async fn known_token_continuity() {
    let h = host(false, Output::Echo, &["result"], 0, false, fresh_session());
    let a = decode(
        &call(h.clone(), json!({"text":EMAIL}), false)
            .await
            .expect("response"),
    );
    let b = decode(
        &call(h.clone(), json!({"text":a["result"]}), false)
            .await
            .expect("response"),
    );
    assert!(
        a == b && h.session.snapshot_entries().len() == 1,
        "known-token-continuity"
    );
    assert!(
        must(h.session.restore_strict_text(text(&b["result"]))) == EMAIL,
        "exact-string-bytes"
    );
}
#[tokio::test]
async fn response_conflict_rolls_back_but_failed_finish_retains_mappings() {
    let h = host(
        false,
        Output::Fixed(json!({"result":FRESH})),
        &["result"],
        3,
        false,
        fresh_session(),
    );
    let r = call(h.clone(), json!({"text":"benign"}), false)
        .await
        .expect("response");
    assert!(
        outcome(Some(&r)) == "FAILED_CLOSED_NO_EGRESS",
        "conflict-no-payload"
    );
    assert!(
        *h.store.events.lock().unwrap() == ["begin", "fail"]
            && h.session.snapshot_entries().iter().all(|e| e.raw != FRESH),
        "rollback-no-losing-mappings"
    );
    let h = host(
        false,
        Output::Fixed(json!({"result":FRESH})),
        &["result"],
        0,
        true,
        fresh_session(),
    );
    let r = call(h.clone(), json!({"text":"benign"}), false)
        .await
        .expect("response");
    assert!(
        outcome(Some(&r)) == "FAILED_CLOSED_NO_EGRESS",
        "finish-no-payload"
    );
    assert!(
        *h.store.events.lock().unwrap() == ["begin", "finish"]
            && h.session.snapshot_entries().iter().any(|e| e.raw == FRESH),
        "failed-finish-retains-committed"
    );
}
#[tokio::test]
async fn timeout_is_unknown_without_partial_wire_observation() {
    let h = host(false, Output::Wait, &["result"], 0, false, fresh_session());
    let r = call(h.clone(), json!({"text":EMAIL}), true).await;
    assert!(outcome(r.as_ref()) == "UNKNOWN_EGRESS", "timeout-unknown");
    let mut c = Counts::default();
    c.observe_host(&h, 2);
    c.add("gold_occurrences_planned", 1);
    c.add("gold_bytes_planned", EMAIL.len());
    let r = receipt(&c, "UNKNOWN_EGRESS", false);
    assert!(
        r["counts"]
            .get("unknown_egress_lower_bound_cases")
            .is_none()
            && r["counts"].get("protected_leaves").is_none(),
        "unknown-not-zero-or-credit"
    );
}
#[tokio::test]
async fn escaped_unicode_and_below_floor_are_occurrence_scoped() {
    const V: &str = "synthetic \"äöü\\value";
    let h = host(
        true,
        Output::Fixed(json!({"result":{"leaf":V}})),
        &["result", "result.leaf"],
        0,
        false,
        fresh_session(),
    );
    let r = call(h.clone(), json!({"text":"benign"}), false)
        .await
        .expect("response");
    let v = decode(&r);
    assert!(
        occurrence(&h.session, text(&v["result"]["leaf"]), V, false) == Verdict::Full,
        "schema-directed-nested-decode"
    );
    assert!(prefix("äöüäö", 9).len() == 8, "whole-codepoint-prefix");
    assert!(
        occurrence(&h.session, "äöüä", "äöüäö", false) == Verdict::Partial(8),
        "unicode-fragment"
    );
    assert!(
        occurrence(&h.session, &PHONE[..7], PHONE, false) == Verdict::Unknown,
        "below-floor-no-credit"
    );
    assert!(
        occurrence(&h.session, "[[g]]missing", PHONE, true) == Verdict::Unknown,
        "missing-anchor-no-credit"
    );
}
#[tokio::test]
async fn observer_passive_token_covered_and_raw_gap_are_distinct() {
    for mode in [0, 1, 2] {
        let h = host(
            false,
            Output::Echo,
            &["result"],
            mode,
            false,
            fresh_session(),
        );
        let r = call(h, json!({"text":format!("literal {EMAIL}")}), false)
            .await
            .expect("response");
        assert!(
            outcome(Some(&r))
                == if mode == 1 {
                    "FAILED_CLOSED_NO_EGRESS"
                } else {
                    "COMPLETED"
                },
            "observer-raw-gap-discriminates"
        );
    }
}
#[tokio::test]
async fn integrity_analogues_have_independent_nonzero_falsifiers() {
    let s = fresh_session();
    let first = must(s.tokenize(&PiiClass::Email, EMAIL));
    let second = must(s.tokenize(&PiiClass::Email, FRESH));
    let h = host(
        true,
        Output::Fixed(json!({"a":second,"b":first})),
        &["a", "b"],
        0,
        false,
        s,
    );
    let r = call(h.clone(), json!({"text":"benign"}), false)
        .await
        .expect("response");
    let v = decode(&r);
    let mut c = Counts::default();
    c.restore(&h.session, text(&v["a"]), EMAIL, EMAIL);
    c.restore(&h.session, text(&v["b"]), FRESH, FRESH);
    assert!(
        c.get("egress_raw_value_mismatches") == 2 && c.get("egress_token_restore_failures") == 0,
        "independent-slot-swap"
    );
    assert!(
        receipt(&c, outcome(Some(&r)), true)["gate_results"]["egress_integrity_analogues"]
            == "FAIL",
        "swap-gate-fail"
    );
    let bad = first.replace("Email_1", "Email_999999");
    assert!(
        h.session.restore_strict_text(&bad).is_err(),
        "reserved-unknown-token"
    );
    c.restore(&h.session, &bad, EMAIL, EMAIL);
    assert!(
        c.get("egress_token_restore_failures") == 1 && c.get("leaf_restore_decision_failures") == 1,
        "restore-error-counted"
    );
    // This second receipt measures a deliberately mutated restore operand, not route egress.
    assert!(
        receipt(&c, "COMPLETED", true)["gate_results"]["egress_integrity_analogues"] == "FAIL",
        "restore-gate-fail"
    );
}
#[tokio::test]
async fn json_text_string_bytes_are_stricter_than_semantic_equality() {
    const ORIGINAL: &str = "{ \"n\": 1 }";
    const CHANGED: &str = "{\"n\":1}";
    let h = host(
        true,
        Output::Fixed(json!({"result":CHANGED})),
        &["result"],
        0,
        false,
        fresh_session(),
    );
    let r = call(h.clone(), json!({"text":ORIGINAL}), false)
        .await
        .expect("response");
    let v = decode(&r);
    let restored = must(h.session.restore_strict_text(text(&v["result"])));
    assert!(
        must(serde_json::from_str::<Value>(&restored))
            == must(serde_json::from_str::<Value>(ORIGINAL)),
        "semantic-equality-control"
    );
    let mut c = Counts::default();
    c.restore(&h.session, text(&v["result"]), ORIGINAL, ORIGINAL);
    assert!(
        c.get("leaf_restore_exact") == 0 && restored != ORIGINAL,
        "string-bytes-discriminate"
    );
    assert!(
        receipt(&c, outcome(Some(&r)), true)["counts"]
            .get("egress_raw_value_mismatches")
            .is_none(),
        "no-authorized-range-no-raw-check"
    );
    assert!(
        receipt(&c, outcome(Some(&r)), true)["gate_results"]["string_byte_reversibility"] == "FAIL",
        "string-gate-fail"
    );
}
#[test]
fn vocabularies_match_committed_artifact() {
    let golden: Value = must(serde_json::from_str(GOLDEN));
    let mirrors = mirrored_vocabularies();
    assert!(
        golden["vocabularies"]
            .as_object()
            .expect("vocabularies")
            .len()
            == mirrors.len(),
        "vocabulary-set-count"
    );
    for (name, values) in mirrors {
        let a: BTreeSet<_> = values.iter().copied().collect();
        let b: BTreeSet<_> = golden["vocabularies"][name]
            .as_array()
            .expect("vocabulary")
            .iter()
            .map(text)
            .collect();
        assert!(a == b, "vocabulary-mirror");
    }
    let rules: Value = must(serde_json::from_str(PATH_RULES));
    assert!(
        rules
            .as_object()
            .expect("rules")
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            == RECEIPT_PATHS.iter().copied().collect(),
        "rule-path-equality"
    );
}
#[test]
fn rust_walker_rejects_nested_paths_types_and_forbidden_counts() {
    let fixture: Value = must(serde_json::from_str(GOLDEN));
    let r = &fixture["receipt"];
    assert!(receipt_allowlisted(r), "positive-receipt");
    for ptr in [
        "/clean_text",
        "/not_measured/extra",
        "/analysis_declaration",
    ] {
        let mut bad = r.clone();
        if ptr == "/clean_text" {
            bad["clean_text"] = json!("synthetic-private-canary");
        } else if ptr == "/not_measured/extra" {
            bad["not_measured"]["extra"] = json!({});
        } else {
            bad["analysis_declaration"] = json!([]);
        }
        assert!(!receipt_allowlisted(&bad), "nested-path-type-refusal");
    }
    for key in ["outcomes", "claim_scope", "gate_results"] {
        let mut bad = r.clone();
        bad.as_object_mut().unwrap().remove(key);
        assert!(!receipt_allowlisted(&bad), "mandatory-receipt-key");
    }
    let mut bad = r.clone();
    bad["outcomes"]["COMPLETED"] = json!(0);
    assert!(!receipt_allowlisted(&bad), "receipt-outcome-identity");
    let mut bad = r.clone();
    bad["not_measured"]["metrics"] = json!([]);
    assert!(!receipt_allowlisted(&bad), "receipt-not-measured-coherence");
    let mut bad = r.clone();
    bad["gate_results"]
        .as_object_mut()
        .unwrap()
        .remove("gold_survival_oracle");
    assert!(!receipt_allowlisted(&bad), "receipt-gate-coverage");
    let mut bad = r.clone();
    bad["counts"]["protection_trace_items"] = json!(0);
    assert!(!receipt_allowlisted(&bad), "forbidden-count-refusal");
}
#[tokio::test]
async fn private_failure_canary_child() {
    if std::env::var_os("GAZE_EVIDENCE_CANARY_CHILD").is_none() {
        return;
    }
    let h = host(false, Output::Error, &["result"], 0, false, fresh_session());
    let r = call(h, json!({"text":EMAIL}), false)
        .await
        .expect("response");
    assert!(
        outcome(Some(&r)) == "FAILED_CLOSED_NO_EGRESS",
        "error-carrier"
    );
    let wire = must(serde_json::to_string(&r));
    assert!(!wire.contains(EMAIL), "error-payload-canary");
    let _ = std::panic::catch_unwind(|| {
        panic!("static-assertion-canary");
    });
    for size in [0, 300000] {
        let payload = format!("{{{}", " ".repeat(size));
        assert!(
            serde_json::from_str::<Value>(&payload).is_err(),
            "malformed-carrier"
        );
    }
    let h = host(false, Output::Wait, &["result"], 0, false, fresh_session());
    assert!(
        call(h, json!({"text":EMAIL}), true).await.is_none(),
        "canary-timeout"
    );
}
#[test]
fn private_failure_canary_captures_stdout_stderr_and_files() {
    let root = std::env::temp_dir().join(format!("gaze-evidence-canary-{}", std::process::id()));
    must(std::fs::create_dir(&root));
    let result = std::process::Command::new(must(std::env::current_exe()))
        .args(["--exact", "private_failure_canary_child", "--nocapture"])
        .env("GAZE_EVIDENCE_CANARY_CHILD", "1")
        .current_dir(&root)
        .output();
    let files = must(std::fs::read_dir(&root)).count();
    must(std::fs::remove_dir_all(&root));
    let result = must(result);
    let mut output = result.stdout;
    output.extend(result.stderr);
    assert!(result.status.success(), "canary-child-success");
    assert!(
        !output.windows(EMAIL.len()).any(|w| w == EMAIL.as_bytes())
            && !output.windows(PHONE.len()).any(|w| w == PHONE.as_bytes())
            && !output.windows(7).any(|w| w == b":Email_"),
        "private-output-canary"
    );
    assert!(files == 0, "private-file-canary");
}

#[tokio::test]
async fn plain_string_carrier_is_decoded_without_json_reparse() {
    let h = host(
        true,
        Output::Fixed(json!(EMAIL)),
        &[],
        0,
        false,
        fresh_session(),
    );
    let r = call(h.clone(), json!({"text":"benign"}), false)
        .await
        .expect("response");
    assert!(
        outcome(Some(&r)) == "COMPLETED" && r.content.len() == 1,
        "plain-carrier"
    );
    let observed = &r.content[0].raw.as_text().expect("text-frame").text;
    assert!(
        occurrence(&h.session, observed, EMAIL, false) == Verdict::Protected,
        "plain-string-oracle"
    );
}

#[test]
fn r2_mixed_raw_comparison_coverage() {
    let s = fresh_session();
    let token = must(s.tokenize(&PiiClass::Email, EMAIL));
    for reverse in [false, true] {
        for mismatch in [false, true] {
            let mut c = Counts::default();
            let mut operands = vec![
                (token.as_str(), if mismatch { FRESH } else { EMAIL }),
                ("plain", "plain"),
            ];
            if reverse {
                operands.reverse();
            }
            for (observed, expected) in operands {
                c.restore(&s, observed, expected, expected);
            }
            let r = receipt(&c, "COMPLETED", false);
            assert!(
                r["counts"].get("egress_raw_value_mismatches").is_none(),
                "raw-incomplete-count"
            );
            assert!(
                r["derivations"]["egress_raw_value_mismatches"] == "not_measured",
                "raw-incomplete-grade"
            );
            assert!(
                r["gate_results"]["egress_integrity_analogues"]
                    == if mismatch { "FAIL" } else { "NOT_EVALUABLE" },
                "raw-coverage-gate"
            );
        }
    }
    let mut c = Counts::default();
    c.restore(&s, &token, EMAIL, EMAIL);
    let r = receipt(&c, "COMPLETED", false);
    assert!(
        r["counts"]["egress_raw_value_mismatches"] == 0
            && r["gate_results"]["egress_integrity_analogues"] == "PASS",
        "raw-complete-pass"
    );
}
#[test]
fn r2_negative_predicate_coverage() {
    let s = fresh_session();
    let token = must(s.tokenize(&PiiClass::Email, EMAIL));
    for values in [
        vec!["unrelated"],
        vec![prefix(EMAIL, MIN_FRAGMENT_BYTES)],
        vec!["unrelated", EMAIL],
        vec![EMAIL, "unrelated"],
        vec!["unrelated", token.as_str()],
        vec![token.as_str(), "unrelated"],
    ] {
        let mut c = Counts::default();
        let fail = values.contains(&token.as_str());
        for observed in values {
            c.negative(&s, observed, EMAIL);
        }
        let r = receipt(&c, "COMPLETED", false);
        assert!(
            r["counts"].get("false_positive_occurrences").is_none()
                && r["counts"].get("false_positive_bytes").is_none(),
            "negative-incomplete-count"
        );
        assert!(
            r["derivations"]["false_positive_occurrences"] == "not_measured",
            "negative-incomplete-grade"
        );
        assert!(
            r["gate_results"]["false_positive_negative_control"]
                == if fail { "FAIL" } else { "NOT_EVALUABLE" },
            "negative-coverage-gate"
        );
    }
    let mut c = Counts::default();
    c.negative(&s, EMAIL, EMAIL);
    assert!(
        receipt(&c, "COMPLETED", false)["gate_results"]["false_positive_negative_control"]
            == "PASS",
        "negative-complete-pass"
    );
}
#[test]
fn r2_producer_grade_and_identity_binding() {
    let base: Value = must(serde_json::from_str::<Value>(GOLDEN))["receipt"].clone();
    for grade in [
        "planned_inventory",
        "private_authored_records",
        "observed_subset_lower_bound",
        "observer_native",
    ] {
        let mut r = base.clone();
        r["derivations"]["tool_invocations"] = json!(grade);
        assert!(!receipt_allowlisted(&r), "producer-metric-grade");
    }
    for (cell, policy) in [
        ("synthetic.evaluator.v1", "authored.records.v1"),
        ("synthetic.mcp.core.v1", "controlled.email_only.v1"),
        ("synthetic.mcp.controlled.v1", "core.rule_floor.v1"),
    ] {
        let mut r = base.clone();
        r["cell_id"] = json!(cell);
        r["policy_identity"] = json!(policy);
        assert!(!receipt_allowlisted(&r), "producer-cell-policy");
    }
    let mut e = base.clone();
    e["route_id"] = json!("evaluator.private.v1");
    e["cell_id"] = json!("synthetic.evaluator.v1");
    e["policy_identity"] = json!("authored.records.v1");
    for id in ROUTE_IDS {
        e["route_status"][*id] = json!(if *id == "evaluator.private.v1" {
            "IMPLEMENTED"
        } else {
            "NOT_IMPLEMENTED"
        });
    }
    e["counts"] = json!({"gold_bytes_planned":30});
    e["derivations"] = json!(
        METRIC_IDS
            .iter()
            .map(|m| (
                *m,
                if *m == "gold_bytes_planned" {
                    "planned_inventory"
                } else {
                    "not_measured"
                }
            ))
            .collect::<BTreeMap<_, _>>()
    );
    e["not_measured"]["metrics"] = json!(
        METRIC_IDS
            .iter()
            .filter(|m| **m != "gold_bytes_planned")
            .collect::<Vec<_>>()
    );
    assert!(receipt_allowlisted(&e), "honest-evaluator");
    // Exhaust the closed metric/grade matrix independently of the validator helpers.
    for evaluator in [false, true] {
        for metric in METRIC_IDS {
            let expected = if evaluator {
                match *metric {
                    "gold_bytes_planned" | "gold_occurrences_planned" => "planned_inventory",
                    "gold_occurrences_surviving_egress"
                    | "gold_occurrences_partially_surviving_egress"
                    | "gold_occurrences_attribution_not_measured"
                    | "gold_bytes_surviving_egress"
                    | "false_positive_occurrences"
                    | "false_positive_bytes"
                    | "unknown_egress_lower_bound_cases" => "private_authored_records",
                    _ => "not_measured",
                }
            } else if matches!(
                *metric,
                "false_positive_occurrences" | "false_positive_bytes"
            ) {
                "egress_reconstructed"
            } else {
                text(&base["derivations"][*metric])
            };
            for grade in DERIVATIONS {
                let mut r = if evaluator { e.clone() } else { base.clone() };
                r["counts"] = json!({});
                r["intervals"] = json!({});
                r["derivations"] = json!(
                    METRIC_IDS
                        .iter()
                        .map(|m| (*m, "not_measured"))
                        .collect::<BTreeMap<_, _>>()
                );
                r["derivations"][*metric] = json!(grade);
                if COUNTING_GRADES.contains(grade) {
                    r["counts"][*metric] = json!(0);
                }
                r["not_measured"]["metrics"] = json!(
                    METRIC_IDS
                        .iter()
                        .filter(|m| r["derivations"][**m] == "not_measured")
                        .collect::<Vec<_>>()
                );
                let valid = *grade == "not_measured"
                    || *grade == expected
                    || (evaluator
                        && expected == "private_authored_records"
                        && *grade == "observed_subset_lower_bound");
                assert!(receipt_allowlisted(&r) == valid, "producer-metric-matrix");
            }
        }
    }
    for grade in [
        "route_native",
        "private_authored_records",
        "observed_subset_lower_bound",
        "invariant_enforced_not_counted",
    ] {
        let mut r = e.clone();
        r["derivations"]["gold_bytes_planned"] = json!(grade);
        if grade == "invariant_enforced_not_counted" {
            r["counts"] = json!({});
        }
        assert!(!receipt_allowlisted(&r), "evaluator-metric-grade");
    }
}
#[test]
fn r2_planned_interval_refused() {
    let mut r = must(serde_json::from_str::<Value>(GOLDEN))["receipt"].clone();
    r["intervals"]["gold_bytes_planned"] = json!({"point":0.0,"low":0.0,"high":0.0,"method_id":"grouped_paired_percentile_v1","conditional":false,"basis":"paired_completed"});
    assert!(!receipt_allowlisted(&r), "planned-interval-refused");
}
