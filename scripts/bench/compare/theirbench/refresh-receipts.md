# Refresh a pre-receipt row

See [benchmark reproduction](README.md#reproduce) for the pinned runtime and source checkout setup.

Run each row into a fresh report and prediction directory on a clean checkout.
Set `PYTHONHASHSEED=0` before Python starts. Keep each tool's locked runtime and
pinned model paths from the comparison. Capture one `--benchmark` and `--tool`,
then run the same command with `--rescore`. Run that dataset's own evaluator on
the exact prediction file with `--system` and its composed label map.

Admit the row and record its numeric history:

```bash
uv run --project scripts/bench --python 3.13 python scripts/bench/compare/theirbench/refresh_receipts.py \
    --report "$REPORT" --own "$OWN_SCORER_RESULT" --tool "$TOOL"
uv run --project scripts/bench --python 3.13 python scripts/bench/compare/theirbench/render_theirbench.py render
```

The refresh command accepts only explicitly marked pre-receipt rows. It uses
`add_tool` and `check_own_result`, preserves all other rows, and records every
replaced numeric value. A changed quality metric needs `--metric-reason` with
an evidence-based explanation. Timing changes receive a fresh-capture note.
`refresh_receipts.py --check` verifies the generated history page.

The [receipt history](../../../../docs/reference/benchmarks/third-party-receipt-history.md)
records the campaign's fixed hash seed. The original Presidio runs did not
retain that seed or their predictions. To inspect equal-score type ties, capture
`presidio-en` on PIIBench with seeds 0 and 1 into separate directories, then run
`explain_presidio_order.py --first FILE --second FILE --data DIR --en-model DIR --output FILE`
in the comparison runtime. It rechecks changed type pairs against the pinned
analyzer and emits counts, labels and digests only. It refuses coordinate changes
or unequal-score pairs. This explains order sensitivity without exporting text
or choosing a seed to improve a benchmark score.

The README's retained Gaze baseline keeps its original receipt and source SHA.
The [panel manifest](../../../../docs/reference/benchmarks/readme-byte-panels.json)
pins the original aggregate's compressed snapshot, Git commit and blob separately
from the current aggregate SHA. A later aggregate refresh updates the current pin;
it must preserve the historical pin. The panel renderer verifies both files and
rejects changes to the retained Gaze row, dataset identity or release provenance.
