# Baseline-lock DEV quality: negative result, no promotion

The baseline-lock candidate produced **36.13% fewer surviving PII bytes on 254 common rows** (884 fewer, 2447 to 1563), but added **856 false-positive bytes**, including **787 negative-example false-positive bytes**, and had **two fail-closed rows with 85 unavailable gold bytes**. The unchanged quality gates failed. No promotion, confirmation run or rerun follows this result.

This is **per-row actual-output count non-regression, not raw-byte containment**. Equal counts can conceal different leaked bytes. Exact corpus-byte containment is unavailable. Failed rows are unavailable, never protected successes.

## Measured identity and scope

Measured clean, signed source: `b94c546515b78914357a24dedcc09ce7f08f634b` in `agent/baseline-lock-quality-7414`.
Worker7414 performed one reviewed four-arm frozenDEV256 run following separate root grants for nine model-free stages, freeze, seven synthetic smoke requests, and DEV. Root7342 owns acceptance and closure. Reviewer7415 separately reviews actual metadata proof.

This report and its [JSON companion](2026-09-10-baseline-lock-dev-quality.json) are docs-only packaging after measurement. Original receipts remain bound to the measured HEAD; the final runtime source diff against that HEAD must be empty. The earlier [source contract](2026-09-10-baseline-lock-harness-source.md) is historical source-only preparation, not current runtime status. No SDK, model, core, assembly, scorer, threshold, gold or executable source changed during packaging.

## Common four-arm comparison

Every number below uses the same **254 completed reversible rows, 11231 gold bytes**. All four arms restore exactly **254/254**, with **zero deletion actions**.

| Arm | Surviving PII bytes | Full-span escapes | FP bytes | Negative FP bytes |
| --- | ---: | ---: | ---: | ---: |
| Control | 2447 | 370 | 2608 | 628 |
| Unrestricted Redact | 1591 | 244 | 3429 | 1415 |
| Semantic candidate | 1734 | 251 | 3239 | 1225 |
| Baseline-lock candidate | 1563 | 241 | 3464 | 1415 |

Control to lock: surviving bytes -884; full-span escapes -129; FP +856; negative FP +787. Across common rows, gross worse rows **0**, gross added surviving bytes **0**, gross added full-span escapes **0**. The count invariant passes; the separate quality result fails with `false_positive_or_negative_regression` and `additional_refusal_or_error`.

Lock versus semantic on the same rows reduces surviving bytes by 171 and full-span escapes by 10, while adding 225 FP bytes and 190 negative FP bytes. Lock versus unrestricted reduces surviving bytes by 28 and full-span escapes by 3, adds 35 FP bytes, and leaves negative FP unchanged. These comparisons do not excuse failed control-relative gates.

## Full planned availability, separate from common counts

Every arm has **256 planned rows and 11316 planned gold bytes**. Completed-only surviving counts below cannot credit unavailable rows as protected.

| Arm | Completed/planned rows | Measured/planned gold bytes | Completed surviving bytes | Fail-closed rows |
| --- | ---: | ---: | ---: | ---: |
| Control | 256/256 | 11316/11316 | 2463 | 0 |
| Unrestricted Redact | 254/256 | 11231/11316 | 1591 | 2 |
| Semantic candidate | 254/256 | 11231/11316 | 1734 | 2 |
| Baseline-lock candidate | 254/256 | 11231/11316 | 1563 | 2 |

Control has 256 exact restores. Each augmented arm has 254 exact restores and the same two fail-closed rows: `dataiku-test-3525` (49 gold bytes) and `dataiku-test-3477` (36). Their status is `fail_closed`, stage `clean`, reason `recognizer_detect`; underlying causes remain unknown. Their output counts and exact-restore fields remain null. All arms have zero unmeasured rows, completed-nonreversible rows and restore failures. No unavailable row enters the common denominator or passes vacuously.

The complete run used the reviewed single repetition, 1024 planned producer requests and 256 validator requests, zero warmups, no duplicate inference and no retry. All four producer populations reached 256 attempted rows. The unchanged output-proof-v1 scorer and existing Arrow take-before-Python-text loader were used; no confirmation2654 access or new frozen IDs.

## Audit, source-gold and model binding

Candidate audit binding is schema-valid, complete for all planned terminals and consistent with output outcomes; all three gates pass, unknown terminal rows 0. Typed refusal records complete failure evidence, not quality success. Duplicate/malformed records and sparse evidence remain rejection conditions. Candidate count-only audit is not the semantic arm's normalized interval schema and cannot establish raw corpus containment.

Frozen ID SHA256: `5a37e640fa43c3808a8d5aae2965a5c2e72eb815a0b7363fa82f89e3ff943427`.
Ordered ID digest: `3993b14aeded90b056854d29172de148424b6e7b0eb967395e6b78de2fbbce36`.
Freeze SHA256: `2b46d99548a0377c0e79e2e105419d8629c6f6a540d0c621d4c7c3b1860ba8de`.
Source-gold SHA256: `2ac74d55ca1612f3247929a7a5e61e654cabc75d25fa96330b0690da13bc5379`.
Smoke output SHA256: `5d28e2d3895aceb10cd00331e6a585987a072a76c327b9f1019e4df71c135914`.

The old cf62d278 bridge remains pinned at `273968f5638cac53c9528b61bc974230811acc700b3bc5742765b60873c6a403`; private patched-CoreML manifest is `f6d3c6ff73b7070ceba87e1afbbf97c5552b79f7e1fd1e048396ac6d409b2d7d`. Davlan/Kiji pins remain in the freeze and receipts. No worker7413 derivative was integrated.

