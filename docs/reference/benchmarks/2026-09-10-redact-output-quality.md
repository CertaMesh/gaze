# Private Redact actual-output quality, 10 September 2026

Neither candidate passed the frozen development quality gates. Adding the private patched-CoreML derivative to the existing NER reduced surviving PII bytes by **34.98% on 250 common reversible rows**, but increased false-positive bytes by **30.10%** and introduced **six refusals**. Replacing NER increased leakage. No candidate was selected and no confirmation inference ran.

This measures Gaze `pipeline_text/clean_for_bench` output, using replay-verified source-byte provenance and exact restore checks. The candidate is a **private patched-CoreML derivative, not stock Redact**. This is controlled development evidence, not production promotion or a universal zero-leak claim. Known Dataiku development contamination means the data is not an independent holdout.

## Paired result

False-positive (FP) bytes are UTF-8 source bytes outside the benchmark gold PII annotations that Gaze replaced. They are treated as non-PII by those annotations.

All arms planned the same 256 documents, 11,316 gold UTF-8 bytes and 1,275 gold spans. The control completed and restored all 256. Each candidate completed and restored 250, refused the same six, and left 85 gold bytes unavailable for measurement. Refusals receive no zero-leak credit. There were zero unmeasured outcomes, restore failures, or irreversible redactions in the completed final run.

The comparison below uses the **same 250 completed, reversible documents and 11,231 gold bytes** in each arm.

| Arm | Surviving PII bytes | Gold bytes protected | FP bytes | Spans not fully protected | Exact restore |
|---|---:|---:|---:|---:|---:|
| Existing `pass2-ner` control | 2,447 | 78.21% | 2,608 | 370 | 250/250 |
| `pass2-ner-redact`, augmentation | 1,591 | 85.83% | 3,393 | 244 | 250/250 |
| `rule-floor-redact`, replacement | 5,580 | 50.32% | 1,487 | 746 | 250/250 |

Augmentation protects 856 additional gold bytes, reducing the leak rate from 21.7879% to 14.1661% on this common population. It adds 785 FP bytes. Replacement leaves 3,133 additional gold bytes exposed, despite lower total FP bytes. Both worsen 35 common negative documents and add six refusals. Augmentation fails `false_positive_or_negative_regression` and `additional_refusal_or_error`; replacement also fails byte-improvement and full-span gates. **Augmentation is the better leakage candidate, but neither is accepted.**

For the complete 256-row control, surviving PII is 2,463/11,316 bytes, FP is 2,648, exact restore is 256/256, and 372 gold spans are not fully protected. These integers match the frozen earlier baseline; the new evidence additionally verifies actual output replay. Do not compare the candidate's 1,591 bytes directly with 2,463 without accounting for the six unavailable rows.

`full_span_escapes` counts gold spans lacking full protection, including partial escapes. Fully surviving spans on the common population are control 361, augmentation 232, replacement 737. The JSON preserves both measures separately.

## Existing-output diagnosis

Both candidates have six `fail_closed` outcomes with reason `recognizer_detect` and stage `clean`: three German and three English rows, comprising four A4 Unicode/mixed-language negatives and two Dataiku positives. Their unavailable gold total is 85 bytes. The underlying private bridge error codes were not retained. Alignment, numeric failure, timeout and other causes are **unknown**, not inferred from the category or source capabilities.

FP attribution by predicted class and vendor disposition is also **not retained**. Saved per-gold-label recall is not a substitute for FP class attribution. Vendor disposition counters are diagnostic rather than exhaustive, and no DEV disposition distribution can be reconstructed from these artifacts. Further diagnosis requires separate instrumentation; this report performed no new inference.

Available A4 category FP byte counts, control to augmentation, are: invalid identifiers 51 to 519; code/log syntax 0 to 169; Unicode/mixed-language 0 to 63; commerce identifiers 0 to 51. Documentation/network stays 317, public entities 260, generic roles 0, and temporal/numeric 0. Unicode/mixed-language has 12 completed of 16 planned rows in each candidate; all other negative categories complete. This is 628 to 1,379 A4 FP bytes, an increase of 751. Common positive-row FP rises from 1,980 to 2,014, accounting for the remaining 34-byte increase. Replacement has A4 FP 1,245; its public-entity FP is 126 and the other category totals match augmentation. Full category/language/count distributions are in the JSON.

## Frozen inputs and execution

The exact ordered dev256 IDs were copied from the baseline before any candidate output. Digest: `3993b14aeded90b056854d29172de148424b6e7b0eb967395e6b78de2fbbce36`. The complete remaining 2,654 IDs were frozen at the same time, digest `23c460833925cf7471b399b02f5551ded4f9da4a687cf42baf395341e7291350`. The JSON retains both arrays. Sorted proof populations have digest `1ccaa27930d14088eb538ab6d880e71af9a104a37e87081f46c1f5f39cacd2af`; this is a different ordering, not a different population. Every final arm shares source/gold digest `2ac74d55ca1612f3247929a7a5e61e654cabc75d25fa96330b0690da13bc5379`.

