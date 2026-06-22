use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use gaze::{
    Action, ClassRule, DefaultRule, EmittedTokenSpan, LocaleTag, PiiClass, Pipeline, RawDocument,
    RawMatch, RecognizerSpec, Rulepack, RulepackSource, Scope, Session,
};
use gaze_recognizers::{embedded, NormalizerKind, RegexDetector, ValidatorKind};
use serde::{Deserialize, Serialize};

#[test]
#[ignore]
fn coverage_loop_reports_labels_against_core_pipeline() -> Result<(), Box<dyn Error>> {
    let root = coverage_root();
    let fixtures = load_fixtures(&root.join("corpus"))?;
    assert!(
        !fixtures.is_empty(),
        "coverage-loop corpus must not be empty"
    );

    let mut report = CoverageReport::new();

    for fixture in fixtures {
        report.record_fixture();
        let locale_chain = fixture
            .labels
            .locale_chain
            .iter()
            .map(|locale| LocaleTag::parse(locale))
            .collect::<Result<Vec<_>, _>>()?;
        let core = core_pipeline()?;
        let session = Session::new(Scope::Ephemeral)?;
        let (_, manifest, _) = core.clean_with_safety_net(
            &session,
            RawDocument::Text(fixture.body.clone()),
            &locale_chain,
        )?;

        for span in &fixture.labels.spans {
            assert_eq!(
                span.license_origin, "synthetic-rust-generator",
                "unexpected license origin in {}",
                fixture.labels.fixture_id
            );
            let class = class_from_label(&span.class_id)?;
            let verdict = classify_label(span.byte_start..span.byte_end, &class, &manifest);
            report.record(
                &span.class_id,
                fixture
                    .labels
                    .locale_chain
                    .first()
                    .map(String::as_str)
                    .unwrap_or("global"),
                verdict,
            );
        }
    }

    let target = workspace_target_dir();
    fs::create_dir_all(&target)?;
    fs::write(target.join("coverage-report.md"), report.to_markdown())?;
    fs::write(
        target.join("coverage-report.json"),
        serde_json::to_string_pretty(&report)?,
    )?;

    println!("{}", report.to_markdown());
    enforce_trend_gate(&root.join("baseline.json"), &report)?;
    Ok(())
}

