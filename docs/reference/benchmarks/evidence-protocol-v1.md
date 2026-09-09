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
The evaluator identifies itself as `evaluator.private.v1`, cell
`synthetic.evaluator.v1`, policy `authored.records.v1`; its receipt marks the MCP
route NOT_IMPLEMENTED. Only the emitting component is IMPLEMENTED.

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

Counting grades are `route_native`, `observer_native`, `egress_reconstructed`,
`planned_inventory`, `private_authored_records`, `observed_subset_lower_bound`.
The last grade means a sum over explicitly observed operands only, never an
exact full-cell count. The evaluator declares its full per-metric provenance
in `EVALUATOR_DERIVATIONS`: authored inventory denominators, private authored
record sums, and not_measured for every unperformed runtime check. Availability
can downgrade a declared metric to not_measured or an observed subset lower bound;
it never upgrades provenance merely because a numeric field exists.
Both validators bind the emitting route to its cell/policy pair and a per-metric
allowed derivation map. MCP core pairs with core.rule_floor.v1; MCP controlled
pairs with controlled.email_only.v1. Evaluator plans allow planned_inventory;
evaluator survival, attribution, false-positive and unknown-case counts allow
private_authored_records or observed_subset_lower_bound. Other evaluator metrics
are not_measured. Route observer metrics allow observer_native; survival,
attribution, false positives and the two counted integrity analogues allow
egress_reconstructed; planned/restore/tool/terminal/protected-leaf counts allow
route_native. Geometry and manifest invariants retain their respective
construction/invariant grades. Every metric permits a not_measured downgrade.
No route metric uses observed_subset_lower_bound: incomplete comparisons are
omitted. A foreign producer grade, a grade moved to the wrong metric, or a
wrong cell/policy pair refuses with protocol_identity_mismatch (Rust: false).
Non-counting grades are `invariant_enforced_not_counted`,
`not_applicable_by_construction`, `not_measured`. Counts are exactly the
counting-grade subset of the complete derivation inventory. Every metric at
`not_measured` is listed, with no numeric zero. Every BLOCKED gate is listed
exactly in `not_measured.blocked_gates`. All gate IDs must be present.

Stamps are forbidden in emitted receipts. Structural validation separately
calls the unchanged `gaze_bench_score.git_metadata(repo_root)` itself to bind
**validator** code identity. It does not authenticate the producer. The stamp
`build_attestation` carries `source_revision` (40 lowercase hex), `dirty`,
`source` (`live_repository` or `test_seam`). It does not invent toolchain or
feature declarations; those unused stamped fields are removed.
Without a repository root, only the explicitly fixture-only `attestation_probe`
seam is allowed. A seam yields NOT_EVALUABLE, never PASS. When a live root is
provided, live dirty state wins; a probe cannot override it.

Only the private evaluator checks its actual inventory and order against
explicit nonempty custody membership_order and adds `local_membership_order_verified` and
`local_membership_order_proof_method = private_membership_order_v1`.
External aggregate validation stamps no membership fact. Producer membership
is NOT_MEASURED and `producer_membership_order_proof` remains BLOCKED.

## Outcomes and aggregation

Ordered states: `NOT_STARTED`, `COMPLETED`, `FAILED_CLOSED_NO_EGRESS`,
`ERROR_PROTOCOL`, `UNKNOWN_EGRESS`. Planned absent cases are NOT_STARTED;
completed requires actual decoded client response. No-payload requires a
received error frame with exactly one text content block in the committed
control vocabulary and no other data-bearing content. Neither client Ok nor
Err alone classifies dispatch. The actual result must serialize to exactly the
allowlisted error shape: no structuredContent, result/content metadata,
annotations or other data-bearing surfaces. Adversarial classifier tests mutate
a clone of an actual received safe error, not a claimed production leak.
The route observes a subset of OUTCOME_STATES; it does not classify ERROR_PROTOCOL. Wire codes collapse causes and never establish
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
intervals. A full-cell interval requires completed outcomes in every relevant arm and
`conditional == false`. Candidate consumers derive base margins and the paired
intersection from the asymmetric table. Unknown in either arm blocks paired
leak intervals; zero pairs permit no numeric paired interval; any noncompleted
remainder requires conditional=true. An omitted interval means NOT_EVALUABLE,
so unmeasured metrics need no interval entry. A numeric interval is forbidden
for observed_subset_lower_bound counts. Rejection, missing records and errors are never
zero-leak substitutions. Observations on failed/skipped leaves are not counted
without actual evidence.

`overall`: any FAIL wins; otherwise any BLOCKED or NOT_EVALUABLE yields
NOT_EVALUABLE; only complete all-PASS coverage yields PASS.

