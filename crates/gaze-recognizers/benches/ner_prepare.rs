//! Tokenization, window planning, and i64 input buffers only; excludes ONNX inference.
//! Run with GAZE_NER_MODEL_DIR pointing at the pinned Davlan bundle.

use std::path::PathBuf;
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use tokenizers::{Encoding, Tokenizer};

#[path = "../tests/support/ner_preparation_fixtures.rs"]
mod fixture_set;
#[path = "../src/ner/backend/preparation.rs"]
mod preparation;

fn input_buffers(encoded: Encoding) {
    let ids: Vec<i64> = encoded.get_ids().iter().map(|&id| i64::from(id)).collect();
    let mask: Vec<i64> = encoded
        .get_attention_mask()
        .iter()
        .map(|&id| i64::from(id))
        .collect();
    let types = vec![0i64; encoded.len()];
    black_box((&ids, &mask, &types, encoded.get_offsets()));
}

fn baseline(tokenizer: &Tokenizer, input: &str) {
    let encoded = preparation::planning_encoding(tokenizer, input).unwrap();
    let ranges = preparation::chunk_ranges_from_encoding(&encoded, input);
    drop(encoded);
    for range in ranges {
        input_buffers(preparation::encode_input(tokenizer, &input[range]).unwrap());
    }
}

fn candidate(tokenizer: &Tokenizer, input: &str) {
    for window in preparation::prepare_windows(tokenizer, input).unwrap() {
        input_buffers(window.unwrap().1);
    }
}

type Prepare = fn(&Tokenizer, &str);

fn ner_prepare(c: &mut Criterion) {
    let directory =
        PathBuf::from(std::env::var_os("GAZE_NER_MODEL_DIR").expect("NER model directory"));
    gaze_recognizers::verify_davlan_ner_bundle(&directory).expect("pinned Davlan bundle");
    let tokenizer = Tokenizer::from_file(directory.join("tokenizer.json")).unwrap();
    let fixtures = fixture_set::fixtures();
    let mut group = c.benchmark_group("ner_prepare");
    group.sample_size(20);
    group.warm_up_time(Duration::from_millis(200));
    group.measurement_time(Duration::from_millis(500));
    // Alternate side order across five rounds on the same loaded machine.
    for round in 0..5 {
        if let Ok(load) = std::process::Command::new("uptime").output() {
            eprintln!(
                "round {round}: {}",
                String::from_utf8_lossy(&load.stdout).trim()
            );
        }
        for (name, input) in fixtures
            .iter()
            .filter(|(name, _)| matches!(name.as_str(), "short" | "medium" | "json_tool_call"))
        {
            let sides: [(&str, Prepare); 2] = if round % 2 == 0 {
                [("baseline", baseline), ("candidate", candidate)]
            } else {
                [("candidate", candidate), ("baseline", baseline)]
            };
            for (side, prepare) in sides {
                group.bench_with_input(
                    BenchmarkId::new(format!("{name}/{side}"), round),
                    input,
                    |b, input| b.iter(|| prepare(black_box(&tokenizer), black_box(input))),
                );
            }
        }
    }
    group.finish();
}

criterion_group!(benches, ner_prepare);
criterion_main!(benches);
