# Inactive JWT payload supplemental oracle

This is source preparation for opt-in secrets protection, not an activated
benchmark layer or an accepted gate. The supplemental acceptance contract is
**UNDECIDED**. Nothing imports this module into the canonical runner or changes
its generator version, shipping policy, scored labels, scoring mechanics or
historical tables.

Each dev/test partition contains 152 positives, 152 matched benign counterweights
and eight controls. The 38 positive families are the 28 explicit cue spellings,
bare JWT, compact JSON `token`, compact JSON `api_key`, and unsupported multiline
cue with bare fallback, bare and cued prefixed JWT contexts, a dotted tail and an ellipsis. Four templates vary colon, equals, space and lexical
delimiters, quoting, records and punctuation. The synthetic personal claim is
only `{"sub":"alice@example.invalid"}`. Signature bytes name their synthetic
origin and partition. No detector supplies gold: `inserted` concatenates prefix,
value and suffix and records their exact UTF-8 byte offsets. Payload offsets
refer to the base64url segment, never to ordinary EMAIL gold.

Each matched benign case carries its expected unchanged outcome. Across the
matrix, short malformed segments and short build, order and code identifiers
challenge the cue or JSON/bare surface. They contain no personal claims. Complete
JWTs beside dots and hyphens are personal positives, never benign counterweights.
Long cued opaque runs remain credentials even if they contain no personal claim. The
separate controls preserve supported opaque cued credentials, published AWS
example shapes and a **bare** nonpersonal JWT; lowercase/short issuer and code
controls stay unchanged. These credentials are not FP merely because they lack
PII. A cued nonpersonal JWT would exercise the same whole-token defect, so the
unchanged JWT control deliberately uses the existing bare arm.

## Why the default gate cannot establish this gain

The setup policy excludes secrets. PII contract v2 excludes SECURITYTOKEN gold
and treats `custom:security_token` outside scored gold as neutral. The source
fix changes the existing opt-in recognizer, not that ruling. Default v1 also
cannot establish the gain when the secrets detector is absent. Identical fresh
setup-policy scorecards are necessary no-regression evidence but are blind to
this opt-in fix. AGENTS requires a demonstrated gain and says a blind benchmark
is not a pass. This separate payload oracle makes the defect visible without
relabeling transport bytes, changing contract credit or activating credentials.
Its existence does **not** resolve whether supplemental evidence satisfies that
requirement. Root and independent review own that decision and must escalate to
the user if applying it requires a substantive exception.

## Proposed supplemental criteria, awaiting decision

Measure fresh base/current-candidate builds using the exact same committed
corpus, native wrapper and explicit opt-in policy bytes, same machine and run
window. Bind build commits, immutable binary SHA-256, policy SHA-256, corpus
hashes and source hashes to every result. Native requests have only ID/text,
never gold. A handshake pins build commit, protocol and policy. Native responses
contain actual clean text, selected class/source, raw/clean manifest offsets,
per-token restore, strict document restore and snapshot export/import restore.
No models are configured. Dependencies may still require compilation.

Proposed conditions, reported per partition and kind:

1. Candidate personal documents protect every whole value in exactly one
   `custom:security_token` substitution sourced by `security_token.anchored`.
   Whole-value and encoded-payload leaked bytes are zero. At least one partition
   removes leaked whole-value or payload bytes versus base, and neither measure
   rises in any partition. Count whole-value bytes once for the FP/net comparison;
   payload is a separately reported overlapping view, not additive gain.
2. Benign value, cue, context and sentence punctuation bytes remain intact.
   Candidate benign FP and punctuation FP bytes do not rise in any partition.
   Added FP bytes across partitions must be strictly fewer than removed leaked
   whole-value bytes. Every control retains its worked expected behavior;
   supported credentials are exempt only on their inserted value, not context.
3. No refusals, invalid manifests, strict/imported restore failures or control
   failures in the candidate. Neither side may hide a leaked document behind a
   refusal: refused positives retain their full leak count. Exact-restore and
   valid-manifest counts cannot fall. Malformed/provenance-invalid records abort
   the run instead of yielding a partial successful scorecard.
4. Fresh native regression RED, candidate GREEN and assertion mutants remain
   required. Remove cue-JWT arm: native capture-selection test must fail, even
   though bare fallback may protect the payload. Reject dotted/hyphenated JWT context or drop capture 2: corresponding native
   recall/ownership tests must fail on assertions, not compiler errors. The Python assertion mutants
   separately prove partial-header leak, FP and restore checks are sensitive.
5. Fresh entire-PII default setup-policy v2 **and** v1 base/current-candidate
   no-regression runs remain required later. Use each side's own setup policy,
   identical corpus/seed/machine and report every AGENTS layer metric. All
   displayed historical releases must be remeasured with any adopted harness
   change, or the document must state why a tag cannot run. Source-only results,
   local-only coverage and old scorecards cannot satisfy this obligation.

`measure` collects evidence and returns nonzero for refusal/restore failure,
protocol errors, zero/missing/extra records, subprocess failure or pin drift.
Exit zero means collection completed, **not** supplemental or shipping acceptance.
The report always says `acceptance: UNDECIDED`; it has no pass/waiver switch.
Native command/build transcripts supply freshness and commit-to-binary custody;
the build-commit environment pin is an attestation, not proof by itself.

## Python-only preparation

The bench project supports Python 3.11 through 3.13. Its ordinary locked Python
bootstrap is `uv sync --project scripts/bench --locked`. No native tools are
needed for:

```sh
python3.11 -m unittest discover -s scripts/bench -p test_jwt_payload_oracle.py
python3.12 -m unittest discover -s scripts/bench -p test_jwt_payload_oracle.py
python3.13 -m unittest discover -s scripts/bench -p test_jwt_payload_oracle.py
python3.11 scripts/bench/jwt_payload_oracle.py validate
```

The tests use worked protocol observations and Python-only failing/empty
subprocesses. They never simulate a successful native executable. Three isolated
Python scoring mutants must fail their intended assertion. After editing a
pinned source, explicitly run `generate`, review corpus changes, then test.

## Future native commands, NOTRUN during preparation

Only the owner of the serial compile/measurement window may execute these.
In separate fresh base/candidate trees, overlay this exact probe source and
policy fixture if absent, preserving the tree's own detector code. Record the
source hashes and exact overlay diff. Compile each tree using its exact original
source commit as `GAZE_JWT_BUILD_COMMIT`; preserve build log and exit status.
Do not claim the overlay tree is clean or use the harness commit as detector base.

```sh
GAZE_JWT_BUILD_COMMIT=<exact-detector-commit> cargo build --locked -p gaze-recognizers --example jwt_payload_probe
# Copy the built probe into immutable per-side evidence paths; hash both copies.
python3.11 scripts/bench/jwt_payload_oracle.py measure \
  --base-bin <immutable-base-probe> --base-sha256 <base-binary-sha256> \
  --candidate-bin <immutable-candidate-probe> --candidate-sha256 <candidate-binary-sha256> \
  --base-commit <fresh-base-commit> --candidate-commit <current-candidate-commit> \
  --output <new-supplemental-report.json>
```

Run the existing native JWT regression/mutant plan separately. No Cargo, rustc,
Gaze, CLI, setup, models, OCR, detection measurement or historical remeasurement
was run for this preparation. No root allocation, policy activation, merge,
push, tag or gate acceptance is implied by these commands.
