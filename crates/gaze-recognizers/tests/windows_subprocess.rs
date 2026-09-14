#![cfg(all(windows, feature = "safety-net-openai", feature = "safety-net-kiji"))]

use gaze_recognizers::safety_net::{
    kiji_distilbert::{KijiDistilbertBackend, SubprocessKijiBackend, SubprocessKijiConfig},
    openai_filter::{
        OpenAiFilterBackend, SubprocessOpenAiFilterBackend, SubprocessOpenAiFilterConfig,
    },
};
use gaze_types::SafetyNetError;
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn infer(
    kiji: bool,
    mode: &str,
    diagnostics: bool,
    input: &str,
    marker: &Path,
) -> Result<(), SafetyNetError> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/windows_subprocess.py");
    let args = vec![
        script.into_os_string(),
        mode.into(),
        if kiji { "kiji" } else { "opf" }.into(),
        marker.as_os_str().to_owned(),
    ];
    if kiji {
        SubprocessKijiBackend::new(
            SubprocessKijiConfig::new("python")
                .with_args(args)
                .with_timeout(Duration::from_secs(2))
                .with_max_input_bytes(input.len().max(1))
                .with_max_stdout_bytes(1024)
                .with_stderr_diagnostics(diagnostics),
        )
        .unwrap()
        .infer(input)
        .map(|_| ())
    } else {
        SubprocessOpenAiFilterBackend::new(
            SubprocessOpenAiFilterConfig::new("python")
                .with_args(args)
                .with_timeout(Duration::from_secs(2))
                .with_max_input_bytes(input.len().max(1))
                .with_max_stdout_bytes(1024)
                .with_stderr_diagnostics(diagnostics),
        )
        .unwrap()
        .infer(input)
        .map(|_| ())
    }
}

#[test]
fn success_diagnostics_on_off_and_backpressure_preserve_input() {
    let dir = tempfile::tempdir().unwrap();
    for kiji in [false, true] {
        for diagnostics in [false, true] {
            for mode in ["success", "noisy", "echo-count"] {
                let input = if mode == "echo-count" {
                    "w".repeat(2 * 1024 * 1024)
                } else {
                    "clean".into()
                };
                infer(kiji, mode, diagnostics, &input, &dir.path().join("unused")).unwrap();
            }
        }
    }
}

#[test]
fn unicode_prefix_and_strict_output_errors() {
    let dir = tempfile::tempdir().unwrap();
    for kiji in [false, true] {
        let error = infer(kiji, "unicode", true, "clean", dir.path()).unwrap_err();
        let SafetyNetError::Runtime { message } = error else {
            panic!("{error:?}")
        };
        assert!(!message.contains("alice"), "{message}");
        assert!(message.ends_with("[truncated]"), "{message}");
        for mode in ["invalid-json", "invalid-utf8"] {
            assert!(matches!(
                infer(kiji, mode, true, "clean", dir.path()),
                Err(SafetyNetError::InvalidOutput { .. })
            ));
        }
        for mode in ["stdout-cap", "broken-stdin"] {
            let start = Instant::now();
            let error =
                infer(kiji, mode, true, &"w".repeat(2 * 1024 * 1024), dir.path()).unwrap_err();
            assert!(matches!(error, SafetyNetError::Runtime { .. }), "{error:?}");
            assert!(
                !error.to_string().contains("timed out"),
                "must preserve IO error: {error:?}"
            );
            assert!(start.elapsed() < Duration::from_secs(4));
        }
    }
}

#[test]
fn descendant_held_pipes_cancel_and_close_owned_handles() {
    for kiji in [false, true] {
        for fd in 0..=2 {
            let dir = tempfile::tempdir().unwrap();
            let marker = dir.path().join("witness");
            let input = if fd == 0 {
                "w".repeat(2 * 1024 * 1024)
            } else {
                "clean".into()
            };
            let start = Instant::now();
            let error = infer(kiji, &format!("hold{fd}"), true, &input, &marker).unwrap_err();
            assert!(error.to_string().contains("timed out"), "{error:?}");
            assert!(
                start.elapsed() < Duration::from_secs(4),
                "kiji={kiji} fd={fd}"
            );
            assert!(marker.with_extension("ready").exists());
            let end = Instant::now() + Duration::from_secs(4);
            while !marker.with_extension("closed").exists() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(10));
            }
            let closed = std::fs::read_to_string(marker.with_extension("closed"))
                .unwrap_or_else(|error| panic!("descendant must observe EOF/broken pipe: kiji={kiji} fd={fd}, {error}; fixture error: {:?}", std::fs::read_to_string(marker.with_extension("error"))));
            if fd == 0 {
                assert!(closed.parse::<usize>().unwrap() < input.len());
            }
        }
    }
}
