# Invisible separator verification

The measured candidate is `9e87f4df7bef7b23db1824c8224d2fe78dc37bb3`. The independently built base is exact main `3fb02d70e756102c871c51471cfd76d7ce6de4bc`. Both builds use default `gaze-cli` features. `release-build.json` binds their executable hashes. `provenance.json` pins every record and the measured production Git objects.

`clean-equality.json` records 2,144 comparisons with zero differences. Each binary runs all 1,072 committed inputs without policy and with its own setup policy. Both binaries exit zero in every comparison and exercise 767 detections. A one-byte output mutation fails comparison in both modes. Only token session prefixes are normalized in the compared clean fields.

`card-code-matrix.json.gz` contains all 2,068 observations. The fixtures cover PAN lengths 13 through 19, verification codes of three and four digits, 22 visible separator classes, one no-gap control, and 24 removed-separator classes. Every candidate cell protects at least the digits main protects. Every removed-separator cell also protects at least main's equivalent visible-space layout. All manifest substitutions round-trip.

U+2011 is the sole exception to exact canonical visible-control identity because its fold to ASCII hyphen is an intended change. It still permits no coverage loss. Two cells gain coverage. Every other visible or no-gap control matches main exactly. `card-code-reuse.json` binds the earlier measurement to the byte-identical rebuilt executables after every stored cell and input hash was revalidated.

`main-gap-probe.json` shows that the shared card-window scan gap was pre-existing on main. The two-ZWJ and two-ZWNJ cases lose complete union coverage on main and protect the complete union in the candidate. Visible-space and leading-ZWJ positive controls pass on both.

`compilation-work.json` records zero transformed regex compilations for plain input and 11 for the first ZWSP input, with 4,036,216 extra immutable compiled bytes. Its compilation timing is incidental instrumentation, not a CPU benchmark.

## Pre-existing visible-code gaps

These gaps remain within the frozen scope. A cell has a gap if main leaves at least one verification-code digit raw. All 22 visible separator classes have a gap in at least one fixture for every row. The fixtures use three synthetic Luhn-valid PANs per length, with one additional Amex fixture at length 15.

| PAN digits | Code digits | Main cells with a code gap / visible cells |
| --- | --- | --- |
| 13 | 3 | 46/66 |
| 13 | 4 | 66/66 |
| 14 | 3 | 66/66 |
| 14 | 4 | 66/66 |
| 15 | 3 | 68/88 |
| 15 | 4 | 88/88 |
| 16 | 3 | 66/66 |
| 16 | 4 | 66/66 |
| 17 | 3 | 66/66 |
| 17 | 4 | 66/66 |
| 18 | 3 | 66/66 |
| 18 | 4 | 66/66 |
| 19 | 3 | 66/66 |
| 19 | 4 | 66/66 |

The harnesses are `scripts/ci/card_code_matrix.py`, `scripts/ci/probe_card_window_gaps.py`, and `scripts/verify_clean_equality.py`. Input generators and hash-pinned equality inputs are committed. The shared normalizer's composition argument is in [Normalization compatibility](../invisible-normalization-proof.md).

This evidence covers compatibility and separator behavior. Full-corpus scored-label v2 and v1 benchmark certification remains required before merging the detection change.

## Phone validator fixture preconditions

`phone-fixtures.json` records direct `gaze-types` probes on pre-merge `9e87f4df` and main `3b3808c6`. Both reject the Ofcom mobile fixture under `E164Phone`, accept it under `PhoneNumber`, and accept the tested BNetzA and NANPA fictional fixtures under `E164Phone`. The validator matrix therefore uses the BNetzA fixture for its E164 variant. This corrects the test input without changing validation behavior.

Pre-merge, main and integrated `c09fbf43` all pin `phonenumber 0.3.9+9.0.21` with the same checksum and build metadata `9.0.21`. No separate phone-metadata crate appears in their lockfiles. The E164 helper source hashes also match. Reproduce the two runtime probes with `scripts/ci/probe_phone_fixtures.py` through the `invisible-phone-fixtures` Anvil check and pinned review worktrees.
