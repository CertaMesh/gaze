#!/usr/bin/env python3
"""Score Gaze and every compared tool on competitors' own benchmarks.

Report-only: these sets never inform Gaze rule design or tuning. The byte
and entity metrics come from the same `ComparisonMetrics` code as the main
comparison; tool configurations come from `compare.build_backend`, so a
competitor row here is the exact configuration of its main-corpus row.
Every gold label counts (no scored-label contract), as in the sealed sets.

Outputs: the aggregate JSON (--output) and, for scoring with each
benchmark's own evaluator, per-document character spans under
--predictions-dir. Predictions contain offsets and labels only, never text,
and stay out of the repository.
"""

from __future__ import annotations

import argparse
import dataclasses
import hashlib
import json
import re
import os
import platform
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable, Mapping, Sequence

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))

import compare  # noqa: E402
import pii_tracer  # noqa: E402
import backends  # noqa: E402
from cpu_contention import ForeignCpuSampler  # noqa: E402
import loaders  # noqa: E402
from comparison_metrics import ComparisonMetrics  # noqa: E402
import tagged_gaze  # noqa: E402

score = loaders.score
BENCHMARKS = ("presidio-research", "piibench-commercial", "pii-trace")
GAZE_ROWS = ("gaze-rules-only", "gaze-rules-ner", "gaze-full")
# compare.TOOLS plus the tool whose adapter lives beside the pinned comparison.
ROSTER = (*compare.TOOLS, pii_tracer.TOOL)
VENDOR_TUNED = HERE / "vendor-tuned.json"


TAGGED_ROW = re.compile(r"gaze-(v\d+\.\d+\.\d+)")


def tagged_row_version(name: str) -> str | None:
    """`gaze-v0.15.1` -> `v0.15.1`; None for every other row name."""
    match = TAGGED_ROW.fullmatch(name)
    return match.group(1) if match else None


def _git(root: Path, *args: str) -> str:
    return subprocess.check_output(["git", *args], cwd=root, text=True).strip()


BUILD_COMMAND = ("cargo", "build", "--locked", "-q", "-p", "gaze-recognizers",
                 "--example", "clean_for_bench", "--features", "safety-net-nym", "--release")


def _tool_version(root: Path, *command: str) -> str:
    return subprocess.check_output(command, cwd=root, text=True).strip()


def verify_release_checkout(tag: str, root: Path) -> dict[str, object]:
    """A tagged Gaze row is measured from a clean checkout of the tag itself.

    The tag is resolved as `refs/tags/<tag>` (never a branch of that name) and the checkout
    must be exactly that commit with no local changes.
    """
    commit = tagged_gaze.tag_commit(tag, compare.REPO)
    if _git(root, "rev-parse", "HEAD") != commit:
        raise SystemExit(f"{root} is not at the commit of tag {tag} ({commit[:12]})")
    if _git(root, "status", "--porcelain"):
        raise SystemExit(f"{root} has local changes; a tagged row needs a clean checkout")
    return {"tag": tag, "commit": commit, "crates_tree": _git(root, "rev-parse", "HEAD:crates")}


def build_tagged_binary(tag: str, root: Path, build_dir: Path) -> tuple[Path, dict[str, object]]:
    """Build the benchmark binary FROM the verified checkout into a fresh directory.

    A pre-existing build directory (or binary) is refused: an executable left by any earlier
    build could otherwise be attributed to the tag. The checkout must still be clean after the
    build. Returns the binary and the exact build record.
    """
    if build_dir.exists():
        raise SystemExit(f"{build_dir} already exists; a tagged row builds its binary in a fresh directory")
    env = {**os.environ, "CARGO_TARGET_DIR": str(build_dir)}
    subprocess.run(BUILD_COMMAND, cwd=root, env=env, check=True)
    binary = build_dir / "release" / "examples" / "clean_for_bench"
    if not binary.is_file():
        raise SystemExit(f"the {tag} build produced no {binary}")
    if _git(root, "status", "--porcelain"):
        raise SystemExit(f"building {tag} changed the checkout")
    return binary, {
        "command": " ".join(BUILD_COMMAND), "profile": "release",
        "rustc": _tool_version(root, "rustc", "-Vv"), "cargo": _tool_version(root, "cargo", "-V"),
        "cargo_lock_sha256": hashlib.sha256((root / "Cargo.lock").read_bytes()).hexdigest(),
        "rust_toolchain_sha256": hashlib.sha256((root / "rust-toolchain.toml").read_bytes()).hexdigest(),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
    }