## Private paired estimator

Frozen `PlannedInventory` holds ordered `(key, group_id, stratum, weight)` and
optional independently authored gold occurrence/byte denominators. A missing
plan operand stays unavailable even when both records are absent. DocRecord
observed_metrics explicitly names evaluated count operands; default numeric
fields do not establish availability. An observed zero remains numeric. Missing
records, unobserved unknown frames and failed preflight have no egress counts.
A mixed cell exports only explicitly graded observed subset sums.
An authoritative plan bounds each case's surviving bytes and the sum of full,
partial and attribution-unknown occurrences. Optional record denominators must
agree with known plan denominators; they default to unavailable, not zero.
Missing records retain available planned counts without measured egress zeros.
Plan counts are not paired estimands: their intervals are NOT_EVALUABLE or absent;
both consumers refuse numeric plan intervals.

The unknown-egress case predicate is three-valued: any observed positive byte,
full-occurrence or partial-occurrence operand proves positive; all three observed
zeros prove zero. Attribution alone proves neither. A missing operand otherwise
leaves that case unavailable. An observed subset gets observed_subset_lower_bound
and a NOT_EVALUABLE gate; only complete predicate coverage permits PASS. The count
is omitted when no case predicate is known.
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

Measurement gates require a performed check and evaluated denominator. Missing
restore/negative/gold checks are NOT_EVALUABLE, ambiguity prevents a gold PASS,
and actual token swaps or byte mismatches produce FAIL receipts. Each restore
attempt contributes an expected raw-comparison operand; exactly one authorized
range contributes a performed comparison. Raw mismatch counts are omitted unless
all expected comparisons were performed. A negative-control predicate is known
only for full preservation or exact owned-token restoration; partial/unknown
verdicts remain unavailable. Its counts are likewise omitted on incomplete
coverage. Known failures override incompleteness for both gates. Tests that
mutate a restore operand label that receipt as a test-only falsifier.

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

These are actual implementation/helper edits and their exact targeted tests.
They do not edit assertions. Each target must collect one test and fail its
assertion with the listed marker; Python also requires zero test errors, Rust
requires the named test FAILED line. A compile failure is never a kill.
`AssertionError` means the named Python target's assertion, not a private
exception message. Dedicated sink markers distinguish stdout, stderr and files.

The old M7 shape mechanism is retained as MUT-ATTESTATION-SHAPE; the binding ID
now replaces the live helper result with the supplied probe and must fail the
helper-call assertion. Stamp separation checks `local_membership_order_verified`,
`local_membership_order_proof_method`, and `build_attestation`.
Python canaries fork with a disposable child cwd, capture both descriptors
privately, exercise complete success/refusal/export paths, inspect actual files,
and remove the directory. Forking preserves the in-memory implementation mutant.
No probe runs with the original checkout as its child cwd. Closed-exception
violations use a dedicated child exit status and static parent marker; no private
exception text is printed. MUT-VOCAB-PY uses the same one-test, zero-error,
expected-marker kill predicate and records failure_marker like every other row.
The 67 prior probes remain; MUT-INVENTORY now bypasses unknown-key lookup using
the first planned case, and MUT-ROUTE-AVAILABILITY disables the expanded
availability guard. Producer-declaration tests inspect pre-validation output;
separate consumer counterexamples prove refusal, so a typed refusal cannot be
misreported as an assertion kill.

