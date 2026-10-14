# Nym personal names

The pinned Nym model emits `GIVEN_NAME` and `SURNAME`. Both explicitly map to
`Name`; the other unsupported labels still fail policy loading. This changes
the former contract that excluded all personal-name labels. The purpose is to
add a reversible backstop for names the primary pipeline misses, including
some lowercase names, without downloading another model.

Adjacent name parts join over inline whitespace into one value. This includes
multiple given names and surnames. Line breaks, punctuation, unlabelled words
and intervening tokens prevent a join. The existing shared word rule still
expands partial-word findings. The manifest keeps the exact original bytes;
the audit label keeps every contributing `LABEL>=THRESHOLD`, and confidence is
the minimum over the contributing parts. Rule and NER tokens keep precedence.

## Offline calibration

The two thresholds were frozen at **0.5** before running the benchmark. The
calibration set contains six invented two/three-part names in title case,
lowercase and uppercase across prose, chat and tool-call JSON (54 inputs), plus
20 benign phrases on the same surfaces (60 inputs). Controls include
`contact technical support` and ordinary uses of brown, baker, miller, young,
cook, rose, may and will. No benchmark corpus or benchmark gold was used.

Each text was scored once with the pinned int8 bundle, one inference thread.
The production decoder replayed the captured pieces at each declared
threshold. Each label was measured separately against the full name interval.
Choose the threshold recovering the most name bytes with zero benign false
positives; break equal-recall ties in favor of the higher threshold.

| Label | Threshold | Recovered name bytes | False-positive bytes |
|---|---:|---:|---:|
| GIVEN_NAME | 0.5 | 185 | 0 |
| GIVEN_NAME | 0.7 | 149 | 0 |
| GIVEN_NAME | 0.9 | 128 | 0 |
| GIVEN_NAME | 0.95 | 112 | 0 |
| GIVEN_NAME | 0.99 | 97 | 0 |
| SURNAME | 0.5 | 188 | 0 |
| SURNAME | 0.7 | 183 | 0 |
| SURNAME | 0.9 | 118 | 0 |
| SURNAME | 0.95 | 99 | 0 |
| SURNAME | 0.99 | 36 | 0 |

These are calibration results from before name-part joining, not benchmark
gains. The other four op-B thresholds remain unchanged. An explicit
`[safety_net.nym]` allowlist replaces the default; adopters can exclude or raise
either personal-name threshold without changing the model bundle.

Reproduce using Rust 1.96.0, two build jobs and the already installed pinned
bundle (the test does not download models):

```sh
export CARGO_BUILD_JOBS=2
export RUSTC="$(rustup which --toolchain 1.96.0 rustc)"
export RUSTDOC="$(rustup which --toolchain 1.96.0 rustdoc)"
export GAZE_NYM_MODEL_DIR="$HOME/.local/share/gaze/models/nym-small-int8"
export GAZE_NYM_NAME_CAPTURE="$PWD/target/nym-name-calibration.json"
cargo test -p gaze-recognizers --features safety-net-nym,test-support \
  --test nym_safety_net offline_capture_person_name_calibration \
  -- --ignored --exact --nocapture
```

Inputs and captured model pieces are committed under
`crates/gaze-recognizers/tests/fixtures/nym_name_{calibration,pieces}.json`.
The capture records the model revision and bundle digest. Normal tests replay
the pieces without needing model files and check exact restore on all inputs.

## Limits

Only four of the 18 lowercase positive inputs produced any above-threshold
personal-name pieces, and none produced both personal-name labels. Some other
names were classified as usernames; others remained unrecognized. This
mapping does not solve every lowercase full-name leak.

Zero false positives on these controls cannot establish precision on arbitrary
prose. Neural decisions are deterministic for the pinned model and input but
less explainable than rules. Default adoption requires the separate mechanism
arm and both benchmark gain contracts, including benign counterweights,
refusals and exact restore. Thresholds must not be tuned on that test corpus.
