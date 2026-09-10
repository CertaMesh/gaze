# Fourth-arm actual-output harness: source ready, no runtime authorization

Worker7414, project4, MemPalace identity krishans-macbook-pro-codex.
Root7342 alone owns machine allocation, review acceptance, patch application and loop closure.
Task: task_gaze_live_lock_quality_harness_20260910. BUILD source-only.

## Custody and authorized inputs

Own checkout: `/Users/krishankoenig/Workspace/EmpireTwo/gaze-quality-7414`.
Branch: `agent/baseline-lock-quality-7414`.
Exact accepted base: `b62ebb82a4b99092e866b421b637fe72534745ea`.
Parent: `4e756074c1e5d70e6b9d97aed752519e9648a39b`, containing measured runner ancestry `1cd67a27e1eab96bfcc84145a89fbdb1bbe99afa`.
Implementation commits: `0539d89ff0baf6952eecdc65daa0e6fc472d1531` and `0fb884616b6d061a1648c3607d0506cbf4056d3d`.
Source-review corrections follow the original packaging commit; the final handoff identifies the exact updated HEAD and patch SHA.
Any later runtime must build from that final clean checkout HEAD, not the earlier implementation HEAD.

Whoami was the first coordination call and identified worker7414/project4. Claimed the task through MemPalace.
Read ai-work-modes and local development workflow before isolated worktree creation.
Read full9415 final,9417,9418 and9431 revision3. Scratchpad9429 revision4 contains only its heading.
Root explicitly substituted the complete final local `~/.local/share/gaze-private/lock-7407-proof/REPORT.md` and accepted report artifact `art_20260910T123652_6207951f420d` plus9431rev3.
Local report is8432 bytes, SHA256 `a4528a0671c7f0527d36d0b5db9e4e879e729ac0c62f1beb97267a6ce62ceb06`, and equals fetched artifact content exactly.
This substitution is authoritative; no reconstruction from callbacks was used.

Fetched accepted patch `art_20260910T123652_9a68fd64a04c` equals the exact local `git diff --abbrev=7 4e756074..b62ebb82` content.
Independently hashed that diff: `dedd42a91d83bd3b2a291900ee8619269bfd76022d91c6cb21f72274f3ce6361`.
Donor gaze-lock-7407 remains clean at b62ebb82. Its six accepted commits have G signatures and DCO trailers.
No donor, SDK, model or core/assembly source was changed.

## Source changes

Only four executable/test files plus this report change:

- `scripts/bench/run_no_opf_benchmark.py`: three added lines register `pass2-ner-redact-baseline-lock-candidate` and add `benchmark-baseline-lock` only when selected. Existing three arms keep exact build feature selection and default configs are unchanged.
- `scripts/bench/baseline_lock_stage.py`: fresh required UTC deadline; fixed exact stage commands; clean pre/post HEAD plus tracked-source digest, selected environment, toolchain executable hashes, output/log hashes and create-new receipts. Reuses the unchanged tracked supervisor's `supervise(..., deadline=...)`, including absolute and monotonic caps and owned detached-descendant cleanup.
- `scripts/bench/baseline_lock_dev.py`: separate four-arm driver, original frozen loader and scorer reused. Distinct audit files per arm/phase/reference build, full planned rows, typed count-only lock-audit join, reference equivalence, genuine synthetic smoke, unchanged comparisons and extra per-row count invariant.
- `scripts/bench/test_baseline_lock_dev.py`:19 synthetic test methods written, not executed.

Source hashes:

| File | SHA256 |
| --- | --- |
| baseline_lock_dev.py | 55a5c5e35ce6f53f302fb199f28b86af1b919cd53017070aa14a2fab0ef33be2 |
| baseline_lock_stage.py | 4ce72ea77b9cc2a9a1fc46ada179b032e3cc702480e9434b823c22d6f08de3e7 |
| test_baseline_lock_dev.py | e35104eeb78fe7c759e41b96d0a0a1775ef204eff5ceed3c693816d99bf4766d |
| run_no_opf_benchmark.py | 01db3462fdf04894c32741278fbd04c7db5c176d6023e8c7e4ef80a17a42923e |

