# Evidence protocol v1

This is the normative contract for the optional, offline T1 evidence path.
It does not change schema v4, runtime detectors, policies, models or baselines.
Correctness, reversibility and auditability take precedence over performance.

## Claim ceiling

Every receipt declares `synthetic_harness_capability_only`. T1 proves three
components separately: recursive aggregate receipt validation, private grouped
paired arithmetic on synthetic records, and observations on one real rmcp
transport through `PiiEnvelope::dispatch`.

T1 makes **no claim of end-to-end paired evaluation of route observations**.
The Rust route exports aggregates; the Python evaluator consumes independently
authored in-memory records. There is no route-to-evaluator per-document bridge,
persisted or salted. Neither component proves detection completeness, corpus
fitness, generalization, promotion readiness or the historical IBAN discrepancy.
Other routes remain `NOT_IMPLEMENTED`.

## Boundary and safe failures

Zone P contains args, responses, restored strings, gold coordinates, private
membership/order, pairing keys, per-document statistics, snapshots, manifests,
logger entries and suspects. They remain in memory. Zone R contains only the
closed aggregate receipt. Zone A may read Zone R only. No value hashes, raw
strings, document IDs, arbitrary metadata, clocks, paths, hostnames or session
identifiers may cross the boundary, including through nested containers,
exceptions, assertion operands or Debug formatting. Synthetic fixture membership
constants are the sole persisted membership exception.

Hazards that must never be serialized or printed:

* `crates/gaze/src/session.rs`: `RestoreEvent.raw_sha256` is a value hash;
  `RestoreEvent` derives Debug. `RestoredTextWithProvenance` deliberately does
  not expose Debug, Display or serialization.
* `crates/gaze-mcp-core/src/manifest.rs`: all free-form `FailureReason` fields.
  `ToolError.class` is filled by the dispatcher's closed `ToolError::class()`,
  but an adopter can construct the public non-exhaustive failure variant with
  arbitrary strings. Neither failure field is an export surface.
* `crates/gaze-mcp-core/src/tool.rs`: tool-error strings and boxed errors;
  only the mapped class code may leave.
* `crates/gaze-mcp-core/src/dispatch.rs`: `UnknownTool(String)` and
  `Redaction(String)` can echo private inputs.
* Observer `RedactionEntry` timestamps (`created_at`), `session_id`, source IDs,
  artifact/tokenizer SHA fields; manifests and suspect spans stay private.
* `scripts/bench/gaze_bench_score.py`: `identified_document_population` persists
  IDs and is not reused. Its child stderr-to-file runner is not reused either.
  A future subprocess adapter must drain bounded memory pipes before any private
  run. Redacting a file afterwards is insufficient.

Both emitters and consumers recursively validate every path, container type
(including empty containers), leaf type and vocabulary. Boolean is not integer.
Refusals contain a closed code only, never an offending key/path/value. Test
assertions compare private values as booleans with static messages. Canary
probes cover success, nested refusals, errors, assertions, exceptions, oversize,
truncation and timeout; an injected private-output mutation must fail them.

## Receipt schema and stamps

The hand-authored `mcp_route_v1.golden.json` contains `receipt` and
`vocabularies`. Python and Rust mirror every vocabulary and `RECEIPT_PATHS`
by set equality. Fixtures are never automatically regenerated. A deliberate
contract change updates the fixture and both mirrors together.

Emitted keys (candidate adds the asymmetric table):

```
protocol_id: "gaze-evidence"; protocol_version: 1
arm_id: ARM_IDS; route_id: ROUTE_IDS; cell_id: CELL_IDS
route_status: complete ROUTE_IDS -> ROUTE_STATUSES
policy_identity: POLICY_IDENTITIES
population_handle: lowercase hex, 32..64 characters
class_commitment_table: {id: TABLE_IDS, version: positive integer}
claim_scope: CLAIM_SCOPES
planned_case_count: nonnegative integer
outcomes: complete OUTCOME_STATES -> nonnegative integer
asymmetric_outcome_table: complete OUTCOME_STATES x OUTCOME_STATES -> integer
counts: subset METRIC_IDS -> nonnegative integer
derivations: complete METRIC_IDS -> DERIVATIONS
not_measured: {metrics: unique METRIC_IDS[], blocked_gates: unique GATE_IDS[]}
gate_results: complete GATE_IDS -> GATE_RESULTS
error_codes: subset CONTROL_REFUSAL_CODES -> nonnegative integer
analysis_declaration: null or declared object
intervals: subset counted METRIC_IDS -> "NOT_EVALUABLE" or interval
```