fn core_pipeline() -> Result<Pipeline, Box<dyn Error>> {
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core-extended").expect("core-extended embedded rulepack"),
    ))?;
    let mut builder = Pipeline::builder()
        .rule(DefaultRule::new(Action::Tokenize))
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
        .rule(ClassRule::new(
            PiiClass::Custom("phone".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("ip_address".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("eth_address".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("postal_code".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("iban".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("credit_card".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("ssn".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("nino".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("pan".to_string()),
            Action::Tokenize,
        ));

    for spec in rulepack
        .recognizers
        .iter()
        .filter(|recognizer| recognizer.enabled)
    {
        if let Some(collision) = spec.collision.clone() {
            builder = builder.register_collision(spec.id.clone(), collision);
        }
        if matches!(spec.matcher, RawMatch::Regex { .. }) {
            builder = builder.recognizer(regex_from_spec(&rulepack, spec)?);
        }
    }

    Ok(builder.build()?)
}

fn regex_from_spec(
    rulepack: &Rulepack,
    spec: &RecognizerSpec,
) -> Result<RegexDetector, Box<dyn Error>> {
    let RawMatch::Regex {
        pattern,
        pattern_template,
        capture_groups,
    } = &spec.matcher
    else {
        unreachable!("caller filters non-regex recognizers");
    };
    let pattern = match (pattern.as_deref(), pattern_template.as_deref()) {
        (Some(pattern), None) => pattern.to_string(),
        (None, Some(template)) => lower_pattern_template(rulepack, template)?,
        _ => return Err(format!("invalid regex recognizer pattern shape for {}", spec.id).into()),
    };
    let exclusions = spec
        .context
        .as_ref()
        .map(|context| context.exclusions.clone())
        .unwrap_or_default();
    let validator_kind = spec
        .validator
        .as_ref()
        .map(|validator| ValidatorKind::parse(&validator.kind))
        .transpose()?;
    let normalizer_kind = spec
        .normalizer
        .as_ref()
        .map(|normalizer| NormalizerKind::parse(&normalizer.kind))
        .transpose()?;

    Ok(RegexDetector::with_rulepack_fields(
        &pattern,
        spec.class.clone(),
        &spec.id,
        spec.locales.clone(),
        spec.scoring.base,
        spec.scoring.priority,
        spec.token.family.as_deref().unwrap_or("counter"),
        capture_groups.clone(),
        exclusions,
        validator_kind,
        normalizer_kind,
    )?)
}

fn lower_pattern_template(rulepack: &Rulepack, template: &str) -> Result<String, Box<dyn Error>> {
    let Some(locale) = &rulepack.locale else {
        return Err("pattern template requires locale buckets".into());
    };
    let mut pattern = template.to_string();
    for (bucket, values) in &locale.buckets {
        let marker = format!("{{locale.{bucket}}}");
        if pattern.contains(&marker) {
            let alternation = values
                .names
                .iter()
                .map(|value| regex::escape(value))
                .collect::<Vec<_>>()
                .join("|");
            pattern = pattern.replace(&marker, &alternation);
        }
    }
    if pattern.contains("{locale.") {
        return Err(format!("unresolved locale pattern template: {pattern}").into());
    }
    Ok(pattern)
}

fn load_fixtures(corpus_dir: &Path) -> Result<Vec<Fixture>, Box<dyn Error>> {
    let mut label_paths = fs::read_dir(corpus_dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    label_paths.retain(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".labels.json"))
    });
    label_paths.sort();

    label_paths
        .into_iter()
        .map(|labels_path| {
            let labels = serde_json::from_str::<Labels>(&fs::read_to_string(&labels_path)?)?;
            let text_path = labels_path.with_file_name(format!("{}.txt", labels.fixture_id));
            let body = fs::read_to_string(text_path)?;
            Ok(Fixture { body, labels })
        })
        .collect()
}

fn classify_label(
    label_span: Range<usize>,
    label_class: &PiiClass,
    manifest: &[EmittedTokenSpan],
) -> Verdict {
    let overlapping = manifest
        .iter()
        .filter(|span| ranges_overlap(&span.raw_span, &label_span))
        .collect::<Vec<_>>();

    if overlapping.is_empty() {
        return Verdict::Uncovered;
    }

    if overlapping.iter().any(|span| {
        span.class == *label_class
            && span.raw_span.start <= label_span.start
            && span.raw_span.end >= label_span.end
    }) {
        return Verdict::Covered;
    }

    if overlapping.iter().any(|span| span.class == *label_class) {
        return Verdict::PartialBleed;
    }

    Verdict::ClassMismatch
}

fn ranges_overlap(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

fn class_from_label(label: &str) -> Result<PiiClass, Box<dyn Error>> {
    match label {
        "Email" => Ok(PiiClass::Email),
        "Name" => Ok(PiiClass::Name),
        "Location" => Ok(PiiClass::Location),
        "Organization" => Ok(PiiClass::Organization),
        custom if custom.starts_with("custom:") => PiiClass::from_policy_name(custom)
            .ok_or_else(|| format!("invalid custom class label {custom:?}").into()),
        other => Err(format!("unknown class label {other:?}").into()),
    }
}

fn coverage_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/coverage-loop")
}

fn workspace_target_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .join("target")
}

fn enforce_trend_gate(
    baseline_path: &Path,
    current: &CoverageReport,
) -> Result<(), Box<dyn Error>> {
    if env::var("GAZE_COVERAGE_LOOP_INFO_ONLY")
        .map(|value| value == "1")
        .unwrap_or(true)
    {
        return Ok(());
    }
    if !baseline_path.exists() {
        eprintln!(
            "coverage-loop baseline missing at {}; trend gate remains informational",
            baseline_path.display()
        );
        return Ok(());
    }

    let baseline = serde_json::from_str::<CoverageReport>(&fs::read_to_string(baseline_path)?)?;
    let regressions = current.uncovered_regressions(&baseline);
    if regressions.is_empty() {
        return Ok(());
    }

    Err(format!(
        "coverage-loop uncovered regressions:\n{}",
        regressions.join("\n")
    )
    .into())
}

#[derive(Debug)]
struct Fixture {
    body: String,
    labels: Labels,
}

#[derive(Debug, Deserialize)]
struct Labels {
    fixture_id: String,
    locale_chain: Vec<String>,
    spans: Vec<LabelSpan>,
}

#[derive(Debug, Deserialize)]
struct LabelSpan {
    byte_start: usize,
    byte_end: usize,
    class_id: String,
    license_origin: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct CoverageReport {
    schema_version: u8,
    phase: String,
    generated_at_utc: String,
    rulepack_versions: BTreeMap<String, String>,
    oracle: String,
    leak_set: BTreeMap<String, BTreeMap<String, BucketCounts>>,
    totals: ReportTotals,
}

impl CoverageReport {
    fn new() -> Self {
        Self {
            schema_version: 1,
            phase: "2-5".to_string(),
            generated_at_utc: "2026-05-10T00:00:00Z".to_string(),
            rulepack_versions: BTreeMap::from([
                ("core".to_string(), "0.5.1".to_string()),
                ("core-extended".to_string(), "0.5.1".to_string()),
            ]),
            oracle: "labels.json (Phase 2-5 synthetic corpus)".to_string(),
            leak_set: BTreeMap::new(),
            totals: ReportTotals::default(),
        }
    }

    fn record_fixture(&mut self) {
        self.totals.fixture_count += 1;
    }

    fn record(&mut self, class_id: &str, locale: &str, verdict: Verdict) {
        let counts = self
            .leak_set
            .entry(class_id.to_string())
            .or_default()
            .entry(locale.to_string())
            .or_default();
        match verdict {
            Verdict::Covered => {
                counts.covered += 1;
                self.totals.covered_total += 1;
            }
            Verdict::Uncovered => {
                counts.uncovered += 1;
                self.totals.uncovered_total += 1;
            }
            Verdict::PartialBleed => {
                counts.partial_bleed += 1;
                self.totals.partial_bleed_total += 1;
            }
            Verdict::ClassMismatch => {
                counts.class_mismatch += 1;
                self.totals.class_mismatch_total += 1;
            }
        }
        self.totals.labeled_span_total += 1;
    }

    fn to_markdown(&self) -> String {
        let mut output = String::from(
            "# Coverage Loop Report\n\n| Class | Locale | Covered | Uncovered | PartialBleed | ClassMismatch |\n|---|---:|---:|---:|---:|---:|\n",
        );
        for (class_id, locales) in &self.leak_set {
            for (locale, counts) in locales {
                output.push_str(&format!(
                    "| {class_id} | {locale} | {} | {} | {} | {} |\n",
                    counts.covered, counts.uncovered, counts.partial_bleed, counts.class_mismatch
                ));
            }
        }
        output
    }

    fn uncovered_regressions(&self, baseline: &CoverageReport) -> Vec<String> {
        let mut regressions = Vec::new();
        for (class_id, locales) in &self.leak_set {
            for (locale, current) in locales {
                let baseline_uncovered = baseline
                    .leak_set
                    .get(class_id)
                    .and_then(|baseline_locales| baseline_locales.get(locale))
                    .map(|counts| counts.uncovered)
                    .unwrap_or(0);
                if current.uncovered > baseline_uncovered {
                    regressions.push(format!(
                        "{class_id}/{locale}: uncovered {} > baseline {}",
                        current.uncovered, baseline_uncovered
                    ));
                }
            }
        }
        regressions
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct BucketCounts {
    covered: usize,
    uncovered: usize,
    partial_bleed: usize,
    class_mismatch: usize,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct ReportTotals {
    fixture_count: usize,
    labeled_span_total: usize,
    uncovered_total: usize,
    partial_bleed_total: usize,
    class_mismatch_total: usize,
    covered_total: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Covered,
    Uncovered,
    PartialBleed,
    ClassMismatch,
}

#[cfg(test)]
mod selfcheck {
    use super::*;

    #[test]
    fn classifier_returns_uncovered_for_unmatched_span() {
        let verdict = classify_label(0..5, &PiiClass::Email, &[]);
        assert_eq!(verdict, Verdict::Uncovered);
    }

    #[test]
    fn trend_gate_flags_uncovered_regression() {
        let mut baseline = CoverageReport::new();
        baseline.record("Email", "global", Verdict::Covered);

        let mut current = CoverageReport::new();
        current.record("Email", "global", Verdict::Uncovered);

        assert_eq!(
            current.uncovered_regressions(&baseline),
            vec!["Email/global: uncovered 1 > baseline 0"]
        );
    }
}
