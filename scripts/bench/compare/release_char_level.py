#!/usr/bin/env python3
"""Character-level F-scores for tagged Gaze releases, from their committed records.

The comparison report scores an unreleased Gaze build and competitors; the panels
show tagged releases only. This tool measures each release the same way the
comparison scores a tool: it replays the release's committed observation record
(its final protection trace per document) over the same corpus, applies the
headline scored-label contract, and feeds the spans to `ComparisonMetrics`, so
the released Gaze and every competitor share one character-level scorer.

`record` needs the corpus (`--dataset`, the Dataiku holdout parquet) and writes
`docs/reference/benchmarks/release-char-level.json` plus a text-free evidence file,
`release-char-level-evidence.json.gz`: per layer C document the scored gold byte
intervals, the byte length, the positions of UTF-8 continuation bytes (so code
points can be counted without the text) and, per release, the contract-ignored
intervals. `check` needs neither corpus nor model: it hashes the evidence and the
committed observation record, RECOMPUTES every byte and character count from them
(trace spans from the record, intervals from the evidence), and requires them to
equal the stored row, the release history's byte counts and the release tag's tree.

    uv run --project scripts/bench python scripts/bench/compare/release_char_level.py \\
      record --dataset target/bench-data/dataiku-en-de/test.parquet
    python3 scripts/bench/compare/release_char_level.py check
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import subprocess
import sys
from pathlib import Path

BENCH = Path(__file__).resolve().parents[1]
REPO = BENCH.parents[1]
sys.path.insert(0, str(BENCH))
sys.path.insert(0, str(Path(__file__).resolve().parent))

import gaze_bench_score as score  # noqa: E402
import render_benchmark_doc as history_doc  # noqa: E402
import scorecard_record as record  # noqa: E402
from comparison_metrics import ComparisonMetrics  # noqa: E402
from tagged_gaze import require_release_tag, require_tag, tag_commit  # noqa: E402

BENCH_DIR = REPO / "docs/reference/benchmarks"
OUTPUT = BENCH_DIR / "release-char-level.json"
EVIDENCE = BENCH_DIR / "release-char-level-evidence.json.gz"
METRICS = Path(__file__).with_name("comparison_metrics.py")
CONTRACT = "v3"
SCHEMA_VERSION = 1


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def observation_of(entry: dict) -> dict | None:
    """The release's committed observation record, wherever its row keeps it."""
    if entry.get("observation_record"):
        return entry["observation_record"]
    return next(
        (r["observation_record"] for r in entry.get("contract_results", ()) if r.get("observation_record")),
        None,
    )


class GitError(ValueError):
    """A git object the check needs is missing (for example, tags were not fetched)."""


def _crates_of(commit: str) -> str:
    try:
        return subprocess.check_output(
            ["git", "rev-parse", f"{commit}:crates"], cwd=REPO, text=True, stderr=subprocess.PIPE
        ).strip()
    except (subprocess.CalledProcessError, FileNotFoundError) as error:
        detail = error.stderr.strip() if isinstance(error, subprocess.CalledProcessError) else str(error)
        raise GitError(f"cannot read crates/ of {commit[:12]} ({detail})") from error


def tag_crates_tree(version: str) -> str:
    """The `crates/` tree of the commit the release TAG points at.

    The tag is resolved as `refs/tags/<version>` (`tagged_gaze.tag_commit`), never as a
    bare name, so a version-shaped branch cannot stand in for a release.
    """
    try:
        return _crates_of(tag_commit(version))
    except ValueError as error:  # UntaggedGazeError or GitError
        raise GitError(str(error)) from error


def commit_crates_tree(commit: str) -> str:
    """The `crates/` tree of a commit sha (recording only; needs the commit locally)."""
    return _crates_of(commit)


def _release_records(history: dict) -> list[dict]:
    """Every non-provisional release that has a committed observation record."""
    return [
        entry for entry in history["releases"]
        if observation_of(entry) is not None and not entry.get("provisional")
    ]


def continuation_intervals(text: str) -> list[list[int]]:
    """Merged byte intervals of UTF-8 continuation bytes: code points = bytes minus these."""
    out: list[list[int]] = []
    for offset, byte in enumerate(text.encode("utf-8")):
        if (byte & 0xC0) == 0x80:
            if out and out[-1][1] == offset:
                out[-1][1] = offset + 1
            else:
                out.append([offset, offset + 1])
    return out