An interval has exactly finite numeric `point`, `low`, `high` (low <= high),
`method_id`, boolean `conditional`, and `basis` (`paired_completed` or
`full_cell`). No unknown field is accepted at any depth.

Counting grades are `route_native`, `observer_native`, `egress_reconstructed`.
Non-counting grades are `invariant_enforced_not_counted`,
`not_applicable_by_construction`, `not_measured`. Counts are exactly the
counting-grade subset of the complete derivation inventory. Every metric at
`not_measured` is listed, with no numeric zero. Every BLOCKED gate is listed
exactly in `not_measured.blocked_gates`. All gate IDs must be present.

Stamps are forbidden in emitted receipts. Structural validation separately
calls the unchanged `gaze_bench_score.git_metadata(repo_root)` itself to bind
**validator** code identity. It does not authenticate the producer. The stamp
`build_attestation` carries `source_revision` (40 lowercase hex), `dirty`,
`source` (`live_repository` or `test_seam`), `declared_feature_graph_id` and
`declared_toolchain_id`. The last two are unverified closed declarations.
Without a repository root, only the explicitly fixture-only `attestation_probe`
seam is allowed. A seam yields NOT_EVALUABLE, never PASS. When a live root is
provided, live dirty state wins; a probe cannot override it.

Only the private evaluator checks its actual inventory and order against
custody and adds `local_membership_order_verified` and
`local_membership_order_proof_method = private_membership_order_v1`.
External aggregate validation stamps no membership fact. Producer membership
is NOT_MEASURED and `producer_membership_order_proof` remains BLOCKED.

## Outcomes and aggregation

Ordered states: `NOT_STARTED`, `COMPLETED`, `FAILED_CLOSED_NO_EGRESS`,
`ERROR_PROTOCOL`, `UNKNOWN_EGRESS`. Planned absent cases are NOT_STARTED;
completed requires actual decoded client response. No-payload requires a
received error frame with exactly one text content block in the committed
control vocabulary and no other data-bearing content. Neither client Ok nor
Err alone classifies dispatch. Wire codes collapse causes and never establish
an underlying Rust error variant.

`planned_case_count == sum(outcomes)`; attempted is derived by subtracting
NOT_STARTED. Candidate receipts carry the complete base-by-candidate table;
its candidate margin equals outcomes and its total equals planned count.
The evaluator independently reconciles both margins against its two arms.
Base receipts never carry the table.

Unknown egress never earns protection. Observed fragments remain lower bounds
in **evaluator-only** fixtures. The rmcp client exposes no partial response;
T1 adds no wire tap. Thus route `unknown_egress_lower_bound_cases` is
`not_measured` with no count. A timeout proves only UNKNOWN_EGRESS/no credit.
Any UNKNOWN_EGRESS, even without an observed fragment, forbids exact leak-family
intervals. A full-cell interval requires no unknown or not-started outcomes
and `conditional == false`. Rejection, missing records and errors are never
zero-leak substitutions. Observations on failed/skipped leaves are not counted
without actual evidence.

`overall`: any FAIL wins; otherwise any BLOCKED or NOT_EVALUABLE yields
NOT_EVALUABLE; only complete all-PASS coverage yields PASS.

## Private paired estimator

Frozen `PlannedInventory` holds ordered `(key, group_id, stratum, weight)`.
No duplicate keys; groups have one stratum and one positive finite weight.
Records cannot alter this metadata. Unknown/duplicate additions are refused.
Finalization fills absent cases in both arms. The paired set is the intersection
of COMPLETED cases, never the union. Grouping is by original document/session
containing all transforms, turns or chunks. No cross-population pooling.

The analysis declaration has exactly confidence_level (finite, 0 < c < 1),
resample_count (positive integer), seed (nonnegative integer), strata (unique
nonempty closed IDs covering the inventory), weighting (`inventory_group`),
multiplicity_treatment (`synthetic_none`), coverage_target and acceptance_limit
(finite numbers in [0,1]). These are synthetic conventions, not production
thresholds. Missing/malformed declarations make every interval NOT_EVALUABLE.
Deterministic class gates remain independent of statistical power.

