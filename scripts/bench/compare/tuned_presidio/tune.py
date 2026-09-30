#!/usr/bin/env python3
"""Select tuned Presidio configurations on the validation half, then measure them.

    select   validation records and documents only; writes the search log and the choice
    measure  every document; replays the frozen choices and scores them with
             compare.measure, reproduces the comparison's presidio-all row from the same
             records (anchor), runs each choice live and requires identical output, and
             scores Presidio Research's own tuned analyzer; writes the public report

Both run in the comparison environment (../requirements.lock) with PYTHONHASHSEED=0.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import corpus  # noqa: E402
import pool  # noqa: E402
import search  # noqa: E402
import space  # noqa: E402

compare = corpus.compare
REPO = compare.REPO
BENCH_DOCS = REPO / "docs/reference/benchmarks"
REPORT = BENCH_DOCS / "presidio-tuned.json"
SELECTION = BENCH_DOCS / "presidio-tuned-selection.json"
SEARCH_LOG = BENCH_DOCS / "presidio-tuned-search-log.jsonl.gz"
POOL_PASSES = ("base", "dslim", "davlan", "openmed", "gliner")
ROWS = {
    "authors": "presidio-tuned-presidio-research",
    "leak-first": "presidio-tuned-own-leak-first",
    "f2": "presidio-tuned-own-f2",
}
VENDOR_TUNED = HERE.parent / "theirbench/vendor-tuned.json"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def tuned_mapping() -> dict[str, tuple[str, ...]]:
    """The comparison's presidio table, Presidio Research's extra entities, then the space's."""
    base = compare.load_mapping()["presidio"]
    extra = json.loads(VENDOR_TUNED.read_text(encoding="utf-8"))["presidio-research"]["extra_labels"]
    mapping = {**base, **{k: tuple(v) for k, v in extra.items()}}
    for entity, labels in space.EXTRA_LABELS.items():
        if entity in mapping:
            raise ValueError(f"space.EXTRA_LABELS redefines {entity}")
        mapping[entity] = tuple(labels)
    return mapping


def contracts() -> dict[str, object]:
    return {"v3": compare.runner.load_scored_label_contract(REPO, compare.CONTRACTS["v3"]),
            "agentic": compare.agentic.load_contract(REPO)}


def code_digests() -> dict[str, str]:
    return {name: sha256(HERE / name) for name in
            ("space.py", "pool.py", "search.py", "corpus.py", "produce.py", "tune.py")}


def git_state() -> dict[str, object]:
    return {
        "harness_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
        "harness_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=REPO, text=True).strip()),
    }


def require_hash_seed() -> None:
    if os.environ.get("PYTHONHASHSEED") != "0":
        raise SystemExit("set PYTHONHASHSEED=0: Presidio de-duplicates through a set")


def pool_meta(pool_dir: Path) -> dict[str, object]:
    metas = {}
    for name in POOL_PASSES:
        meta = json.loads((pool_dir / f"{name}.meta.json").read_text(encoding="utf-8"))
        if meta["space_sha256"] != sha256(HERE / "space.py"):
            raise SystemExit(f"pool pass {name} was recorded under a different search space")
        if meta.get("smoke"):
            raise SystemExit(f"pool pass {name} is a smoke run")
        if meta["harness_dirty"]:
            raise SystemExit(f"pool pass {name} was recorded from a dirty tree")
        metas[name] = meta
    return metas


# --------------------------------------------------------------------------
# select