def verify_tagged_pins(tag: str, policy: Path, ner_model_dir: Path) -> dict[str, object]:
    """The policy and both model bundles must equal the release's pinned digests."""
    import tomllib

    pins = tagged_gaze.RELEASE_PINS.get(tag)
    if pins is None:
        raise SystemExit(f"no pinned policy and model digests for {tag}; add them to tagged_gaze.RELEASE_PINS")
    raw = policy.read_bytes()
    normalized = compare.normalized_policy_sha256(policy, hashlib.sha256(raw).hexdigest())
    nym_dir = Path(tomllib.loads(raw.decode("utf-8"))["safety_net"]["nym"]["model_dir"])
    found = {
        "policy_home_normalized_sha256": normalized,
        "ner_model_tree_sha256": compare.digest_tree(ner_model_dir),
        "nym_model_tree_sha256": compare.digest_tree(nym_dir),
    }
    for key, expected in pins.items():
        if found[key] != expected:
            raise SystemExit(f"{tag}: {key} is {found[key]}, the pinned digest is {expected}")
    return {**found, "policy_sha256": hashlib.sha256(raw).hexdigest()}


def validate_tagged_args(args: argparse.Namespace) -> None:
    """A tagged row builds its own binary and is measured into a fresh report."""
    if args.tool or args.gaze_binary is not None or args.gaze_release_root is None or args.gaze_policy is None:
        raise SystemExit("--gaze-release-tag needs --gaze-release-root and --gaze-policy, "
                         "builds its own binary (no --gaze-binary) and takes no --tool")
    if args.output.exists():
        raise SystemExit(f"{args.output} exists; a tagged row is measured into a fresh report, never resumed")


def check_resume(report: dict[str, object], head: str, dirty: bool) -> None:
    """A resumed report keeps its recorded harness state only while that is still true."""
    if report["harness_revision"] != head or report["harness_dirty"] or dirty:
        raise SystemExit("resuming needs the harness at the report's recorded clean commit "
                         f"({str(report['harness_revision'])[:8]}); it is at {head[:8]}"
                         + (" with local changes" if dirty else ""))
    if any(tagged_row_version(name) for name in report["rows"]):
        raise SystemExit("a tagged row is never carried into a resumed report; measure it fresh")


def harness_state() -> tuple[str, bool]:
    """(HEAD, has local changes) of the harness repository, read now, every run."""
    return (_git(compare.REPO, "rev-parse", "HEAD"), bool(_git(compare.REPO, "status", "--porcelain")))


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def reproduction_record(prior: Path, name: str, release: dict[str, object]) -> dict[str, object]:
    """Compare this tagged run with an earlier one; refuse unless the predictions are identical."""
    before = json.loads(prior.read_text(encoding="utf-8"))["provenance"][name]["release"]
    for key in ("tag", "commit", "crates_tree", "policy_home_normalized_sha256",
                "ner_model_tree_sha256", "nym_model_tree_sha256", "prediction_sha256"):
        if before[key] != release[key]:
            raise SystemExit(f"{name} does not reproduce: {key} differs ({before[key]} vs {release[key]})")
    return {"prediction_sha256": before["prediction_sha256"],
            "binary_sha256": before["build"]["binary_sha256"],
            "harness_revision": before["measured_with"]["harness_revision"],
            "harness_dirty": before["measured_with"]["harness_dirty"]}


def vendor_tuned_entry(benchmark: str) -> dict[str, object]:
    """The vendor's best published setup for its own benchmark (vendor-tuned.json), or exit."""
    entry = json.loads(VENDOR_TUNED.read_text(encoding="utf-8")).get(benchmark)
    if entry is None:
        raise SystemExit(f"{benchmark} has no vendor-tuned setup; it keeps the declared best configuration")
    return entry


