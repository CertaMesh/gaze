# Repaired private Redact, frozen DEV256 actual-output quality

Neither repaired candidate passes the unchanged quality gates. Both improve measured
leakage against the existing NER control on common reversible rows, but increase
false-positive bytes and still refuse two positive documents. The semantic candidate
also exposes **143 more gold bytes** than unrestricted repaired Redact while removing
**190 false-positive bytes**. No candidate is promoted and no confirmation ran.

This is one development experiment on the existing public Dataiku and reserved A4
negative fixtures. It measures actual Gaze `pipeline_text/clean_for_bench` output,
replay-verified original-byte protection and exact restore. The model is the private
patched-CoreML derivative with the accepted scoped Unicode SDK repair, not stock
Redact. Existing development contamination prevents an independent holdout claim.

## Availability and paired denominator

Every arm planned the same ordered **256 documents, 11,316 gold UTF-8 bytes and
1,275 gold spans**. The control completed and exactly restored all 256. Both repaired
arms completed and exactly restored 254 and refused the same two positive documents.
There were zero unmeasured outcomes, restore failures or deletion actions in the
final planned-row evidence. The two refusals leave **85 gold bytes and 11 gold spans
unavailable**, with no zero-leak or restoration credit.

The table uses the identical **254 reversible documents, 11,231 gold bytes and
1,264 gold spans** in each arm, comprising 164 positive and 90 negative documents.

| Arm | Surviving PII bytes | FP bytes | Negative FP bytes | Full-span escapes |
|---|---:|---:|---:|---:|
| Integrated `pass2-ner` control | 2,447 | 2,608 | 628 | 370 |
| Unrestricted `pass2-ner-redact`, repaired bridge | 1,591 | 3,429 | 1,415 | 244 |
| `pass2-ner-redact-semantic-candidate`, same bridge | 1,734 | 3,239 | 1,225 | 251 |

All three table cells have 254 exact restores. Full-span escapes count gold spans
lacking complete protection, including partial escapes. Fully surviving gold spans
are respectively 361, 232 and 241. False-positive bytes are replaced UTF-8 source
bytes outside the benchmark gold annotations; the annotations treat them as non-PII.

For all 256 control rows, the baseline reproduces exactly: **2,463 surviving bytes,
2,648 FP bytes, 372 full-span escapes, 363 fully surviving spans and 256 exact
restores**. Do not compare a candidate's 254-row leaked-byte total directly with the
256-row control total. The unavailable two control rows account for 16 surviving
bytes and 40 FP bytes.

Both strict paired comparisons fail `false_positive_or_negative_regression` and
`additional_refusal_or_error`. On common reversible rows, unrestricted Redact reduces
surviving bytes by 856 (34.98%) but adds 821 FP bytes; the semantic arm reduces
surviving bytes by 713 (29.14%) but adds 631 FP bytes. Negative FP bytes rise by 787
and 597 respectively. These partial leakage gains do not satisfy the fixed joint
acceptance rule.

## What the repair and semantic arm changed in this measurement

The Unicode repair recovers four of the prior six unavailable documents. All four
are negative fixtures with **zero gold bytes**. One has zero FP bytes; three have
12 FP bytes each. Thus the repair restores availability on four rows and exposes
36 additional measurable FP bytes. It restores **zero of the previously unavailable
85 gold bytes**. The old 250 completed unrestricted rows retain their prior aggregate
1,591 surviving bytes and 3,393 FP bytes; the repaired 254-row total is 1,591 and 3,429.

The two remaining unavailable documents are `dataiku-test-3525` (49 gold bytes,
5 spans) and `dataiku-test-3477` (36 gold bytes, 6 spans). Both arms report
`recognizer_detect` at stage `clean`. The semantic sidecar records `detector_error`
for both. **Current underlying bridge error codes and exact throw sites are unknown.**
The earlier diagnosis of alignment failures in the old bridge does not establish
the current private error code. No new model run was used to infer it.

Against the unrestricted repaired arm, semantic admission saves 190 FP bytes,
all in the negative invalid-identifier category (519 to 329), but exposes 143
additional gold bytes, increases full-span escapes by seven, and increases fully
surviving spans by nine. Availability and restore outcomes are identical. Raw-byte
label-level causality is not established by the normalized audit alone; the recorded
143/190 trade-off comes from actual output on identical documents.