def _pairs(intervals) -> list[tuple[int, int]]:
    return [(int(start), int(end)) for start, end in intervals]


def counts_from_evidence(spans, gold, ignored, cont, size) -> dict:
    """Byte and code-point counts of one document, from intervals only (no text).

    Mirrors `ComparisonMetrics.add`: predictions wholly inside ignored bytes are dropped,
    the rest merged and stripped of ignored bytes; characters are bytes minus the
    continuation bytes inside the interval.
    """
    gold = _pairs(gold)
    ignored = _pairs(ignored)
    cont = _pairs(cont)
    retained = [
        span for span in spans
        if not ignored or not score.interval_is_covered(span, ignored)
    ]
    predicted = score.subtract_intervals(score.merge_intervals(retained), ignored)
    leaked = score.subtract_intervals(gold, predicted)
    tp_bytes = score.intersection_length(gold, predicted)

    def chars(intervals) -> int:
        return score.interval_length(intervals) - score.intersection_length(intervals, cont)

    tp_chars = chars(gold) - chars(leaked)
    return {
        "leaked_bytes": score.interval_length(gold) - tp_bytes,
        "false_positive_bytes": score.interval_length(predicted) - tp_bytes,
        "total_bytes": size - score.interval_length(ignored),
        "tp": tp_chars,
        "fn": chars(leaked),
        "fp": chars(predicted) - tp_chars,
    }


def recompute(entry: dict, evidence: dict, bench_dir: Path) -> dict:
    """Recount one release from its committed observation record and the evidence."""
    version, arm = entry["version"], history_doc.shipped_default_arm(entry)
    _, observations = record._read(bench_dir / observation_of(entry)["file"])
    ignored = evidence["ignored"].get(version, {})
    total = {"leaked_bytes": 0, "false_positive_bytes": 0, "total_bytes": 0, "tp": 0, "fn": 0, "fp": 0}
    seen = set()
    for row in observations:
        if row["layer"] != "C" or row["config"] != arm:
            continue
        uid = row["document_id"]
        document = evidence["documents"].get(uid)
        if document is None:
            raise ValueError(f"{version}: record document {uid} has no evidence")
        spans = [
            (item["raw_start"], item["raw_end"])
            for item in row["response"]["final_protection_trace"]
        ]
        counts = counts_from_evidence(
            spans, document["gold"], ignored.get(uid, ()), document["cont"], document["size"]
        )
        for key in total:
            total[key] += counts[key]
        seen.add(uid)
    if seen != set(evidence["documents"]):
        raise ValueError(f"{version}: the record and the evidence cover different documents")
    return total


def check_evidence_structure(evidence: dict) -> None:
    """Interval shapes an honest recording always has; a forged `cont` cannot fake them.

    Continuation bytes of one UTF-8 character number 1 to 3 and follow their lead byte,
    so each merged run is 1 to 3 bytes long, never at byte 0, and inside the document.
    Gold and ignored intervals must be sorted, disjoint and inside the document too.
    """
    for uid, document in evidence["documents"].items():
        size = document["size"]
        previous_end = 0
        for start, end in document["cont"]:
            if not (1 <= end - start <= 3):
                raise ValueError(
                    f"{uid}: continuation run [{start}, {end}) is not 1-3 bytes; UTF-8 "
                    "characters have at most 3 continuation bytes"
                )
            if start < 1 or end > size or (previous_end and start <= previous_end):
                raise ValueError(f"{uid}: continuation run [{start}, {end}) has no lead byte or leaves the document")
            previous_end = end
        for name, intervals in (("gold", document["gold"]),):
            _check_intervals(uid, name, intervals, size)
    for version, documents in evidence["ignored"].items():
        for uid, intervals in documents.items():
            _check_intervals(f"{version}/{uid}", "ignored", intervals, evidence["documents"][uid]["size"])


def _check_intervals(where: str, name: str, intervals, size: int) -> None:
    previous = 0
    for start, end in intervals:
        if not (previous <= start < end <= size):
            raise ValueError(f"{where}: {name} interval [{start}, {end}) is unsorted, overlapping or outside the document")
        previous = end


def write_evidence(path: Path, evidence: dict) -> str:
    """Deterministic gzip JSON (no file name, fixed mtime, sorted keys); returns its SHA-256."""
    payload = json.dumps(evidence, sort_keys=True, separators=(",", ":")).encode("utf-8")
    # No stored file name and a fixed mtime: the bytes depend on the content only.
    with path.open("wb") as raw, gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0) as handle:
        handle.write(payload)
    return _sha256(path)


