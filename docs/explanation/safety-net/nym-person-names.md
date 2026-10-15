# Nym personal names

The pinned Nym model emits `GIVEN_NAME` and `SURNAME`. Both explicitly map to
`Name`; the other unsupported labels still fail policy loading. This changes
the former contract that excluded all personal-name labels. The purpose is to
add a reversible backstop for names the primary pipeline misses, including
some lowercase names, without downloading another model.

Adjacent name parts join over inline whitespace into one value. This includes
multiple given names and surnames. Line breaks, punctuation, unlabelled words
and intervening tokens prevent a join. The same record barrier applies before
same-label BIO `I-` continuation, so model continuation labels cannot swallow
line breaks (LF, CR, NEL, line separator or paragraph separator). The existing shared word rule still
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
gains. Replaying after joining recovers 189 SURNAME bytes at 0.5: the extra byte
is an internal separator. Model pieces are identical, all controls still have
zero false positives, and the selected thresholds are unchanged.
The other four op-B thresholds remain unchanged. An explicit
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

## Record-boundary verification

The Name record barrier runs before BIO continuation as well as before joining
adjacent parts. Decoder and public-pipeline regressions cover same-label B/I,
same-label B/B and mixed-label B/B across LF, CR, CRLF, NEL, line separator and
paragraph separator; clean output retains the separator and restore is exact.
A direct backend assertion also checks that a fully manifest-covered Name token
produces no suspect before the pipeline's independent ownership guard.

The [read-only audit](../../../scripts/bench/audit_name_record_boundaries.py)
reconstructs the corpus and verifies its hashes against each measured record.
The [pinned result](nym-name-record-boundary-audit.json) enumerates all eight
ordinary/own-arm v2/v1 records and both committed own-arm records: each has
17,780 documents. Candidate records have 4,335 Name spans, including 129 from
Nym; **zero Nym Name spans cross a record separator**. Each record also contains
the same 18 pre-existing primary NER Name spans across LF, explicitly listed in
the result. Those spans are unchanged by this Nym-only fix. Thus no measured
arm exercises the narrowed Nym continuation path; the reported gain is retained
without another inference run. This is an impact enumeration, not a new benchmark
measurement or a general claim that Name spans never cross line breaks.

Run with the local pinned dataset (no models or benchmark lease required):

```sh
uv run --project scripts/bench python scripts/bench/audit_name_record_boundaries.py \
  --record base=PATH_TO_BASE_OBSERVATIONS.jsonl.gz \
  --record candidate=PATH_TO_CANDIDATE_OBSERVATIONS.jsonl.gz --out audit.json
```

Repeat `--record LABEL=PATH` for every arm to enumerate. The output records each
observation file's SHA-256, the corpus hashes and the audit script's SHA-256,
without copying any document values.
