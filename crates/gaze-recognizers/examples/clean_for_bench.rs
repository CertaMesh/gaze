use std::io::{self, BufRead, Write};

use gaze::{CleanDocument, LocaleTag, RawDocument, Scope, Session};
use gaze_assembly::CorePipelineConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct Request {
    fixture_id: String,
    locale_chain: Vec<String>,
    text: String,
}

#[derive(Debug, Serialize)]
struct Response {
    fixture_id: String,
    clean_text: String,
    manifest_spans: Vec<ManifestSpan>,
}

#[derive(Debug, Serialize)]
struct ManifestSpan {
    raw_start: usize,
    raw_end: usize,
    clean_start: usize,
    clean_end: usize,
    class: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = serde_json::from_str(&line)?;
        let locale_chain = request
            .locale_chain
            .iter()
            .map(|locale| LocaleTag::parse(locale))
            .collect::<Result<Vec<_>, _>>()?;
        let core = CorePipelineConfig::new()
            .with_locale(&locale_chain)
            .with_bundled_rulepack("core-extended")
            .build()?;
        let session = Session::new(Scope::Ephemeral)?;
        let (clean_doc, manifest, _) = core.pipeline().clean_with_safety_net(
            &session,
            RawDocument::Text(request.text),
            &locale_chain,
        )?;
        let CleanDocument::Text(clean_text) = clean_doc else {
            return Err("expected text clean document".into());
        };
        let manifest_spans = manifest
            .into_iter()
            .map(|span| ManifestSpan {
                raw_start: span.raw_span.start,
                raw_end: span.raw_span.end,
                clean_start: span.clean_span.start,
                clean_end: span.clean_span.end,
                class: span.class.to_canonical_str(),
            })
            .collect();
        serde_json::to_writer(
            &mut stdout,
            &Response {
                fixture_id: request.fixture_id,
                clean_text,
                manifest_spans,
            },
        )?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }

    Ok(())
}