For a metric, each paired group's delta is the mean of candidate-minus-base
per-record values in that group. A stratum mean weights group deltas by the
inventory group weights. Combine stratum means using each stratum's original
sum of group weights. Bootstrap draws the original number of paired groups
with replacement independently within each stratum, preserving all records
of each drawn group and the original stratum mass. Seed plus draw index seeds
an independent deterministic draw. Quantile at p uses sorted samples at
`max(0, ceil(p*n)-1)`, capped at n-1. Point is the original weighted estimator.
The interval is conditional if either arm has any noncompleted remainder and
is accompanied by the full asymmetric table. Tiny synthetic exhaustive draws
provide an independent oracle; no production confidence or sample size is set.

## Route measurement and occurrence attribution

`crates/gaze-mcp-rmcp/src/frontend.rs` dispatches into
`crates/gaze-mcp-core/src/dispatch.rs`: carrier preflight, strict argument
protection in a transaction, begin, argument commit, sealed ToolCtx invoke,
response preflight, strict response protection in a fresh transaction, response
commit, finish, egress. A failed finish retains committed mappings but returns
no response. A losing response commit retains no losing mappings. These are
separate tests. Real `tokio::io::duplex` + rmcp client/server exercise this path.

The rule-floor assembly is `CorePipelineConfig::new().build()`.
The controlled assembly loads one embedded Email regex via Rulepack and
`build_pipeline`, with `Context::from_json_str` with empty `dictionaries`, `class_map` and `fields`, Global locale chain and
one Email Tokenize policy rule. No production policy changes. Empty assembly
or unexpected class is a stop condition. The phone fixture is from the
repository's synthetic DE range. Public `Session::tokenize` pre-registers its
owned phone token in the same session. Tool constants emit raw phone, its
known token, a prefix and a duplicated anchor window. Email output must still
be tokenized. Anchors must mint no tokens.

Each authored occurrence owns one declared string slot or unique benign anchor
window, with one occurrence per window. Repeated equal values stay separate.
For plain-string payloads read the one text frame; for `json_in_text`,
`crates/gaze-mcp-rmcp/src/adapter.rs` serializes nonstrings into one text frame,
so parse that frame again under the authored schema before selecting a leaf.
Measure decoded UTF-8 leaf bytes, not JSON escape spelling on the wire.

Verdict precedence: ambiguous/missing/duplicate/overlapping anchors or multiple
authorized ranges yields attribution_not_measured, no protection credit.
Otherwise full authored value surviving wins; then a proper contiguous known
fragment of at least 8 bytes on complete UTF-8 code points; then an exact owned
token restoring to the independently authored value with no extra unprotected
gold; otherwise attribution_not_measured. Unknown or below-floor fragments
never earn protection. No global substring association or diff-built trace.
The four-slot controlled case must yield exactly 1 full, 1 partial, 1 protected,
1 attribution_not_measured. Mid-code-point prefixes round down safely.

Counted integrity analogues are exactly `egress_token_restore_failures`
(actual RestoreError on observed token text) and `egress_raw_value_mismatches`
(restored occurrence versus independent authored expected slot value). Swap
two different valid owned tokens to falsify the second without a restore error.
The four geometry analogues `egress_clean_bounds_invalid`,
`egress_authorized_range_bounds_invalid`,
`egress_authorized_range_non_monotonic`, `egress_overlapping_clean_spans`
are non-counting by construction: session.rs constructs output ranges while
appending, then merges ordered adjacent ranges. Checking them against that
same output is tautological. They are not schema-v4 counters; the schema-v4
six counters plus `spans` key have no equivalent T1 claim (`spans` is not a
counter). Their gate remains BLOCKED.

Observer snapshots and logger entries are native observations with a caveat:
installing a SafetyNet activates a stage a no-net pipeline skips. The passive
observer returns an empty suspect vector. A valid suspect crossing a literal
raw non-token gap must cause Residual; a token-covered suspect may be accepted.
This is distinct from checking passive emptiness. Correlation is ordinal within
one serial dispatch, using the exact authored argument/response traversal,
including early-failing leaves. Count observed and unobserved leaves separately;
do not claim complete coverage from the observed subset. `field_path` is None.

