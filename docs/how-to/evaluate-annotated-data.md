# Evaluate Gaze on your annotated data

Run `gaze eval` only in the owner's environment. It uses the `gaze clean`
pipeline and prints aggregate counts/classes, never source text, spans, clean
text, tokens, or document IDs.

## Prepare JSONL

One object per line: original `text` and `spans` with half-open `[start, end)`
UTF-8 byte offsets and policy labels (`email`, `name`, `location`, `organization`,
or `custom:<name>`). Spans must be sorted, nonempty, non-overlapping, within text,
and end/start on character boundaries. No PII: `"spans": []`.

Schema example:

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

Omit `--policy` to use the bundled `core` policy, as in policy-free `gaze clean`. `eval` accepts the same policy, context, NER, rulepack, session, and safety-net flags as `clean`, including `--context-json`, `--nym-model-dir`, and safety-net mode and fallback. `--locale` accepts a comma-separated priority chain. A policy's Nym safety net is loaded when configured and available; a missing bundle fails the run. Each document gets a fresh session. The command reads at most 64 MiB per input file and prints no partial report if a row fails validation.

Invalid rows return a typed `EvalSchema` error with the JSONL line number and no source text.

## Read the score

| Metric | Definition |
|---|---|
| `pii_bytes` | Union of gold UTF-8 byte ranges |
| `predicted_bytes` | Union of protected raw-text ranges |
| `true_positive_bytes` | Intersection of gold and protected ranges |
| `leaked_bytes` | Gold bytes minus true positives |
| `false_positive_bytes` | Predicted bytes minus true positives |
| `document_leak_rate` | Fraction of documents with any unprotected gold byte |
| `zero_leak_document_rate` | Complement of document leak rate; empty gold counts as zero leak |

Byte precision/recall use `0/0 = 1`. F1 is `0` when both are zero, and `1` when
both gold and predicted sets are empty. The benchmark uses a different F1
edge-case convention.

Per-class results group mapped gold labels: gold, covered, leaked bytes,
entities, and fully covered entities. Any predicted class covers a gold byte;
this is recall, not typed precision. Missing annotations can look like false
positives. `eval` applies no benchmark scored-label contracts, exclusions, or
gold-gap credit. See [benchmark methodology](../reference/benchmarks/README.md).
