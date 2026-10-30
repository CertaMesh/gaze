#![cfg(feature = "test-support")]
use std::path::Path;

#[test]
fn one_zwsp_document_records_actual_lazy_compilation_work() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut rows = Vec::new();
    for (name, input) in [
        (
            "plain",
            "Order received. Card: 4111111111111111; email cedar@example.invalid.",
        ),
        (
            "zwsp",
            "Order received. Card: 4111\u{200b}111111111111; email cedar@example.invalid.",
        ),
    ] {
        gaze_recognizers::take_source_regex_compilations();
        let pipeline = gaze_assembly::CorePipelineConfig::new()
            .with_locale(&[gaze::LocaleTag::EnUs])
            .build()
            .unwrap();
        assert!(gaze_recognizers::take_source_regex_compilations().is_empty());
        let session = gaze::Session::new(gaze::Scope::Ephemeral).unwrap();
        let output = pipeline.pseudonymize_text(&session, input).unwrap();
        let gaze::CleanDocument::Text(clean) = output else {
            panic!("text document expected");
        };
        assert_eq!(session.restore_strict_text(&clean).unwrap(), input);
        let compiled = gaze_recognizers::take_source_regex_compilations();
        let bytes = compiled
            .iter()
            .map(|(_, pattern, _)| {
                regex_automata::meta::Regex::new(pattern)
                    .unwrap()
                    .memory_usage()
            })
            .sum::<usize>();
        if name == "plain" {
            assert!(compiled.is_empty());
        } else {
            assert!(!compiled.is_empty());
            assert!(
                bytes <= 5 * 1024 * 1024,
                "one gap document compiled {bytes} bytes"
            );
        }
        rows.push(serde_json::json!({"document": name, "transformed_regexes_compiled": compiled.len(), "extra_immutable_compiled_bytes": bytes, "compile_nanoseconds": compiled.iter().map(|(_, _, ns)| ns).sum::<u64>()}));
    }
    let output = root.join("target/separator-checks/source-regex-cold-work.json");
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    std::fs::write(&output, serde_json::to_vec_pretty(&serde_json::json!({"schema": "gaze-source-regex-cold-work-v1", "rulepacks": ["core"], "locale": "en-US", "cases": rows})).unwrap()).unwrap();
    eprintln!("COLD WORK DONE: {rows:?}");
}