| Identifier | Actual edit | Exact target assertion/test | Expected marker |
|---|---|---|---|
| MUT-AGGREGATION | `if any(v != 'PASS' for v in gates.values()):` → `if False:` | `test_evidence_protocol.AggregationTests.test_all_blocked_is_not_evaluable_not_pass` | `AssertionError` |
| MUT-ATTESTATION-BINDING | `meta = legacy.git_metadata(Path(repo_root))` → `meta = attestation_probe` | `test_evidence_protocol.StampTests.test_clean_probe_cannot_override_live_dirty_tree` | `live-helper-binding` |
| MUT-ATTESTATION-SHAPE | `require(type(meta['revision']) is str and re.fullmatch('[0-9a-f]{40}', meta['revision']) is not None and type(meta['dirty']) is bool, 'attestation_shape_invalid')` → `pass` | `test_evidence_protocol.StampTests.test_placeholder_source_revision_is_refused` | `AssertionError` |
| MUT-BLOCKED-BOOKKEEPING | `require(set(r['not_measured']['blocked_gates']) == {k for k,v in r['gate_results'].items() if v == 'BLOCKED'}, 'derivation_conflict')` → `pass` | `test_evidence_protocol.ReceiptTests.test_blocked_gate_absent_from_blocked_gates_is_refused` | `AssertionError` |
| MUT-CANARY-PRIVATE-VALUE | `require(type(text) is str` → `print('synthetic-private-canary') ↵     require(type(text) is str` | `test_evidence_protocol.CanaryTests.test_failures_do_not_emit_private_values_or_files` | `protocol-stdout-boundary` |
| MUT-CANARY-RUST | `let wire = must(serde_json::to_string(&r));` → `eprintln!("{EMAIL}"); ↵     let wire = must(serde_json::to_string(&r));` | `private_failure_canary_captures_stdout_stderr_and_files` | `private-output-canary` |
| MUT-CLAIM-SCOPE | `MANDATORY_KEYS <= set(r)` → `MANDATORY_KEYS - {'claim_scope'} <= set(r)` | `test_evidence_protocol.ReceiptTests.test_missing_claim_scope_is_refused` | `AssertionError` |
| MUT-CLASS-FIELDS | `require(type(row) is dict and set(row) == required, 'class_commitment_invalid')` → `pass`; `all(type(row[k]) is str and row[k] for k in ('partial_scope','rationale'))` → `all(type(row.get(k,'fixture')) is str and row.get(k,'fixture') for k in ('partial_scope','rationale'))` | `test_evidence_protocol.ClassCommitmentTests.test_missing_mandatory_row_field_is_a_load_error` | `AssertionError` |
| MUT-CLASS-LOAD | `pair in {('EMAIL','global'), ('PHONE','de')} and pair not in seen` → `pair not in seen` | `test_evidence_protocol.ClassCommitmentTests.test_unknown_label_region_pair_is_a_load_error` | `AssertionError` |
| MUT-CONDITIONALITY | `conditional=any(self.outcomes(a)['COMPLETED'] != len(self.inventory) for a in ep.ARM_IDS)` → `conditional=False` | `test_evidence_eval.PairingTests.test_conditional_flag_set_when_remainder_non_empty` | `AssertionError` |
| MUT-CONTAINER-TYPES | `if kind == 'nullable_object'` → `if kind == 'object' and node == []: return ↵     if kind == 'nullable_object'` | `test_evidence_protocol.DirectBoundaryTests.test_empty_container_types` | `AssertionError` |
| MUT-COUNTING-SUBSET | `require(all(k in c for k,v in d.items() if v in COUNTING_GRADES), 'uncounted_measurable_metric')` → `pass` | `test_evidence_protocol.ReceiptTests.test_actual_measurement_requires_count` | `AssertionError` |
| MUT-DECLARATION-ABSENT | `self.finalize()` → `if declaration is None: declaration = __import__('test_evidence_protocol').declaration() ↵         self.finalize()` | `test_evidence_eval.DeclarationTests.test_absent_declaration_is_not_evaluable` | `AssertionError` |
| MUT-DECLARATION-PARTIAL | `self.finalize()` → `if type(declaration) is dict: declaration.setdefault('confidence_level', .5) ↵         self.finalize()` | `test_evidence_eval.DeclarationTests.test_partial_declaration_is_not_evaluable` | `AssertionError` |
| MUT-DERIVATION-COVERAGE | `require(set(d) == METRIC_IDS, 'derivation_coverage_incomplete')` → `pass`; `require(set(r['not_measured']['metrics']) == {k for k,v in d.items() if v == 'not_measured'}, 'derivation_conflict')` → `pass` | `test_evidence_protocol.ReceiptTests.test_metric_without_declared_derivation_is_refused` | `AssertionError` |
| MUT-EMITTER-FORBIDDEN-COUNT | `assert!(receipt_allowlisted(&r), "emitter-conformance");` → `r["counts"]["protection_trace_items"] = json!(0); ↵     assert!(receipt_allowlisted(&r), "emitter-conformance");` | `protected_success_and_golden_receipt` | `emitter-conformance` |
| MUT-EVALUATOR-AVAILABILITY | `if values: result[metric] = sum(values)` → `result[metric] = sum(values)` | `test_evidence_eval.ExportTests.test_availability_identity_and_full_derivation_map` | `missing-observation-counts` |
| MUT-EVALUATOR-EMPTY-STAMP | `bool(self.inventory.keys()) and bool(custody.get('membership_order'))` → `True` | `test_evidence_eval.ExportTests.test_empty_or_missing_membership_is_refused` | `AssertionError` |
| MUT-EVALUATOR-FILE | `self.finalize()` → `Path('synthetic-private-output').write_text('synthetic-private-canary') ↵         self.finalize()` (export_receipt) | `test_evidence_eval.ExportTests.test_evaluator_canary_no_output_or_file_writes` | `evaluator-file-boundary` |
| MUT-EVALUATOR-GRADES | `result = EVALUATOR_DERIVATIONS.copy()` → `result = dict.fromkeys(ep.METRIC_IDS, 'egress_reconstructed')` | `test_evidence_eval.ExportTests.test_availability_identity_and_full_derivation_map` | `evaluator-declared-grades` |
| MUT-EVALUATOR-IDENTITY | `route_id='evaluator.private.v1'` → `route_id='mcp.rmcp.duplex.v1'`; `if k == 'evaluator.private.v1'` → `if k == 'mcp.rmcp.duplex.v1'` | `test_evidence_eval.ExportTests.test_availability_identity_and_full_derivation_map` | `evaluator-identity` |
| MUT-EVALUATOR-STDERR | `self.finalize()` → `print('synthetic-private-canary', file=__import__('sys').stderr) ↵         self.finalize()` (export_receipt) | `test_evidence_eval.ExportTests.test_evaluator_canary_no_output_or_file_writes` | `evaluator-stderr-boundary` |
| MUT-EVALUATOR-STDOUT | `self.finalize()` → `print('synthetic-private-canary') ↵         self.finalize()` (export_receipt) | `test_evidence_eval.ExportTests.test_evaluator_canary_no_output_or_file_writes` | `evaluator-stdout-boundary` |
| MUT-EVALUATOR-VACUOUS-GATE | `if r['derivations']['unknown_egress_lower_bound_cases'] == 'private_authored_records':` → `if True:` | `test_evidence_eval.ExportTests.test_availability_identity_and_full_derivation_map` | `unexercised-evaluator-gates` |
| MUT-EXTRA-SURFACES | `if no_payload_surfaces(r)` → `if true` | `no_payload_classifier_rejects_extra_surfaces` | `extra-surface-unknown` |
| MUT-FAILED-FINISH | `async fn finish_call(&self, _: CallHandle, _: SnapshotRef)` → `async fn finish_call(&self, handle: CallHandle, _: SnapshotRef)`; `if self.fail_finish {` → `if self.fail_finish { ↵             self.fail_call(handle, FailureReason::Other { message: "synthetic".into() }).await?;` | `response_conflict_rolls_back_but_failed_finish_retains_mappings` | `failed-finish-retains-committed` |
| MUT-FULL-CELL-BASIS | `require(not interval['conditional'] and not incomplete, 'conditional_reported_as_full_cell')` → `pass` | `test_evidence_protocol.ReceiptTests.test_full_cell_requires_complete_known_outcomes` | `AssertionError` |
| MUT-GATE-FP | `assert!(receipt_allowlisted(&r), "emitter-conformance");` → `r["gate_results"]["false_positive_negative_control"] = json!("PASS"); ↵     assert!(receipt_allowlisted(&r), "emitter-conformance");` | `missing_measurements_and_ambiguous_only_gates` | `unperformed-gate` |
| MUT-GATE-GOLD | `assert!(receipt_allowlisted(&r), "emitter-conformance");` → `r["gate_results"]["gold_survival_oracle"] = json!("PASS"); ↵     assert!(receipt_allowlisted(&r), "emitter-conformance");` | `missing_measurements_and_ambiguous_only_gates` | `unperformed-gate` |
| MUT-GATE-INTEGRITY | `assert!(receipt_allowlisted(&r), "emitter-conformance");` → `r["gate_results"]["egress_integrity_analogues"] = json!("PASS"); ↵     assert!(receipt_allowlisted(&r), "emitter-conformance");` | `integrity_analogues_have_independent_nonzero_falsifiers` | `swap-gate-fail` |
| MUT-GATE-STRING | `assert!(receipt_allowlisted(&r), "emitter-conformance");` → `r["gate_results"]["string_byte_reversibility"] = json!("PASS"); ↵     assert!(receipt_allowlisted(&r), "emitter-conformance");` | `json_text_string_bytes_are_stricter_than_semantic_equality` | `string-gate-fail` |
| MUT-GROUPING | `result.extend(groups[rng.choice(ids)])` → `result.append(rng.choice(groups[rng.choice(ids)]))` | `test_evidence_eval.GroupingTests.test_resample_draws_groups_not_records` | `AssertionError` |
| MUT-HANDLE-OPACITY | `require(type(node) is str and re.fullmatch('[0-9a-f]{32,64}', node) is not None, 'handle_shape_invalid')` → `pass` | `test_evidence_protocol.ReceiptTests.test_readable_population_handle_in_custody_is_still_refused` | `AssertionError` |
| MUT-INTERVAL-DECLARATION | `require(declaration_valid(r['analysis_declaration']), 'interval_without_declaration')` → `pass` | `test_evidence_protocol.ReceiptTests.test_interval_without_declaration_is_refused` | `AssertionError` |
| MUT-INVENTORY | `planned = self.inventory.case(key)` → `planned = self.inventory.case(self.inventory.keys()[0])` | `test_evidence_eval.PlannedInventoryTests.test_unknown_key_is_refused` | `AssertionError` |
| MUT-LEAF-TYPES | `if kind == 'nullable_object'` → `if kind not in ('object','nullable_object','interval','array'): return ↵     if kind == 'nullable_object'` | `test_evidence_protocol.ReceiptTests.test_bool_where_int_required_is_refused` | `AssertionError` |
| MUT-LEAK-COMPUTED | `self.add("gold_occurrences_surviving_egress", 1);` → `self.add("gold_occurrences_surviving_egress", 0);` | `controlled_four_slot_occurrence_oracle` | `four-exact-verdicts` |
| MUT-LOCAL-MEMBERSHIP-PROOF | `tuple(custody.get('membership_order', ())) == self.inventory.keys()` → `True` | `test_evidence_eval.ExportTests.test_wrong_local_order_fails_membership_proof` | `AssertionError` |
| MUT-NEGATIVE-COVERAGE-COUNT | `c.1.get("negative_compared") != c.1.get("negative")` → `false` | `r2_negative_predicate_coverage` | `negative-incomplete-count` |
| MUT-NEGATIVE-COVERAGE-GATE | `c.1.get("negative_compared") != c.1.get("negative")` → `false` | `r2_negative_predicate_coverage` | `negative-coverage-gate` |
| MUT-NO-PAYLOAD | `r.is_error != Some(true) => "COMPLETED"` → `(r.is_error == Some(true) &#124;&#124; r.is_error != Some(true)) => "COMPLETED"` | `undeclared_carrier_has_positive_no_payload_and_unobserved_leaves` | `positive-no-payload` |
| MUT-OBSERVER-COVERAGE | `require(r['gate_results']['source_attribution_events'] != 'PASS', 'observer_coverage_incomplete')` → `pass` | `test_evidence_protocol.ReceiptTests.test_observer_coverage_is_not_inferred_for_unobserved_leaves` | `AssertionError` |
| MUT-OBSERVER-RAW-GAP | `mode, ↵             session: session.clone(),` → `mode: if mode == 0 {1} else {mode}, ↵             session: session.clone(),` | `protected_success_and_golden_receipt` | `completed-single-carrier` |
| MUT-OCCURRENCE-ORACLE | `if observed.matches("[[g]]").count() != 1 &#124;&#124; observed.matches("[[/g]]").count() != 1 { ↵             return Verdict::Unknown;` → `if observed.matches("[[g]]").count() != 1 &#124;&#124; observed.matches("[[/g]]").count() != 1 { ↵             return Verdict::Full;` | `controlled_four_slot_occurrence_oracle` | `four-exact-verdicts` |
| MUT-OUTCOME-COVERAGE | `o = r['outcomes']` → `o = r['outcomes'] ↵     o.setdefault('NOT_STARTED', 0)` | `test_evidence_protocol.ReceiptTests.test_all_five_states_are_mandatory` | `AssertionError` |
| MUT-OUTCOME-IDENTITY | `require(sum(o.values()) == r['planned_case_count'], 'outcome_identity_violation')` → `pass` | `test_evidence_protocol.ReceiptTests.test_planned_count_must_equal_outcome_sum` | `AssertionError` |
| MUT-PAIR-HONESTY | `if all(self._records[a][k].outcome == 'COMPLETED' for a in ep.ARM_IDS)` → `if any(self._records[a][k].outcome == 'COMPLETED' for a in ep.ARM_IDS)` | `test_evidence_eval.PairingTests.test_missing_pair_leaves_intersection_and_is_not_zero_leak` | `AssertionError` |
| MUT-PAIRED-BASE-MARGIN | `margins.append({a:sum(table[a].values()) for a in OUTCOME_STATES})` → `pass`; `paired_count = table['COMPLETED']['COMPLETED']` → `paired_count = r['outcomes']['COMPLETED']` | `test_evidence_protocol.ReceiptTests.test_both_arm_margins_gate_paired_intervals` | `AssertionError` |
| MUT-PATH-NESTED | `require(path in RECEIPT_PATHS` → `if path != '$': return ↵     require(path in RECEIPT_PATHS` | `test_evidence_protocol.ReceiptTests.test_unknown_nested_key_is_refused` | `AssertionError` |
| MUT-PATH-TOP | `walk(r)` → `pass` | `test_evidence_protocol.ReceiptTests.test_unknown_top_level_key_is_refused` | `AssertionError` |
| MUT-PLAN-BOUND | `ep.require(bound is None or observed <= bound, 'outcome_identity_violation')` → `pass` | `test_evidence_eval.AuthoritativePlanTests.test_plan_bounds_without_record_denominators` | `AssertionError` |
| MUT-PLAN-ESTIMAND | `if metric in PLAN_METRICS:` → `if False:` | `test_evidence_eval.ExportTests.test_planned_counts_have_no_paired_estimand` | `AssertionError` |
| MUT-PLAN-INTERVAL-PY | `require(metric not in ('gold_occurrences_planned','gold_bytes_planned'), 'protocol_identity_mismatch')` → `pass` | `test_evidence_protocol.ReceiptTests.test_numeric_planned_intervals_refused` | `AssertionError` |
| MUT-PLAN-INTERVAL-RUST | `) && v != "NOT_EVALUABLE"` → `) && false && v != "NOT_EVALUABLE"` | `r2_planned_interval_refused` | `planned-interval-refused` |
| MUT-PLAN-RECONCILE | `ep.require(authoritative is None or declared is None or authoritative == declared, 'inventory_conflict')` → `pass` | `test_evidence_eval.AuthoritativePlanTests.test_plan_bounds_and_record_conflicts` | `AssertionError` |
| MUT-PRODUCER-CELL | `require((r['cell_id'], r['policy_identity']) in {('synthetic.mcp.core.v1','core.rule_floor.v1'),('synthetic.mcp.controlled.v1','controlled.email_only.v1')}, 'protocol_identity_mismatch')` → `pass` | `test_evidence_protocol.ReceiptTests.test_producer_metric_grade_and_identity_binding` | `AssertionError` |
| MUT-PRODUCER-CELL-RUST | `if !match route {` → `if false && !match route {` | `r2_producer_grade_and_identity_binding` | `producer-cell-policy` |
| MUT-PRODUCER-GRADE | `require(all(grade in allowed_derivations(r['route_id'], metric) for metric,grade in d.items()), 'protocol_identity_mismatch')` → `pass` | `test_evidence_protocol.ReceiptTests.test_producer_metric_grade_and_identity_binding` | `AssertionError` |
| MUT-PRODUCER-GRADE-RUST | `allowed_derivation(route, m, text(g))` → `(allowed_derivation(route, m, text(g)) &#124;&#124; true)` | `r2_producer_grade_and_identity_binding` | `producer-metric-grade` |
| MUT-PROTOCOL-FILE | `r = validate_structure(payload)` → `Path('synthetic-private-output').write_text('synthetic-private-canary') ↵     r = validate_structure(payload)` (validate_receipt) | `test_evidence_protocol.CanaryTests.test_failures_do_not_emit_private_values_or_files` | `protocol-file-boundary` |
| MUT-PROTOCOL-STDERR | `r = validate_structure(payload)` → `print('synthetic-private-canary', file=__import__('sys').stderr) ↵     r = validate_structure(payload)` (validate_receipt) | `test_evidence_protocol.CanaryTests.test_failures_do_not_emit_private_values_or_files` | `protocol-stderr-boundary` |
| MUT-PROTOCOL-STDOUT | `r = validate_structure(payload)` → `print('synthetic-private-canary') ↵     r = validate_structure(payload)` (validate_receipt) | `test_evidence_protocol.CanaryTests.test_failures_do_not_emit_private_values_or_files` | `protocol-stdout-boundary` |
| MUT-QUANTILE | `math.ceil(probability * len(samples))-1` → `math.ceil(probability * len(samples))` | `test_evidence_eval.IntervalArithmeticTests.test_non_constant_deltas_match_hand_computed_quantiles` | `AssertionError` |
| MUT-RAW-COVERAGE-COUNT | `"egress_raw_value_mismatches" => c.1.get("raw_compared") != c.1.get("restore"),` → `"egress_raw_value_mismatches" => false,` | `r2_mixed_raw_comparison_coverage` | `raw-incomplete-count` |
| MUT-RAW-COVERAGE-GATE | `c.1.get("raw_compared").copied().unwrap_or(0) != restores` → `false` | `r2_mixed_raw_comparison_coverage` | `raw-coverage-gate` |
| MUT-RAW-VALUE-SWAP | `self.add("egress_raw_value_mismatches", 1);` → `self.add("egress_raw_value_mismatches", 0);` | `integrity_analogues_have_independent_nonzero_falsifiers` | `independent-slot-swap` |
| MUT-REJECTION-CREDIT | `r.outcome == 'COMPLETED' and r.entities > 0 and r.entities == r.entities_fully_covered` → `r.outcome == 'FAILED_CLOSED_NO_EGRESS'` | `test_evidence_eval.PairingTests.test_failed_closed_is_not_protection` | `AssertionError` |
| MUT-ROLLBACK | `if self.mode == 3 && !ctx.manifest.spans.is_empty() {` → `if self.mode == 3 && !ctx.manifest.spans.is_empty() { ↵             must(self.session.tokenize(&PiiClass::Email, FRESH));` | `response_conflict_rolls_back_but_failed_finish_retains_mappings` | `rollback-no-losing-mappings` |
| MUT-ROUTE-AVAILABILITY | `COUNTING_GRADES.contains(&grade) && (!c.0.contains_key(m) &#124;&#124; incomplete)` → `false` | `missing_measurements_and_ambiguous_only_gates` | `absent-measurements` |
| MUT-RUST-WALKER | `fn walk(node: &Value, path: &str, rules: &Value) -> bool {` → `fn walk(node: &Value, path: &str, rules: &Value) -> bool { if path != "$" {return true;}` | `rust_walker_rejects_nested_paths_types_and_forbidden_counts` | `nested-path-type-refusal` |
| MUT-STAMP-SEPARATION | `require(not STAMPED_KEYS.intersection(r), 'stamped_key_in_emitted_receipt')` → `pass`; `walk(r)` → `walk({k:v for k,v in r.items() if k not in STAMPED_KEYS})` | `test_evidence_protocol.ReceiptTests.test_emitted_receipt_carrying_a_stamped_key_is_refused` | `AssertionError` |
| MUT-STRING-BYTES | `if r.text == expected {` → `if must(serde_json::from_str::<Value>(&r.text)) == must(serde_json::from_str::<Value>(expected)) {` | `json_text_string_bytes_are_stricter_than_semantic_equality` | `string-bytes-discriminate` |
| MUT-TOKEN-CORRUPTION | `self.add("egress_token_restore_failures", 1);` → `self.add("egress_token_restore_failures", 0);` | `integrity_analogues_have_independent_nonzero_falsifiers` | `restore-error-counted` |
| MUT-UNKNOWN-INTERVAL | `require(not (unknown and metric in LEAK_FAMILY_METRIC_IDS), 'lower_bound_reported_as_exact')` → `pass`; `require(interval['conditional'], 'conditional_reported_as_full_cell')` → `pass` | `test_evidence_protocol.ReceiptTests.test_unknown_with_no_fragment_still_blocks_exact_interval` | `AssertionError` |
| MUT-UNKNOWN-LOWER-BOUND | `return result` → `result['gold_bytes_surviving_egress'] = 0 ↵         return result` | `test_evidence_eval.PairingTests.test_unknown_egress_observed_fragment_is_retained_as_lower_bound` | `AssertionError` |
| MUT-UNKNOWN-PREDICATE | `return None` → `return False` | `test_evidence_eval.ExportTests.test_unknown_predicate_truth_table_and_mixed_coverage` | `AssertionError` |
| MUT-UNKNOWN-SUBSET | `elif m == 'unknown_egress_lower_bound_cases' and any(` → `elif False and any(` | `test_evidence_eval.ExportTests.test_unknown_predicate_truth_table_and_mixed_coverage` | `AssertionError` |
| MUT-VALIDATOR-ACCEPTS-FORBIDDEN-COUNT | `require(all(d[k] in COUNTING_GRADES for k in c), 'counted_non_measurement')` → `pass` | `test_evidence_protocol.ReceiptTests.test_non_counting_grades_refuse_counts` | `AssertionError` |
| MUT-VOCAB-CLOSURE-MAPS | `require(key in allowed, 'value_out_of_vocabulary')` → `pass` | `test_evidence_protocol.DirectBoundaryTests.test_closed_dynamic_map_keys` | `AssertionError` |
| MUT-VOCAB-CLOSURE-VALUES | `require(node in allowed, 'value_out_of_vocabulary')` → `pass` | `test_evidence_protocol.DirectBoundaryTests.test_closed_dynamic_values` | `AssertionError` |
| MUT-VOCAB-PY | `METRIC_IDS` → `METRIC_IDS` plus `mutation-only` in copied vocabulary | `test_evidence_protocol.VocabularyMirrorTests.test_python_vocabularies_equal_committed_artifact` | `AssertionError` |
| MUT-VOCAB-RUST | `const METRIC_IDS: &[&str] = &[` → `const METRIC_IDS: &[&str] = &["mutation-only",` | `vocabularies_match_committed_artifact` | `vocabulary-mirror` |

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

