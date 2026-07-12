//! Fresh-process determinism regression for the in-process Kiji ORT backend.

#![cfg(feature = "safety-net-kiji")]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

use gaze_recognizers::safety_net::kiji_distilbert::{
    KijiDistilbertBackend, OrtKijiBackend, OrtKijiConfig, RawSpan, SafetyNetError,
};
use serde::Serialize;

const CHILD_MODE: &str = "GAZE_KIJI_ORT_DETERMINISM_CHILD";
const BEGIN_SENTINEL: &str = "KIJI_ORT_DETERMINISM_BEGIN";
const END_SENTINEL: &str = "KIJI_ORT_DETERMINISM_END";
const CHILD_RUNS: usize = 5;

const FIXTURES: &[(&str, &str)] = &[
    (
        "mixed-short",
        "Dr. Schmidt from Example Labs met Alice in Berlin. Contact: alice@example.invalid.",
    ),
    (
        "de-borderline-long",
        "Dr. Schmidt prueft den synthetischen Vorgang GAZE-0001 fuer Example Labs in Berlin. Die Rueckfrage geht ausschliesslich an alice@example.invalid oder an die dokumentierte Testnummer +49 1555 0112233. Danach vergleicht Dr. Schmidt den Vorgang mit GAZE-0002, notiert Example Labs erneut und bestaetigt, dass alle Namen, Adressen und Nummern in diesem Absatz erfunden und nur fuer den deterministischen Test bestimmt sind.",
    ),
    (
        "en-borderline-long",
        "Alice from Example Labs asked Dr. Schmidt to review synthetic order GAZE-0101 in Berlin. The only test contact is alice@example.invalid and the only test phone is +1-555-0101. Dr. Schmidt then compared Example Labs with Example Research, moved the fictional meeting from Berlin to Hamburg, and repeated the synthetic identifiers GAZE-0101 and GAZE-0102 so the input crosses several token and entity boundaries without containing any real personal data.",
    ),
    (
        "punctuation-joins",
        "Example-Labs/Research, Berlin: Dr. Schmidt; alice@example.invalid; GAZE-0201. Example_Labs+Research and Dr.-Schmidt appear beside punctuation so BIO joiner handling is exercised.",
    ),
    (
        "mixed-repetition-long",
        "Dr. Schmidt arbeitet bei Example Labs in Berlin, waehrend Alice fuer Example Research in Hamburg arbeitet. Alice schreibt an alice@example.invalid, Dr. Schmidt nutzt ebenfalls alice@example.invalid als rein synthetischen Testkontakt. The fictional review mentions Example Labs, Example Research, Berlin, Hamburg, Dr. Schmidt, Alice, GAZE-0301, GAZE-0302, +49 1555 0112233, and +1-555-0102 more than once to create a long mixed-language sequence near multiple BIO boundaries.",
    ),
    (
        "title-and-org-chain",
        "Dr. Schmidt, Example Labs Berlin, Example Research Hamburg, Alice, Dr. Schmidt, Example-Labs, Example Research, Berlin/Hamburg, alice@example.invalid, GAZE-0401. This is a synthetic chain intended to make adjacent title, person, organization, location, and miscellaneous labels compete without exposing any real person's information.",
    ),
];

#[derive(Serialize)]
struct ChildRecord<'a> {
    fixture_id: &'a str,
    outcome: CanonicalOutcome,
    trace: BackendTrace<'a>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CanonicalOutcome {
    Spans { spans: Vec<CanonicalSpan> },
    Error { variant: &'static str },
}

#[derive(Serialize)]
struct CanonicalSpan {
    start: usize,
    end: usize,
    class: String,
    score_bits: Option<u32>,
}

impl From<RawSpan> for CanonicalSpan {
    fn from(span: RawSpan) -> Self {
        Self {
            start: span.start,
            end: span.end,
            class: span.label,
            score_bits: span.score.map(f32::to_bits),
        }
    }
}

#[derive(Serialize)]
struct BackendTrace<'a> {
    backend_id: &'a str,
    version: &'a str,
    decoding_params: BTreeMap<&'a str, &'a str>,
}