Before seeing the dev result, root declared selection: require every comparator gate, then prefer fewer surviving bytes, fewer FP bytes, and the replacement arm on an exact tie. Neither qualified, so the complement remains unrun. No tuning, gold-span changes, threshold changes, or confirmation selection after seeing results occurred.

Measured source is signed commit `6220d3d819d8901830ecd775171c8f88c3277729`, integrated from baseline `cdfd997360d759f68b3df0ff17f1be041b4f68f1`. All supplied source/private patch bytes were retrieved and independently SHA-256 matched; both source diffs matched their announced original commits before cherry-pick. Private source/weights remain outside Gaze.

Normal bootstrap, canonical producer with `safety-net-kiji,redact-live`, and nested validator were built at the exact integration checkout using Rust 1.96, explicit RUSTC/RUSTDOC/PATH, two jobs, offline locked dependencies and explicit cached ORT library path. Dependency artifacts were cloned into the isolated checkout to reuse warm caches. Final producer SHA-256: `21c9d188b33b2a27112d7c510f7bb82db4d9403464d2f05a4ee93846de14602c`; validator: `1e52d0575c90fba4274a47b72415677c4fb74edb483a46bfbbd74276f65039e4`.

The existing private bridge was hash-verified and reused, without a Swift rebuild. Original and derivative full bundle file sets, sizes and hashes were verified, including the sole `-inf` to `-65504` attention-mask constant replacement. Original bundle manifest: `6c49b610cdba9eb23ff4c873103780f1f904606b6ff6f86140c09f21aa64f279`; derivative: `f6d3c6ff73b7070ceba87e1afbbf97c5552b79f7e1fd1e048396ac6d409b2d7d`. Bridge: `c37d3c14c1345ed584b25f7c1b84107b01c8b6a1e0638d2129a064501f4913cd`. Full Davlan, Kiji, original Dataiku and ORT hashes were verified. All hashes and command/exit/timing receipts are in the JSON.

Redact threshold 0.6 and ORG enabled remained frozen; existing NER threshold is 0.3. All three arms retain the same deterministic floor and no final safety net. Required narrow metadata telemetry remains enabled without text, row IDs or context. Raw inputs, clean text, manifests and IPC are absent from the report. CLI, MCP, proxy, daemon and streaming are not measured.

Final canonical dev execution took 123.932 seconds. Arm wall times were 23.977 seconds for control, 60.499 for augmentation and 34.534 for replacement. These include startup and are not isolated model benchmarks. Exit 4 is complete failed release-readiness evidence; each paired comparator exited 1 for valid negative quality. Comparator 0 means pass, 1 means failed quality gates, and 2 means invalid evidence. The machine lease and all jobs were released before packaging, recorded in scratchpad 6185 revision 102.

## Integration repairs and retained failures

The first candidate run aborted on invalid provenance metadata. Its control has 256 reversible outcomes and both candidate cells retain all 256 planned rows as unmeasured; an unstarted cell is not credited as zero leakage. Initial synthetic smokes and this failed corpus attempt remain represented in the JSON and local execution receipts.

Two demonstrated metadata repairs were required: commit `42ba5b7` lowercases already-validated vendor labels in Redact source IDs; commit `6220d3d` splits resolver-composed `+` names into atomic trace source IDs. Scorer ID grammar and proof validation were not weakened. Neither repair changes detector spans, gold, threshold, policy, or resolver decisions. Source-ID consumers and source-dependent custom policies may observe the intentional casing/trace metadata changes; universal custom-policy compatibility is not claimed.

Four Python modules passed 133 tests with zero skips. Three Redact batch tests and three protection-trace tests passed, including merged-source attribution and exact restore. Both actual integrated candidate synthetic smokes then completed reversibly with 21/21 protected gold bytes, zero FP, and zero surviving bytes before the final corpus run. Test-compilation and local diagnostic/reporting helper mistakes are retained in execution notes. Independent review 9408 was accepted by root with no MUST/SHOULD findings.

## Five-field review

Verdict: implement the two narrow metadata repairs; retain the experiment as negative evidence.

Opportunity: atomic metadata source identifiers at the detector/trace boundary, using the existing closed per-row outcome and paired-ID structures.

Why: removes invalid provenance without weakening validation; unavailable rows remain distinct from completed zero-leak rows.

Scope: three source/test files for the demonstrated integration bugs and this sanitized report. No speculative cleanup, public push, PR, merge, deployment, replacement promotion or confirmation inference. Larger quality changes and additional diagnostics belong to root's next bounded augmentation task.

Validation: signed source custody, exact builds, 133 Python tests, six focused Rust tests, both actual candidate smokes, 768 final planned outcomes, matching source/gold/population digests, and two valid negative comparisons. Reliability and availability gates fail for the candidates; completed restoration passes. The private derivative and intentional source metadata changes constrain adopter ergonomics and source-ID compatibility.

Machine-readable counts, all planned-row outcomes, frozen IDs, comparisons, failure attempts and provenance: [2026-09-10-redact-output-quality.json](2026-09-10-redact-output-quality.json).
