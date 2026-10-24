# MCP operation ordering evidence

The native operation arm passes both scored-label contracts v2 and v1. Across
50 attempted operations per side, annotated leaked bytes fall from 40 to 0.
False positives, refusals, exact restores, and valid manifests do not change.
This is a finite capture of real `PiiEnvelope::dispatch`, not a completeness or
latency claim.

The base runtime is `f7a794f5668864d3c4336aaa878c6896e92fc009`. The measured
candidate is `6ba82c9684ad87b7c2fc8014abdba262fba83d86`. Each runtime built the
same committed host against its own libraries and lockfile, then ran its own
`gaze setup` policy. Those policies were byte-identical. Both model bundles
have identical hashes and remained unchanged around both captures.

## Native operation results

These counts are the same under v2 and v1. Exact restore and valid manifest
counts refer to completed operations. Refused attempts remain in the population.

| Layer | Attempts per side | Leaked bytes | False-positive bytes | Refusals | Exact restores | Valid manifests |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| A | 6 | 0 → 0 | 0 → 0 | 0 → 0 | 6 → 6 | 6 → 6 |
| D | 20 | 0 → 0 | 180 → 180 | 2 → 2 | 18 → 18 | 18 → 18 |
| R | 24 | 40 → 0 | 40 → 40 | 4 → 4 | 20 → 20 | 20 → 20 |
| Total | 50 | 40 → 0 | 220 → 220 | 6 → 6 | 44 → 44 | 44 → 44 |

No case becomes newly refused. Restore failures and invalid manifests are zero
on both sides. The net annotated byte gain is 40 under each contract.

The 40 annotated bytes include 8 ASCII spaces and 32 other UTF-8 bytes. Two
cases exposed only one space each. The separate diagnostic preserves that
distinction. The frozen fixture and both scoring contracts were not changed
after either v3 capture. Native specification `mcp-operation-order-v3` includes
bare-filename and other-namespace counterweights, plus repeats beside both.
It supersedes earlier captures; both arms were recaptured together on v3.

## Large owned-token compatibility

Full owned-token expansion has an aggregate budget of 16 times the existing
2 MiB sizing constant, currently 32 MiB. Literal input bytes count toward that
budget. Decoded input has no separate cap. The runtime expands every owned token
and applies the real Rule matcher. Only expansion above 32 MiB returns the
new `DispatchError::ResourceLimit` and discards staged state. The rmcp adapter
returns the stable `resource_limit` class without input details.

Four committed real-dispatch counterweights seed a deterministic Rule Name of
2,048 synthetic `X` bytes. They submit 1,025 copies of its owned token, separated
by spaces or adjacent with a final period, through arguments and protected responses.
All four pass on both the base and measured candidate, preserve token-only wire
text, and restore exactly. Restoring the 2 MiB expansion cap after committing the
tests makes all four fail. Above-budget tests assert `ResourceLimit` in both
directions, no partial session changes, and the required manifest lifecycle.
Committed contract tests also admit 2,097,153 public periods and a shrinking
owned-token input in both directions. A Rule-seeded pair separated by one space
keeps the base wire and adds no entry. Independent proof tests accept interior
whitespace and reject non-whitespace or outer raw text without invoking projection.
Single-leaf and two-leaf tests assert admission at 32 MiB minus one and exactly
32 MiB, then refusal one byte over, with rollback and manifest checks.
Mutation probes restore the decoded-input cap, gap projection, gap proof, and
missing rmcp mapping. Each produces executed assertion failures after the tests
were committed. These tests supplement the unchanged 50-case native specification.

## Text layers are unchanged by construction

C/A/D/R text inference was **not recaptured**. Under the explicit reuse ruling,
`text-path-identity.json.gz` records matching base and candidate SHA-256 values
for 2,002 repository inputs outside the isolated MCP change. This includes the
core, types, recognizers, assembly, CLI clean code, existing text drivers,
scorers, corpus inputs, contracts, dependency locks, and build configuration.
New native tools are not imported by the unchanged text modules. The required
CHANGELOG, MCP runtime documentation, and rmcp README edits have separate
before/after hashes in the identity record. The rmcp frontend exception admits
exactly the resource-limit arm and its assertion test, records both source hashes,
and rejects any other adapter edit. These files do not affect text inference.

The only changed existing benchmark fixture is rmcp route evidence. Its sole
change is the verified recognizer-event count from 1 to 3 for discovery. It is
not part of the C/A/D/R text corpus. The checker rejects any broader fixture
edit or unclassified file change. The CI input registry separately admits
exactly the twelve new native evidence paths, with no other registry change.
Document MCP and token-bridge code and tests match the base exactly. Apart from
the explicitly checked rmcp error mapping, crates outside MCP core are unchanged.

Only current-session token mappings expand. Other namespaces retain main's
literal-text behavior and their existing validation stage and error type.
The unchanged real token-bridge integration test proves its owner token passes.
MCP tests also prove current-session tokens still expand and cross-boundary Rule
repeats are covered. A separate opaque-carrier and foreign-namespace contract
remains a follow-up.

The host imports the signed current-session manifest and uses the runtime's
restore assessment. Other-namespace spellings remain literal. The independent
byte oracle never credits those opaque bytes as token coverage and rejects any
changed original. This arm does not validate another owner's namespace.

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
Base recorded `load_1m=5.29` and `memory_free_percent=85`.
Candidate recorded `load_1m=5.94` and `memory_free_percent=86`. The runs used
worker-owned model copies with distinct file identities from the installed
bundles. The same immutable copy set served the two arms serially. Hashes were
checked before and after each capture, then all private copies were deleted.
`authority-and-cleanup.json` records the scope and the local approval digest.

C/A/D/R is source-identical under the approved reuse rule. No new historical
text measurements or native historical-release rows are claimed here.
