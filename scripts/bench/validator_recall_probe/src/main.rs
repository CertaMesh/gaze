use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, Write};

use gaze::{
    Action, Context, DictionaryBundle, LocaleChain, LocaleTag, PiiClass, Pipeline, RuleSpec,
    Rulepack, RulepackSource, SafetyNetFallback, SafetyNetMode, SafetyNetPolicy, Scope, Session,
};
use gaze_types::ValidatorKind;
use serde::{Deserialize, Serialize};

const PROTOCOL_SCHEMA_VERSION: u8 = 1;
const BENCHMARK_ACTIVE_LOCALES: &[LocaleTag] = &[
    LocaleTag::EnUs,
    LocaleTag::DeDe,
    LocaleTag::DeAt,
    LocaleTag::DeCh,
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    fixture_id: String,
    locale_chain: Vec<String>,
    text: String,
    gold_spans: Vec<GoldSpan>,
    measure_predictions: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GoldSpan {
    start: usize,
    end: usize,
    label: String,
    applicable_classes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Handshake {
    schema_version: u8,
    validator_kinds_by_class: BTreeMap<String, Vec<String>>,
    validator_recognizers: Vec<ValidatorRecognizer>,
}

#[derive(Debug, Serialize)]
struct ValidatorRecognizer {
    id: String,
    class: String,
    validator_kind: String,
}

#[derive(Debug, Serialize)]
struct Response {
    fixture_id: String,
    gold_validation: Vec<GoldValidation>,
    predictions: Option<PredictionPair>,
}

#[derive(Debug, Serialize)]
struct GoldValidation {
    start: usize,
    end: usize,
    label: String,
    applicable: bool,
    validator_passed: Option<bool>,
}

#[derive(Debug, Serialize)]
struct PredictionPair {
    validator_backed: Vec<Prediction>,
    shape_only: Vec<Prediction>,
}

#[derive(Debug, Serialize)]
struct Prediction {
    start: usize,
    end: usize,
    class: String,
    source_ids: Vec<String>,
}

struct Probe {
    validator_kinds_by_class: BTreeMap<String, Vec<String>>,
    validator_pipeline: Pipeline,
    shape_only_pipeline: Pipeline,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let probe = Probe::build()?;
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());

    serde_json::to_writer(&mut stdout, &probe.handshake())?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = serde_json::from_str(&line)?;
        let response = probe.measure(request)?;
        serde_json::to_writer(&mut stdout, &response)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }
    Ok(())
}

impl Probe {
    fn build() -> Result<Self, Box<dyn std::error::Error>> {
        let validator_rulepack = Rulepack::load(RulepackSource::Embedded(
            gaze_recognizers::embedded("core-extended")
                .ok_or("embedded core-extended rulepack is unavailable")?,
        ))?;
        let validator_kinds_by_class = validator_kinds_by_class(&validator_rulepack);
        let mut shape_only_rulepack = validator_rulepack.clone();
        for recognizer in &mut shape_only_rulepack.recognizers {
            recognizer.validator = None;
        }

        Ok(Self {
            validator_pipeline: build_rule_floor(validator_rulepack)?,
            shape_only_pipeline: build_rule_floor(shape_only_rulepack)?,
            validator_kinds_by_class,
        })
    }

    fn handshake(&self) -> Handshake {
        let validator_rulepack = Rulepack::load(RulepackSource::Embedded(
            gaze_recognizers::embedded("core-extended")
                .expect("embedded rulepack was present while building the probe"),
        ))
        .expect("embedded rulepack parsed while building the probe");
        let mut validator_recognizers = validator_rulepack
            .recognizers
            .iter()
            .filter_map(|recognizer| {
                recognizer
                    .validator
                    .as_ref()
                    .map(|validator| ValidatorRecognizer {
                        id: recognizer.id.clone(),
                        class: recognizer.class.to_canonical_str(),
                        validator_kind: validator.kind.clone(),
                    })
            })
            .collect::<Vec<_>>();
        validator_recognizers.sort_by(|left, right| left.id.cmp(&right.id));
        Handshake {
            schema_version: PROTOCOL_SCHEMA_VERSION,
            validator_kinds_by_class: self.validator_kinds_by_class.clone(),
            validator_recognizers,
        }
    }

    fn measure(&self, request: Request) -> Result<Response, Box<dyn std::error::Error>> {
        let gold_validation = request
            .gold_spans
            .iter()
            .map(|span| self.validate_gold_span(&request.text, span))
            .collect::<Result<Vec<_>, _>>()?;
        let predictions = if request.measure_predictions {
            let locale_chain = request
                .locale_chain
                .iter()
                .map(|locale| LocaleTag::parse(locale))
                .collect::<Result<Vec<_>, _>>()?;
            Some(PredictionPair {
                validator_backed: run_pipeline(
                    &self.validator_pipeline,
                    &request.text,
                    &locale_chain,
                )?,
                shape_only: run_pipeline(&self.shape_only_pipeline, &request.text, &locale_chain)?,
            })
        } else {
            None
        };

        Ok(Response {
            fixture_id: request.fixture_id,
            gold_validation,
            predictions,
        })
    }

