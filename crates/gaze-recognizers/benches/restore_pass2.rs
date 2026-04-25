use std::ops::Range;

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn is_inside_substitution_span(
    start: usize,
    end: usize,
    spans: &[Range<usize>],
    cursor: &mut usize,
) -> bool {
    while spans.get(*cursor).is_some_and(|span| span.end <= start) {
        *cursor += 1;
    }

    spans
        .get(*cursor)
        .is_some_and(|span| span.start <= start && end <= span.end)
}

fn pass2_substitution_span_scan_n_1000(c: &mut Criterion) {
    let spans = (0..1_000)
        .map(|i| {
            let start = i * 16;
            start..start + 8
        })
        .collect::<Vec<_>>();

    c.bench_function("pass2_substitution_span_scan_n_1000", |b| {
        b.iter(|| {
            let spans = black_box(&spans);
            let mut cursor = 0usize;
            let mut inside_count = 0usize;
            for span in spans {
                if is_inside_substitution_span(span.start, span.end, spans, &mut cursor) {
                    inside_count += 1;
                }
            }
            black_box((inside_count, cursor))
        })
    });
}

criterion_group!(benches, pass2_substitution_span_scan_n_1000);
criterion_main!(benches);