#[test]
#[ignore = "requires the pinned Kiji model bundle; run explicitly to verify fresh-process ORT determinism"]
fn kiji_ort_outputs_are_identical_across_fresh_processes() {
    if std::env::var_os(CHILD_MODE).is_some() {
        run_child();
        return;
    }

    let model_dir = model_dir();
    assert!(
        model_dir.is_dir(),
        "Kiji model bundle is missing at {}",
        model_dir.display()
    );

    let current_exe = std::env::current_exe().expect("resolve current test executable");
    let mut baselines: Option<Vec<u8>> = None;
    for run in 1..=CHILD_RUNS {
        let output = Command::new(&current_exe)
            .arg("--exact")
            .arg("kiji_ort_outputs_are_identical_across_fresh_processes")
            .arg("--ignored")
            .arg("--nocapture")
            .env(CHILD_MODE, "1")
            .env("GAZE_KIJI_DISTILBERT_MODEL_DIR", &model_dir)
            .env("RUST_TEST_THREADS", "1")
            .output()
            .unwrap_or_else(|error| panic!("spawn Kiji determinism child {run}: {error}"));
        assert!(
            output.status.success(),
            "Kiji determinism child {run} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let canonical = extract_canonical_output(&output.stdout);
        if let Some(expected) = &baselines {
            assert_eq!(
                canonical, *expected,
                "Kiji ORT output diverged in fresh child process {run}"
            );
        } else {
            baselines = Some(canonical);
        }
    }

    let canonical_bytes = baselines.as_ref().map_or(0, Vec::len);
    println!(
        "Kiji ORT determinism verified: {CHILD_RUNS} fresh processes, {} synthetic fixtures, {canonical_bytes} canonical bytes",
        FIXTURES.len()
    );
}

fn run_child() {
    let backend = OrtKijiBackend::new(OrtKijiConfig::new(model_dir()))
        .expect("initialize Kiji ORT backend in child process");
    println!("{BEGIN_SENTINEL}");
    for (fixture_id, text) in FIXTURES {
        let outcome = match backend.infer(text) {
            Ok(spans) => CanonicalOutcome::Spans {
                spans: spans.into_iter().map(CanonicalSpan::from).collect(),
            },
            Err(error) => CanonicalOutcome::Error {
                variant: safety_net_error_variant(&error),
            },
        };
        let trace = BackendTrace {
            backend_id: backend.id(),
            version: backend.version(),
            decoding_params: backend
                .decoding_params()
                .iter()
                .map(|(key, value)| (*key, value.as_str()))
                .collect(),
        };
        println!(
            "{}",
            serde_json::to_string(&ChildRecord {
                fixture_id,
                outcome,
                trace,
            })
            .expect("serialize canonical Kiji output")
        );
    }
    println!("{END_SENTINEL}");
}

fn extract_canonical_output(stdout: &[u8]) -> Vec<u8> {
    let stdout = String::from_utf8(stdout.to_vec()).expect("child stdout must be UTF-8");
    let (_, after_begin) = stdout
        .split_once(&format!("{BEGIN_SENTINEL}\n"))
        .expect("child output must include begin sentinel");
    let (canonical, _) = after_begin
        .split_once(&format!("{END_SENTINEL}\n"))
        .expect("child output must include end sentinel");
    canonical.as_bytes().to_vec()
}

fn model_dir() -> PathBuf {
    std::env::var_os("GAZE_KIJI_DISTILBERT_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(default_model_dir)
}

fn default_model_dir() -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_else(|| OsString::from("."));
    PathBuf::from(home).join(".local/share/gaze/models/kiji-distilbert")
}

fn safety_net_error_variant(error: &SafetyNetError) -> &'static str {
    match error {
        SafetyNetError::Unavailable { .. } => "unavailable",
        SafetyNetError::WeightsMissing { .. } => "weights_missing",
        SafetyNetError::ModelUnavailable { .. } => "model_unavailable",
        SafetyNetError::ModelIntegrityMismatch { .. } => "model_integrity_mismatch",
        SafetyNetError::InputTooLarge { .. } => "input_too_large",
        SafetyNetError::Runtime { .. } => "runtime",
        SafetyNetError::InvalidOutput { .. } => "invalid_output",
        _ => "unknown",
    }
}