def select(args: argparse.Namespace) -> None:
    """The whole selection. Reads `<pass>.validation.jsonl` and validation documents only."""
    require_hash_seed()
    comparison = corpus.read_comparison()
    metas = pool_meta(args.pool)
    for name, meta in metas.items():
        if sha256(args.pool / f"{name}.validation.jsonl") != meta["sha256"]["validation"]:
            raise SystemExit(f"{name}.validation.jsonl differs from its metadata")
    layers = corpus.validation_only(corpus.load_measured(args.dataset, comparison))
    with corpus.measured_agentic_contract(comparison):
        applied_contracts = contracts()
    found = search.read_pool(args.pool, POOL_PASSES, "validation")
    docs = search.make_docs(layers, found, applied_contracts)
    corpus.require_validation([doc.uid for doc in docs])
    mapping = tuned_mapping()
    replayer = search.Replayer(docs, mapping, compare.typed_mapping_for_contract(mapping, "v3"))
    log_rows: list[dict] = []
    choices = {}
    for objective in space.OBJECTIVES:
        finals = {}
        engine = search.Search(replayer, objective, log_rows.append)
        for start in space.STARTS:
            started = time.perf_counter()
            config, totals = engine.run(start)
            finals[start] = {"config": config, "validation": totals_dict(totals),
                             "seconds": round(time.perf_counter() - started, 1)}
            print(f"{objective}/{start}: {finals[start]['validation']}", file=sys.stderr, flush=True)
        best = min(space.STARTS, key=lambda s: search.objective_key(
            objective, list(finals[s]["validation"]["totals"])))
        choices[objective] = {"start": best, "finals": finals, "evaluations": engine.evaluations}
    SEARCH_LOG.parent.mkdir(parents=True, exist_ok=True)
    payload = "".join(json.dumps(row, sort_keys=True) + "\n" for row in log_rows).encode("utf-8")
    with SEARCH_LOG.open("wb") as raw, gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0) as handle:
        handle.write(payload)
    selection = {
        "schema_version": 1,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "split": "validation only: SHA-256 first byte of the document id < 128 (comparison_metrics.split_for_id)",
        "validation_documents": {layer: len(documents) for layer, documents in layers.items()},
        "objectives": space.OBJECTIVES,
        "starts": list(space.STARTS),
        "choices": choices,
        "candidates_evaluated": len([row for row in log_rows if not row["accepted"]]),
        "search_log": {"path": compare.portable_path(SEARCH_LOG), "sha256": sha256(SEARCH_LOG),
                       "rows": len(log_rows)},
        "pool": {name: {"validation_sha256": meta["sha256"]["validation"], "models": meta["models"],
                        "recorded_at_revision": meta["harness_revision"]} for name, meta in metas.items()},
        "code_sha256": code_digests(),
        "python_hash_seed": os.environ["PYTHONHASHSEED"],
        **git_state(),
    }
    SELECTION.write_text(json.dumps(selection, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def totals_dict(totals) -> dict[str, object]:
    return {"totals": list(totals), "leaked_bytes": totals[0], "false_positive_bytes": totals[1],
            "char_f2": search.f2_of(totals)}


# --------------------------------------------------------------------------
# measure


def replay_predictor(replayer: search.Replayer, config: dict, uid_index: dict[str, int]):
    def predict(document):
        return replayer.predict(uid_index[document.uid], config)
    return predict


def strip_latency(measured: dict) -> dict:
    """Replayed rows have no inference time of their own."""
    for layers in measured["contracts"].values():
        for cell in layers.values():
            cell["latency"] = {"p50_ms": None, "p95_ms": None, "samples": 0,
                               "status": "not measured: scored from recorded findings"}
    return measured


def anchor(measured: dict, comparison: dict) -> dict[str, object]:
    """The presidio-default start replayed must equal the committed presidio-all row byte for byte."""
    fields = ("leaked_bytes", "false_positive_bytes", "gold_gap_protected_bytes",
              "false_positive_bytes_after_gold_gap", "documents")
    committed = comparison["tools"]["presidio-all"]["contracts"]
    mismatches = [
        f"{version}/{layer}/{field}: {measured['contracts'][version][layer][field]} != {committed[version][layer][field]}"
        for version in compare.CONTRACTS for layer in corpus.LAYERS for field in fields
        if measured["contracts"][version][layer][field] != committed[version][layer][field]
    ]
    return {"row": "presidio-all", "equal": not mismatches, "mismatches": mismatches}


def live_verify(config: dict, paths: pool.ModelPaths, replayer: search.Replayer) -> dict[str, object]:
    analyzers = pool.live_analyzers(config, paths)
    differing, started = [], time.perf_counter()
    for index, doc in enumerate(replayer.docs):
        found = pool.live_analyze(analyzers, config, doc.document.text, doc.language)
        live = compare.resolved_presidio_spans(replayer.anonymizer, doc.document.text, found)
        if live != replayer.predict(index, config):
            differing.append(doc.uid)
        if (index + 1) % 500 == 0:
            print(f"live: {index + 1}/{len(replayer.docs)} ({len(differing)} differ)", file=sys.stderr, flush=True)
    return {"documents": len(replayer.docs), "differing_documents": len(differing),
            "differing_sample": differing[:20], "identical": not differing,
            "seconds": round(time.perf_counter() - started, 1)}


def authors_predictions(authors_dir: Path, documents: dict[str, object]) -> dict[str, list]:
    from presidio_analyzer import RecognizerResult
    from presidio_anonymizer import AnonymizerEngine

    anonymizer, spans = AnonymizerEngine(), {}
    for split in produce_splits():
        with (authors_dir / f"authors.{split}.jsonl").open(encoding="utf-8") as handle:
            for line in handle:
                row = json.loads(line)
                document = documents[row["uid"]]
                found = [RecognizerResult(entity, start, end, score) for start, end, entity, score in row["r"]]
                spans[row["uid"]] = compare.resolved_presidio_spans(anonymizer, document.text, found)
    return spans


def produce_splits() -> tuple[str, str]:
    return ("validation", "test")


def measure(args: argparse.Namespace) -> None:
    require_hash_seed()
    comparison = corpus.read_comparison()
    selection = json.loads(SELECTION.read_text(encoding="utf-8"))
    if selection["code_sha256"]["space.py"] != sha256(HERE / "space.py"):
        raise SystemExit("the selection was made under a different search space")
    metas = pool_meta(args.pool)
    for name, meta in metas.items():
        for split in produce_splits():
            if sha256(args.pool / f"{name}.{split}.jsonl") != meta["sha256"][split]:
                raise SystemExit(f"{name}.{split}.jsonl differs from its metadata")
        if meta["sha256"]["validation"] != selection["pool"][name]["validation_sha256"]:
            raise SystemExit(f"pool pass {name} is not the one selection read")
    authors_meta = json.loads((args.authors / "authors.meta.json").read_text(encoding="utf-8"))
    layers = corpus.load_measured(args.dataset, comparison)
    documents = {d.uid: d for docs in layers.values() for d in docs}
    mapping = tuned_mapping()
    common = frozenset(comparison["common_intersection_labels"])
    report_rows, provenance = {}, {}
    with corpus.measured_agentic_contract(comparison):
        applied_contracts = contracts()
        found = search.read_pool(args.pool, POOL_PASSES, "validation")
        found.update(search.read_pool(args.pool, POOL_PASSES, "test"))
        docs = search.make_docs(layers, found, applied_contracts)
        replayer = search.Replayer(docs, mapping, compare.typed_mapping_for_contract(mapping, "v3"))
        uid_index = {doc.uid: index for index, doc in enumerate(docs)}
        anchor_row = compare.measure("presidio-anchor", replay_predictor(replayer, search.start_config("presidio-default"), uid_index),
                                     layers, mapping, None, common)
        anchored = anchor(anchor_row, comparison)
        print(f"anchor: {anchored['equal']}", file=sys.stderr, flush=True)
        if not anchored["equal"] and not args.allow_anchor_mismatch:
            raise SystemExit("replay does not reproduce presidio-all: " + "; ".join(anchored["mismatches"][:10]))
        paths = pool.ModelPaths(
            spacy={language: getattr(args, f"{language}_model") for language in space.LANGUAGES},
            dslim=args.dslim, davlan=args.davlan, openmed=args.openmed, gliner=args.gliner)
        for objective in space.OBJECTIVES:
            config = selection["choices"][objective]["finals"][selection["choices"][objective]["start"]]["config"]
            name = ROWS[objective]
            measured = strip_latency(compare.measure(name, replay_predictor(replayer, config, uid_index),
                                                     layers, mapping, None, common))
            validation = [measured["contracts"]["v3"][layer]["metrics"]["product_coverage"]["validation"]
                          for layer in corpus.LAYERS]
            expected = selection["choices"][objective]["finals"][selection["choices"][objective]["start"]]["validation"]
            if (sum(v["leaked_bytes"] for v in validation), sum(v["false_positive_bytes"] for v in validation)) != (
                    expected["leaked_bytes"], expected["false_positive_bytes"]):
                raise SystemExit(f"{name}: measured validation totals differ from the search's")
            report_rows[name] = measured
            provenance[name] = {"objective": objective, "objective_rule": space.OBJECTIVES[objective],
                                "config": config, "live_verification": (
                                    live_verify(config, paths, replayer) if not args.skip_live else None)}
        authors_spans = authors_predictions(args.authors, documents)
        measured = strip_latency(compare.measure(ROWS["authors"], lambda d: authors_spans[d.uid],
                                                 layers, mapping, None, common))
        report_rows[ROWS["authors"]] = measured
        vendor = json.loads(VENDOR_TUNED.read_text(encoding="utf-8"))["presidio-research"]
        provenance[ROWS["authors"]] = {
            "setup": vendor["setup"], "source": vendor["source"], "commit": vendor["commit"],
            "producer": {key: authors_meta[key] for key in (
                "analyzer_kwargs", "openmed", "presidio_research_commit", "versions", "sha256", "documents")},
            "language": "every document analyzed with language=\"en\", the only language the setup supports",
            # Scored gold only: labels a contract excludes never count as missed.
            "coverage": authors_coverage(mapping, {span.label for doc in docs for span in doc.document.spans}),
        }
    report = {
        "schema_version": 1,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "description": "Presidio tuned by its authors for their corpus, and Presidio tuned for this corpus on its validation half",
        "corpus": comparison["corpus"],
        "contracts": comparison["contracts"],
        "common_intersection_labels": comparison["common_intersection_labels"],
        "comparison_sha256": sha256(corpus.COMPARISON),
        "selection_sha256": sha256(SELECTION),
        "mapping": {entity: list(labels) for entity, labels in sorted(mapping.items())},
        "anchor": anchored,
        "rows": report_rows,
        "provenance": provenance,
        "chart": chart_choice(report_rows),
        "budget": {**budget(), "candidates_evaluated": selection["candidates_evaluated"],
                   "validation_documents": selection["validation_documents"]},
        "code_sha256": code_digests(),
        "compare_sha256": sha256(Path(compare.__file__)),
        "comparison_metrics_sha256": sha256(HERE.parent / "comparison_metrics.py"),
        "scorer_sha256": sha256(REPO / "scripts/bench/gaze_bench_score.py"),
        "python_hash_seed": os.environ["PYTHONHASHSEED"],
        **git_state(),
    }
    REPORT.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def authors_coverage(mapping: dict[str, tuple[str, ...]], gold_labels: set[str]) -> dict[str, object]:
    """Where Presidio Research's tuned setup and this corpus's labels do not meet."""
    sys.path.insert(0, str(HERE.parent / "theirbench"))
    import presidio_research_repro as repro
    from presidio_analyzer import RecognizerRegistry

    registry = RecognizerRegistry()
    registry.load_predefined_recognizers()
    removed = set(repro.NOTEBOOK5_REMOVED)
    entities = set(repro.OPENMED_MAPPING.values()) | {"TITLE", "DATE_TIME", "AGE"}
    for recognizer in registry.recognizers:
        if type(recognizer).__name__ not in removed and recognizer.supported_language == "en":
            entities.update(recognizer.supported_entities)
    covered = {label for entity in entities for label in mapping[entity]}
    return {
        "entities": sorted(entities),
        "entities_without_a_corpus_label": sorted(e for e in entities if not mapping[e]),
        "corpus_labels_without_an_entity": sorted(gold_labels - covered),
    }


def budget() -> dict[str, object]:
    """Effort Gaze's rules received, in the terms the repository can count."""
    log = subprocess.check_output(
        ["git", "log", "--no-merges", "--format=%h %ad", "--date=short", "--",
         "crates/gaze-recognizers/embedded"], cwd=REPO, text=True).split("\n")
    commits = [line for line in log if line.strip()]
    return {"gaze_rulepack_commits": len(commits), "first": commits[-1].split()[1],
            "last": commits[0].split()[1], "path": "crates/gaze-recognizers/embedded"}


def chart_choice(rows: dict) -> dict[str, object]:
    """The better tuned row for the Presidio bar: higher test-half layer C v3 character F2.

    Choosing on the test half can only favour Presidio; ties keep the authors' setup.
    """
    def f2(name: str) -> float:
        return rows[name]["contracts"]["v3"]["C"]["metrics"]["product_coverage"]["test"]["char_level"]["f2"]
    order = [ROWS["authors"], ROWS["leak-first"], ROWS["f2"]]
    best = max(order, key=lambda name: (f2(name), -order.index(name)))
    return {"row": best, "rule": "highest test-half layer C v3 character-level F2 among the tuned rows",
            "test_f2": {name: f2(name) for name in order}}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    select_cmd = sub.add_parser("select")
    measure_cmd = sub.add_parser("measure")
    for command in (select_cmd, measure_cmd):
        command.add_argument("--dataset", type=Path, required=True)
        command.add_argument("--pool", type=Path, required=True, help="produce.py pool output directory")
    measure_cmd.add_argument("--authors", type=Path, required=True, help="produce.py authors output directory")
    for language in space.LANGUAGES:
        measure_cmd.add_argument(f"--{language}-model", type=Path, required=True)
    for name in ("dslim", "davlan", "openmed", "gliner"):
        measure_cmd.add_argument(f"--{name}", type=Path)
    measure_cmd.add_argument("--skip-live", action="store_true", help="development only; never published")
    measure_cmd.add_argument("--allow-anchor-mismatch", action="store_true", help="development only")
    args = parser.parse_args()
    (select if args.command == "select" else measure)(args)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