Unchanged tracked supervisor SHA256: `755a053768028c72db2312542056e087e69d7bbcf768bc1e7acda5f808b2f564`.
Unchanged prior driver SHA256: `972ea6cebc7f207cac7b60b33ec8cb587f9c46ade8d19679c3d5f3546b071f84`.
Unchanged scorer SHA256: `49bea540976907f38023d102ea82c1a0bd955d8d1e3e5442165838ebdf306241`.
The old supervisor CLI and old target stage are not invoked. No expired deadline is used as a default.

## Evidence semantics and unchanged gates

Arms, in order: pass2-ner; pass2-ner-redact; pass2-ner-redact-semantic-candidate; pass2-ner-redact-baseline-lock-candidate.
One repetition, zero warmups, one future four-arm frozen DEV256 only. No confirmation2654, promotion, threshold, gold, scorer or follow-up-run path.
Existing Arrow `take(indices)` precedes Python text conversion. The frozen ID file SHA, unique256 IDs, ordered digest and source-gold digest are checked before validator/producer inference. Negative JSONL loading retains the prior disclosed transient decoding behavior.

Every arm has full planned output rows before validator inference. Validator/transport failure retains unmeasured rows with null counts; invocation failure follows the unchanged runner's whole-arm invalidation. Audit joins are written even on early failure. Missing audit files produce unknown planned rows, not empty successful batches.
Pairwise unchanged comparator outputs preserve availability, same-row losses, common reversible denominators, surviving PII bytes, full-span escapes, non-PII FP, negative-row FP, exact restore and no-deletion requirements.
`common-four-arm.json` additionally uses the intersection of all four reversible populations while retaining each arm's full planned outcomes and unavailable gold bytes.

Root explicitly approved **per-row actual-output count non-regression**, not raw-byte containment.
Every common completed reversible candidate/control row must have candidate surviving bytes and full-span escapes no greater than control.
Unavailable rows have null verdict/deltas; an empty common population fails. Availability and original quality gates remain separate mandatory results.
Equal counts can conceal different leaked bytes. Exact corpus raw-byte containment is unavailable and must never be claimed.
The accepted implementation's synthetic actual manifest/raw coverage tests support a separate implementation invariant; they do not prove corpus-byte identity.
Baseline FPs remain locked; additions may add FP or refusals. Count non-regression cannot conceal or offset those costs.

The candidate audit is concretely `baseline-lock-candidate-v1`, request ordinal and lifecycle status; only `batch_complete` includes admitted/baseline_overlap/supplemental_overlap integers.
No coordinates or spans are inferred from these counts. Lifecycle counts must be absent; complete-empty has explicit zeros. Ordered begin/batch/terminal binding, exact keys/types,4096 candidate limit,1024-byte record limit and8MiB file bound are checked. Malformed records are not copied to output; valid preceding records and unknown remaining rows persist with schema_valid=false.
Missing final success cannot validate a successful output. Source inspection confirms successful producer publication follows final audit write+flush. A successful audit followed by failed stdout remains unmeasured output, never protected output.
Duplicate JSON object keys are rejected before schema validation, including duplicated ordinal, policy, status or count keys. The valid prefix and all unknown planned rows survive a rejected line.
Evidence binding separately reports schema validity, full planned terminal coverage and per-row terminal consistency. Sparse/malformed evidence cannot pass overall binding even when unknown outputs are unmeasured. Valid terminal refusals/errors can complete failure evidence; this never changes failed/unmeasured output into protection. A completed reversible output requires request_success; a fail_closed output requires request_refusal. An entirely refused but completely audited run can pass evidence binding while failing unchanged quality gates.
The semantic arm keeps its distinct existing normalized-span audit schema and joiner. No normalized semantic interval is treated as a raw lock interval or saved PII byte.