Producer binary SHA256: `43cd622f5ff173283ff3efe173d80a985d4cccb222f1c6bb6b712e7ba6471ed1`.
Reference producer: `ec6e7d85857929f8ffbf61f4f0506f7d1837dc727e1a873f8a02f838020303f9`.
Validator: `384d53293ec000fd75b3ed8b3fc9f9d229d0d8b1b721f1a1464eec08566b1828`.

## Runtime receipts and retained helper failure

All twelve stage receipts bind the same clean measured HEAD and original absolute deadline **2026-09-10T13:44:00+00:00**, never reset or extended. Exact argv, selected environment, toolchain, source, output and log hashes are embedded in the JSON report. Builds use native Rust1.96, jobs2, locked/offline cached ORT, cleared Cargo target overrides and explicit target directories. Only the candidate producer requires the benchmark-baseline-lock feature; validator follows its unchanged functional route. The unchanged supervisor retains absolute and monotonic bounds and detached cleanup.

Receipt directory: `/Users/krishankoenig/Workspace/EmpireTwo/gaze-quality-7414/target/quality-7414/`; stage receipts are `<stage>.json`, with separate started receipts and logs.

- `workspace-bootstrap`: exit 0, 162.059529s, remaining 0; `0f34b9e13847347f604023f1134cf11b0da07e3d09b77469e7d622f1c9f30f1e`.
- `reference-build`: exit 0, 51.713068s, remaining 0; `18db65395997ed022fa332b75003d03432847fcfe2b8eb2ba8b824994fee79f1`.
- `producer-build`: exit 0, 33.525390s, remaining 0; `285e03539fda283b2d3ee0178b96d49d5f33ff9e4414c6b88a00357682ff75fa`.
- `validator-build`: exit 0, 48.008482s, remaining 0; `cef5d944b590a80e2117fb0cff1c751e2e4da1a2aee1feb11f926db5d20315f8`.
- `python-tests`: exit 0, 0.305384s, remaining 0; `f9f0ee33f25f479ec303842f4e6a205769f9370efbd428a04582dbffa1c1cfa2`.
- `runner-tests`: exit 0, 0.176595s, remaining 0; `746951627467b6268b970b0eb39c9eb12901eb401b8844bcc31ebfa6ebeb7996`.
- `observer-tests`: exit 0, 0.175841s, remaining 0; `eb19562b3cecdc4412883ad5917acd5943c616a344c797166f93cdf8191572ec`.
- `legacy-driver-tests`: exit 0, 0.177551s, remaining 0; `03634b2a9137b25f2b16f2857a88815271e0cc7dd5b1a5cf01a41eaa36cf0cc7`.
- `supervisor-tests`: exit 0, 6.284519s, remaining 0; `c09937a7e081170470ae653693df93b795aba1cface86f6346668866f72ab238`.
- `freeze`: exit 0, 1.313733s, remaining 0; `692c7b1130c4df15187660155f62d95a2c03f9f55cfc546754b7203a97840478`.
- `smoke`: exit 0, 75.656295s, remaining 0; `90322000d72ee3e753c04b1e4532a30610e802c4d5d23f29e3caf82fa3fa29a6`.
- `dev`: exit 1, 377.948281s, remaining 0; `504288f3ac40d0b5f694e2a8135e72e8abb43e279bee2eb5aaca760730411edc`.

The Python suites passed **85 tests**: new harness19, runner37, observer24, legacy driver3, supervisor2. Seven synthetic smoke requests passed four actual output/manifest/restore checks and three same-source reference semantic-response comparisons. Three augmentation arms passed exact synthetic raw12..33 coverage. Synthetic implementation coverage is not corpus-byte identity proof.

The first copy-preflight helper failed at13:27:47.931624UTC: system Python lacked `tomllib`. No frozen file was created and neither freeze nor smoke began. The failure record remains `copy-preflight-failure.json`. Root explicitly granted one corrected helper retry with `/Users/krishankoenig/Workspace/EmpireTwo/gaze-quality-7377/scripts/bench/.venv/bin/python`; `copy-preflight-success.json` preserves successful create-new copy/hash verification. No dependency install, source change, stage retry or deadline extension occurred. All nine earlier stage receipts remained unchanged.

DEV exited **1**, a valid negative quality result, after **377.948281 seconds**. The exact release scan at13:40:43.951509UTC was empty, clean source, owned remaining0. Lease was released and scratchpad6185 advanced to revision130 before packaging. No additional runtime occurred during packaging.

## Evidence and review

Full sanitized runtime proof: `target/quality-7414/dev-proof.json`, 798828 bytes, SHA256 `b5f2bb6a9d78cba778d742e7429f747537e2a5ef2a89ebd40a4c91c19a39a697`, MemPalace artifact `art_20260910T134440_ff704175daea`. It contains twelve receipts, four full planned output sidecars, comparisons, binding and a63-file manifest. The JSON companion retains exact receipt and file hashes without raw/canonical text or emitted token values. Positive and negative evidence remain intact.

Reliability: fewer leaked counts on common successful rows, but residual leakage, FP and failures preclude promotion. Reversibility: all common outputs restore exactly and no deletion actions occurred. Agentic-first: full planned request accounting preserves failure ownership. Trust: unchanged gates, typed audits, source/model/freeze custody and count limitations remain explicit. Adopter ergonomics: benchmark-only candidate, no public API or integration change.

Cleanup review: **Verdict: skip. Opportunity: none. Why:** results require documentation, not source cleanup or a speculative abstraction. **Scope:** two report files only; measured source and receipts remain unchanged. **Validation:** existing build/test/freeze/smoke/DEV evidence above; docs-only diff and signed DCO packaging are separately verified in the handoff.

Next action: Root7342 accepts the independent metadata review and decides disposition. No promotion, further inference, confirmation or rerun is authorized.

QUALITY DONE:
