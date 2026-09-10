# Experimental Redact identifier admission, source contract

Status: source implementation and scoped synthetic verification complete. No quality
improvement is established by this change. Root completed initial source review and
explicitly granted the bounded checks. Final independent review remains with root.
No model was used for these checks. No DEV or confirmation data was read by
this worker, and no scorer, gold labels, gates, SDK, or model files were changed.

## Scope and activation

The private `clean_for_bench` example adds two default-off configurations:
`rule-floor-redact-semantic-candidate` and `pass2-ner-redact-semantic-candidate`.
They require `redact-live`, `phone-parser`, Unix, the existing explicit Redact bridge
and model settings, and `GAZE_REDACT_ADMISSION_AUDIT_FILE` naming a new local file.
Existing `rule-floor-redact` and `pass2-ner-redact` remain unrestricted references.
The benchmark producer's stdout schema is unchanged.

The additive `gaze_assembly::build_pipeline_with_recognizer` seam uses the existing
counted registration path. It changes neither floor registration nor NER loading.
The candidate owns one persistent Redact detector. Its format-based recognizer
runs once with the complete document locale context, rather than once per fallback
locale. Candidate construction preserves legacy source IDs, score 1.0 routing,
priority, token family, classes and byte spans; no canonical value replaces input.

## Admission contract

The existing Redact fallible batch entrypoint validates the entire raw reply before
returning any detections: all 22 labels, finite and bounded scores, UTF-8 alignment,
span ordering and bounds, protocol metadata, window completeness and resource caps.
The candidate then checks the adapter's whole detection batch before evaluating any
semantic predicate. A later malformed span cannot be hidden by an earlier invalid
card. Detector errors remain request failures, never empty detections.

Only CREDIT_CARD and PHONE can be semantically excluded. All other 20 labels return
NotApplicable and remain candidates. The predicate has no counters, I/O, thresholds,
model calls, corpus lookup, or mutable policy state.

- Accept retains the exact candidate.
- NotApplicable retains the exact candidate.
- SemanticInvalid explicitly excludes and counts the structurally valid candidate.
- Error refuses the whole request, including all otherwise accepted candidates.

CREDIT_CARD calls the existing Luhn contract on ASCII digits, ASCII whitespace and
hyphens. Its 13–19 digit length rule and checksum are unchanged. Other separators or
Unicode still present in detector input are NotApplicable.

PHONE requires exactly one explicit de-DE or en-US document region, ignoring Global
fallback. Empty, bare-language, unsupported or conflicting region chains retain the
span. No country is inferred from a language or invented by the caller. National
validation reuses the existing Region mapping, assigned-number check and documented
synthetic fixture exceptions. Applicability permits only ASCII digits/whitespace and
`+ - . / ( )`; other characters retain the span.

For the pinned phonenumber 0.3.9 contract:

- Parsed valid supported forms Accept, including supported international fixtures.
- Parsed invalid national forms are SemanticInvalid only with the selected region.
- NoNumber, TooShortNsn and TooLong are SemanticInvalid for applicable national
  forms. They are data rejections, not execution failures.
- InvalidCountryCode and TooShortAfterIdd are NotApplicable.
- Failed `+` or IDD forms are NotApplicable. IDD detection uses the parser's existing
  region metadata, with no new dialing-prefix list or regex.
- MalformedInteger and unavailable required metadata are Error. The closed parser
  match forces explicit review if a future dependency adds error variants.

Existing `ValidatorKind::canonical_form`, its Option result, and validator-veto
behavior are unchanged. The feature-gated typed helper shares the original region
and fixture acceptance rule rather than copying that rule into the benchmark.

Applicability is on **Gaze-normalized detector input**. Gaze already maps fullwidth
ASCII and removes U+200C/U+200D before recognizers run. Original fullwidth forms can
therefore become applicable. Remaining unsupported Unicode is retained. Source tests
exercise valid/invalid fullwidth cards through actual pipeline normalization and
require original-byte exact restore for accepted masking. No original-Unicode
retention claim is made.

## Audit and measurement

The required JSONL sidecar is create-new, capped at 1 MiB per record and 64 MiB per
file. It contains no raw text, canonical values, content hashes, scores, fixture IDs,
or model/validator error text. Evidence includes normalized span endpoints, vendor
label, disposition, SemanticInvalid count and excluded byte count. Coordinates are
explicitly `detector_input_utf8`, never raw-document coordinates.

