use serde_json::Value;
use std::collections::BTreeSet;
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn attribution_invocation_snapshot_covers_model_free_inventory() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../fixtures/perf-invocation-cases.json")).unwrap();
    let expected: Value =
        serde_json::from_str(include_str!("../fixtures/perf-invocations.json")).unwrap();
    let inventory: Value =
        serde_json::from_str(include_str!("../fixtures/perf-hook-inventory.json")).unwrap();
    let mut fired = BTreeSet::new();
    for case in cases {
        let dir = tempfile::tempdir().unwrap();
        let policy = dir.path().join("policy.toml");
        std::fs::write(&policy, case["policy"].as_str().unwrap()).unwrap();
        let daemon = case["daemon"].as_bool().unwrap();
        let requests = case["requests"].as_array().unwrap();
        let payload = if daemon {
            requests
                .iter()
                .map(|r| format!("{r}\n"))
                .collect::<String>()
        } else {
            requests[0]["text"].as_str().unwrap().to_owned()
        };
        let mut command = Command::new(assert_cmd::cargo::cargo_bin!("gaze"));
        // Match the runner's allowlist: user model/policy overrides cannot alter fixtures.
        let environment: Vec<_> = [
            "PATH",
            "HOME",
            "XDG_DATA_HOME",
            "SYSTEMROOT",
            "WINDIR",
            "TMPDIR",
            "TMP",
            "TEMP",
            "LANG",
            "LC_ALL",
        ]
        .into_iter()
        .filter_map(|key| std::env::var_os(key).map(|value| (key, value)))
        .collect();
        command.arg(if daemon { "daemon" } else { "clean" });
        if !daemon && requests[0].get("context").is_some() {
            let context = dir.path().join("context.json");
            std::fs::write(&context, requests[0]["context"].to_string()).unwrap();
            command.arg("--context-json").arg(context);
        }
        let mut child = command
            .env_clear()
            .envs(environment)
            .env("GAZE_PERF", "1")
            .arg("--policy")
            .arg(policy)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let error = case["error"].as_bool().unwrap();
        assert_eq!(output.status.success(), daemon || !error, "{}", case["id"]);
        if daemon {
            let responses: Vec<Value> = String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert_eq!(responses.len(), requests.len());
            for response in responses {
                assert_eq!(response.get("error").is_some(), error);
                if !error {
                    assert!(response.get("clean_text").is_some());
                }
            }
        } else if !error {
            let response: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert!(response.get("clean_text").is_some());
        }
        let mut rows = Vec::new();
        for line in String::from_utf8(output.stderr).unwrap().lines() {
            let Some(json) = line.strip_prefix("GAZE_PERF ") else {
                continue;
            };
            let mut row: Value = serde_json::from_str(json).unwrap();
            for group in ["metrics", "recognizers"] {
                for (key, entry) in row[group].as_object_mut().unwrap() {
                    assert!(entry.as_object_mut().unwrap().remove("ns").is_some());
                    if entry["invocations"].as_u64().unwrap() > 0 {
                        fired.insert(
                            if group == "recognizers" {
                                "recognizers[*]"
                            } else {
                                key
                            }
                            .to_owned(),
                        );
                    }
                }
            }
            for (key, count) in row["counters"].as_object().unwrap() {
                if count.as_u64().unwrap() > 0 {
                    fired.insert(key.clone());
                }
            }
            rows.push(row);
        }
        assert_eq!(rows.len(), requests.len() + 1);
        assert_eq!(
            serde_json::json!(rows),
            expected[case["id"].as_str().unwrap()],
            "invocation snapshot changed: {}",
            case["id"]
        );
    }
    let required: BTreeSet<String> = inventory["model_free"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| key.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        fired, required,
        "model-free inventory requires fixture coverage"
    );
}