`crates/gaze-types/src/lib.rs` states: "At that boundary, raw spans use
expanded-owner input coordinates, not offsets into the literal token-bearing
input. Do not index that literal input with reconstructed raw spans."
`crates/gaze/src/pipeline/protection.rs` states: "Reconstructed safety-manifest
raw coordinates refer to the expanded input interpretation: existing owned
tokens stand for their stored raw bytes."

Logger events still fire when the protection trace is absent. They establish
event provenance, not per-token causation. `protect_gap` passes None for the
trace collector; `protection_trace_items` remains not_measured. No second
detector run, new runtime API or instrumentation is permitted.

Restoration compares exact **owner-authorized string bytes**. Parsed JSON
equality cannot substitute. A JSON-text string with semantically equivalent,
byte-different whitespace/escaping is the discriminating test. Transport wire
whitespace/key order are explicitly outside scope; semantic envelope validity,
framing, restored owner output and agent egress remain separate observables.

## Class commitment and later gates

`class-commitments-v1.json` declares a row schema and fixture-only rows, not a
corpus commitment. Commitment (supported/partial/unsupported), model coverage
(covered/gap/unknown), validator applicability and observed protection are
orthogonal. Every leak remains in totals. Unknown label/region, duplicates or
missing mandatory fields fail loading. Veto causation is unknown_not_measured
unless observed and bound to region/normalization. D1 malformed-person-linked
sensitivity is held policy; fixtures do not resolve it. Completeness is BLOCKED.

Before any real population: freeze protocol/scorer/evaluator/code/policy/model,
loader and full analysis declarations; acquire under custody; conduct blinded
annotation/adjudication; seal final gold plus membership/order; evaluate; then
unblind. Never hash nonexistent gold or amend gold after seeing outcomes;
post-outcome corrections need reserve or new protocol. Independent custodian,
generator provenance and populated commitments remain required. Gretel is only
a candidate. Sizing, safe subprocess producers, trace API review, route expansion,
execution authority and a fresh machine claim are later gates. IBAN attribution
is a separate bounded probe.

Legacy helpers reused unchanged: `git_metadata`, `merge_intervals`,
`interval_length` in `scripts/bench/gaze_bench_score.py`. Explicitly not reused:
`identified_document_population` (IDs), `DIRECT_IDENTIFIER_LABELS` (different
semantics), private Rust example `manifest_integrity` (unavailable surface),
`MetricAccumulator` (no retained paired records). No corpus parity is claimed.

## Stable mutation roster

Identifiers below are unique. Legacy roster labels in the origin column are
mapping references only, never test identifiers. All verdicts start as static
predictions; completion proof must separately name actually executed mutants
and exact failing tests. Compile errors and unrelated refusals do not count.

