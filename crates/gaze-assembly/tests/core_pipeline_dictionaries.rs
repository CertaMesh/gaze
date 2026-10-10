use gaze::{CleanDocument, Scope, Session};
use gaze_assembly::CorePipelineConfig;

#[test]
fn core_pipeline_inline_dictionary_tokenizes_and_restores() {
    let path = std::env::temp_dir().join(format!(
        "gaze-core-inline-{}-{:?}.toml",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(
        &path,
        r#"
schema_version = "0.1.0"
rulepack_id = "tenant-songs"
rulepack_version = "0.1.0"
default_locales = ["global"]

[[recognizers]]
id = "tenant.song"
class = "custom:tenant_song"
enabled = true

[recognizers.match]
kind = "dictionary"
terms = ["tenant-song-xyz"]
case_sensitive = true
"#,
    )
    .expect("write rulepack");
    let built = CorePipelineConfig::new()
        .with_rulepack_path(path.clone())
        .build();
    std::fs::remove_file(path).expect("remove rulepack");
    let core = built.expect("build core pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let input = "Play tenant-song-xyz again.";
    let CleanDocument::Text(clean) = core.pseudonymize_text(&session, input).expect("clean") else {
        panic!("expected text");
    };
    assert!(
        !clean.contains("tenant-song-xyz"),
        "dictionary term leaked: {clean}"
    );
    assert!(
        clean.contains(":Custom:tenant_song_1>"),
        "missing token: {clean}"
    );
    assert_eq!(
        core.pipeline()
            .restore_strict_text(&session, &clean)
            .expect("restore"),
        input
    );
}
