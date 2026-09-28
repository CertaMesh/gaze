//! Owner-side evaluation. Raw corpus text stays in memory and never enters a report.

use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use clap::Args as ClapArgs;
use gaze::{CleanDocument, PiiClass, RawDocument, SafetyNetPolicy, Session};
use serde::{Deserialize, Serialize};

use crate::clean_overrides::CleanOverrides;
#[cfg(feature = "safety-net-nym")]
use crate::commands::DEFAULT_SAFETY_NET_INPUT_LIMIT_BYTES;
use crate::error::CliError;
use crate::pipeline::build::resolve_pipeline;

const MAX_CORPUS_BYTES: u64 = 64 * 1024 * 1024;

#[derive(ClapArgs, Debug)]
pub(crate) struct Args {
    /// Owner-side JSONL corpus with text and UTF-8 byte spans.
    annotated: PathBuf,
    /// Policy TOML; absent means the same bundled core policy as `gaze clean`.
    #[arg(long)]
    policy: Option<PathBuf>,
    /// Active locale fallback chain, comma separated and priority ordered.
    #[arg(long, value_delimiter = ',')]
    locale: Vec<String>,
    /// JSON object mapping corpus labels to Gaze policy class names.
    #[arg(long)]
    label_map: Option<PathBuf>,
    /// Emit machine-readable aggregate metrics.
    #[arg(long)]
    json: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    text: String,
    spans: Vec<GoldSpan>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GoldSpan {
    start: usize,
    end: usize,
    label: String,
}

struct ScoredSpan {
    range: Range<usize>,
    class: String,
}

#[derive(Default, Serialize)]
struct ClassScore {
    gold_bytes: usize,
    covered_bytes: usize,
    leaked_bytes: usize,
    entities: usize,
    fully_covered: usize,
}

#[derive(Default, Serialize)]
struct Score {
    documents: usize,
    documents_without_leaks: usize,
    documents_with_leaks: usize,
    zero_leak_document_rate: f64,
    document_leak_rate: f64,
    pii_bytes: usize,
    predicted_bytes: usize,
    true_positive_bytes: usize,
    leaked_bytes: usize,
    false_positive_bytes: usize,
    byte_precision: f64,
    byte_recall: f64,
    byte_f1: f64,
    per_class: BTreeMap<String, ClassScore>,
}

fn read_limited(path: &Path) -> Result<String, CliError> {
    let file = fs::File::open(path).map_err(|_| CliError::Io)?;
    let metadata = file.metadata().map_err(|_| CliError::Io)?;
    if metadata.len() > MAX_CORPUS_BYTES {
        return Err(CliError::InputTooLarge);
    }
    // A size-limited read also covers files whose size grows after metadata().
    use std::io::Read;
    let mut bytes = Vec::new();
    file.take(MAX_CORPUS_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| CliError::Io)?;
    if bytes.len() as u64 > MAX_CORPUS_BYTES {
        return Err(CliError::InputTooLarge);
    }
    String::from_utf8(bytes).map_err(|_| CliError::EvalSchema)
}

fn label_map(path: Option<&Path>) -> Result<BTreeMap<String, PiiClass>, CliError> {
    let Some(path) = path else {
        return Ok(BTreeMap::new());
    };
    let source = read_limited(path)?;
    let raw: BTreeMap<String, String> =
        serde_json::from_str(&source).map_err(|_| CliError::EvalSchema)?;
    if raw.is_empty() {
        return Err(CliError::EvalSchema);
    }
    raw.into_iter()
        .map(|(label, class)| {
            if label.is_empty() {
                return Err(CliError::EvalSchema);
            }
            let class = PiiClass::from_policy_name(&class).ok_or(CliError::EvalSchema)?;
            Ok((label, class))
        })
        .collect()
}

fn validate_document(
    doc: &Document,
    labels: &BTreeMap<String, PiiClass>,
) -> Result<Vec<ScoredSpan>, CliError> {
    let mut previous_end = 0;
    let mut scored = Vec::with_capacity(doc.spans.len());
    for span in &doc.spans {
        if span.start >= span.end
            || span.start < previous_end
            || span.end > doc.text.len()
            || !doc.text.is_char_boundary(span.start)
            || !doc.text.is_char_boundary(span.end)
        {
            return Err(CliError::EvalSchema);
        }
        let class = labels
            .get(&span.label)
            .cloned()
            .or_else(|| PiiClass::from_policy_name(&span.label))
            .ok_or(CliError::EvalSchema)?;
        scored.push(ScoredSpan {
            range: span.start..span.end,
            class: class.to_canonical_str(),
        });
        previous_end = span.end;
    }
    Ok(scored)
}

fn merge(mut spans: Vec<Range<usize>>) -> Vec<Range<usize>> {
    spans.sort_by_key(|span| (span.start, span.end));
    let mut merged: Vec<Range<usize>> = Vec::new();
    for span in spans {
        if let Some(last) = merged.last_mut() {
            if span.start <= last.end {
                last.end = last.end.max(span.end);
                continue;
            }
        }
        merged.push(span);
    }
    merged
}

fn length(spans: &[Range<usize>]) -> usize {
    spans.iter().map(|span| span.end - span.start).sum()
}

fn intersection(a: &[Range<usize>], b: &[Range<usize>]) -> usize {
    let (mut i, mut j, mut total) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        total += a[i]
            .end
            .min(b[j].end)
            .saturating_sub(a[i].start.max(b[j].start));
        if a[i].end <= b[j].end {
            i += 1;
        } else {
            j += 1;
        }
    }
    total
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        1.0
    } else {
        numerator as f64 / denominator as f64
    }
}

