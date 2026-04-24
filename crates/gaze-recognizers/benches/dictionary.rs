use criterion::{black_box, criterion_group, criterion_main, Criterion};
use gaze::{
    ContextDictionary, DetectContext, DictionaryBundle, PiiClass, Recognizer, TypedContext,
};
use gaze_recognizers::DictionaryRecognizer;
use std::collections::HashMap;

fn build_recognizer(term_count: usize) -> (DictionaryRecognizer, DictionaryBundle) {
    let terms = (0..term_count)
        .map(|index| format!("TERM-{index:05}"))
        .collect::<Vec<_>>();
    let ctx = TypedContext {
        dictionaries: HashMap::from([(
            "orders".to_string(),
            ContextDictionary {
                terms,
                case_sensitive: true,
            },
        )]),
        class_map: HashMap::new(),
        fields: serde_json::Map::new(),
    };
    let bundle = DictionaryBundle::from_context(&ctx);
    (
        DictionaryRecognizer::new(
            "dict/orders",
            PiiClass::Custom("order_id".to_string()),
            "orders",
            true,
            "counter",
        ),
        bundle,
    )
}

fn dictionary_baseline(c: &mut Criterion) {
    let (recognizer_15k, bundle_15k) = build_recognizer(15_000);
    let fields = serde_json::Map::new();
    let ctx_15k = DetectContext {
        locale_chain: &[gaze::LocaleTag::Global],
        dictionaries: &bundle_15k,
        fields: &fields,
        degraded: std::cell::Cell::new(false),
    };
    let input_1kb = "Customer referenced TERM-14999. ".repeat(32);
    c.bench_function("dictionary_15k_terms_1kb", |b| {
        b.iter(|| recognizer_15k.detect(black_box(&input_1kb), &ctx_15k))
    });

    let (recognizer_50k, bundle_50k) = build_recognizer(50_000);
    let ctx_50k = DetectContext {
        locale_chain: &[gaze::LocaleTag::Global],
        dictionaries: &bundle_50k,
        fields: &fields,
        degraded: std::cell::Cell::new(false),
    };
    let input_10kb = "Customer referenced TERM-49999. ".repeat(320);
    c.bench_function("dictionary_50k_terms_10kb", |b| {
        b.iter(|| recognizer_50k.detect(black_box(&input_10kb), &ctx_50k))
    });
}

criterion_group!(benches, dictionary_baseline);
criterion_main!(benches);