`synthetic.evaluator.v1`, `synthetic.mcp.controlled.v1`, `synthetic.mcp.core.v1`.

### CLAIM_SCOPES

`synthetic_harness_capability_only`.

### CONTROL_REFUSAL_CODES

`auth-denied`, `backend-failure`, `backend-unavailable`, `internal`, `invalid-args`, `invalid-session-id`, `limit-exceeded`, `manifest-persistence-failed`, `not-found`, `redaction-failed`, `response-serialization-failed`.

### COUNTING_GRADES

`egress_reconstructed`, `observed_subset_lower_bound`, `observer_native`, `planned_inventory`, `private_authored_records`, `route_native`.

### DECLARATION_FIELDS

`acceptance_limit`, `confidence_level`, `coverage_target`, `multiplicity_treatment`, `resample_count`, `seed`, `strata`, `weighting`.

### DERIVATIONS

`egress_reconstructed`, `invariant_enforced_not_counted`, `not_applicable_by_construction`, `not_measured`, `observed_subset_lower_bound`, `observer_native`, `planned_inventory`, `private_authored_records`, `route_native`.

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

`authored.records.v1`, `controlled.email_only.v1`, `core.rule_floor.v1`.

### RECEIPT_PATHS

`$`, `$.analysis_declaration`, `$.analysis_declaration.acceptance_limit`, `$.analysis_declaration.confidence_level`, `$.analysis_declaration.coverage_target`, `$.analysis_declaration.multiplicity_treatment`, `$.analysis_declaration.resample_count`, `$.analysis_declaration.seed`, `$.analysis_declaration.strata`, `$.analysis_declaration.strata.[]`, `$.analysis_declaration.weighting`, `$.arm_id`, `$.asymmetric_outcome_table`, `$.asymmetric_outcome_table.*`, `$.asymmetric_outcome_table.*.*`, `$.cell_id`, `$.claim_scope`, `$.class_commitment_table`, `$.class_commitment_table.id`, `$.class_commitment_table.version`, `$.counts`, `$.counts.*`, `$.derivations`, `$.derivations.*`, `$.error_codes`, `$.error_codes.*`, `$.gate_results`, `$.gate_results.*`, `$.intervals`, `$.intervals.*`, `$.intervals.*.basis`, `$.intervals.*.conditional`, `$.intervals.*.high`, `$.intervals.*.low`, `$.intervals.*.method_id`, `$.intervals.*.point`, `$.not_measured`, `$.not_measured.blocked_gates`, `$.not_measured.blocked_gates.[]`, `$.not_measured.metrics`, `$.not_measured.metrics.[]`, `$.outcomes`, `$.outcomes.*`, `$.planned_case_count`, `$.policy_identity`, `$.population_handle`, `$.protocol_id`, `$.protocol_version`, `$.route_id`, `$.route_status`, `$.route_status.*`.