Output-only request_begin and terminal request_success/request_refusal/request_error
records bind monotonically numbered producer requests to intervening detector batch
records. Each invocation first writes batch_begin. Adapter-invalid and detector-error
records omit exclusion counts because no complete semantic batch was evaluated. The shared writer carries no locale or admission state. A request with no
batch differs from a complete empty batch or detector_error. Begin without a terminal
record marks an incomplete/unmeasured run. Audit writing, including the terminal
request record, must succeed before a success response can be emitted. I/O, encoding,
lock or resource-limit failures are closed typed failures with static codes.

A completed semantic batch followed by a later floor/NER/request failure is still a
request refusal. Its exclusions are not a false-positive win. Root's paired actual
output scoring must measure recall, false-positive bytes, availability and exact
restore together. Normalized exclusion spans are diagnostic evidence, not raw spans
or a replacement scorer.

Root reports that negative PHONE examples can have bare-language + Global context;
those deliberately remain NotApplicable. The prior diagnosis attributed 190 added
false-positive bytes to CREDIT_CARD and 149 to PHONE, with true-positive impact
unknown and 412 other added false-positive bytes outside this scope. This source
change is not a full quality fix and makes no measured improvement claim.

## Validation

Runtime source commit: `31bb13eb4302069c38987755e14a50600533b24b`, signed G and DCO.
Source readiness was reported before execution. After root's explicit grant, an owned
lease and terminal ran Rust 1.96, jobs=2, offline Cargo and explicit cached ORT. The
machine was released immediately after the final build/format checks. No model,
DEV/confirmation corpus or broad workspace gates ran.

All 26 scoped tests passed:

- gaze-types `phone_candidate`: 2/2.
- gaze-recognizers example `semantic_admission`: 11/11.
- gaze-assembly `injected_context_recognizer_counts_without_weakening_empty_floor_guard`: 1/1.
- gaze-recognizers library `redact_live::tests`: 4/4.
- existing gaze-recognizers `validator_veto` integration suite: 8/8.

Strict Clippy passed for the affected three libraries and private producer with
`-D warnings`; producer build passed. Both used `redact-live`, `safety-net-kiji` and
`safety-net-openai` features, matching available producer arms without invoking their
models. Scoped rustfmt and `git diff --check` passed. The minimal redact-only example
test compilation emitted four existing disabled-feature dead-code warnings; strict
Clippy with the producer feature set passed without suppressions. An initial compile
error from using phonenumber's private error module was corrected to its public
`ParseError` alias before the successful runs.

The built producer SHA256 is
`85de1a908bd8575599c81d20bd4c2c1b2788d372e8165972d9d932e4e5d21f70`.
The companion JSON receipt records exact commands, counts and local log digests.

Synthetic checks cover Luhn/separators, region+Global fallback, bare-language retention,
parser error mapping, other labels, late malformed spans, failed-as-empty prevention,
exclusion counters, actual floor and independent NER-shaped detector isolation,
fullwidth normalization, exact restore, audit request ordering and late audit-write
limits. The NER-shaped detector checks isolation, not real NER model quality.

Root's fresh-agent review and later paired recall/FP/availability/restore experiment
remain necessary before confirming any quality benefit.

## Structure and data-model review

1. Verdict: implement.
2. Opportunity: closed admission outcome plus one shared national acceptance helper;
   one output-only audit sink and the existing counted assembly registration seam.
3. Why: distinguish data rejection, unsupported applicability and execution failure;
   prevent shared country/fixture logic drifting; retain complete-batch validation
   before every exclusion and audit success before emitted success.
4. Scope: gaze-types helper, additive assembly seam, private benchmark candidate and
   synthetic test source. Existing runtime policy/defaults and reference arms remain
   unchanged. Larger raw-provenance or general semantic suppression changes wait.
5. Validation: source inspection, 26 scoped tests, strict Clippy, producer build,
   scoped formatting and diff whitespace checks passed. Final independent review and
   paired actual-output quality measurement remain with root.

Reliability risk: an experimental SemanticInvalid exclusion can expose true PII
outside these validators' supported semantic contract; root explicitly authorized
this candidate-only tradeoff, and recall impact remains unmeasured. Reversibility
uses unchanged manifests and is covered by passing synthetic round-trip tests.
Agentic fit and trust retain fail-closed batch/errors and text-free explicit evidence.
Adopter ergonomics is unchanged for public defaults; the candidate requires deliberate
private activation and a local sidecar. Correctness and availability must be checked
before any quality claim or wider use.