impl Score {
    fn add(&mut self, spans: &[ScoredSpan], predicted: Vec<Range<usize>>) {
        let gold = merge(spans.iter().map(|s| s.range.clone()).collect());
        let predicted = merge(predicted);
        let gold_bytes = length(&gold);
        let predicted_bytes = length(&predicted);
        let covered = intersection(&gold, &predicted);
        self.documents += 1;
        self.documents_without_leaks += usize::from(covered == gold_bytes);
        self.pii_bytes += gold_bytes;
        self.predicted_bytes += predicted_bytes;
        self.true_positive_bytes += covered;
        for span in spans {
            let score = self.per_class.entry(span.class.clone()).or_default();
            let bytes = span.range.end - span.range.start;
            let covered = intersection(std::slice::from_ref(&span.range), &predicted);
            score.gold_bytes += bytes;
            score.covered_bytes += covered;
            score.entities += 1;
            score.fully_covered += usize::from(covered == bytes);
        }
    }

    fn finish(&mut self) {
        self.leaked_bytes = self.pii_bytes - self.true_positive_bytes;
        self.false_positive_bytes = self.predicted_bytes - self.true_positive_bytes;
        for row in self.per_class.values_mut() {
            row.leaked_bytes = row.gold_bytes - row.covered_bytes;
        }
        self.documents_with_leaks = self.documents - self.documents_without_leaks;
        self.zero_leak_document_rate = ratio(self.documents_without_leaks, self.documents);
        self.document_leak_rate = ratio(self.documents_with_leaks, self.documents);
        self.byte_precision = ratio(self.true_positive_bytes, self.predicted_bytes);
        self.byte_recall = ratio(self.true_positive_bytes, self.pii_bytes);
        self.byte_f1 = if self.byte_precision + self.byte_recall == 0.0 {
            f64::from(self.pii_bytes == 0 && self.predicted_bytes == 0)
        } else {
            2.0 * self.byte_precision * self.byte_recall / (self.byte_precision + self.byte_recall)
        };
    }
}

