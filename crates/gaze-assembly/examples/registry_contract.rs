//! Value-free registration census and construction-time SDK admission evidence.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use gaze::{
    Candidate, CollisionMembership, ConflictTier, DetectContext, DetectError, DictionaryBundle,
    PiiClass, Recognizer, RecognizerRegistry, RecognizerRegistryBuilder, RegistryBuildError,
};
use serde_json::{json, Value};

struct Member {
    id: &'static str,
    source: &'static str,
    scans: Arc<AtomicUsize>,
}

impl Recognizer for Member {
    fn id(&self) -> &str {
        self.id
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(&self, text: &str, _: &DetectContext<'_>) -> Result<Vec<Candidate>, DetectError> {
        self.scans.fetch_add(1, Ordering::SeqCst);
        Ok(vec![Candidate::new(
            0..text.len(),
            PiiClass::Name,
            self.id,
            1.0,
            0,
            None,
            "counter",
            self.source,
            ConflictTier::None,
            vec![],
        )])
    }
}

fn admission() -> Value {
    // Only this standalone evidence process suppresses the expected panic hook.
    std::panic::set_hook(Box::new(|_| {}));
    let mut rows = Vec::new();
    for case in ["valid", "forward", "orphan", "duplicate"] {
        for fallible in [false, true] {
            let scans = Arc::new(AtomicUsize::new(0));
            let member = |id, source| Member {
                id,
                source,
                scans: Arc::clone(&scans),
            };
            let alpha = CollisionMembership::new("class_alpha", "alpha", 10, None);
            let beta = CollisionMembership::new("class_alpha", "beta", 20, None);
            let mut builder = RecognizerRegistryBuilder::default();
            if case == "forward" {
                builder = builder.register_collision("first", alpha.clone());
            }
            builder = builder
                .register(member("first", "first-source"))
                .register(member(
                    if case == "duplicate" {
                        "first"
                    } else {
                        "second"
                    },
                    "second-source",
                ))
                .register_collision("first", alpha)
                .register_collision(
                    if case == "duplicate" {
                        "first"
                    } else {
                        "second"
                    },
                    beta,
                );
            if case == "orphan" {
                builder = builder.register_collision(
                    "missing",
                    CollisionMembership::new("class_alpha", "beta", 0, None),
                );
            }
            let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if fallible {
                    fallible_build(builder)
                } else {
                    Ok(builder.build())
                }
            }));
            let construction_scans = scans.load(Ordering::SeqCst);
            let observed = match built {
                Ok(Ok(registry)) => {
                    let dictionaries = DictionaryBundle::default();
                    let ctx = DetectContext::new(&[], &dictionaries);
                    let candidates = registry.detect_all("sample", &ctx).expect("scan");
                    let scan_sources = candidates
                        .iter()
                        .map(|c| c.source.clone())
                        .collect::<Vec<_>>();
                    let lookup_sources = candidates
                        .iter()
                        .map(|c| {
                            registry
                                .recognizer(&c.recognizer_id)
                                .expect("lookup")
                                .detect("sample", &ctx)
                                .expect("lookup scan")[0]
                                .source
                                .clone()
                        })
                        .collect::<Vec<_>>();
                    json!({"status": "admitted", "construction_scans": construction_scans,
                        "scan_sources": scan_sources, "lookup_sources": lookup_sources,
                        "first_wins": registry.family_policy().compare("first", "second")})
                }
                Ok(Err(kind)) => json!({"status": kind, "construction_scans": construction_scans}),
                Err(payload) => {
                    let fixed =
                        payload.downcast_ref::<&str>() == Some(&"invalid recognizer registration");
                    json!({"status": if fixed { "fixed_panic" } else { "unexpected_panic" },
                        "construction_scans": construction_scans})
                }
            };
            rows.push(
                json!({"case": case, "entrypoint": if fallible { "try_build" } else { "build" },
                "observed": observed}),
            );
        }
    }
    json!({"schema_version": 1, "scope": "construction admission; no corpus refusal or leak credit", "rows": rows})
}

fn fallible_build(builder: RecognizerRegistryBuilder) -> Result<RecognizerRegistry, &'static str> {
    builder.try_build().map_err(|error| match error {
        RegistryBuildError::DuplicateRecognizerId { .. } => "duplicate_id",
        RegistryBuildError::OrphanCollisionMembership { .. } => "orphan_membership",
    })
}

fn census(path: &std::path::Path) -> Result<Value, Box<dyn std::error::Error>> {
    let policy = gaze::Policy::load_for_cli(path)?;
    let inputs = gaze_assembly::resolve_policy_inputs(&policy, None, None, None)?;
    let context = gaze::Context {
        dictionaries: Default::default(),
        class_map: Default::default(),
        fields: Default::default(),
        record_match_kinds: Default::default(),
        record_value_rejections: Default::default(),
    };
    let pipeline = gaze_assembly::build_pipeline(
        &policy,
        &context,
        &inputs.rulepacks,
        &inputs.locale_chain,
        Some(inputs.ner_threshold),
    )?;
    let registry = pipeline.registry();
    let rows = registry.recognizer_ids().map(|id| {
        let recognizer = registry.recognizer(id).expect("admitted recognizer");
        let membership = registry.family_policy().membership(id).map(|m| json!({
            "family": m.family, "variant": m.variant, "precedence": m.precedence,
            "mandatory_anchor": m.mandatory_anchor
        }));
        json!({"id": id, "class": format!("{:?}", recognizer.supported_class()),
            "classes": recognizer.possible_classes().iter().map(|class| format!("{class:?}")).collect::<Vec<_>>(),
            "locales": recognizer.locales().iter().map(|locale| locale.as_str()).collect::<Vec<_>>(),
            "locale_basis": format!("{:?}", recognizer.locale_basis()),
            "token_family": recognizer.token_family(), "membership": membership})
    }).collect::<Vec<_>>();
    Ok(json!({"schema_version": 1, "ordered_registrations": rows}))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let argument = std::env::args()
        .nth(1)
        .ok_or("supply a policy path or --admission")?;
    let report = if argument == "--admission" {
        admission()
    } else {
        census(std::path::Path::new(&argument))?
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
