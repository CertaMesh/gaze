mod builder;
mod generators;
mod templates;

/// Stable seed for the curated coverage corpus.
///
/// The value is the ASCII bytes for `gaze-v2` interpreted as a compact hex
/// marker. Re-snapping the corpus means running
/// `cargo run -p xtask -- coverage-corpus --regenerate`, then committing the
/// regenerated corpus, build manifest, and coverage baseline together. Changing
/// this constant is a breaking baseline change and must land with an explicit
/// drift-baseline acknowledgement in the same PR.
pub const COVERAGE_CORPUS_SEED: u64 = 0x6761_7a65_7632;

pub use builder::{run, Args};