### REFUSAL_CODES

`attestation_shape_invalid`, `class_commitment_invalid`, `conditional_reported_as_full_cell`, `counted_non_measurement`, `declaration_invalid`, `derivation_conflict`, `derivation_coverage_incomplete`, `duplicate_json_key`, `gate_coverage_incomplete`, `handle_shape_invalid`, `input_limit`, `interval_without_declaration`, `inventory_conflict`, `lower_bound_reported_as_exact`, `malformed_json`, `membership_order_proof_failed`, `missing_mandatory_key`, `non_finite_number`, `observer_coverage_incomplete`, `outcome_identity_violation`, `protocol_identity_mismatch`, `stamped_key_in_emitted_receipt`, `uncounted_measurable_metric`, `unknown_path`, `value_out_of_vocabulary`, `wrong_type`.

### ROUTE_IDS

`daemon.jsonl.v1`, `evaluator.private.v1`, `mcp.rmcp.duplex.v1`, `ocr.document.v1`, `proxy.http.v1`, `session.episode.v1`, `stream.v1`, `structured.core.v1`, `text.clean_for_bench.v1`.

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
The executable roster and targeted proof receipts below determine the probe
count; there is no fixed-count acceptance target.
Rust probes run with `python3.13 scripts/bench/test_evidence_eval.py
--rust-mutation-proof` under the machine lease, modifying only evidence_route.rs
and restoring its exact committed bytes after each probe.
