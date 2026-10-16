# Source completeness research reproduction

These are isolated research inputs and prototypes, not production rules or a
release benchmark. Findings are maintained in Solo; this directory contains
only reproducible methods, synthetic fixtures and policy deltas.

Use Rust 1.96.0 and Python 3.11+. Follow repository machine ownership rules.
The probe never samples clocks, latency or RSS. Build with two jobs; inference
uses one Nym thread. Verify installed pinned model bundles with `gaze setup`.

```sh
CARGO_BUILD_JOBS=2 cargo build -p gaze-cli -j 2
target/debug/gaze setup --non-interactive --force --policy-out target/r2/setup.toml
python3 scripts/research/r2/policies.py target/r2/setup.toml target/r2/policies
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$PWD/target" cargo build --locked \
  --manifest-path scripts/research/r2/probe/Cargo.toml -j 2
python3 scripts/research/r2/experiment.py run --split dev \
  --binary target/debug/r2-source-probe --policy target/r2/policies/full.toml \
  --out target/r2/full-dev.json
```

Repeat for `holdout` and the policy arms `rules`, `rules+ner`, `name-0.1`,
`name-0.9`. All source-completion ablations replay each captured response;
they do not rerun inference. Production exact restore and manifest alignment
are checked independently of prototype span rendering. A refusal remains
explicit and cannot earn recall credit.

`experiment.py generate` regenerates the frozen JSON inputs. Dev and holdout
use disjoint invented identities on predeclared transformation families;
this is a small synthetic holdout, not a natural-text or unseen-family test.
Public references are invented. Their empty gold reflects the declared task
intent; URL and organization false positives can reflect an intentional
default policy and must not be called recognizer defects without that caveat.
The held-out negative templates are intentionally matched to dev: the tiny
negative verifier therefore only demonstrates the value of verification on
known patterns and cannot establish generalization or justify shipping its
word list. No thresholds or templates may be changed after holdout exposure.

Gold counts exact UTF-8 source bytes, including separators internal to a
declared value. Coverage ignores predicted class (privacy coverage, not
classification accuracy). Incorrect merges count one emitted interval touching
two distinct gold occurrences. Reconstruction is exact concatenation of the
declared ordered fragments only, not an LLM or auxiliary-information attack.
Each document is fresh unless its explicit session matches another row; turns
are processed in file order. Split fields/chunks are modeled as sequential
texts, not asserted to reproduce a particular transport implementation.

The custom slice has no layer C/A/D/R membership or scored-label credit
metadata. Its bytes are reported as synthetic stress bytes, never mislabeled
as v1/v2 release scores. Full-corpus arms require the normal fresh-base and
candidate gate with both contracts, refusals, exact restore and manifests,
plus independent counterweights and historical remeasurement if adopted into
the published harness. Queue those runs while the machine lease is occupied.
