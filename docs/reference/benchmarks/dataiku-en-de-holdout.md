# English/German Synthetic PII Holdout

## Decision

Gaze uses the English and German rows from the upstream test split of
[`DataikuNLP/kiji-pii-training-data`](https://huggingface.co/datasets/DataikuNLP/kiji-pii-training-data)
as its primary product-language benchmark. The split is evaluation-only: it
must never enter Gaze training, fine-tuning, prompt examples, dictionaries,
rule authoring, or threshold selection.

This replaces OpenPII as the headline English/German comparison surface. It
does not remove OpenPII: that corpus remains a secondary multilingual and
Unicode-offset stress test, including its Japanese slice.

The Dataiku dataset is a good primary fit because it is synthetic, Apache-2.0
licensed, ungated, and supplies exact value/start/end annotations for English
and German. The selected test rows cover 29 observed PII labels across nine
language-region combinations.

The word `kiji` in the dataset name does not mean this benchmark uses Gaze's
current Kiji SafetyNet model. Gaze's Kiji backend is the separately pinned
`onnx-community/distilbert-NER-ONNX` bundle. The benchmark runner records both
model directories independently.

## Immutable provenance

| Field | Pinned value |
| --- | --- |
| Repository | `DataikuNLP/kiji-pii-training-data` |
| Revision | `0275550f0b1f1b8f2dc9356fd31ac1c788b8228b` |
| File | `data/test-00000-of-00001.parquet` |
| License | `Apache-2.0` |
| Declared data kind | synthetic PII only |
| Full test rows | `5,150` |
| File bytes | `2,013,107` |
| SHA-256 | `916c63792345bf3c2e0888941b3d14526c43b7c7fe8af60e0d283fed71b1234d` |
| Selected English rows | `1,033` |
| Selected German rows | `853` |
| Selected annotations | `14,719` |
| Observed selected labels | `29` |
| Selection seed | Not applicable; the complete pinned EN/DE selection is used without sampling or shuffling |

The runner refuses any file whose byte size or digest differs. It also verifies
the full row count, validates every selected annotation boundary and annotated
substring, and converts the source character offsets to UTF-8 byte offsets
before scoring.

## Scoring contract

The primary question is whether annotated PII bytes would reach a downstream
LLM. Gold spans and Gaze prediction spans are merged before their intersection
is measured in UTF-8 bytes.

The whole-pipeline scorecard is non-compensating:

- safety: PII-byte recall, leaked bytes, full-entity recall, and zero-leak
  documents;
- reversibility: exact restored text must equal the original byte-for-byte;
- trust: every manifest span must be ordered, in bounds, restorable, and map to
  the corresponding raw value;
- availability: pipeline completion and typed error counts are reported
  separately; initial SafetyNet suspects show how often strict mode would
  reject a completed document;
- precision: false-positive bytes remain visible and cannot be traded away
  silently;
- latency: warm median and p95 are compared only after the correctness gates
  pass.

The three default cells are the deterministic floor, floor plus current Pass 2
NER, and the complete Pass 2 + Kiji SafetyNet pipeline using the shipped
`Resolve`/`Redact` policy. For the final cell, the scorer maps SafetyNet action
spans from pre-safety clean text back to the raw document, verifies exact
restore, validates the final manifest, and runs a post-policy SafetyNet scan.

An optional `full-stack-opf-resolve` cell exercises OpenAI Privacy Filter
through the same contract. It is excluded from the default because it requires
a separately installed verified checkpoint and warmed daemon. See
[`v0.12-opf-daemon-sample.md`](v0.12-opf-daemon-sample.md).

Run the pinned benchmark:

```bash
cd <repo-root>
uv run --with pyarrow scripts/bench/dataiku_en_de_gaze_bench.py
```

The first run downloads the pinned 2 MB Parquet file into ignored
`target/bench-data/`. Later runs can add `--no-download`. Generated result JSON
is written to `target/bench-data/dataiku-en-de/gaze-scorecard.json` by default.
The checked-in baseline normalizes local paths as `<repo-root>` and
`<model-cache>`; do not promote a generated absolute home-directory path into
benchmark evidence.

## Limits and contamination rule

Every selected row contains annotated PII. The split therefore measures
false-positive bytes only within positive documents and cannot replace a
negative-only corpus. Synthetic templates also underrepresent OCR errors,
streaming boundaries, JSON tool calls, tenant identifiers, and ambiguous
natural language.

The test split is isolated from training, but upstream train and test data may
share a generator and templates. If a future model uses another split from the
same repository, this score must be labelled same-generator evaluation and
cannot be its only promotion gate. Gaze still needs an independent synthetic
negative set and an agentic-domain holdout. A finance-domain secondary candidate
is
[`gretelai/synthetic_pii_finance_multilingual`](https://huggingface.co/datasets/gretelai/synthetic_pii_finance_multilingual),
whose English and German test files should be pinned and adapted before use.