def tuned_mapping(entry: Mapping[str, object], mappings: Mapping[str, Mapping[str, Sequence[str]]],
                  benchmark: str) -> dict[str, tuple[str, ...]]:
    """The comparison's table for the tool's family plus the tuned setup's extra entities."""
    family_map = {**mappings[entry["family"]], **entry["extra_labels"]}
    return loaders.compose_mapping(entry["family"], family_map, benchmark)


def validate_vendor_tuned_args(args: argparse.Namespace) -> None:
    """A vendor-tuned row is scored alone, into a fresh report, from a vendor-env producer's findings."""
    if (args.tool or args.gaze_release_tag or args.tuned_raw is None or args.tuned_meta is None):
        raise SystemExit("--vendor-tuned needs --tuned-raw and --tuned-meta (tuned_presidio.py) "
                         "and takes no --tool or --gaze-release-tag")
    if args.output.exists():
        raise SystemExit(f"{args.output} exists; a vendor-tuned row is measured into a fresh report")


def identity_sha256(benchmark: str, identity: Mapping[str, object]) -> str:
    """The dataset digest a benchmark's identity pins (what own scorers must have read)."""
    return identity["test_5k_sha256"] if benchmark == "piibench-commercial" else identity["sha256"]


def measure_vendor_tuned(
    entry: Mapping[str, object], args: argparse.Namespace, splits: Mapping[str, Sequence[score.Document]],
    mapping: Mapping[str, Sequence[str]], common: frozenset[str], predictions_dir: Path,
    entry_identity: Mapping[str, object],
) -> tuple[dict[str, object], dict[str, object]]:
    """Score the tuned analyzer's findings like every other Presidio row.

    Overlaps resolve with the comparison's own pinned resolver (`resolved_presidio_spans`),
    then the spans go through the same ComparisonMetrics and label validation.
    """
    from presidio_analyzer import RecognizerResult
    from presidio_anonymizer import AnonymizerEngine

    meta = json.loads(args.tuned_meta.read_text(encoding="utf-8"))
    if meta.get("smoke_limit") or meta["raw_sha256"] != sha256_file(args.tuned_raw):
        raise SystemExit("the tuned metadata is a smoke run or does not describe --tuned-raw")
    if meta["presidio_research_commit"] != entry["commit"]:
        raise SystemExit("the tuned findings were produced at a different presidio-research commit")
    try:
        tagged_gaze.check_model_receipt(meta["openmed"], entry["model"], "the tuned producer's receipt")
    except ValueError as error:
        raise SystemExit(str(error)) from error
    if meta["dataset_sha256"] != identity_sha256(args.benchmark, entry_identity):
        raise SystemExit("the tuned findings were produced on a different dataset than the one scored")
    rows = [json.loads(line) for line in args.tuned_raw.read_text(encoding="utf-8").splitlines()]
    (documents,) = splits.values()
    if [row["index"] for row in rows] != list(range(len(documents))):
        raise SystemExit("the tuned findings do not cover every document in order")
    anonymizer, cells = AnonymizerEngine(), Cells(mapping, common)
    name = entry["row"]
    with (predictions_dir / f"{name}.test.jsonl").open("w", encoding="utf-8") as sink:
        for index, (document, row) in enumerate(zip(documents, rows)):
            found = [RecognizerResult(entity, start, end, score_) for start, end, entity, score_ in row["results"]]
            predictions = compare.resolved_presidio_spans(anonymizer, document.text, found)
            compare.validate_labels(predictions, mapping)
            cells.add(document, predictions)
            sink.write(json.dumps({"index": index, "spans": char_spans(document, predictions)}) + "\n")
    provenance = {
        "vendor_tuned": {
            key: entry[key] for key in ("setup", "source", "commit", "caption")
        } | {"producer": meta, "raw_sha256": meta["raw_sha256"],
             "prediction_sha256": sha256_file(predictions_dir / f"{name}.test.jsonl"),
             "resolver": {"anonymizer_version": compare.package_version("presidio-anonymizer"),
                          "strategy": "MERGE_SIMILAR_OR_CONTAINED"}},
    }
    return {"test": {**cells.result(), "latency": latency([])}}, provenance


