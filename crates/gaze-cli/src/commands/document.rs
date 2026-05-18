//! `gaze document` subcommand — wraps `gaze_document::clean` for the CLI.
//!
//! Available only when the binary is built with `--features document`.

use std::path::PathBuf;

use crate::error::CliError;

#[derive(Debug)]
pub(crate) struct CleanArgs {
    pub input: PathBuf,
    pub out: Option<PathBuf>,
    pub agent_out: Option<PathBuf>,
    pub owner_out: Option<PathBuf>,
}

pub(crate) fn run_clean(args: CleanArgs) -> Result<(), CliError> {
    let (agent_out, owner_out) = resolve_output_dirs(args.out, args.agent_out, args.owner_out)?;
    let bundle = gaze_document::clean(
        &args.input,
        gaze_document::AgentBundleDir::new(agent_out)
            .map_err(|err| CliError::DocumentDetail(err.to_string()))?,
        gaze_document::OwnerBundleDir::new(owner_out)
            .map_err(|err| CliError::DocumentDetail(err.to_string()))?,
    )
    .map_err(|err| CliError::DocumentDetail(err.to_string()))?;

    let summary = serde_json::json!({
        "ok": true,
        "input": bundle.source_path.display().to_string(),
        "agent_out_dir": bundle.agent_out_dir.display().to_string(),
        "owner_out_dir": bundle.owner_out_dir.display().to_string(),
        "bundle_version": bundle.report.bundle_version,
        "clean_char_count": bundle.report.clean_char_count,
        "pii_token_count": bundle.report.pii_token_count,
        "pii_tokens_by_class": bundle.report.pii_tokens_by_class.iter().map(|c| {
            serde_json::json!({ "class": c.class, "count": c.count })
        }).collect::<Vec<_>>(),
        "ocr": {
            "mean_confidence": bundle.report.ocr_mean_confidence,
            "word_count": bundle.report.ocr_word_count,
            "lang": bundle.report.ocr_lang,
        },
    });
    println!("{}", serde_json::to_string(&summary).expect("json summary"));
    Ok(())
}

fn resolve_output_dirs(
    out: Option<PathBuf>,
    agent_out: Option<PathBuf>,
    owner_out: Option<PathBuf>,
) -> Result<(PathBuf, PathBuf), CliError> {
    match (out, agent_out, owner_out) {
        (Some(out), None, None) => {
            let agent = out.join("agent");
            let owner = out.join("owner");
            tracing::info!(
                agent_out = %agent.display(),
                owner_out = %owner.display(),
                "`--out` shorthand inferred split document bundle paths"
            );
            Ok((agent, owner))
        }
        (None, Some(agent), Some(owner)) => Ok((agent, owner)),
        _ => Err(CliError::DocumentDetail(
            "gaze document clean requires either `--out PATH` or both `--agent-out PATH --owner-out PATH`".to_string(),
        )),
    }
}
