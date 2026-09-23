//! Development-split threshold sweep for the Nym recognizer (single-pass Stage A).
//!
//! Reads `{"id", "text"}` JSONL on stdin. For each document it normalizes the text exactly as the
//! pipeline does before any recognizer runs, runs the pinned Nym model once on that normalized
//! text, and replays the captured piece scores through the production decoder at every grid
//! threshold of every recognizer label while the other labels stay at op-B. Every decoded span is
//! mapped back to raw bytes through the normalization map. One JSONL line per document:
//! `{"id", "spans": {"LABEL": {"0.50": [[start, end], ...], ...}}, "joint": [[start, end, "LABEL"]]}`
//! where `joint` is the decode at `--joint LABEL=T,...` (op-B when omitted).
//!
//! `scripts/bench/nym_recognizer_dev_sweep.py` builds the development split, runs this binary and
//! applies the selection rule. Needs `GAZE_NYM_MODEL_DIR`; `GAZE_NYM_INTRA_THREADS` is honoured.

use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};

use gaze_recognizers::safety_net::nym::test_support::{capture, decode_captured};
use gaze_recognizers::safety_net::nym::{NymLabel, NymOperatingPoint, NymSafetyNet};
use serde::{Deserialize, Serialize};

const LABELS: [NymLabel; 4] = [
    NymLabel::BuildingNumber,
    NymLabel::DateOfBirth,
    NymLabel::LicensePlate,
    NymLabel::Username,
];

const GRID: [f32; 16] = [
    0.30, 0.35, 0.40, 0.45, 0.50, 0.55, 0.60, 0.65, 0.70, 0.75, 0.80, 0.85, 0.90, 0.95, 0.97, 0.99,
];

#[derive(Deserialize)]
struct Request {
    id: String,
    text: String,
}

#[derive(Serialize)]
struct Response {
    id: String,
    spans: BTreeMap<String, BTreeMap<String, Vec<[usize; 2]>>>,
    joint: Vec<(usize, usize, String)>,
}

fn with_threshold(label: NymLabel, threshold: f32) -> NymOperatingPoint {
    let op_b = NymOperatingPoint::op_b();
    NymOperatingPoint::new(
        op_b.iter().map(|(enabled, default)| {
            (enabled, if enabled == label { threshold } else { default })
        }),
    )
    .expect("op-B with one threshold moved is valid")
}

fn parse_joint(raw: &str) -> Result<NymOperatingPoint, Box<dyn std::error::Error>> {
    let mut pairs = Vec::new();
    for item in raw.split(',') {
        let (label, threshold) = item.split_once('=').ok_or("--joint wants LABEL=T,...")?;
        let label = NymLabel::parse(label).ok_or("unknown label in --joint")?;
        pairs.push((label, threshold.parse::<f32>()?));
    }
    Ok(NymOperatingPoint::new(pairs)?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut joint = NymOperatingPoint::op_b();
    while let Some(arg) = args.next() {
        if arg == "--joint" {
            joint = parse_joint(&args.next().ok_or("--joint requires a value")?)?;
        } else {
            return Err(format!("unknown argument {arg}").into());
        }
    }
    let net = NymSafetyNet::from_env()?;
    net.preload()?;
    let grid_points = LABELS
        .iter()
        .flat_map(|label| {
            GRID.iter()
                .map(move |threshold| (*label, *threshold, with_threshold(*label, *threshold)))
        })
        .collect::<Vec<_>>();

    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = serde_json::from_str(&line)?;
        let (normalized, map) = gaze::normalize_for_tests(&request.text);
        let (offsets, scores) = capture(&net, &normalized)?;
        let to_raw = |range: &std::ops::Range<usize>| [map[range.start].0, map[range.end - 1].1];
        let mut spans: BTreeMap<String, BTreeMap<String, Vec<[usize; 2]>>> = BTreeMap::new();
        for (label, threshold, op) in &grid_points {
            let decoded = decode_captured(&normalized, &offsets, &scores, op)?;
            spans.entry(label.to_string()).or_default().insert(
                format!("{threshold:.2}"),
                decoded
                    .iter()
                    .filter(|(_, decoded_label, _)| decoded_label == label)
                    .map(|(range, _, _)| to_raw(range))
                    .collect(),
            );
        }
        let joint = decode_captured(&normalized, &offsets, &scores, &joint)?
            .iter()
            .map(|(range, label, _)| {
                let [start, end] = to_raw(range);
                (start, end, label.to_string())
            })
            .collect();
        serde_json::to_writer(
            &mut stdout,
            &Response {
                id: request.id,
                spans,
                joint,
            },
        )?;
        stdout.write_all(b"\n")?;
    }
    stdout.flush()?;
    Ok(())
}
