# Invisible separator verification

The historical release/equality candidate is `9e87f4df7bef7b23db1824c8224d2fe78dc37bb3`. The independently built base is exact main `3fb02d70e756102c871c51471cfd76d7ce6de4bc`. Both builds use default `gaze-cli` features. `release-build.json` binds their executable hashes. `provenance.json` pins every record and the measured production Git objects.

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

## Empty hard-boundary captures

The unchanged explicit-field normalization contract passes on exact main `3b3808c6`. A quoted value containing only removed characters exposed a branch regression: the hard-boundary scan matched its marker, then mapped that capture to an empty normalized span. The existing recovery guard correctly refused the invalid candidate.

The fix discards empty mapped hard-boundary spans, matching the transformed-view contract. The recovery guard stays unchanged. `field-geometry-main.json` records the main probe. `empty-boundary-regression.json` records the focused failure before the fix, the passing test after it, and the mutation that re-admits the empty candidate. The focused test covers 19 ignorable classes, invisible-only fields and real neighbouring values with strict restore.

## Final verification records

`gates.json` retains the measured source, command exit and log hash/tail for the accepted components: the both-parent normalizer proof at `c51eb461`, 722 validator cases plus all 41 mutations and the affected all-feature suite at `1af30df8`, and the complete post-rewrite check at `22857900`. The first two enclosing checks later failed; their successful components are identified individually. The final post-rewrite check passed in full, including all 13 mechanically selected `regex.rs` edit-site mutations, focused regex/preference/legacy tests, formatting and workspace all-feature/all-target Clippy with warnings denied.

`pr-ritual.json` records the full workspace all-feature suite (3,542 passing Rust tests) at `a27c78a6` and the 13 xtask gate contracts at their actual source. The final matrix completes across resource boundaries: its unchanged fail-fast source and pinned log prove the successful prefix; the interrupted all-feature workspace step and following default workspace suite both passed fresh at `c080d409`: 3,542 all-feature and 3,333 default-feature Rust tests, with build cleanup between them. Interrupted attempts retain their nonzero exits, and no unexecuted step receives credit. The full workspace run also repeats both 4,174-scalar sweeps and the existing normalize contracts from both parents. Later commits add verification tooling and evidence; separator production logic remains unchanged after `22857900`; the later main merge adds outcomes policy and refusal shapes, covered by the separate `merge-focus.json` receipt: formatting and 11 nonempty focused test commands on the actual integrated source.

The dependency advisory check remains **failed**, separately from those passing checks. `main-advisories.json` proves that exact main `3b3808c6` and the candidate both fail `cargo deny check advisories` for the same `RUSTSEC-2026-0414` advisory in `pulldown-cmark 0.13.4`. Version, checksum and dependencies match. Bans, licenses and sources pass. The dependency correction is tracked separately; this branch changes neither that package nor the deny policy.

## Process-cold CPU and peak RSS

`cold-build.json` binds default-release executable hashes for exact main `3b3808c6` and candidate build `01747b79`. `cold-current-main.json` contains seven alternating rounds per side/input, 28 successful CLI cleans in total. Every sample verifies the required complete values, rejects duplicate/unexpected entries and exactly restores by manifest substitution after the timed region.

| Side / input | CPU median [range], ms | Peak RSS median [range], MiB | Protected values |
| --- | --- | --- | --- |
| Main / plain | 158.043 [155.261–162.239] | 38.047 [37.922–39.812] | 2 |
| Candidate / plain | 158.494 [157.887–167.160] | 38.344 [37.984–38.469] | 2 |
| Main / ZWSP | 156.028 [155.087–162.450] | 38.359 [38.203–38.438] | 1 |
| Candidate / ZWSP | 203.792 [200.527–206.134] | 50.250 [50.047–50.312] | 2 |

Plain CPU shows no measurable difference within these ranges. ZWSP candidate work protects both card and email while main leaves the card raw, so its extra CPU/RSS is not an equal-work throughput comparison. The sample covers the complete fresh CLI process, including loader, rule construction, detection and JSON output. It does not isolate scan overhead; the OS page cache was not flushed. First-plain wall outliers preclude a wall-speed claim.

The user's browser was active. Before/after load averages were `[3.580, 3.767, 4.201]` and `[3.417, 3.725, 4.181]` on 18 cores, with 83% free memory at both endpoints. The capture ran in a reserved quiet window; its machine lease was released afterward. These descriptive samples do not provide the separate full-corpus scored-label v2/v1 certification still required before merge.

## Round 3: valid joined values

A joined value that validates on its own now wins over a shorter valid hard-boundary value. Previously, `+49 89 12345` plus a zero-width joiner plus `678` protected only the first part, leaving up to four digits raw. A joined value that fails its validator still yields to the hard-boundary value, so a glued neighbour digit cannot hide a valid identifier. The trade-off is over-redaction: in `Tel. 030 12345678` plus a zero-width space plus `9`, the `9` joins the token. That costs false-positive bytes, not a leak.

`round3.json` records the rerun at candidate `775a1147` against base `3fb02d70`. Both default-feature release builds are rebuilt and their hashes recorded. The 2,144 clean-equality comparisons still show zero differences (`clean-equality-round3.json`). All 2,068 card/code cells match the committed matrix observation for observation, with no coverage regressions. All 43 behavioural mutations are rejected, including restoring the old preference. `scripts/ci/probe_split_exposure.py` places one of five ignorables at every inner position of 22 phone, IPv4, card, IBAN, SSN and email values: the candidate leaves no value character raw in any of the 1,715 cases and restores each byte-exactly.