## Meaningful planned validation, not executed

The19 new synthetic methods cover refusal ordering, omitted lifecycle counts, complete-empty, missing final terminal, malformed/wrong-policy records, unknown fields, mismatched ordinals, duplicate batches, boolean/negative/over-cap counts, partial trailing records, missing audit, flushed-audit/failed-output separation and create-new path isolation. Literal duplicate-field JSON lines prevent a dict-based test from erasing its own falsifier. Sparse unknown/unmeasured rows, complete refusal evidence and malformed complete evidence exercise the separate completeness/consistency gate. Hostile inherited Cargo target overrides and command-to-hashed-output path consistency have a synthetic guard.
They also cover per-row regression hidden by aggregate improvement, extra FP, equal-count non-containment, unavailable/refused rows and empty common populations, the shared four-arm denominator, validator failure before any producer call, exactly four singleton arms/zero warmups, smoke missing/duplicate/stale/wrong-row/reference/flush failures, and stale receipt source/command/output/deadline/cleanup/log rejection.
Feature tests require exact unchanged argv for the prior Redact arms, ordinary pass2 fallback and marker only for the new arm.
Existing runner, actual-output observer, legacy driver and detached timeout/SIGINT supervisor suites are additional required stages.
No Python import, test, Cargo, rustc, Swift, model/inference, corpus materialization, lease or machine workload was executed in this phase.
Only source reads/edits, git/hash checks, signed commits and authorized coordination/report writes occurred. `git diff --check` passed; that is not test proof.

## Proposed bounded runtime contract, root authorization still required

Dependencies: independent source review and root acceptance of the final patch; root selects final reviewed model pin, allocates the machine, establishes one fresh absolute UTC deadline including the existing five-second cleanup reserve, and provides the existing frozen ID file at `target/quality-7414/frozen-dev.json`.
No frozen file was copied during source preparation. Existing bridge and private model must already be available. No new SDK/model bytes are included.
Existing bridge cf62d278 lineage remains pinned through the prior driver SHA `273968f5638cac53c9528b61bc974230811acc700b3bc5742765b60873c6a403`, and patched-CoreML manifest SHA `f6d3c6ff73b7070ceba87e1afbbf97c5552b79f7e1fd1e048396ac6d409b2d7d` with exact files/sizes/hashes. Davlan/Kiji validation is unchanged.
Worker7413's different Unicode mapping repair is not integrated or assumed complete. Prior driver supports a fixed pin, so no runtime provider polymorphism or new mutable pin input was introduced. A different model pin requires root's separate reviewed disposition before runtime.

Each stage is a separate durable root-owned invocation from this exact checkout:

```sh
python3 scripts/bench/baseline_lock_stage.py <stage> --deadline-utc '<ROOT_GRANTED_UTC_DEADLINE>'
```

The angle-bracket deadline is a documentation placeholder, not an executable grant. Every stage receives the identical deadline value.
Do not invoke the driver directly, bypass receipts, override resources/hooks or reuse earlier-run receipts.