def load_evidence(path: Path) -> dict:
    with gzip.open(path, "rb") as handle:
        return json.loads(handle.read())


def measure_release(entry: dict, layer_c: list[score.Document], mapping: dict,
                    typed_mapping: dict, evidence: dict) -> dict:
    """Character-level metrics of the release's default arm on layer C under v3."""
    import compare  # noqa: E402  (imports the corpus stack lazily)

    contract = compare.runner.load_scored_label_contract(REPO, compare.CONTRACTS[CONTRACT])
    arm = history_doc.shipped_default_arm(entry)
    observation = observation_of(entry)
    path = BENCH_DIR / observation["file"]
    _, observations = record._read(path)
    by_id = {document.uid: document for document in layer_c}
    cell = ComparisonMetrics(mapping, None, typed_mapping)
    seen = 0
    for row in observations:
        if row["layer"] != "C" or row["config"] != arm:
            continue
        response = row["response"]
        if "pipeline_error_code" in response:
            raise ValueError(f"{entry['version']} refused {row['document_id']}")
        document = by_id[row["document_id"]]
        predictions = score.final_trace_predictions(document, response)
        compare.validate_labels(predictions, mapping)
        applied = score.apply_scored_label_contract([document], contract)[0]
        cell.add(applied, predictions)
        gold, ignored, _retained = score.contract_scoring_view(applied, predictions)
        evidence["documents"].setdefault(document.uid, {
            "gold": [list(pair) for pair in gold],
            "cont": continuation_intervals(document.text),
            "size": len(document.text.encode("utf-8")),
        })
        if ignored:
            evidence["ignored"].setdefault(entry["version"], {})[document.uid] = [list(p) for p in ignored]
        seen += 1
    if seen != len(layer_c):
        raise ValueError(f"{entry['version']}: record covers {seen} of {len(layer_c)} documents")
    result = cell.result()
    return {
        "arm": arm,
        "contract": CONTRACT,
        "documents": seen,
        "leaked_bytes": result["leaked_bytes"],
        "false_positive_bytes": result["false_positive_bytes"],
        "total_bytes": result["total_bytes"],
        "char_level": result["char_level"],
        "record_sha256": observation["sha256"],
        "crates_tree": tag_crates_tree(entry["version"]),
    }


def build(dataset: Path) -> tuple[dict, dict]:
    import compare

    history = history_doc.load_history(history_doc.DEFAULT_HISTORY)
    layers, identity = compare.load_corpus(dataset, None)
    mapping = compare.load_mapping()["gaze"]
    typed = compare.typed_mapping_for_contract(mapping, CONTRACT)
    releases = {}
    evidence: dict = {"schema_version": SCHEMA_VERSION, "documents": {}, "ignored": {}}
    for entry in _release_records(history):
        require_release_tag(entry["version"], "release char-level")
        components = entry["dataset"]["integrity"]["component_sha256"]
        if (
            identity["main_dataset"]["sha256"] != components["dataiku"]
            or identity["negative_corpus_sha256"] != components["negative_corpus"]
        ):
            raise ValueError(f"{entry['version']}: the loaded corpus is not the one it was measured on")
        row = measure_release(entry, layers["C"], mapping, typed, evidence)
        # Recording happens where the measured commit exists; the offline check reads the tag only.
        if commit_crates_tree(entry["commit"]) != row["crates_tree"]:
            raise ValueError(f"{entry['version']}: the tag and the measured commit hold different crates trees")
        releases[entry["version"]] = row
    return evidence, {
        "schema_version": SCHEMA_VERSION,
        "note": (
            "Character-level (Unicode code point), label-agnostic, micro P/R/F of each tagged "
            "release's shipped default on layer C under the v3 contract, scored by the same "
            "ComparisonMetrics as the competitors."
        ),
        "metrics_sha256": _sha256(METRICS),
        "releases": releases,
    }


def _close(left: float, right: float) -> bool:
    return abs(left - right) <= 1e-9