def tool_family(name: str) -> str:
    for family in ("presidio", "gliner", "datafog-core", "scrubadub", "opf", "pii-tracer"):
        if name.startswith(family):
            return family
    if name.startswith("datafog-"):
        return "datafog-python"
    if name.startswith("gaze"):
        return "gaze"
    raise ValueError(f"unknown tool {name}")


def load_benchmark(args: argparse.Namespace) -> tuple[dict[str, list[score.Document]], dict[str, object]]:
    """Return {"test": documents} plus "validation" when the set publishes one."""
    if args.benchmark == "presidio-research":
        documents, identity = loaders.load_presidio_research(args.presidio_research_checkout)
        return {"test": documents}, identity
    if args.benchmark == "pii-trace":
        return loaders.load_pii_trace(args.pii_trace_data)
    splits, identity = loaders.load_piibench_commercial(args.piibench_data)
    return splits, identity


def preflight_sample(documents: Sequence[score.Document], per_source: int) -> list[score.Document]:
    taken: dict[str, int] = {}
    sample = []
    for document in documents:
        if taken.get(document.source_dataset, 0) < per_source:
            taken[document.source_dataset] = taken.get(document.source_dataset, 0) + 1
            sample.append(document)
    return sample


def common_intersection(composed: Mapping[str, Mapping[str, Sequence[str]]]) -> frozenset[str]:
    """Gold labels every compared system claims; unsupported classes drop out."""
    covered = [frozenset(label for labels in table.values() for label in labels) for table in composed.values()]
    return frozenset.intersection(*covered)


class Cells:
    def __init__(self, mapping: Mapping[str, Sequence[str]], common: frozenset[str]) -> None:
        typed = backends.typed_mapping(dict(mapping))
        self.views = {
            "product_coverage": ComparisonMetrics(mapping, None, typed, include_per_label_bytes=True),
            "common_intersection": ComparisonMetrics(mapping, common, typed),
        }

    def add(self, document: score.Document, predictions: Sequence[score.Span]) -> None:
        for view in self.views.values():
            view.add(document, predictions)

    def result(self) -> dict[str, object]:
        return {name: view.result() for name, view in self.views.items()}


def char_spans(document: score.Document, predictions: Sequence[score.Span]) -> list[list[object]]:
    """Byte offsets back to character offsets, for the benchmarks' own scorers."""
    offsets = score.char_to_byte_offsets(document.text)
    to_char = {byte: index for index, byte in enumerate(offsets)}
    return [[to_char[span.start], to_char[span.end], span.label] for span in predictions]


def measure_tool(
    name: str, predictor: Callable[[score.Document], list[score.Span]],
    splits: Mapping[str, Sequence[score.Document]], mapping: Mapping[str, Sequence[str]],
    common: frozenset[str], predictions_dir: Path,
) -> dict[str, object]:
    result: dict[str, object] = {}
    for split, documents in splits.items():
        cells, timers = Cells(mapping, common), []
        with (predictions_dir / f"{name}.{split}.jsonl").open("w", encoding="utf-8") as sink:
            for index, document in enumerate(documents):
                started = time.perf_counter()
                predictions = predictor(document)
                timers.append((time.perf_counter() - started) * 1000)
                if name == "opf":
                    # As compare.measure does: OPF's native secret is scored as custom:secret.
                    predictions = [dataclasses.replace(s, label="custom:secret") if s.label == "secret" else s
                                   for s in predictions]
                compare.validate_labels(predictions, mapping)
                cells.add(document, predictions)
                sink.write(json.dumps({"index": index, "spans": char_spans(document, predictions)}) + "\n")
        result[split] = {**cells.result(), "latency": latency(timers)}
    return result