1. `workspace-bootstrap`: normal workspace/all-features build, locked/offline/jobs2.
2. `reference-build`: same-checkout clean_for_bench with safety-net-kiji,redact-live in an isolated reference target directory.
3. `producer-build`: same-checkout producer with safety-net-kiji,redact-live,benchmark-baseline-lock.
4. `validator-build`: unchanged same-checkout validator functional route, locked/offline/jobs2, explicit dedicated native validator target directory. No experimental feature flag. Root explicitly superseded the initial validator-marker requirement: recognizers forwards its marker through a dev-dependency alias, while the validator uses recognizers as a normal dependency and has no wrapper caller. The marker was not a feature-activation or parity proof. Only the candidate producer requests benchmark-baseline-lock; synthetic producer-reference equivalence remains a separate gate.
5. Separate `python-tests`, `runner-tests`, `observer-tests`, `legacy-driver-tests`, `supervisor-tests` stages. All must pass on final clean source.
6. `freeze`: validate all nine stage receipts, binary/model/toolchain/source hashes, exact ID/source-gold contract; write exclusive freeze.
7. `smoke`: seven synthetic requests total, one per four feature-build arms and one per three reference-build arms. Compare every serialized semantic response field excluding timing in memory for the three reference arms. Actual Gaze observer validates manifest replay/restore; augmented outputs require exact raw12..33 tokenization and zero surviving gold. Original Redact arms require Redact provenance; lock may retain baseline provenance. Complete lock audit proves provider completion independently of which candidates survived. Exact arm set, one synthetic row each, freeze digest and successful smoke-stage output hashes are prerequisites for DEV. This is synthetic reference equivalence, not full-corpus byte equivalence.
8. `dev`: exactly1024 producer requests plus256 validator requests, no duplicate corpus inference. Persist full output proofs, audit joins, common denominator and unchanged comparisons. Exit1 can be a valid negative quality result and is retained; it is not a rerun instruction.
9. Root immediately verifies cleanup/owned_remaining0 and releases the machine, then harvests receipts and artifacts. No follow-up run or candidate promotion is automatic.

Environment retains Rust1.96.0 executable paths and hashes, jobs2, locked/offline builds, disabled ORT download and exact cached ORT path. PATH explicitly includes the toolchain, standard Homebrew and system tool locations. Audit destination/locale/Python optimization overrides and inherited CARGO_TARGET_DIR/CARGO_BUILD_TARGET are removed before child invocation. Workspace/producer explicitly target `target`, reference targets `target/quality-7414/reference-build`, validator targets `target/validator-recall-probe`; hashed paths match these native debug outputs. No new architecture is selected. This binds the declared selected environment and executable hashes, not every inherited OS/environment byte.
Every child stage is supervised using the original reviewed implementation, with a fresh explicit absolute deadline and its monotonic cap; owned separately grouped bridge descendants are cleaned even after normal parent exit. Bound metadata finalization follows child cleanup as in the existing custody design.
Build/freeze/smoke logs and outputs are hashed. DEV output hashes are retained even for negative/failing stages. Creation markers prevent a second attempt silently replacing the first.

## Five project axes

Reliability: unchanged actual-output and fail-closed gates plus per-row count checks; fewer escaped PII bytes remains unmeasured. A successful count check is not containment proof.
Reversibility: actual manifest replay and exact restoration remain required; deletion cannot win. Unavailable rows remain charged separately.
Agentic-first: full planned request order and typed lifecycle evidence preserve request ownership and distinguish refusal, unknown and complete-empty.
Trust: signed exact source, artifact/report equality, explicit candidate, bounded receipts and honest count-only diagnostic semantics. Runtime proof remains pending.
Ergonomics: original defaults/three-arm source paths remain unchanged; one explicit fourth arm and private driver. Extra custody code and a reference build are deliberate benchmark costs, not public API changes.

## Cleanup review

Verdict: implement.
Opportunity: the existing explicit arm registry, a small separate stage-command contract and concrete lock-audit parser.
Why: removes suffix-based candidate feature ambiguity, audit-file collisions, shifted request joins, false-zero lifecycle counts, stale custody and aggregate-only masking without changing the scorer or core ownership.
Scope: four benchmark source/test files and this report. Reuse existing frozen loader, scorer/comparator and reviewed supervisor. No general provider registry, new scorer API, core/assembly/SDK changes or speculative abstraction.
Validation: read-only source/caller review, accepted artifact/report exact equality, source hashes, signature/DCO/clean checks and whitespace checks. Nineteen new synthetic tests plus existing suites and all runtime stages remain unexecuted pending independent source review and root grant. Reviewer7415's duplicate-key, sparse-audit and Cargo target custody MUSTs are addressed in source; final independent acceptance remains pending.

Next action: root7342 harvest the exact source-ready patch and commission independent source review. Root alone grants runtime and closes loops.

SOURCE READY:
