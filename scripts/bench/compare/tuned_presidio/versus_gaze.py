#!/usr/bin/env python3
"""Tuned Presidio beside Gaze on the same test half: leaked, false-positive and refused, per class.

Offline, after `tune.py select` has frozen the choices. Systems:

- the three tuned Presidio rows (replayed from the recordings, as `tune.py measure` scores them);
- a Gaze build measured by `measure_gaze_build.py` (`--gaze-build`, e.g. current main);
- the latest tagged release, from its committed observation records. Its agentic record was made
  on an older generator, so on A/D/R it covers the documents that generator made (a subset);
  every system is also scored on exactly that subset.

Documents a system refused are counted apart (Refused) and left out of its byte counts.
Writes JSON and a Markdown summary to `--out` (outside the repository): the public pages
show tagged releases only, so an untagged build's numbers stay out of the tree.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import corpus  # noqa: E402
import search  # noqa: E402
import tune  # noqa: E402

compare = corpus.compare
score = compare.score


def release_predictions(layers: dict[str, list]) -> tuple[str, dict[str, dict]]:
    """uid -> {"spans", "refused"} of the latest tagged release's shipped default arm."""
    sys.path.insert(0, str(HERE.parents[1]))
    import render_benchmark_doc as history_doc
    import scorecard_record as record

    history = history_doc.load_history(history_doc.DEFAULT_HISTORY)
    entry = [e for e in history["releases"] if not e.get("provisional")][-1]
    arm = history_doc.shipped_default_arm(entry)
    by_id = {d.uid: d for docs in layers.values() for d in docs}
    result = {}
    for block in (entry["observation_record"], entry["agentic_layers"]["observation_record"]):
        _, rows = record._read(tune.BENCH_DOCS / block["file"])
        for row in rows:
            if row["config"] != arm or row["document_id"] not in by_id:
                continue
            response = row["response"]
            if "pipeline_error_code" in response:
                result[row["document_id"]] = {"spans": [], "refused": response["pipeline_error_code"]}
            else:
                result[row["document_id"]] = {
                    "spans": score.final_trace_predictions(by_id[row["document_id"]], response), "refused": None}
    return entry["version"], result


def build_predictions(path: Path, layers: dict[str, list]) -> dict[str, dict]:
    result = {}
    for layer in corpus.LAYERS:
        for line in (path / f"{layer}.jsonl").read_text(encoding="utf-8").splitlines():
            row = json.loads(line)
            result[row["uid"]] = {"spans": [score.Span(s, e, label) for s, e, label in row["spans"]],
                                  "refused": row["refused"]}
    if set(result) != {d.uid for docs in layers.values() for d in docs}:
        raise SystemExit("the Gaze build spans do not cover the corpus")
    return result


