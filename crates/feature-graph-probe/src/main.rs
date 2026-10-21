//! Runs published library code in the exact feature graph an adopter gets.
//!
//! `cargo test -p <crate>` compiles the crate's dev-dependencies into the same
//! graph, and Cargo feature unification can re-enable features (for example
//! `gaze-types/phone-parser`) that a `--no-default-features` adopter never has.
//! A `bin` target never pulls dev-dependencies, so each feature of this crate
//! selects one library graph and this binary exercises it at runtime.
//!
//! Every enabled probe prints one `feature-graph-probe: ... ok` line; the
//! `ci-feature-matrix` xtask requires the exact line for the graph it built.

/// The bundled `core` rulepack, read from source so the `gaze-pii`
/// no-default-features graph does not need `gaze-recognizers`.
#[cfg(any(feature = "gaze-pii-no-default", feature = "gaze-pii-default"))]
const EMBEDDED_CORE: &str = include_str!("../../gaze-recognizers/embedded/core.toml");

/// Without a graph feature there is nothing to probe; refuse rather than pass.
#[cfg(not(any(
    feature = "gaze-pii-no-default",
    feature = "gaze-pii-default",
    feature = "gaze-assembly-default"
)))]
fn main() {
    eprintln!("feature-graph-probe: select a graph with --features <graph>");
    std::process::exit(2);
}

#[cfg(any(
    feature = "gaze-pii-no-default",
    feature = "gaze-pii-default",
    feature = "gaze-assembly-default"
))]
fn main() {
    let mut probes = 0usize;

    #[cfg(any(feature = "gaze-pii-no-default", feature = "gaze-pii-default"))]
    {
        let rulepack = gaze::Rulepack::load(gaze::RulepackSource::Embedded(EMBEDDED_CORE))
            .unwrap_or_else(|error| {
                panic!("embedded core rulepack must load in this graph: {error}")
            });
        assert!(
            !rulepack.activated_classes().is_empty(),
            "embedded core rulepack activated no classes"
        );
        println!(
            "feature-graph-probe: gaze-pii embedded core rulepack loaded, {} ok",
            phone_parser_state()
        );
        probes += 1;
    }

    #[cfg(feature = "gaze-assembly-default")]
    {
        let core = gaze_assembly::CorePipelineConfig::new()
            .build()
            .unwrap_or_else(|error| panic!("bundled core pipeline must build: {error}"));
        let session = gaze::Session::new(gaze::Scope::Ephemeral).expect("ephemeral session");
        let input = "Contact dr.schmidt@example.invalid today.";
        let gaze::CleanDocument::Text(clean) = core
            .pseudonymize_text(&session, input)
            .unwrap_or_else(|error| panic!("bundled core pipeline must clean: {error}"))
        else {
            panic!("text input must yield text output");
        };
        assert!(
            !clean.contains("dr.schmidt@example.invalid"),
            "bundled core pipeline left the email raw: {clean}"
        );
        println!(
            "feature-graph-probe: gaze-assembly bundled core pipeline tokenized email, {} ok",
            phone_parser_state()
        );
        probes += 1;
    }

    assert!(probes > 0, "feature-graph-probe: no probe ran");
}

/// Reports whether phone validators parse in this graph, so the xtask can
/// prove the probe ran with the feature set it claims.
#[cfg(any(
    feature = "gaze-pii-no-default",
    feature = "gaze-pii-default",
    feature = "gaze-assembly-default"
))]
fn phone_parser_state() -> &'static str {
    if gaze_types::ValidatorKind::parse("phone_number").is_ok() {
        "phone-parser=on"
    } else {
        "phone-parser=off"
    }
}
