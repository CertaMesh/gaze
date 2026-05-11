//! `gaze document` subcommand — wraps `gaze_document::clean` for the CLI.
//!
//! Available only when the binary is built with `--features document`.

use std::path::PathBuf;

use crate::error::CliError;

#[derive(Debug)]
pub(crate) struct CleanArgs {
    pub input: PathBuf,
    pub out: PathBuf,
}

pub(crate) fn run_clean(args: CleanArgs) -> Result<(), CliError> {
    let bundle = gaze_document::clean(&args.input, &args.out)
        .map_err(|err| CliError::DocumentDetail(err.to_string()))?;

    let summary = serde_json::json!({
        "ok": true,
        "input": bundle.source_path.display().to_string(),
        "out_dir": bundle.out_dir.display().to_string(),
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
