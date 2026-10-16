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

`--normalize-view` is a separate offline prototype: per-scalar NFKC plus
ZWSP deletion creates a detector view with a byte-to-source map. Predictions
project back onto whole original characters, and only those source intervals
are replaced. Production restore is tested on the view; original-source
restore uses the prototype renderer. These distinct proofs do not establish
production integration or session-state preservation. Per-scalar NFKC does
not compose adjacent NFD scalars or fold arbitrary homoglyphs. Do not mistake
the normalized output for an authorized replacement of the original text.

Native corpus and generated layer slices can be reproduced without opening
any external corpus:

```sh
python3 scripts/research/r2/repo_slices.py generate target/r2
python3 scripts/research/r2/experiment.py run --split holdout \
  --fixtures target/r2/repo-holdout.json --binary target/debug/r2-source-probe \
  --policy target/r2/policies/full.toml --out target/r2/repo-full-holdout.json
python3 scripts/research/r2/repo_slices.py score target/r2/repo-holdout.json \
  --observations target/r2/repo-full-holdout.observations.json
python3 -m unittest discover -s scripts/research/r2 -v
```

The native coverage-loop sample is separate from layer C and has no official
release v1/v2 semantics. Generated A/D/R samples use disjoint dev/test
partitions, eight deterministic family-balanced inputs per layer, restricted
to labels ruled on by v2. Their v1/v2 counts are *sample* byte views, not full
release gate results; headline all-gold and validity twins still need the
complete normal gate before adoption. Sample selection is frozen in code.

For a small stability check, `repeatability.py --binary ... --policy ...
--out target/r2/repeatability.json` runs five alternating raw/view trials
over six predeclared diagnostic categories from each split in one loaded
production pipeline. Each side uses isolated sessions. It reports work/error
counts, byte ranges and coverage variation across random session identities,
without measuring time. This checks repeatability on that diagnostic slice;
it is not five independent populations or a new model-family comparison.
Ordinary stress cells have one inference trial; offline source-expansion
replays are not additional model trials.

`experiment.py replay --split dev --out target/r2/full-dev.json` re-scores
saved observations without inference. Use `--fixtures` for repository slices.
Raw gold-source misses include internal spaces and punctuation. Supplemental
alphanumeric/combining-mark byte counts distinguish leaked private content
from layout-only source incompleteness; they do not replace the source score.

Design comparison and handoff follow the Exhaust the Design Space, Explain
the Number, Benchmark Checklist and Prototype principles. These instruments
are throwaway research, with no production runtime or policy defaults changed.
No performance recommendation may be derived from their debug executables.
