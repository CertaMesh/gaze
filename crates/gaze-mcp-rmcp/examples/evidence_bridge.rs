//! Private synthetic transport fixture. No evidence receipt is exported.
#[cfg(feature = "transport-stdio")]
#[allow(dead_code)]
#[path = "../tests/support/evidence_harness.rs"]
mod evidence_harness;
#[cfg(feature = "transport-stdio")]
use evidence_harness::*;
#[cfg(feature = "transport-stdio")]
use std::io::{BufRead, Read, Write};

#[cfg(feature = "transport-stdio")]
async fn observe(request: &Value) -> Option<Value> {
    let object = request.as_object()?;
    if object.len() != 4
        || request["format"] != "synthetic_bridge_v1"
        || request["scenario"] != "phone_pair_v1"
    {
        return None;
    }
    let arm = request["arm"].as_str()?;
    let ordinal = request["ordinal"].as_u64()?;
    if !["base", "candidate"].contains(&arm) || ordinal > 1 {
        return None;
    }
    let session = fresh_session();
    let token = must(session.tokenize(&PiiClass::custom("phone"), PHONE));
    let a = if arm == "base" {
        token.as_str()
    } else if ordinal == 0 {
        PHONE
    } else {
        prefix(PHONE, 8)
    };
    let h = host(
        true,
        Output::Fixed(json!({"a":a,"b":token,"n":"fixture-note"})),
        &["a", "b", "n"],
        0,
        false,
        session.clone(),
    );
    let response = call(h, json!({"text":"fixture-note"}), false).await;
    let state = outcome(response.as_ref());
    let mut gold = Vec::new();
    let mut negative = Vec::new();
    let mut restore = Vec::new();
    if state == "COMPLETED" {
        let value = decode(response.as_ref()?);
        for slot in ["a", "b"] {
            let mut counts = Counts::default();
            let verdict = counts.score(&session, text(&value[slot]), PHONE, false);
            let name = match verdict {
                Verdict::Full => "full",
                Verdict::Partial(_) => "partial",
                Verdict::Protected => "protected",
                Verdict::Unknown => "unknown",
            };
            gold.push(json!({"slot":slot,"verdict":name,"surviving_bytes":counts.get("gold_bytes_surviving_egress")}));
        }
        let mut counts = Counts::default();
        counts.negative(&session, text(&value["n"]), "fixture-note");
        let verdict = if counts.1.get("negative_compared") != Some(&1) {
            "uncompared"
        } else if counts.get("false_positive_occurrences") == 1 {
            "protected"
        } else {
            "full"
        };
        negative.push(json!({"slot":"n","verdict":verdict,"false_positive_bytes":counts.get("false_positive_bytes")}));
        for slot in ["a", "b", "n"] {
            let mut counts = Counts::default();
            let expected = if slot == "n" { "fixture-note" } else { PHONE };
            counts.restore(&session, text(&value[slot]), expected, expected);
            restore.push(json!({"slot":slot,"decision_success":counts.get("leaf_restore_decision_failures")==0,"exact":counts.get("leaf_restore_exact")==1}));
        }
    }
    Some(
        json!({"format":"synthetic_bridge_v1","kind":"observation","arm":arm,"ordinal":ordinal,"scenario":"phone_pair_v1","outcome":state,"gold":gold,"negative":negative,"restore":restore}),
    )
}

#[cfg(feature = "transport-stdio")]
fn emit(value: &Value) -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, value)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

#[cfg(feature = "transport-stdio")]
#[tokio::main]
async fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    if emit(&json!({"format":"synthetic_bridge_v1","kind":"ready"})).is_err() {
        return;
    }
    let stdin = std::io::stdin();
    loop {
        let mut frame = Vec::new();
        let read = stdin.lock().take(4097).read_until(b'\n', &mut frame);
        if matches!(read, Ok(0)) {
            break;
        }
        if read.is_err() || frame.len() > 4096 || frame.last() != Some(&b'\n') {
            break;
        }
        // The fixed parent uses literal closed keys; reject duplicates and aliases.
        let unique_keys = [
            b"\"format\"".as_slice(),
            b"\"arm\"",
            b"\"ordinal\"",
            b"\"scenario\"",
        ]
        .iter()
        .all(|key| {
            frame
                .windows(key.len())
                .filter(|window| window == key)
                .count()
                == 1
        });
        let response = if !unique_keys || frame.contains(&b'\\') {
            None
        } else {
            match serde_json::from_slice::<Value>(&frame) {
                Ok(request) => observe(&request).await,
                Err(_) => None,
            }
        };
        let refused = response.is_none();
        let value = response.unwrap_or_else(
            || json!({"format":"synthetic_bridge_v1","kind":"refused","code":"protocol"}),
        );
        if emit(&value).is_err() || refused {
            break;
        }
    }
}

// Without the selected transport, compilation does not imply an executable bridge.
#[cfg(not(feature = "transport-stdio"))]
fn main() {
    std::process::exit(2);
}