| Identifier | Origin | Mechanism |
|---|---|---|
| MUT-PATH-TOP | M1 | add `clean_text` to `RECEIPT_PATHS` |
| MUT-PATH-NESTED | M2 | make the Python walk stop at the top level |
| MUT-LEAF-TYPES | M3 | accept any leaf type once the path is known |
| MUT-VOCAB-CLOSURE-MAPS | M4 | allow arbitrary `counts` and `gate_results` keys |
| MUT-VOCAB-CLOSURE-VALUES | M5 | allow arbitrary `error_codes`, `strata`, `derivations` values |
| MUT-HANDLE-OPACITY | M6 | check the handle only against the custody record |
| MUT-ATTESTATION-BINDING | M7 | accept a placeholder `source_revision` |
| MUT-OUTCOME-COVERAGE | M8 | default a missing outcome key to 0 |
| MUT-OUTCOME-IDENTITY | M9 | drop the `planned_case_count == sum(outcomes)` check |
| MUT-INVENTORY | M10 | let `add` accept an unknown key and skip `finalize`'s `NOT_STARTED` fill |
| MUT-DECLARATION-ABSENT | M11a | treat `None` as a permissive default |
| MUT-DECLARATION-PARTIAL | M11b | default `confidence_level` to 0.95 |
| MUT-AGGREGATION | M12 | make `overall` ignore `BLOCKED` |
| MUT-PAIR-HONESTY | M13 | count an unpaired case as zero leak |
| MUT-CONDITIONALITY | M14 | always report `conditional: false` |
| MUT-REJECTION-CREDIT | M15 | count `FAILED_CLOSED_NO_EGRESS` as protected |
| MUT-NO-PAYLOAD | M16 | classify on the client `Result` discriminant instead of `is_error` + content |
| MUT-UNKNOWN-LOWER-BOUND | M17 | zero the leak counts for `UNKNOWN_EGRESS` cases |
| MUT-GROUPING | M18 | resample records instead of groups |
| MUT-QUANTILE | M19 | off-by-one in percentile selection |
| MUT-VOCAB-PY | M20 | add one metric id on the Python side only |
| MUT-VOCAB-RUST | M21 | add one metric id on the Rust side only |
| MUT-RUST-WALKER | M22 | emit one extra key from the Rust emitter |
| MUT-OBSERVER-RAW-GAP | M23 | make the `SafetyNet` observer return one suspect |
| MUT-OBSERVER-COVERAGE | M24 | compute a coverage metric over observed leaves while `observer_leaves_unobserved > 0` |
| MUT-CANARY-PRIVATE-VALUE | M25 | route a private value into a refusal message |
| MUT-STAMP-SEPARATION | M26 | let the emitter write `membership_order_proof_verified` |
| MUT-LOCAL-MEMBERSHIP-PROOF | M27 | derive the proof flag from the receipt instead of the private check |
| MUT-CLAIM-SCOPE | M28 | accept a receipt without `claim_scope` |
| MUT-DERIVATION-COVERAGE | M29 | skip the `counts`↔`derivations` cross-check |
| MUT-EMITTER-FORBIDDEN-COUNT | M30 | publish a count for a `not_measured` metric |
| MUT-BLOCKED-BOOKKEEPING | M31 | omit a `BLOCKED` gate from `not_measured.blocked_gates` |
| MUT-FAILED-FINISH | M32 | allow a second terminal call after `finish_call` |
| MUT-LEAK-COMPUTED | M33 | hard-code `gold_occurrences_surviving_egress = 0` in the emitter |
| MUT-OCCURRENCE-ORACLE | M34 | fall back to whole-value substring search |
| MUT-CLASS-LOAD | M35 | accept an unknown label/region pair |
| MUT-CLASS-FIELDS | M36 | accept a row missing a mandatory field |
| MUT-STRING-BYTES | M37 | compare parsed-JSON equality instead of string bytes |

Additional amendment probes: MUT-VALIDATOR-ACCEPTS-FORBIDDEN-COUNT, MUT-ROLLBACK, MUT-COUNTING-SUBSET, MUT-UNKNOWN-INTERVAL, MUT-INTERVAL-DECLARATION, MUT-FULL-CELL-BASIS, MUT-RAW-VALUE-SWAP, MUT-TOKEN-CORRUPTION, MUT-CONTAINER-TYPES.

Corrected mechanisms govern the origin mapping: OBSERVER-RAW-GAP crosses
literal non-token bytes; UNKNOWN-LOWER-BOUND is evaluator-only; FAILED-FINISH
retains committed mappings with one terminal attempt, ROLLBACK loses no mappings;
STRING-BYTES changes JSON spelling with semantic equality. EMITTER-FORBIDDEN-COUNT
kills emitter conformance; VALIDATOR-ACCEPTS-FORBIDDEN-COUNT kills consumer
refusal. BLOCKED-BOOKKEEPING kills its bookkeeping test alone. COUNTING-SUBSET
checks positive golden acceptance plus each non-counting grade and omitted
count. UNKNOWN-INTERVAL, INTERVAL-DECLARATION and FULL-CELL-BASIS each exercise
the matching named refusal. RAW-VALUE-SWAP swaps distinct owned tokens;
TOKEN-CORRUPTION must actually yield RestoreError. CONTAINER-TYPES substitutes
empty arrays/objects at paths requiring another type.

## Closed vocabulary reference

These are the committed T1 sets. Changes require agreement of both language
mirrors and the golden vocabulary artifact; arbitrary identifiers are refused.

### ARM_IDS

`base`, `candidate`.

### ATTESTATION_SOURCES

`live_repository`, `test_seam`.

### BASIS_IDS

`full_cell`, `paired_completed`.

### BLOCKED_GATES

`class_commitment_completeness`, `manifest_integrity_six_counter_schema_v4`, `per_token_protection_trace`, `producer_membership_order_proof`, `unknown_egress_route_fragment_observation`.

### CELL_IDS

`synthetic.mcp.controlled.v1`, `synthetic.mcp.core.v1`.

### CLAIM_SCOPES

