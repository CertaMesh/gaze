use std::path::PathBuf;

use crate::error::CliError;
use crate::pipeline::{run_clean, CleanOptions};

pub(crate) struct Args {
    pub(crate) policy: Option<PathBuf>,
    pub(crate) format: String,
    pub(crate) session_ttl: Option<u64>,
    pub(crate) session_scope: Option<String>,
    pub(crate) locale: Vec<String>,
    pub(crate) ner_threshold: Option<f32>,
    pub(crate) ner_model_dir: Option<PathBuf>,
    pub(crate) ner_locale: Option<String>,
    pub(crate) rulepack_bundled: Vec<String>,
    pub(crate) rulepack_paths: Vec<PathBuf>,
    pub(crate) max_bytes: u64,
    pub(crate) context_json: Option<PathBuf>,
    pub(crate) audit_db: Option<PathBuf>,
}

pub(crate) fn run(args: Args) -> std::result::Result<(), CliError> {
    run_clean(CleanOptions {
        policy: args.policy.as_deref(),
        format: &args.format,
        session_ttl: args.session_ttl,
        session_scope: args.session_scope.as_deref(),
        locale: &args.locale,
        ner_threshold: args.ner_threshold,
        ner_model_dir: args.ner_model_dir,
        ner_locale: args.ner_locale.as_deref(),
        rulepack_bundled: &args.rulepack_bundled,
        rulepack_paths: args.rulepack_paths,
        max_bytes: args.max_bytes,
        context_json: args.context_json.as_deref(),
        audit_db: args.audit_db.as_deref(),
    })
}
