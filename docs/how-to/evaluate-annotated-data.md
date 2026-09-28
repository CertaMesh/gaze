# Evaluate Gaze on your annotated data

`gaze eval` runs the same policy-driven text cleaning pipeline used by `gaze clean` and scores its protected raw-text spans against owner-supplied annotations. Run it only in the data owner's environment. The command prints aggregate counts and class names; it does not print source text, span contents, clean text, tokens, or document identifiers.

## Prepare JSONL

Put one JSON object on each line. `text` is the original text. Each `spans` item has a half-open `[start, end)` range in **UTF-8 bytes** and a Gaze policy class label (`email`, `name`, `location`, `organization`, or `custom:<name>`). Spans must be sorted, non-overlapping, nonempty, within the text, and start/end at UTF-8 character boundaries. A document with no PII uses `"spans": []`.

This dummy row illustrates the byte-offset schema; replace it with your owner-side annotations:

```json
{"text":"abc","spans":[{"start":0,"end":3,"label":"custom:term"}]}
```

If your annotation names differ, provide a JSON object mapping each name to a Gaze policy class. For example, `labels.json`:

```json
{"EMAIL_ADDRESS":"email","PERSON":"name"}
```

```sh
gaze eval annotated.jsonl --label-map labels.json --policy gaze.toml --locale en --json
```

Omit `--policy` to use the bundled `core` policy, as in policy-free `gaze clean`. `--locale` accepts a comma-separated priority chain. A policy's Nym safety net is loaded when configured and available; a missing bundle fails the run. Each document gets a fresh session. The command reads at most 64 MiB per input file and prints no partial report if a row fails validation.

The evaluator does not accept `gaze clean`'s explicit OpenAI Filter or safety-net registry flags. Scores from this command therefore describe the policy-selected pipeline, not a run that used those command-line overrides.

## Read the score

`pii_bytes` is the union of gold UTF-8 byte ranges. `predicted_bytes` is the union of Gaze's protected raw-text ranges. `true_positive_bytes` is their intersection. `leaked_bytes = pii_bytes - true_positive_bytes`; `false_positive_bytes = predicted_bytes - true_positive_bytes`. `document_leak_rate` is the fraction of documents with at least one unprotected gold byte; `zero_leak_document_rate` is its complement. Documents with no gold spans count as zero leak. Byte precision, recall, and F1 follow the benchmark's byte definitions, including its `0/0 = 1` convention.

The per-class table groups gold spans by their mapped class and reports gold, covered, and leaked bytes, plus entity and fully covered entity counts. Protection by *any* predicted class covers a gold byte, matching the benchmark's recall table. The table does not claim typed precision. This command scores supplied annotations only; unannotated PII can appear as false positives, so annotation completeness matters. It does not apply the benchmark's scored-label contracts, exclusions, or gold-gap credit.

For evaluation definitions and the published benchmark's separate contract, see [Benchmark methodology](../reference/benchmarks/README.md).