`synthetic_harness_capability_only`.

### CONTROL_REFUSAL_CODES

`auth-denied`, `backend-failure`, `backend-unavailable`, `internal`, `invalid-args`, `invalid-session-id`, `limit-exceeded`, `manifest-persistence-failed`, `not-found`, `redaction-failed`, `response-serialization-failed`.

### COUNTING_GRADES

`egress_reconstructed`, `observer_native`, `route_native`.

### DECLARATION_FIELDS

`acceptance_limit`, `confidence_level`, `coverage_target`, `multiplicity_treatment`, `resample_count`, `seed`, `strata`, `weighting`.

### DERIVATIONS

`egress_reconstructed`, `invariant_enforced_not_counted`, `not_applicable_by_construction`, `not_measured`, `observer_native`, `route_native`.

### ERROR_CODES

`auth-denied`, `backend-failure`, `backend-unavailable`, `internal`, `invalid-args`, `invalid-session-id`, `limit-exceeded`, `manifest-persistence-failed`, `not-found`, `redaction-failed`, `response-serialization-failed`.

### FEATURE_GRAPH_IDS

`workspace.all_features`, `workspace.default`.

### GATE_IDS

`build_attestation_clean_source`, `claim_scope_present`, `class_commitment_completeness`, `class_commitment_schema_load`, `cross_language_vocabulary_equality`, `declaration_gating`, `egress_integrity_analogues`, `false_positive_negative_control`, `gold_survival_oracle`, `local_membership_order_proof`, `manifest_integrity_six_counter_schema_v4`, `no_payload_positive_observation`, `outcome_identities`, `paired_grouped_interval_arithmetic`, `per_token_protection_trace`, `planned_inventory_reconciliation`, `producer_membership_order_proof`, `receipt_path_allowlist`, `rejection_is_not_protection`, `source_attribution_events`, `stamped_field_separation`, `string_byte_reversibility`, `unknown_egress_lower_bound`, `unknown_egress_route_fragment_observation`, `vocabulary_closure`.

### GATE_RESULTS

`BLOCKED`, `FAIL`, `NOT_EVALUABLE`, `PASS`.

### LEAK_FAMILY_METRIC_IDS

`gold_bytes_surviving_egress`, `gold_occurrences_attribution_not_measured`, `gold_occurrences_partially_surviving_egress`, `gold_occurrences_surviving_egress`.

### METHOD_IDS

`grouped_paired_percentile_v1`.

### METRIC_IDS

`egress_authorized_range_bounds_invalid`, `egress_authorized_range_non_monotonic`, `egress_clean_bounds_invalid`, `egress_overlapping_clean_spans`, `egress_raw_value_mismatches`, `egress_token_restore_failures`, `false_positive_bytes`, `false_positive_occurrences`, `gold_bytes_planned`, `gold_bytes_surviving_egress`, `gold_occurrences_attribution_not_measured`, `gold_occurrences_partially_surviving_egress`, `gold_occurrences_planned`, `gold_occurrences_surviving_egress`, `leaf_restore_decision_failures`, `leaf_restore_exact`, `manifest_raw_entry_agreement_enforced`, `manifest_span_monotonicity_enforced`, `manifest_terminal_events`, `observer_leaves_observed`, `observer_leaves_unobserved`, `observer_manifest_spans_observed`, `observer_recognizer_source_events`, `protected_leaves`, `protection_trace_items`, `tool_invocations`, `unknown_egress_lower_bound_cases`.

### MULTIPLICITY_IDS

`synthetic_none`.

### OUTCOME_STATES

`COMPLETED`, `ERROR_PROTOCOL`, `FAILED_CLOSED_NO_EGRESS`, `NOT_STARTED`, `UNKNOWN_EGRESS`.

### POLICY_IDENTITIES

`controlled.email_only.v1`, `core.rule_floor.v1`.

### RECEIPT_PATHS