def measure_gaze(
    name: str, args: argparse.Namespace, policy: Path,
    splits: Mapping[str, Sequence[score.Document]], mapping: Mapping[str, Sequence[str]],
    common: frozenset[str], predictions_dir: Path, diagnostics: Path,
) -> dict[str, object]:
    result: dict[str, object] = {}
    for split, documents in splits.items():
        cells = Cells(mapping, common)
        spans: dict[str, list[list[object]]] = {}

        def record(_config: str, document: score.Document, response: dict[str, object], _m: object) -> None:
            if "pipeline_error_code" in response:
                raise RuntimeError(f"{name} refused {document.uid}; refusal handling must be disclosed first")
            predictions = score.final_trace_predictions(document, response)
            compare.validate_labels(predictions, mapping)
            cells.add(document, predictions)
            spans[document.uid] = char_spans(document, predictions)

        run = score.run_config(
            compare.REPO, args.gaze_binary, "policy-file", documents, args.gaze_model_dir,
            None, None, None, 0.5, diagnostics / name / split,
            policy_path=policy, record_document=record,
        )
        with (predictions_dir / f"{name}.{split}.jsonl").open("w", encoding="utf-8") as sink:
            for index, document in enumerate(documents):
                sink.write(json.dumps({"index": index, "spans": spans[document.uid]}) + "\n")
        clean = run["latency_ms"]["clean_ms"]
        result[split] = {**cells.result(), "latency": {
            "p50_ms": round(clean["median"], 3), "p95_ms": round(clean["p95"], 3), "samples": len(documents)}}
    return result


# Tool families whose typed cells stay held: their label maps pass through labels
# the comparison's typed-scoring fix touched (collision families, secrets).
TYPED_HOLD_PATTERN = ("custom:family:", "secret", "password", "token", "credential")


def typed_hold(mappings: Mapping[str, Mapping[str, Sequence[str]]]) -> list[str]:
    return sorted(family for family, table in mappings.items()
                  if any(marker in label for label in table for marker in TYPED_HOLD_PATTERN))


DERIVED_CELLS = ("typed_entities", "char_level")


def rescore(report: dict, splits: Mapping[str, Sequence[score.Document]],
            composed: Mapping[str, Mapping[str, Sequence[str]]], common: frozenset[str],
            predictions_dir: Path) -> None:
    """Recompute every row from stored predictions; only derived cells may change.

    Typed-entity and character-level cells are derived from the same stored spans;
    every byte, entity-coverage and document count must come back identical."""
    for name, result in report["rows"].items():
        mapping = composed[tool_family(name)]
        for split, documents in splits.items():
            cells = Cells(mapping, common)
            rows = (predictions_dir / f"{name}.{split}.jsonl").read_text(encoding="utf-8").splitlines()
            if len(rows) != len(documents):
                raise SystemExit(f"{name}/{split}: stored predictions do not cover the documents")
            for document, line in zip(documents, rows):
                stored = json.loads(line)["spans"]
                cells.add(document, compare.byte_spans(document.text, [tuple(span) for span in stored]))
            fresh = cells.result()
            for view, values in fresh.items():
                old = result[split][view]
                moved = sorted(key for key in values if key not in DERIVED_CELLS and values[key] != old[key])
                if moved:
                    raise SystemExit(f"rescore moved non-typed metrics for {name}/{split}/{view}: {moved}")
                for key in DERIVED_CELLS:
                    old[key] = values[key]


