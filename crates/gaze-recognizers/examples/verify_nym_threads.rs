//! Proves Nym piece scores are bit-identical across ONNX Runtime intra-op thread counts.
//!
//! Reads `{"text": ...}` JSON lines on stdin and scores each text with one thread and with
//! every count in `GAZE_NYM_VERIFY_THREADS` (default "2,4,8"), comparing tokenizer offsets,
//! labels, begin flags and every score bit. Exits non-zero on the first difference.

use std::io::{self, BufRead};
use std::num::NonZeroUsize;

use gaze_recognizers::safety_net::nym::{test_support, NymConfig, NymSafetyNet};
use serde::Deserialize;

#[derive(Deserialize)]
struct Input {
    text: String,
}

fn net(threads: NonZeroUsize) -> Result<NymSafetyNet, Box<dyn std::error::Error>> {
    let net = NymSafetyNet::new(NymConfig::from_env()?.with_intra_threads(threads));
    net.preload()?;
    Ok(net)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let counts = std::env::var("GAZE_NYM_VERIFY_THREADS").unwrap_or_else(|_| "2,4,8".into());
    let counts = counts
        .split(',')
        .map(|n| n.trim().parse::<NonZeroUsize>())
        .collect::<Result<Vec<_>, _>>()?;
    let base = net(NonZeroUsize::MIN)?;
    let others = counts
        .iter()
        .map(|&n| net(n).map(|net| (n, net)))
        .collect::<Result<Vec<_>, _>>()?;
    let (mut documents, mut pieces) = (0usize, 0usize);
    for line in io::stdin().lock().lines() {
        let input: Input = serde_json::from_str(&line?)?;
        let (offsets, scores) = test_support::capture(&base, &input.text)?;
        let bits = |s: &[test_support::PieceScore]| {
            s.iter()
                .map(|p| (p.label, p.mass.to_bits(), p.is_begin))
                .collect::<Vec<_>>()
        };
        let expected = bits(&scores);
        for (threads, other) in &others {
            let (other_offsets, other_scores) = test_support::capture(other, &input.text)?;
            if other_offsets != offsets || bits(&other_scores) != expected {
                return Err(format!(
                    "document {}: {threads} threads changed Nym piece scores",
                    documents + 1
                )
                .into());
            }
        }
        documents += 1;
        pieces += scores.len();
    }
    if documents == 0 || pieces == 0 {
        return Err("thread equivalence proof scored no pieces".into());
    }
    println!(
        "{}",
        serde_json::json!({"documents": documents, "pieces": pieces, "threads": counts.iter().map(|n| n.get()).collect::<Vec<_>>()})
    );
    Ok(())
}
