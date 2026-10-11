# Labelled personal national IDs with checksum errors

Generator v18 treats explicitly labelled Aadhaar and NIR values as personal data
when their checksum fails. A mistyped personal national ID still identifies a
person; checksum validity is diagnostic metadata, not permission to treat that
value as benign. This extends the personal-ID ownership ruling already applied
to labelled Steuer-ID, CPF, BSN and NHS twins and to IBAN/payment-card gold.

Only the following cells change, identically in each partition:

| Class | Prose cells | Log cells | JSON cells | New gold bytes |
| --- | ---: | ---: | ---: | ---: |
| Aadhaar | 10 | 10 | 10 | 360 |
| NIR | 10 | 10 | 10 | 450 |
| Total | 20 | 20 | 20 | 810 |

These 60 documents move from benign layer D to gold layer A with `invalid`
validity. Their document IDs change only the layer component. Text, inserted
values, UTF-8 offsets, order, group/template identities, seeds and every other
cell stay fixed. Each partition has 5,018 A, 3,314 D and 1,390 R documents.
The [cell-level diff](personal-id-checksum-v18-cell-diff.json) lists every changed
record by partition and class. Tests compare every v17/v18 record, assert the exact family/surface counts
and preserve v17's corpus and contract byte pins. V18 regenerates twice with
identical hashes. Deliberately reverting one Aadhaar prose cell to benign fails
the pinned corpus and cell-diff tests; restoring the source makes them pass.

The existing personal national-ID checksum twins outside this coverage slice
already carry gold. CNPJ company-ID checksum decoys and ETH-address checksum decoys
remain benign. Public `caseId`, account-nested order/ticket references and
`customer` / `batch_reference` coverage controls also remain benign. Inventory
and batch reference twins keep their original labels even when their numeric
shape resembles a personal national ID. No recognizer or gate-credit exception
changes. All values remain synthetic.

The stricter gold adds measured leaked bytes for every displayed release.
Any reduced D false positives arise from moving personal data out of the benign
population, and do not establish a runtime precision improvement. The current
[release table](README.md#measured-adjacency-layer-history) is generated from
fresh tag-bound v18 measurements. The frozen v17 corpus, contract, scorecards,
observations and ledger remain available for comparison. Layer C inputs and
published scores are unchanged.

The fresh v18 measurements below are compared with the frozen v17 ledger.
The canonical v1/v2/v4 replays agree on every byte metric, cell, refusal,
exact restore and manifest count. All five arms have zero refusals.

| Release / arm | Added A leak B | A FP change B | D FP change B | R leak / FP change B |
| --- | ---: | ---: | ---: | ---: |
| `v0.16.0` `policy-file` | +810 | +0 | +0 | 0 / 0 |
| `v0.15.1` `policy-file` | +810 | +0 | +0 | 0 / 0 |
| `v0.15.0` `policy-file` | +810 | +0 | +0 | 0 / 0 |
| `v0.14.0` `full-stack-kiji-resolve` | +810 | +250 | -248 | 0 / 0 |
| `v0.14.0` `pass2-ner` | +810 | +0 | +0 | 0 / 0 |

The committed [measurement ledger](agentic-adjacency-v18-history.json)
retains exact-restoration and valid-manifest counts, including historical
failures. Moving a document between A and D can move its restore failure
between those layers; it does not repair the release. The bound
[canonical replay evidence](personal-id-checksum-v18-contract-replay.json)
records clean tag revisions, binary hashes, observation hashes and
`cache_replay=false` for each measurement.

V0.14.0 Kiji has an additional producer-identity effect. Its
[benchmark producer](../../../crates/gaze-recognizers/examples/clean_for_bench.rs)
derives a fixed session hex prefix from the document ID. Moving D to A changes
that ID and thus the synthetic tokens that Kiji sees in subsequent passes.
Eight relabelled NIR documents change their name-class fallback spans; all
compared traces and restore fields outside the 60 relabelled cells are identical.
This is not random run-to-run noise. The
[controlled identity probe](personal-id-checksum-v18-kiji-identity-diff.json)
holds text and locales fixed and repeats each old/new ID twice: all 32 responses
reproduce their corresponding recorded trace, restore and fallback fields.
The [replay script](../../../scripts/bench/check_personal_id_checksum_identity.py)
binds the measured binary hash and uses the recorded NER threshold.

The 248 historical D false-positive bytes move to A, plus 2 bytes from the
changed token prefixes. Eleven historical D restore failures move to A, plus
two additional failures from this identity effect. The published v18 Kiji
counts therefore retain A/D/R restore failures of 81/4/37, versus 68/15/37 in
v17. These are measurement changes, not a runtime repair or precision gain.

| New document ID (old ID differs only by D/A) | Old → new session hex | FP byte delta | Exact restore, old → new |
| --- | --- | ---: | --- |
| `agentic-test-A-nir-000-coverage_prose_invalid` | `41c1a662` → `6fe8a595` | +2 | true → false |
| `agentic-test-A-nir-003-coverage_json_invalid` | `2b6dddc6` → `ab3e9efc` | -1 | false → false |
| `agentic-test-A-nir-004-coverage_prose_invalid` | `a2c4b016` → `10095212` | -2 | false → true |
| `agentic-test-A-nir-005-coverage_prose_invalid` | `ae4610ee` → `d5ea3993` | +2 | true → false |
| `agentic-test-A-nir-006-coverage_prose_invalid` | `04f115ff` → `d1577026` | -2 | false → true |
| `agentic-test-A-nir-007-coverage_prose_invalid` | `3c085173` → `497b04c7` | +2 | true → false |
| `agentic-test-A-nir-008-coverage_prose_invalid` | `2612ea09` → `792b8ccb` | -1 | false → false |
| `agentic-test-A-nir-009-coverage_prose_invalid` | `19c85e7c` → `a64b5ae0` | +2 | true → false |
