# P0-908 NER Fail-Closed Design

## Decision

Use a fallible recognizer contract end to end:

```rust
Recognizer::detect(...) -> Result<Vec<Candidate>, DetectError>
```

The shared `DetectError` type lives in `gaze-types`. NER backend runtime
failures map to `DetectError::Backend`, registry aggregation returns `Result`,
and the pipeline aborts outbound redaction on recognizer failure.

## Blast Radius

- `gaze-types`: `Recognizer::detect` becomes fallible and exposes `DetectError`.
- `gaze`: `RecognizerRegistry::detect_all` and `detect_all_resolved` propagate
  errors; `pipeline::Error` gains a recognizer-detection variant.
- `gaze-recognizers`: regex, dictionary, anchored, and NER recognizers implement
  the fallible contract. NER no longer maps backend failure to an empty result.
- `gaze-cli`, `gaze-assembly`, and `gaze-mcp-core`: consume the existing core
  pipeline `Result`, so recognizer failures surface as core pipeline errors.

## Fail-Closed Proof

Backend failure is no longer representable as an empty candidate list at the
recognizer boundary. Registry detection short-circuits on `Err`, and pipeline
redaction uses that `Result` before translating spans, logging, or emitting
clean text. A NER backend failure therefore prevents partially cleaned output
from leaving the pipeline.

Long NER input is scanned through bounded overlapping chunks before backend
execution; chunk failures are propagated as recognizer errors.

## Long-Input Chunking Invariant

NER chunk windows are measured in the model tokenizer's real WordPiece token
offsets, not whitespace words. The ORT backend uses a 480-token payload budget,
leaving room for `[CLS]` and `[SEP]` under the 512-token model ceiling, and a
30-token overlap between adjacent windows.

The overlap is a security invariant, not a throughput knob:

```text
overlap_tokens >= longest detectable entity + margin
stride = budget - overlap
```

Current NER PII entities are assumed to be short in WordPiece space: personal
names are typically 2-4 tokens, and common location/organization spans are
well below the 30-token overlap. The margin protects entities that land on a
window edge and prevents a surname/given-name split from becoming a leak
surface. Spans are remapped to original byte offsets before overlap
de-duplication, so an entity detected in both windows emits one manifest span.

Residual risk remains for an entity longer than the overlap, especially long
organization names or pathological fragmented input. Pass-3 SafetyNet should
rescan the reassembled clean output as defense in depth for any boundary miss
that tokenizer-window overlap cannot catch.
