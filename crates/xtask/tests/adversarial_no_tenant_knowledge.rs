#![allow(dead_code)]

use std::fs;

#[path = "../src/no_tenant_knowledge.rs"]
mod no_tenant_knowledge;

use no_tenant_knowledge::{scan_root, TenantKnowledgeError};

#[test]
fn seeded_order_id_in_production_scope_fails_gate_and_mentions_literal() {
    let temp = tempfile::tempdir().expect("tempdir");
    let src_dir = temp.path().join("crates/gaze/src");
    fs::create_dir_all(&src_dir).expect("create mock production src");
    let fixture = src_dir.join("lib.rs");
    fs::write(&fixture, "fn fixture() {\n    let _ = \"order_id\";\n}\n")
        .expect("write seeded production fixture");

    let result = scan_root(temp.path());
    fs::remove_file(&fixture).expect("remove seeded production fixture");

    let error = result.expect_err("seeded tenant literal must fail");
    let output = error.to_string();
    assert!(
        matches!(error, TenantKnowledgeError::DenylistHit { .. }),
        "expected denylist hit, got {output}"
    );
    assert!(
        output.contains("order_id"),
        "gate output must mention seeded literal: {output}"
    );
}

#[test]
fn allow_marker_hard_fails_in_production_but_passes_outside_production_scope() {
    let temp = tempfile::tempdir().expect("tempdir");
    let src_dir = temp.path().join("crates/gaze/src");
    fs::create_dir_all(&src_dir).expect("create mock production src");
    let production_fixture = src_dir.join("lib.rs");
    fs::write(
        &production_fixture,
        "// allow(tenant-fixture)\nfn fixture() {\n    let _ = \"order_id\";\n}\n",
    )
    .expect("write production allow-marker fixture");

    let result = scan_root(temp.path());
    fs::remove_file(&production_fixture).expect("remove production allow-marker fixture");

    let error = result.expect_err("production allow marker must hard-fail");
    let output = error.to_string();
    assert!(
        matches!(
            error,
            TenantKnowledgeError::AllowMarkerInProductionScope { .. }
        ),
        "expected production allow-marker failure, got {output}"
    );
    assert!(
        output.contains("AllowMarkerInProductionScope"),
        "gate output must identify allow-marker policy failure: {output}"
    );

    fs::write(src_dir.join("lib.rs"), "pub fn production_code() {}\n")
        .expect("write benign production fixture");
    let test_dir = temp.path().join("crates/gaze/tests");
    fs::create_dir_all(&test_dir).expect("create mock tests dir");
    let test_fixture = test_dir.join("fixture.rs");
    fs::write(
        &test_fixture,
        "// allow(tenant-fixture)\nfn fixture() {\n    let _ = \"order_id\";\n}\n",
    )
    .expect("write tests allow-marker fixture");

    let result = scan_root(temp.path());
    fs::remove_file(&test_fixture).expect("remove tests allow-marker fixture");

    result.expect("allow marker outside production scope should pass");
}