    fn validate_gold_span(
        &self,
        text: &str,
        span: &GoldSpan,
    ) -> Result<GoldValidation, Box<dyn std::error::Error>> {
        let value = text
            .get(span.start..span.end)
            .ok_or("gold span is not on UTF-8 boundaries")?;
        let validator_kinds = span
            .applicable_classes
            .iter()
            .flat_map(|class| {
                self.validator_kinds_by_class
                    .get(class)
                    .into_iter()
                    .flatten()
            })
            .collect::<BTreeSet<_>>();
        let applicable = !validator_kinds.is_empty();
        let validator_passed = applicable.then(|| {
            validator_kinds.iter().any(|kind| {
                ValidatorKind::parse(kind)
                    .expect("embedded rulepack validator kind must be supported")
                    .validates(value)
            })
        });
        Ok(GoldValidation {
            start: span.start,
            end: span.end,
            label: span.label.clone(),
            applicable,
            validator_passed,
        })
    }
}

fn validator_kinds_by_class(rulepack: &Rulepack) -> BTreeMap<String, Vec<String>> {
    let mut inventory = BTreeMap::<String, BTreeSet<String>>::new();
    for recognizer in &rulepack.recognizers {
        if let Some(validator) = &recognizer.validator {
            inventory
                .entry(recognizer.class.to_canonical_str())
                .or_default()
                .insert(validator.kind.clone());
        }
    }
    inventory
        .into_iter()
        .map(|(class, kinds)| (class, kinds.into_iter().collect()))
        .collect()
}

fn build_rule_floor(rulepack: Rulepack) -> Result<Pipeline, Box<dyn std::error::Error>> {
    let mut policy = benchmark_policy(&rulepack);
    let active_locales = benchmark_locale_chain(&policy, &rulepack);
    policy.rulepacks.auto_activate_locale_gated = true;
    Ok(gaze_assembly::build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        None,
    )?)
}

fn benchmark_policy(rulepack: &Rulepack) -> gaze::Policy {
    let mut seen = BTreeSet::<PiiClass>::new();
    let mut rules = Vec::new();
    for recognizer in rulepack
        .recognizers
        .iter()
        .filter(|recognizer| recognizer.enabled)
    {
        if seen.insert(recognizer.class.clone()) {
            rules.push(RuleSpec::Class {
                class: recognizer.class.clone(),
                action: Action::Tokenize,
            });
        }
        if let Some(family_class) = recognizer.collision.as_ref().and_then(|collision| {
            collision
                .mandatory_anchor
                .as_ref()
                .map(|_| PiiClass::Custom(format!("family:{}", collision.family)))
        }) {
            if seen.insert(family_class.clone()) {
                rules.push(RuleSpec::Class {
                    class: family_class,
                    action: Action::Tokenize,
                });
            }
        }
    }
    rules.push(RuleSpec::Default {
        action: Action::Tokenize,
    });

    let mut policy = gaze::Policy::default();
    policy.rules = rules;
    policy.rulepacks.auto_activate_locale_gated = true;
    policy
}

fn benchmark_locale_chain(policy: &gaze::Policy, rulepack: &Rulepack) -> LocaleChain {
    let mut defaults = rulepack.default_locales.clone();
    for locale in BENCHMARK_ACTIVE_LOCALES {
        if !defaults.iter().any(|existing| existing == locale) {
            defaults.push(locale.clone());
        }
    }
    LocaleChain::merge_cli_policy_rulepack_default(None, policy.locale.as_deref(), Some(&defaults))
}

fn empty_context() -> Context {
    Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
    }
}

fn run_pipeline(
    pipeline: &Pipeline,
    text: &str,
    locale_chain: &[LocaleTag],
) -> Result<Vec<Prediction>, Box<dyn std::error::Error>> {
    let session = Session::new(Scope::Ephemeral)?;
    let (_, _, _, trace) = pipeline
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            text,
            locale_chain,
            &DictionaryBundle::default(),
            SafetyNetPolicy::new(SafetyNetMode::Strict, SafetyNetFallback::Redact),
        )?;
    Ok(trace
        .into_iter()
        .map(|item| Prediction {
            start: item.raw_start(),
            end: item.raw_end(),
            class: item.class().to_canonical_str(),
            source_ids: item.source_ids().to_vec(),
        })
        .collect())
}