def check_char_level(version: str, row: dict) -> None:
    """The stored P/R/F2 must follow from its own counts, and stay inside the byte counts."""
    char = row["char_level"]
    tp, fp, fn = char["tp"], char["fp"], char["fn"]
    precision = tp / (tp + fp) if tp + fp else 0.0
    recall = tp / (tp + fn) if tp + fn else 0.0
    denominator = 4 * precision + recall
    f2 = 5 * precision * recall / denominator if denominator else 0.0
    for name, expected in (("precision", precision), ("recall", recall), ("f2", f2)):
        if not _close(char[name], expected):
            raise ValueError(f"{version}: stored {name} {char[name]} != {expected} from its tp/fp/fn")
    if fn > row["leaked_bytes"]:
        raise ValueError(f"{version}: {fn} missed characters exceed {row['leaked_bytes']} leaked bytes")
    if fp > row["false_positive_bytes"]:
        raise ValueError(f"{version}: {fp} false-positive characters exceed "
                         f"{row['false_positive_bytes']} false-positive bytes")


def check(data: dict, history: dict, bench_dir: Path = BENCH_DIR) -> None:
    """Offline verification. Needs no corpus, but reads the committed evidence and
    observation records and resolves the release TAGS in git (CI fetches tags; the
    measured commits need not be on origin)."""
    if data["metrics_sha256"] != _sha256(METRICS):
        raise ValueError("release-char-level.json was measured with another comparison_metrics.py; rerun `record`")
    evidence_path = bench_dir / EVIDENCE.name
    if _sha256(evidence_path) != data["evidence_sha256"]:
        raise ValueError("release-char-level-evidence.json.gz does not match its recorded hash")
    evidence = load_evidence(evidence_path)
    check_evidence_structure(evidence)
    for version, row in data["releases"].items():
        require_release_tag(version, "release char-level")
        entry = next((e for e in history["releases"] if e["version"] == version), None)
        if entry is None:
            raise ValueError(f"{version} is not a release row in the history")
        observation = observation_of(entry)
        if row["record_sha256"] != observation["sha256"]:
            raise ValueError(f"{version}: record hash differs from the release history")
        if _sha256(bench_dir / observation["file"]) != observation["sha256"]:
            raise ValueError(f"{version}: the committed record file does not match its recorded hash")
        view = history_doc.contract_view(entry, history_doc.HEADLINE_CONTRACT)
        arm = view["arms"][history_doc.shipped_default_arm(entry)]
        if row["leaked_bytes"] != arm["surviving_pii_utf8_bytes"]:
            raise ValueError(
                f"{version}: leaked bytes {row['leaked_bytes']} differ from the history's "
                f"{arm['surviving_pii_utf8_bytes']}"
            )
        raw = history_doc.contract_view(entry, 2)  # v2 counts raw false positives, as ComparisonMetrics does
        if raw is not None:
            expected = raw["arms"][history_doc.shipped_default_arm(entry)]["false_positive_utf8_bytes"]
            if row["false_positive_bytes"] != expected:
                raise ValueError(
                    f"{version}: false-positive bytes {row['false_positive_bytes']} differ from "
                    f"the history's {expected}"
                )
        check_char_level(version, row)
        # The decisive step: recount from the record and the evidence, compare to the row.
        counted = recompute(entry, evidence, bench_dir)
        stored = {
            "leaked_bytes": row["leaked_bytes"], "false_positive_bytes": row["false_positive_bytes"],
            "total_bytes": row["total_bytes"], "tp": row["char_level"]["tp"],
            "fn": row["char_level"]["fn"], "fp": row["char_level"]["fp"],
        }
        if counted != stored:
            differing = {k: (stored[k], counted[k]) for k in stored if stored[k] != counted[k]}
            raise ValueError(f"{version}: stored counts differ from the recount (stored, recounted): {differing}")
        # The release tag (on origin) must still hold the detection code that was measured.
        tag_tree = tag_crates_tree(version)
        if row["crates_tree"] != tag_tree:
            raise ValueError(
                f"{version}: the tag's crates tree {tag_tree[:12]} differs from the stored "
                f"row's {row['crates_tree'][:12]}"
            )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    rec = sub.add_parser("record")
    rec.add_argument("--dataset", type=Path, required=True)
    sub.add_parser("check")
    args = parser.parse_args(argv)
    history = history_doc.load_history(history_doc.DEFAULT_HISTORY)
    if args.command == "record":
        evidence, data = build(args.dataset)
        data["evidence_sha256"] = write_evidence(EVIDENCE, evidence)
        check(data, history)
        OUTPUT.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"wrote {OUTPUT.relative_to(REPO)} and {EVIDENCE.relative_to(REPO)}")
        return 0
    check(json.loads(OUTPUT.read_text(encoding="utf-8")), history)
    print("release-char-level.json matches the release history and metrics implementation")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