def tally(system: dict[str, dict], layers: dict[str, list], contracts: dict, mapping: dict,
          only: set[str] | None) -> dict:
    """Test-half cells per contract and layer, plus per-class leaked bytes."""
    out: dict = {}
    for version, contract in contracts.items():
        out[version] = {}
        for layer, documents in layers.items():
            cell = {"documents": 0, "refused": 0, "leaked_bytes": 0, "false_positive_bytes": 0,
                    "per_class_leaked": defaultdict(int), "per_class_gold": defaultdict(int)}
            accumulator = score.MetricAccumulator()
            for document in documents:
                if corpus.split_for_id(document.uid) != "test" or (only is not None and document.uid not in only):
                    continue
                if document.uid not in system:
                    continue
                cell["documents"] += 1
                entry = system[document.uid]
                if entry["refused"]:
                    cell["refused"] += 1
                    continue
                applied = score.apply_scored_label_contract(
                    [document], contract["agentic"] if layer in {"A", "D", "R"} else contract["main"])[0]
                accumulator.add(compare.mapped_document(applied, mapping), entry["spans"])
                gold, ignored, retained = score.contract_scoring_view(applied, entry["spans"])
                predicted = score.subtract_intervals(
                    score.merge_intervals((s.start, s.end) for s in retained), ignored)
                for span in applied.spans:
                    length = span.end - span.start
                    cell["per_class_gold"][span.label] += length
                    cell["per_class_leaked"][span.label] += length - score.intersection_length(
                        [(span.start, span.end)], predicted)
            result = accumulator.result()
            cell["leaked_bytes"] = result["utf8_bytes"]["leaked"]
            cell["false_positive_bytes"] = result["utf8_bytes"]["false_positive"]
            cell["false_positive_bytes_after_gold_gap"] = result.get("gold_gap", {}).get(
                "false_positive_bytes_after_gold_gap")
            cell["per_class_leaked"] = dict(cell["per_class_leaked"])
            cell["per_class_gold"] = dict(cell["per_class_gold"])
            out[version][layer] = cell
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dataset", type=Path, required=True)
    parser.add_argument("--pool", type=Path, required=True)
    parser.add_argument("--authors", type=Path, required=True)
    parser.add_argument("--gaze-build", type=Path, required=True, help="measure_gaze_build.py --out")
    parser.add_argument("--gaze-build-name", default="Gaze main")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    tune.require_hash_seed()
    if args.out.resolve().is_relative_to(tune.REPO):
        raise SystemExit("write this outside the repository")
    comparison = corpus.read_comparison()
    selection = json.loads(tune.SELECTION.read_text(encoding="utf-8"))
    layers = corpus.load_measured(args.dataset, comparison)
    mapping = tune.tuned_mapping()
    gaze_mapping = compare.load_mapping()["gaze"]
    systems: dict[str, tuple[dict, dict]] = {}
    with corpus.measured_agentic_contract(comparison):
        agentic_contract = compare.agentic.load_contract(tune.REPO)
        contracts = {version: {"main": compare.runner.load_scored_label_contract(tune.REPO, path),
                               "agentic": agentic_contract} for version, path in compare.CONTRACTS.items()}
        found = search.read_pool(args.pool, tune.POOL_PASSES, "validation")
        found.update(search.read_pool(args.pool, tune.POOL_PASSES, "test"))
        docs = search.make_docs(layers, found, {"v3": contracts["v3"]["main"], "agentic": agentic_contract})
        replayer = search.Replayer(docs, mapping, mapping)
        for objective in ("leak-first", "f2"):
            choice = selection["choices"][objective]
            config = choice["finals"][choice["start"]]["config"]
            systems[tune.ROWS[objective]] = ({doc.uid: {"spans": replayer.predict(i, config), "refused": None}
                                              for i, doc in enumerate(docs)}, mapping)
        documents = {d.uid: d for docs_ in layers.values() for d in docs_}
        authors = tune.authors_predictions(args.authors, documents)
        systems[tune.ROWS["authors"]] = ({uid: {"spans": spans, "refused": None} for uid, spans in authors.items()},
                                         mapping)
        systems[args.gaze_build_name] = (build_predictions(args.gaze_build, layers), gaze_mapping)
        version, release = release_predictions(layers)
        systems[f"Gaze {version}"] = (release, gaze_mapping)
        subset = {uid for uid in release}
        report = {"split": "test", "gaze_build": json.loads((args.gaze_build / "meta.json").read_text()),
                  "release": version, "full": {}, "release_subset": {}}
        for name, (predictions, labels) in systems.items():
            if not name.startswith("Gaze v"):
                report["full"][name] = tally(predictions, layers, contracts, labels, None)
            report["release_subset"][name] = tally(predictions, layers, contracts, labels, subset)
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "versus-gaze.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    lines = []
    for view in ("full", "release_subset"):
        lines.append(f"## {view} (test half)")
        lines.append("| Contract | Layer | System | Docs | Refused | Leaked B | FP B | FP B after v3 gold gap |")
        lines.append("| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |")
        for version in ("v3", "v2", "v1"):
            for layer in corpus.LAYERS:
                for name, cells in report[view].items():
                    c = cells[version][layer]
                    gap = c["false_positive_bytes_after_gold_gap"] if version == "v3" else None
                    lines.append(f"| {version} | {layer} | {name} | {c['documents']:,} | {c['refused']:,} | "
                                 f"{c['leaked_bytes']:,} | {c['false_positive_bytes']:,} | "
                                 f"{'' if gap is None else f'{gap:,}'} |")
        lines.append("")
    lines.append("## Per-class leaked bytes, v3, full test half (gold bytes in brackets)")
    names = list(report["full"])
    lines.append("| Layer | Class | " + " | ".join(names) + " |")
    lines.append("| --- | --- | " + " | ".join("---:" for _ in names) + " |")
    for layer in corpus.LAYERS:
        classes = sorted(report["full"][names[0]]["v3"][layer]["per_class_gold"])
        for label in classes:
            gold = report["full"][names[0]]["v3"][layer]["per_class_gold"][label]
            lines.append(f"| {layer} | {label} ({gold:,}) | " + " | ".join(
                f"{report['full'][n]['v3'][layer]['per_class_leaked'].get(label, 0):,}" for n in names) + " |")
    (args.out / "versus-gaze.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