All 90 negative documents are available in each repaired arm. Other negative FP
category totals are unchanged between the repaired arms: code/log syntax 169,
commerce identifiers 51, documentation/network 317, public entities 260,
Unicode/mixed language 99, generic roles 0, and temporal/numeric 0. Positive FP is
2,014 in both repaired arms, against 1,980 on the common control population.

## Audit evidence and limits

The create-new admission sidecar contains all 256 request begins, 256 batch begins,
254 complete batches, 254 successful request terminals, two detector errors and
two refusal terminals. The report preserves **every request in full planned order**,
including failed requests, with all retained/excluded labels, dispositions and
normalized span endpoints. Coordinates remain explicitly `detector_input_utf8`.
No raw corpus text, canonical values or wire replies are retained.

There are 21 SemanticInvalid spans totaling 337 normalized detector-input bytes:
17 CREDIT_CARD spans and four PHONE spans. CREDIT_CARD also has two Accept spans;
PHONE has nine Accept and 21 NotApplicable spans. Other labels remain
NotApplicable. Bare-language plus Global PHONE context deliberately remains
NotApplicable; the experiment does not invent a country from language.

These 337 normalized bytes are **diagnostic exclusions, not raw PII bytes saved**,
and are not substituted for the observer or scorer. Refusal records have no
fabricated exclusion counts. Unknown error details remain unknown. Required narrow
bridge telemetry is unchanged and receives no added document text or context.

## Frozen source, binaries and execution

The own isolated checkout started at exact signed base
`69f5ef9ef8d6d0f62025ae27c50eacb5339eb968`. The semantic artifact's actual 59,450
bytes were fetched and independently SHA-256 verified as
`188cb0acc5a15eb5c131c0d37125a6ac5c220b828c246b79d4fefb135566492c`, then compared
byte-for-byte with the exact local `--binary --abbrev=8` diff. The two signed+DCO
semantic commits were integrated unchanged through final
`4b6ca0673344dbab62d8f0b1ed10fc4859745aa4`.

Measured source is signed+DCO
`1cd67a27e1eab96bfcc84145a89fbdb1bbe99afa`. Subsequent report packaging does not
change runtime source. Reviewer 7389's semantic review (9416) and reviewer 7392's
runner/deadline/evidence review (9417) were accepted by root before inference.

The ordered ID digest is
`3993b14aeded90b056854d29172de148424b6e7b0eb967395e6b78de2fbbce36`; the copied
`frozen-dev.json` file SHA is separately
`5a37e640fa43c3808a8d5aae2965a5c2e72eb815a0b7363fa82f89e3ff943427`. The unchanged
selected source/gold digest is
`2ac74d55ca1612f3247929a7a5e61e654cabc75d25fa96330b0690da13bc5379`.

The DEV-only loader reuses diagnosis 7381's Arrow physical-row selection before
converting selected text to Python. It checks canonical bounds, gold value equality,
locale/source fields, exact ID order and the source/gold digest before inference.
Arrow reads parquet storage; negative JSONL lines are transiently decoded to select
IDs and only selected documents are retained. This is not a claim of physical
non-read. No positive confirmation row was converted to Python text. Nonselected negative
JSONL records were discarded after ID selection; none were scored or sent to a
producer/validator. No confirmation ID file was opened.

Both producer and nested validator were rebuilt from the own checkout with Rust
1.96.0, explicit compiler paths, jobs=2, locked/offline dependencies and the existing
cached ORT archive. Workspace bootstrap was compile-only. APFS-cloned warm artifacts
were never accepted merely because files existed. Each successful build receipt
binds the intended command, selected environment, pre/post clean source HEAD and
the resulting executable hash. Freeze validates those receipts and hashes all
relied-on build/test receipts.

Final producer SHA:
`ef147e2f4ad2284a0d8d0dc265c98e7184805b5b3cb70f2519328a45b7ee850d`.
Final validator SHA:
`ec0fe583d924545ed66258aeaaab7ddc5be744b80fcbcf085906390ffd8b9a91`.
The new immutable bridge SHA is
`273968f5638cac53c9528b61bc974230811acc700b3bc5742765b60873c6a403`, from accepted
private SDK final `cf62d27821a1758bb1483daed81b80346ae9cfc9`. The unchanged private
model's complete manifest SHA is
`f6d3c6ff73b7070ceba87e1afbbf97c5552b79f7e1fd1e048396ac6d409b2d7d`.
The freeze verifies its full file set, sizes and hashes, plus pinned Davlan/Kiji
bundles. No SDK or model was rebuilt or modified by this worker.

