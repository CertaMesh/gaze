# MCP operation ordering evidence

The native operation arm passes both scored-label contracts v2 and v1. Across
44 attempted operations per side, annotated leaked bytes fall from 40 to 0.
False positives, refusals, exact restores, and valid manifests do not change.
This is a finite capture of real `PiiEnvelope::dispatch`, not a completeness or
latency claim.

The base runtime is `f7a794f5668864d3c4336aaa878c6896e92fc009`. The measured
candidate is `df85d4b8e02705a2ac85b02fa3ec7353d0fb24dd`. Each runtime built the
same committed host against its own libraries and lockfile, then ran its own
`gaze setup` policy. Those policies were byte-identical. Both model bundles
have identical hashes and remained unchanged around both captures.

## Native operation results

These counts are the same under v2 and v1. Exact restore and valid manifest
counts refer to completed operations. Refused attempts remain in the population.

| Layer | Attempts per side | Leaked bytes | False-positive bytes | Refusals | Exact restores | Valid manifests |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| A | 6 | 0 → 0 | 0 → 0 | 0 → 0 | 6 → 6 | 6 → 6 |
| D | 16 | 0 → 0 | 180 → 180 | 2 → 2 | 14 → 14 | 14 → 14 |
| R | 22 | 40 → 0 | 30 → 30 | 4 → 4 | 18 → 18 | 18 → 18 |
| Total | 44 | 40 → 0 | 210 → 210 | 6 → 6 | 38 → 38 | 38 → 38 |

No case becomes newly refused. Restore failures and invalid manifests are zero
on both sides. The net annotated byte gain is 40 under each contract.

The 40 annotated bytes include 8 ASCII spaces and 32 other UTF-8 bytes. Two
cases exposed only one space each. The separate diagnostic preserves that
distinction. The frozen fixture and both scoring contracts were not changed
after either v2 capture. Native specification `mcp-operation-order-v2` adds
bare-filename counterweights and repeat cases beside those filenames. It
supersedes the v1 capture; both arms were recaptured together on v2.

## Text layers are unchanged by construction

C/A/D/R text inference was **not recaptured**. Under the explicit reuse ruling,
`text-path-identity.json.gz` records matching base and candidate SHA-256 values
for 2,005 repository inputs outside the isolated MCP change. This includes the
core, types, recognizers, assembly, CLI clean code, existing text drivers,
scorers, corpus inputs, contracts, dependency locks, and build configuration.
New native tools are not imported by the unchanged text modules.

The only changed existing benchmark fixture is rmcp route evidence. Its sole
change is the verified recognizer-event count from 1 to 3 for discovery. It is
not part of the C/A/D/R text corpus. The checker rejects any broader fixture
edit or unclassified file change. The CI input registry separately admits
exactly the twelve new native evidence paths, with no other registry change.
Document MCP integration assertions change only inside `cfg(test)`; their
production prefix is byte-identical. Foreign and malformed placeholders now
fail before invocation with `Protection(Provenance)`, a deliberate fail-closed
improvement over adopter-specific errors after invocation. Broad bare filenames
retain their original behavior.

The four derived scorecards label text layers `unchanged_by_construction` and
retain the actual native operation counts. They do not invent zero text counts
or present old observations as fresh inference. `gate.json` applies the native
v2 and v1 comparison after verifying the source identity. The final publication
runtime must also match the measured MCP runtime.

## Replay and verify

Check out this evidence revision. These commands run no inference and need no
model copies:

```sh
uv run --project scripts/bench --group comparison-test python \
  scripts/bench/mcp_operation_gate.py \
  --base-native docs/reference/benchmarks/gates/mcp-operation-order/base-native.json.gz \
  --candidate-native docs/reference/benchmarks/gates/mcp-operation-order/candidate-native.json.gz \
  --text-path-identity docs/reference/benchmarks/gates/mcp-operation-order/text-path-identity.json.gz \
  --publication-revision HEAD --output-dir target/mcp-evidence-replay
```

The gate independently replays the signed session manifests, typed JSON shape,
actual owner restore results, token identities, call snapshot hashes, and byte
coverage. It rejects altered scores or provenance. Arguments that reached a
tool still count if later response protection fails.

The public archives remove absolute invocation paths and replace model paths
in the review policy template. Actual captured policy, binary, model and source
digests remain intact. Protected leaves, signed manifests, and scores are
unchanged. The private originals remain available for local audit. Public
archives are replayable without the private paths.

## Approval and cleanup

The original orchestrator and load-coordinator GO approved leak-only captures
alongside #773, outside bench-machine, at nice 10. The standing recapture ruling
requires `load_1m < 12` and `memory_free_percent >= 60`, checked before each arm.
Base recorded `load_1m=4.20` and `memory_free_percent=86`.
Candidate recorded `load_1m=3.95` and `memory_free_percent=85`. The runs used
worker-owned model copies with distinct file identities from the installed
bundles. The same immutable copy set served the two arms serially. Hashes were
checked before and after each capture, then all private copies were deleted.
`authority-and-cleanup.json` records the scope and the local approval digest.

C/A/D/R is source-identical under the approved reuse rule. No new historical
text measurements or native historical-release rows are claimed here.
