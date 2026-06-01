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
