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
  the fallible contract. NER maps neither backend failure nor malformed
  model output to an empty result (see Model Output Boundary).
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

## Model Output Boundary

The fallible contract above only holds if the backend actually reports a
failure. Between the ONNX session and the BIO decode there is a second
boundary -- the raw output tensor -- and a malformed tensor there must not be
read as "this document contains no PII".

`OrtBackend::detect` funnels every model result through one validation
function before any label selection, softmax, or span filtering runs. These
four conditions each fail closed with `NerRuntimeError::Output`, never with an
empty span list:

| Model output | Outcome |
| --- | --- |
| No output tensor at all | `Output("missing logits tensor")` |
| Rank/dimensions other than `[1, seq_len, num_labels]` | `Output("invalid logits tensor shape")` |
| Flat buffer length != `seq_len * num_labels` | `Output("invalid logits dimensions")` |
| Any non-finite value (`NaN`, `+Inf`, `-Inf`) | `Output("nonfinite logits")` |

The non-finite scan covers every value in the tensor, including `O` rows,
low-confidence rows, and special-token rows. Restricting it to the argmax
label or to above-threshold rows would let corruption hide behind exactly the
rows the decoder discards -- and `NaN` loses every `>` comparison in the
argmax fold, so a corrupt row silently reports `O` with maximum plausibility.

An empty result stays representable only where it is genuinely correct: an
empty token sequence, zero-width offsets, or a well-formed tensor whose spans
all fall outside the document.

This mirrors the Kiji DistilBERT safety-net decoder, which already rejects
invalid classifier width, mismatched offsets, bad logit length, and non-finite
values. The ORT NER path was the outlier, not the precedent.

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