The pre-inference freeze SHA is
`8ed85ee0a0d0b90165aa07cd353843bc506f6af4bedbe7a4fc7258f0e9ba3967`.
`GAZE_NER_LOCALE` is explicitly unset. NER threshold 0.3, Redact threshold 0.6,
ORG enabled, unchanged floor/NER, no final safety net, one repetition and zero
warmups are fixed. Both repaired arms use the same bridge and model. The driver
has no replacement or confirmation path and does not use `--skip-build`.

All 66 focused Python checks passed with zero skips: 37 runner, 24 actual-output
observer, three audit/smoke-binding and two supervision tests. The two genuine
new-bridge synthetic smokes each prove the expected Redact raw interval 12..33,
21/21 gold bytes protected, zero FP bytes, zero surviving bytes and exact restore.
Their exact arm set and one-row evidence bind to the freeze digest and a successful
same-source smoke-stage receipt before DEV. Smoke took 22.378 seconds.

All final commands used the tracked `scripts/bench/redact_repaired_stage.py`.
The earlier local `target/quality-7390/stage.py` was rejected and archived; its
invocations are explicitly superseded. The tracked supervisor pins the original
absolute **11:14:11 UTC** deadline with a monotonic remaining-time cap and five-second
cleanup reserve. It tracks proven descendant ancestry and process birth identities,
including the bridge's separate process group, then uses bounded TERM/KILL cleanup
and reaps its direct child. Synthetic timeout and SIGINT checks both prove that a
detached TERM-ignoring child is gone and owned remaining count is zero. Start markers
and final sanitized receipts use create-new paths.

The single DEV stage took **151.754 seconds**, including validator measurement,
three arms, audit join and both unchanged paired comparisons. Per-arm wall times
were control 23.829, unrestricted 69.575 and semantic 54.807 seconds, including
startup. These are not isolated model benchmarks. Exit 1 denotes valid negative
paired quality; it is not incomplete collection. Every final stage left zero owned
processes. The machine lease was released in scratchpad 6185 revision 112, before
the deadline and before report packaging. No further inference ran.

## Retained failed preparation and review

Preliminary successful builds and the initial two audit tests are retained as
superseded receipts. The tests exposed an unclosed audit input warning, repaired
with a context manager. Reviewer 7392 then required an executable aggregate deadline,
source/build custody and exact smoke binding. The rejected local wrapper and
preliminary receipts remain hashed in the JSON. The final tracked implementation
and all seven final-source build/test receipts supersede them. No preliminary
corpus inference, discarded DEV attempt or model rerun occurred.

Verdict: implement the narrow benchmark harness guards and retain this negative
experiment without promotion.

Opportunity: explicit arm membership, frozen ordered request evidence and one
supervised stage receipt contract.

Why: removes suffix-based arm misclassification, refusal-shifted audit attribution,
unbound copied binaries, stale smoke acceptance and missing deadline cleanup. It
does not duplicate or relax scoring or admission policy.

Scope: six benchmark source/test files and this sanitized report; accepted semantic
source was integrated unchanged. Public defaults, reference arms, core scorer,
gold, thresholds and admission helpers are unchanged. Larger policy or Unicode
repairs remain separate root decisions.

Validation: verified artifact bytes/signatures/DCO, 66 focused tests, seven bound
build/test receipts, two genuine restore smokes, all 768 planned outcomes, exact
source/gold digest, both strict comparisons and all 256 normalized audit joins.

The five axes remain explicit: **reliability** fails the candidate FP/refusal gates;
**reversibility** passes on every completed row without deletion wins;
**agentic fit** retains ordered per-request failure accounting and locale context;
**trust** uses frozen custody and keeps unknown errors unknown; **adopter ergonomics**
leaves defaults/reference behavior unchanged and requires deliberate candidate audit
activation. Correctness gates are not traded for the partial leakage reduction.

Root owns result acceptance and any later work. There is no quality promotion,
confirmation, public push, PR, merge, private/customer corpus, vendor contact or
private SDK/model content in this artifact.

Full count-only planned-row proofs, comparisons, normalized admission records,
frozen configuration, stage receipts and evidence digests:
[machine-readable report](2026-09-10-redact-repaired-output-quality.json).