def latency(timers: Sequence[float]) -> dict[str, object]:
    return {"p50_ms": round(score.percentile(timers, 0.5), 3) if timers else None,
            "p95_ms": round(score.percentile(timers, 0.95), 3) if timers else None,
            "samples": len(timers)}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--benchmark", choices=BENCHMARKS, required=True)
    parser.add_argument("--presidio-research-checkout", type=Path)
    parser.add_argument("--piibench-data", type=Path, help="piibench_commercial.py --output-dir")
    parser.add_argument("--pii-trace-data", type=Path, help="data/train.parquet of perplexity-ai/PII-TRACE")
    parser.add_argument("--tool", action="append", choices=[*ROSTER, *GAZE_ROWS])
    parser.add_argument("--vendor-tuned", action="store_true",
                        help="score the vendor's own tuned setup for this benchmark (vendor-tuned.json) as its row")
    parser.add_argument("--tuned-raw", type=Path, help="tuned_presidio.py --output")
    parser.add_argument("--tuned-meta", type=Path, help="tuned_presidio.py --meta")
    parser.add_argument("--gaze-release-tag", help="measure this release tag as row gaze-<tag> (only)")
    parser.add_argument("--gaze-release-root", type=Path, help="clean checkout of that tag; the binary is built from it")
    parser.add_argument("--gaze-build-dir", type=Path,
                        help="fresh CARGO_TARGET_DIR for the tagged build; must not exist (default: target/tagged-builds/<tag>-<commit>)")
    parser.add_argument("--reproduces", type=Path,
                        help="a prior tagged report; this run must produce byte-identical predictions")
    parser.add_argument("--gaze-binary", type=Path)
    parser.add_argument("--gaze-model-dir", type=Path)
    parser.add_argument("--gaze-policy-rules", type=Path)
    parser.add_argument("--gaze-policy-rules-ner", type=Path)
    parser.add_argument("--predictions-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--rescore", action="store_true",
                        help="recompute metrics from stored predictions with the pinned metric code; no models run")
    parser.add_argument("--preflight", type=int, metavar="N",
                        help="STEER 4 label preflight: first N documents per source; never published")
    backends.add_tool_arguments(parser)
    args = parser.parse_args()
    pinned = backends.verify_pinned_comparison()
    splits, identity = load_benchmark(args)
    if args.preflight:
        splits = {split: preflight_sample(documents, args.preflight) for split, documents in splits.items()}
    mappings = {**compare.load_mapping(), pii_tracer.TOOL: pii_tracer.load_label_map()}
    release = None
    if args.gaze_release_tag:
        validate_tagged_args(args)
        release = verify_release_checkout(args.gaze_release_tag, args.gaze_release_root)
        release.update(verify_tagged_pins(args.gaze_release_tag, args.gaze_policy, args.gaze_model_dir))
        build_dir = args.gaze_build_dir or (
            compare.REPO / "target" / "tagged-builds" / f"{args.gaze_release_tag}-{release['commit'][:12]}")
        args.gaze_binary, build = build_tagged_binary(args.gaze_release_tag, args.gaze_release_root, build_dir)
        release["build"] = build
        selected = [f"gaze-{args.gaze_release_tag}"]
    elif args.vendor_tuned:
        validate_vendor_tuned_args(args)
        tuned = vendor_tuned_entry(args.benchmark)
        selected = [tuned["row"]]
    else:
        selected = args.tool or [*GAZE_ROWS, *ROSTER]
    # The whole roster, not just --tool: the common intersection must not depend
    # on which subset one invocation runs (runs resume into one report).
    composed = {
        family: loaders.compose_mapping(family, mappings[family], args.benchmark)
        for family in sorted({tool_family(name) for name in (*GAZE_ROWS, *ROSTER)})
    }
    common = common_intersection(composed)
    args.predictions_dir.mkdir(parents=True, exist_ok=True)
    for family, table in composed.items():
        # Read by the benchmarks' own scorers when they replay these spans.
        (args.predictions_dir / f"labels.{family}.json").write_text(
            json.dumps({label: list(labels) for label, labels in table.items()}, indent=2) + "\n",
            encoding="utf-8")
    args.output_existed = args.output.exists()
    report = json.loads(args.output.read_text(encoding="utf-8")) if args.output.exists() else {
        "schema_version": 1, "benchmark": args.benchmark, "identity": identity,
        "report_only": "never used to design or tune Gaze rules",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "harness_revision": harness_state()[0], "harness_dirty": harness_state()[1],
        "gaze_crates_tree": compare.crates_tree("HEAD"),
        "label_maps_sha256": compare.digest_file(loaders.LABEL_MAPS),
        "comparison_revision": backends.COMPARISON_REVISION, "comparison_sha256": pinned,
        "chart_configs": backends.chart_configs(),
        "mapping_sha256": compare.digest_file(compare.MAP_PATH),
        "hardware": platform.platform(), "device": "cpu",
        "common_intersection_labels": sorted(common),
        "splits": {split: {"documents": len(docs), "ids_sha256": score.document_ids_digest([d.uid for d in docs])}
                   for split, docs in splits.items()},
        "rows": {}, "provenance": {}, "preflight": args.preflight,
    }
    if report["identity"] != identity or report["common_intersection_labels"] != sorted(common):
        raise SystemExit("--output was produced for a different benchmark identity or roster")
    head, dirty = harness_state()
    if args.output_existed:
        check_resume(report, head, dirty)
    report["typed_hold"] = typed_hold(mappings)
    if args.rescore:
        if not report["rows"]:
            raise SystemExit("--rescore needs a measured --output report")
        rescore(report, splits, composed, common, args.predictions_dir)
        report["rescored_with"] = {
            "comparison_revision": backends.COMPARISON_REVISION, "comparison_sha256": pinned,
            "harness_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=compare.REPO, text=True).strip(),
            "harness_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=compare.REPO, text=True).strip()),
            "at": datetime.now(timezone.utc).isoformat(),
        }
        compare.write_report(args.output, report)
        print(f"THEIRBENCH_RESCORED {args.benchmark} {len(report['rows'])} rows", file=sys.stderr)
        return 0
    with tempfile.TemporaryDirectory(prefix="gaze-theirbench-") as scratch:
        for name in selected:
            if name in report["rows"]:
                continue
            mapping = composed[tool_family(name)]
            if args.vendor_tuned:
                mapping = tuned_mapping(tuned, mappings, args.benchmark)
                (args.predictions_dir / f"labels.{name}.json").write_text(
                    json.dumps({label: list(labels) for label, labels in mapping.items()}, indent=2) + "\n",
                    encoding="utf-8")
                report["rows"][name], report["provenance"][name] = measure_vendor_tuned(
                    tuned, args, splits, mapping, common, args.predictions_dir, identity)
            elif name in GAZE_ROWS or tagged_row_version(name):
                policy = {"gaze-rules-only": args.gaze_policy_rules,
                          "gaze-rules-ner": args.gaze_policy_rules_ner,
                          "gaze-full": args.gaze_policy}.get(name, args.gaze_policy)
                if policy is None or args.gaze_binary is None or args.gaze_model_dir is None:
                    raise SystemExit(f"{name} needs --gaze-binary, --gaze-model-dir and its policy")
                with ForeignCpuSampler() as watch:
                    report["rows"][name] = measure_gaze(name, args, policy, splits, mapping, common,
                                                        args.predictions_dir, Path(scratch))
                report["provenance"][name] = {"policy_sha256_home_normalized": compare.normalized_policy_sha256(
                    policy, compare.digest_file(policy)), "cpu": watch.result()}
                if tagged_row_version(name):
                    release["prediction_sha256"] = sha256_file(args.predictions_dir / f"{name}.test.jsonl")
                    release["measured_with"] = {"harness_revision": head, "harness_dirty": dirty}
                    if args.reproduces:
                        release["reproduces"] = reproduction_record(args.reproduces, name, release)
                    report["provenance"][name]["release"] = release
            else:
                backend, provenance, _ = backends.build_backend(name, args, mappings, Path(scratch))
                if backend is None:
                    report["provenance"][name] = provenance
                    continue
                try:
                    backend.predict(score.Document("warmup", "alice@example.invalid", "en", "", "synthetic", ()))
                    with ForeignCpuSampler() as watch:
                        report["rows"][name] = measure_tool(name, backend.predict, splits, mapping, common,
                                                            args.predictions_dir)
                    # The merge ties this row's own-scorer result to these exact predictions.
                    report["provenance"][name] = {**provenance, "cpu": watch.result(), "prediction_sha256":
                                                  sha256_file(args.predictions_dir / f"{name}.test.jsonl")}
                finally:
                    if hasattr(backend, "close"):
                        backend.close()
            compare.write_report(args.output, report)
            print(f"THEIRBENCH_DONE {args.benchmark} {name}", file=sys.stderr, flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