pub(crate) fn run(args: Args) -> Result<(), CliError> {
    let source = read_limited(&args.annotated)?;
    let labels = label_map(args.label_map.as_deref())?;
    let resolved = resolve_pipeline(
        args.policy.as_deref(),
        &CleanOverrides::default(),
        &args.locale,
        None,
        None,
        None,
    )?;
    let pipeline = resolved.pipeline;
    let pipeline = if resolved.policy.safety_net.backend == gaze::SafetyNetPolicyBackend::Nym {
        #[cfg(feature = "safety-net-nym")]
        {
            let model_dir = std::env::var_os("GAZE_NYM_MODEL_DIR").map(PathBuf::from);
            gaze_assembly::attach_nym_safety_net(
                pipeline,
                &resolved.policy,
                model_dir.as_deref(),
                Some(DEFAULT_SAFETY_NET_INPUT_LIMIT_BYTES),
                None,
            )
            .map_err(|_| {
                CliError::SafetyNetPolicyConfigDetail(
                    "policy Nym bundle unavailable; run gaze setup --safety-net nym".into(),
                )
            })?
        }
        #[cfg(not(feature = "safety-net-nym"))]
        return Err(CliError::SafetyNetConfigDetail(
            "policy safety net nym requires the safety-net-nym feature".into(),
        ));
    } else {
        pipeline
    };
    let mut score = Score::default();
    for (index, line) in source.lines().enumerate() {
        let schema_error = || CliError::EvalSchemaLine { line: index + 1 };
        if line.trim().is_empty() {
            return Err(schema_error());
        }
        let doc: Document = serde_json::from_str(line).map_err(|_| schema_error())?;
        let spans = validate_document(&doc, &labels).map_err(|_| schema_error())?;
        let session = Session::from_policy(&resolved.policy).map_err(|_| CliError::Pipeline)?;
        let (clean, manifest, _) = pipeline
            .clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text(doc.text.clone()),
                resolved.locale_chain.as_slice(),
                &resolved.dictionaries,
                SafetyNetPolicy::default(),
            )
            .map_err(|_| CliError::Pipeline)?;
        if !matches!(clean, CleanDocument::Text(_)) {
            return Err(CliError::Pipeline);
        }
        let predicted = manifest.into_iter().map(|span| span.raw_span).collect();
        score.add(&spans, predicted);
    }
    if score.documents == 0 {
        return Err(CliError::EvalSchema);
    }
    score.finish();
    if args.json {
        let json = serde_json::to_string(&score).map_err(|_| CliError::Pipeline)?;
        println!("{json}");
    } else {
        println!(
            "Documents: {} (leak rate: {:.2}%; zero leak: {:.2}%)",
            score.documents,
            score.document_leak_rate * 100.0,
            score.zero_leak_document_rate * 100.0
        );
        println!(
            "UTF-8 bytes: PII {} | protected {} | leaked {} | false positive {}",
            score.pii_bytes,
            score.true_positive_bytes,
            score.leaked_bytes,
            score.false_positive_bytes
        );
        println!(
            "Byte precision {:.4} | recall {:.4} | F1 {:.4}",
            score.byte_precision, score.byte_recall, score.byte_f1
        );
        println!("Class\tGold bytes\tCovered\tLeaked\tEntities\tFully covered");
        for (class, row) in &score.per_class {
            println!(
                "{class}\t{}\t{}\t{}\t{}\t{}",
                row.gold_bytes,
                row.covered_bytes,
                row.leaked_bytes,
                row.entities,
                row.fully_covered
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_accounting_matches_benchmark_intersection_contract() {
        let mut score = Score::default();
        score.add(
            &[ScoredSpan {
                range: 3..5,
                class: "email".into(),
            }],
            std::iter::once(4..7).collect(),
        );
        score.finish();
        assert_eq!(score.pii_bytes, 2);
        assert_eq!(score.predicted_bytes, 3);
        assert_eq!(score.true_positive_bytes, 1);
        assert_eq!(score.leaked_bytes, 1);
        assert_eq!(score.false_positive_bytes, 2);
        assert_eq!(score.documents_without_leaks, 0);
        assert_eq!(score.document_leak_rate, 1.0);
        assert_eq!(score.per_class["email"].fully_covered, 0);
    }

    #[test]
    fn zero_true_positives_have_zero_f1_unless_both_sets_are_empty() {
        let mut missed = Score::default();
        missed.add(
            &[ScoredSpan {
                range: 0..2,
                class: "email".into(),
            }],
            std::iter::once(3..5).collect(),
        );
        missed.finish();
        assert_eq!(missed.byte_f1, 0.0);

        let mut empty = Score::default();
        empty.add(&[], vec![]);
        empty.finish();
        assert_eq!(empty.byte_f1, 1.0);
    }
}