`$`, `$.analysis_declaration`, `$.analysis_declaration.acceptance_limit`, `$.analysis_declaration.confidence_level`, `$.analysis_declaration.coverage_target`, `$.analysis_declaration.multiplicity_treatment`, `$.analysis_declaration.resample_count`, `$.analysis_declaration.seed`, `$.analysis_declaration.strata`, `$.analysis_declaration.strata.[]`, `$.analysis_declaration.weighting`, `$.arm_id`, `$.asymmetric_outcome_table`, `$.asymmetric_outcome_table.*`, `$.asymmetric_outcome_table.*.*`, `$.cell_id`, `$.claim_scope`, `$.class_commitment_table`, `$.class_commitment_table.id`, `$.class_commitment_table.version`, `$.counts`, `$.counts.*`, `$.derivations`, `$.derivations.*`, `$.error_codes`, `$.error_codes.*`, `$.gate_results`, `$.gate_results.*`, `$.intervals`, `$.intervals.*`, `$.intervals.*.basis`, `$.intervals.*.conditional`, `$.intervals.*.high`, `$.intervals.*.low`, `$.intervals.*.method_id`, `$.intervals.*.point`, `$.not_measured`, `$.not_measured.blocked_gates`, `$.not_measured.blocked_gates.[]`, `$.not_measured.metrics`, `$.not_measured.metrics.[]`, `$.outcomes`, `$.outcomes.*`, `$.planned_case_count`, `$.policy_identity`, `$.population_handle`, `$.protocol_id`, `$.protocol_version`, `$.route_id`, `$.route_status`, `$.route_status.*`.

### REFUSAL_CODES

`attestation_shape_invalid`, `class_commitment_invalid`, `conditional_reported_as_full_cell`, `counted_non_measurement`, `declaration_invalid`, `derivation_conflict`, `derivation_coverage_incomplete`, `duplicate_json_key`, `gate_coverage_incomplete`, `handle_shape_invalid`, `input_limit`, `interval_without_declaration`, `inventory_conflict`, `lower_bound_reported_as_exact`, `malformed_json`, `membership_order_proof_failed`, `missing_mandatory_key`, `non_finite_number`, `observer_coverage_incomplete`, `outcome_identity_violation`, `protocol_identity_mismatch`, `stamped_key_in_emitted_receipt`, `uncounted_measurable_metric`, `unknown_path`, `value_out_of_vocabulary`, `wrong_type`.

### ROUTE_IDS

`daemon.jsonl.v1`, `mcp.rmcp.duplex.v1`, `ocr.document.v1`, `proxy.http.v1`, `session.episode.v1`, `stream.v1`, `structured.core.v1`, `text.clean_for_bench.v1`.

### ROUTE_STATUSES

`IMPLEMENTED`, `NOT_IMPLEMENTED`.

### STAMPED_KEYS

`build_attestation`, `local_membership_order_proof_method`, `local_membership_order_verified`.

### STRATUM_IDS

`synthetic_de`, `synthetic_en`.

### TABLE_IDS

`class-commitments-v1`.

### TOOLCHAIN_IDS

`rust.workspace_pinned`.

### WEIGHTING_IDS

`inventory_group`.

## Reproducible focused proof

Run Python unittest discovery with Python 3.13; assert a nonzero collection.
Run `cargo test --offline --locked -p gaze-mcp-rmcp --test evidence_route
-- --test-threads=1` under the pinned Rust 1.96.0 toolchain and live machine
serialization. Only synthetic data is used. The optional
`python3.13 scripts/bench/test_evidence_protocol.py --mutation-proof` performs
in-memory Python function mutations, restores every function, and reports
only identifiers and actual failing test names. It never reports raw failures.
Each invocation names its targeted kill set; it does not imply that untargeted
tests passed. The static roster remains the full review checklist. A mutant
that survives or fails compilation is not a killed mutant.

Rust test-local counter mutations likewise require compiling and executing the
named test. Adversarial corrupted-token restore probes are evaluator-helper
falsifiers using synthetic post-observation operands, not claimed route-native
unknown-token egress. The actual token-swap fixture does traverse the route.
The timeout route observes no partial bytes. These distinctions are mandatory.

Normal workspace clippy/all-feature tests, documentation, MSRV, deny and xtask
CI gates remain separate root/PR obligations; they are not replaced by this
focused proof. No new screenshots are needed because no visible output changes.

Additional mutation `MUT-CANARY-RUST` emits a synthetic private value inside the
canary child; only the parent output-capture test is its targeted kill set.
The complete roster is 48 mechanisms: 35 Python and 13 Rust opt-in probes.
Rust probes run with `python3.13 scripts/bench/test_evidence_eval.py
--rust-mutation-proof` under the machine lease, modifying only evidence_route.rs
and restoring its exact committed bytes after each probe.
