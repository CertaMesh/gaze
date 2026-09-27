#!/usr/bin/env python3
"""Score standalone PII tools on the canonical Gaze documents and byte scorer."""

from __future__ import annotations

import argparse
import dataclasses
import hashlib
import importlib.metadata
import json
import os
import platform
import select
import socket
import subprocess
import sys
import tempfile
import time
from collections import defaultdict
from datetime import datetime, timezone
from functools import lru_cache
from pathlib import Path
from typing import Callable, Sequence

BENCH = Path(__file__).resolve().parents[1]
REPO = BENCH.parents[1]
sys.path.insert(0, str(BENCH))

import agentic_layers as agentic  # noqa: E402
import dataiku_en_de_gaze_bench as dataiku  # noqa: E402
import gaze_bench_score as score  # noqa: E402
import run_no_opf_benchmark as runner  # noqa: E402

MAP_PATH = Path(__file__).with_name("label-map.json")
CONTRACTS = {
    "v1": None,
    "v2": Path("docs/reference/benchmarks/scored-labels-v2.json"),
    "v3": Path("docs/reference/benchmarks/scored-labels-v3.json"),
}
TOOLS = ("presidio-en", "presidio-en-de", "gliner", "opf")


def digest_file(path: Path) -> str:
    return score.sha256_file(path)


def portable_path(path: Path) -> str:
    resolved = path.resolve()
    for root, prefix in ((REPO, ""), (Path.home(), "$HOME/")):
        if resolved.is_relative_to(root):
            return prefix + resolved.relative_to(root).as_posix()
    return resolved.as_posix()


@lru_cache(maxsize=None)
def digest_tree(path: Path) -> str:
    if not path.is_dir():
        raise FileNotFoundError(f"model directory missing: {path}")
    digest = hashlib.sha256()
    for file in sorted(item for item in path.rglob("*") if item.is_file()):
        digest.update(file.relative_to(path).as_posix().encode())
        digest.update(bytes.fromhex(digest_file(file)))
    return digest.hexdigest()


def byte_spans(text: str, found: Sequence[tuple[int, int, str]]) -> list[score.Span]:
    offsets = score.char_to_byte_offsets(text)
    result = []
    for start, end, label in found:
        if not isinstance(start, int) or not isinstance(end, int) or not 0 <= start < end <= len(text):
            raise ValueError("tool returned an invalid character span")
        result.append(score.Span(offsets[start], offsets[end], label))
    return result


def load_mapping() -> dict[str, dict[str, tuple[str, ...]]]:
    raw = json.loads(MAP_PATH.read_text(encoding="utf-8"))
    if set(raw) != {"presidio", "gliner", "opf"}:
        raise ValueError("label map must cover each tool")
    return {
        tool: {label: tuple(labels) for label, labels in table.items()}
        for tool, table in raw.items()
    }


def validate_labels(spans: Sequence[score.Span], mapping: dict[str, tuple[str, ...]]) -> None:
    unknown = sorted({span.label for span in spans} - mapping.keys())
    if unknown:
        raise ValueError(f"tool emitted unmapped labels {unknown}; review label-map.json")


def mapped_document(
    document: score.Document,
    mapping: dict[str, tuple[str, ...]],
) -> score.Document:
    if document.gold_gap is None:
        return document
    # The byte scorer is class-agnostic. Only v3's repeat credit needs a
    # reviewed semantic mapping from each tool's native labels to gold labels.
    gold_labels = {span.label for span in document.spans}
    compatible = frozenset(
        ("custom:secret" if label == "secret" else label, gold)
        for label, labels in mapping.items()
        for gold in labels
        if gold in gold_labels
    )
    return dataclasses.replace(document, gold_gap=score.GoldGapRule(compatible))


def load_pack(path: Path) -> tuple[dict[str, list[score.Document]], dict[str, object]]:
    partitions: dict[str, list[score.Document]] = defaultdict(list)
    seen: set[str] = set()
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        row = json.loads(line)
        uid, partition, text = row["id"], row["partition"], row["text"]
        if partition not in {"dev", "sealed"} or not isinstance(uid, str) or uid in seen:
            raise ValueError(f"{path}:{number}: invalid or duplicate id/partition")
        seen.add(uid)
        if row["language"] not in {"en", "de"} or not isinstance(text, str):
            raise ValueError(f"{path}:{number}: invalid text/language")
        encoded = text.encode("utf-8")
        spans = []
        for gold in row["gold"]:
            start, end, label = gold["start"], gold["end"], gold["label"]
            if not isinstance(start, int) or not isinstance(end, int) or not 0 <= start < end <= len(encoded):
                raise ValueError(f"{path}:{number}: invalid gold bounds")
            if encoded[start:end].decode("utf-8", errors="ignore").encode("utf-8") != encoded[start:end]:
                raise ValueError(f"{path}:{number}: gold cuts a UTF-8 character")
            spans.append(score.Span(start, end, label))
        layer = row["layer"]
        if not isinstance(layer, str) or not layer:
            raise ValueError(f"{path}:{number}: invalid layer")
        partitions[f"{layer}/{path.stem}/{partition}"].append(
            score.Document(
                uid=f"{path.stem}/{uid}", text=text,
                language=row["language"], region=row.get("region", ""),
                source_dataset=f"variant-pack/{path.stem}", spans=tuple(spans),
            )
        )
    if not seen:
        raise ValueError(f"empty variant pack: {path}")
    return dict(partitions), {"path": portable_path(path), "sha256": digest_file(path), "documents": len(seen)}


def load_corpus(dataset: Path, pack_dir: Path | None) -> tuple[dict[str, list[score.Document]], dict[str, object]]:
    positives, dataiku_report = dataiku.load_documents(dataset)
    negatives, _ = runner.load_negative_documents(
        REPO / "crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl"
    )
    main, sample = score.stratified_sample(positives + negatives, None, score.DEFAULT_SAMPLE_SEED)
    if len(main) != len(positives) + len(negatives):
        raise ValueError("main corpus selection omitted documents")
    generated = agentic.prepare(REPO)
    layers = {
        "C": main,
        "A": generated.identifiers,
        "D": generated.lookalikes,
        "R": generated.repeats,
    }
    packs = []
    if pack_dir is not None and pack_dir.exists():
        for path in sorted(pack_dir.glob("*.jsonl")):
            extra, provenance = load_pack(path)
            if set(layers) & set(extra):
                raise ValueError(f"pack layer collides with canonical layer: {path}")
            layers.update(extra)
            packs.append(provenance)
    all_ids = [document.uid for documents in layers.values() for document in documents]
    if len(all_ids) != len(set(all_ids)):
        raise ValueError("document id collision across benchmark layers")
    identity = {
        "main_dataset": dataiku_report["integrity"],
        "negative_corpus_sha256": digest_file(REPO / "crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl"),
        "main_selection_digest": sample["evaluated_document_ids_digest"],
        "agentic": generated.manifest,
        "packs": packs,
        "layers": {
            layer: {"documents": len(documents), "ids_sha256": score.document_ids_digest([d.uid for d in documents])}
            for layer, documents in layers.items()
        },
    }
    return layers, identity


def preflight_contracts(layers: dict[str, list[score.Document]]) -> None:
    main = [document for layer, documents in layers.items() if layer not in {"A", "D", "R"} for document in documents]
    for version, path in CONTRACTS.items():
        contract = runner.load_scored_label_contract(REPO, path)
        score.apply_scored_label_contract(main, contract)
    agentic.apply_contract(
        [document for layer in ("A", "D", "R") for document in layers[layer]],
        agentic.load_contract(REPO),
    )


class Presidio:
    def __init__(self, en_model: Path, de_model: Path | None) -> None:
        from presidio_analyzer import AnalyzerEngine, RecognizerRegistry
        from presidio_analyzer.nlp_engine import NlpEngineProvider
        from presidio_anonymizer import AnonymizerEngine
        from presidio_anonymizer.entities import ConflictResolutionStrategy

        models = [{"lang_code": "en", "model_name": str(en_model)}]
        languages = ["en"]
        if de_model is not None:
            models.append({"lang_code": "de", "model_name": str(de_model)})
            languages.append("de")
        engine = NlpEngineProvider(nlp_configuration={"nlp_engine_name": "spacy", "models": models}).create_engine()
        registry = RecognizerRegistry(supported_languages=languages)
        registry.load_predefined_recognizers(languages=languages)
        self.analyzer = AnalyzerEngine(nlp_engine=engine, registry=registry, supported_languages=languages)
        self.anonymizer = AnonymizerEngine()
        self.conflict_resolution = ConflictResolutionStrategy.MERGE_SIMILAR_OR_CONTAINED
        self.languages = languages

    def predict(self, document: score.Document) -> list[score.Span]:
        if document.language not in self.languages:
            return []
        found = self.analyzer.analyze(text=document.text, language=document.language)
        # This is the anonymizer's default conflict and whitespace resolution.
        # Its output item offsets refer to replacement text, so capture the
        # resolved raw spans before invoking the same default replacement.
        copied = self.anonymizer._copy_recognizer_results(found)
        copied.sort(key=lambda item: (item.start, item.end))
        resolved = self.anonymizer._remove_conflicts_and_get_text_manipulation_data(
            copied, self.conflict_resolution
        )
        resolved = self.anonymizer._merge_entities_with_spaces_between(document.text, resolved)
        output = self.anonymizer.anonymize(text=document.text, analyzer_results=found)
        if len(output.items) != len(resolved):
            raise RuntimeError("Presidio anonymizer did not process every resolved span")
        return byte_spans(document.text, [(item.start, item.end, item.entity_type) for item in resolved])


class Gliner:
    def __init__(self, path: Path, labels: Sequence[str]) -> None:
        from gliner import GLiNER

        self.model = GLiNER.from_pretrained(str(path)).to("cpu")
        self.labels = list(labels)

    def predict(self, document: score.Document) -> list[score.Span]:
        found = self.model.predict_entities(document.text, self.labels)
        return byte_spans(document.text, [(item["start"], item["end"], item["label"]) for item in found])


class Opf:
    def __init__(self, python: Path, checkpoint: Path, scratch: Path) -> None:
        self.socket = scratch / "opf.sock"
        self.process = subprocess.Popen(
            [str(python), str(BENCH / "opf_daemon.py"), "serve", "--socket", str(self.socket),
             "--checkpoint", str(checkpoint), "--device", "cpu"],
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
        )
        assert self.process.stdout is not None
        if not select.select([self.process.stdout], [], [], 120)[0]:
            self.process.kill()
            self.process.wait()
            raise RuntimeError("OPF daemon startup timed out")
        ready = self.process.stdout.readline().strip()
        if ready != f"OPF_DAEMON_READY {self.socket}":
            self.process.kill()
            self.process.wait()
            raise RuntimeError("OPF daemon failed to start")

    def predict(self, document: score.Document) -> list[score.Span]:
        payload = json.dumps({"text": document.text}, ensure_ascii=False).encode("utf-8")
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as conn:
            conn.settimeout(60)
            conn.connect(str(self.socket))
            conn.sendall(payload)
            conn.shutdown(socket.SHUT_WR)
            response = bytearray()
            while chunk := conn.recv(65536):
                response.extend(chunk)
        result = json.loads(response)
        if result.get("text") != document.text or "error" in result:
            raise RuntimeError(f"OPF failed to process {document.uid}")
        return byte_spans(document.text, [(s["start"], s["end"], s["label"]) for s in result["detected_spans"]])

    def close(self) -> None:
        self.process.terminate()
        self.process.wait(timeout=10)


def package_version(name: str) -> str:
    return importlib.metadata.version(name)


def model_info(path: Path) -> dict[str, str]:
    meta = json.loads((path / "meta.json").read_text(encoding="utf-8"))
    return {"version": meta["version"], "license": meta["license"], "sha256": digest_tree(path)}


def opf_runtime_info(python: Path) -> dict[str, object]:
    raw = subprocess.check_output(
        [str(python), "-c",
         "import importlib.metadata as m, json; d=m.distribution('opf'); "
         "print(json.dumps({'version':d.version,'direct_url':d.read_text('direct_url.json')}))"],
        text=True,
    )
    info = json.loads(raw)
    result: dict[str, object] = {"version": info["version"], "python": portable_path(python)}
    if info["direct_url"]:
        from urllib.parse import unquote, urlparse

        source = json.loads(info["direct_url"])["url"]
        if source.startswith("file:"):
            checkout = Path(unquote(urlparse(source).path))
            result["source_revision"] = subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=checkout, text=True
            ).strip()
            result["source_dirty"] = bool(subprocess.check_output(
                ["git", "status", "--porcelain"], cwd=checkout, text=True
            ).strip())
    return result


def gaze_row(path: Path, version: str, corpus: dict[str, object]) -> dict[str, object]:
    card = json.loads(path.read_text(encoding="utf-8"))
    if card["dataset"]["sampling"]["evaluated_document_ids_digest"] != corpus["main_selection_digest"]:
        raise ValueError(f"{path}: Gaze and competitor main document IDs differ")
    if card["layers"]["generator"]["corpus_sha256"] != corpus["agentic"]["corpus_sha256"]:
        raise ValueError(f"{path}: Gaze and competitor agentic corpus differ")
    components = card["dataset"]["integrity"]["component_sha256"]
    if (components["dataiku"] != corpus["main_dataset"]["sha256"]
            or components["negative_corpus"] != corpus["negative_corpus_sha256"]):
        raise ValueError(f"{path}: Gaze and competitor dataset hashes differ")
    expected = runner.load_scored_label_contract(REPO, CONTRACTS[version])
    actual = card["scoring"]["scored_label_contract"]
    if actual["id"] != expected.contract_id or actual["file_sha256"] != expected.sha256:
        raise ValueError(f"{path}: Gaze and competitor scored-label contracts differ")
    config = "policy-file"
    rows = {"C": card["runs"]}
    for layer in corpus["layers"]:
        if layer == "C":
            continue
        if layer not in card["layers"] or "runs" not in card["layers"][layer]:
            raise ValueError(f"{path}: Gaze has no matching results for layer {layer}")
        if layer not in {"A", "D", "R"}:
            population = card["layers"][layer].get("population", {})
            if population.get("evaluated_document_ids_digest") != corpus["layers"][layer]["ids_sha256"]:
                raise ValueError(f"{path}: Gaze and competitor pack IDs differ for {layer}")
        rows[layer] = card["layers"][layer]["runs"]
    table = {}
    for layer, runs in rows.items():
        run = next((run for run in runs if run["config"] == config), None)
        if run is None:
            raise ValueError(f"{path}: missing {config} run in layer {layer}")
        metrics = run["metrics"]
        latency = run["latency_ms"]["clean_ms"]
        table[layer] = {
            "leaked_bytes": metrics["utf8_bytes"]["leaked"],
            "false_positive_bytes": metrics["utf8_bytes"]["false_positive"],
            "gold_gap_protected_bytes": metrics.get("gold_gap", {}).get("gold_gap_protected_bytes", 0),
            "false_positive_bytes_after_gold_gap": metrics.get("gold_gap", {}).get("false_positive_bytes_after_gold_gap"),
            "documents": metrics["documents"],
            "latency": {"p50_ms": round(latency["median"], 3), "p95_ms": round(latency["p95"], 3), "samples": metrics["documents"]},
        }
        if table[layer]["documents"] != corpus["layers"][layer]["documents"]:
            raise ValueError(f"{path}: Gaze and competitor document counts differ in {layer}")
    return {"scorecard": path.as_posix(), "scorecard_sha256": digest_file(path),
            "gaze_revision": card["gaze"]["revision"], "layers": table}


def measure(
    name: str, predictor: Callable[[score.Document], list[score.Span]],
    layers: dict[str, list[score.Document]], mapping: dict[str, tuple[str, ...]],
) -> dict[str, object]:
    contracts = {
        key: runner.load_scored_label_contract(REPO, path)
        for key, path in CONTRACTS.items()
    }
    agentic_contract = agentic.load_contract(REPO)
    output: dict[str, object] = {"tool": name, "contracts": {}}
    for version in contracts:
        output["contracts"][version] = {}
    for layer, documents in layers.items():
        timers = []
        accumulators = {key: score.MetricAccumulator() for key in contracts}
        for document in documents:
            start = time.perf_counter()
            predictions = predictor(document)
            timers.append((time.perf_counter() - start) * 1000)
            validate_labels(predictions, mapping)
            if name == "opf":
                predictions = [dataclasses.replace(s, label="custom:secret") if s.label == "secret" else s for s in predictions]
            for version, contract in contracts.items():
                applied = (
                    score.apply_scored_label_contract([document], agentic_contract)[0]
                    if layer in {"A", "D", "R"}
                    else score.apply_scored_label_contract([document], contract)[0]
                )
                accumulators[version].add(mapped_document(applied, mapping), predictions)
        latency = {"p50_ms": round(score.percentile(timers, 0.5), 3),
                   "p95_ms": round(score.percentile(timers, 0.95), 3),
                   "samples": len(timers)}
        for version, accumulator in accumulators.items():
            result = accumulator.result()
            output["contracts"][version][layer] = {
                "leaked_bytes": result["utf8_bytes"]["leaked"],
                "false_positive_bytes": result["utf8_bytes"]["false_positive"],
                "gold_gap_protected_bytes": result.get("gold_gap", {}).get("gold_gap_protected_bytes", 0),
                "false_positive_bytes_after_gold_gap": result.get("gold_gap", {}).get("false_positive_bytes_after_gold_gap"),
                "documents": result["documents"], "latency": latency,
            }
    return output


def write_report(path: Path, report: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tool", choices=[*TOOLS, "all"], default="all")
    parser.add_argument("--dataset", type=Path, required=True)
    parser.add_argument("--en-model", type=Path)
    parser.add_argument("--de-model", type=Path)
    parser.add_argument("--gliner-model", type=Path)
    parser.add_argument("--opf-python", type=Path)
    parser.add_argument("--opf-checkpoint", type=Path)
    parser.add_argument(
        "--pack-dir", type=Path,
        default=REPO / "docs/reference/benchmarks/variant-packs",
    )
    parser.add_argument("--output", type=Path, required=True)
    for version in CONTRACTS:
        parser.add_argument(f"--gaze-scorecard-{version}", type=Path)
    args = parser.parse_args()
    selected = TOOLS if args.tool == "all" else (args.tool,)
    if args.pack_dir != REPO / "docs/reference/benchmarks/variant-packs" and not args.pack_dir.is_dir():
        raise FileNotFoundError(f"variant pack directory missing: {args.pack_dir}")
    layers, corpus = load_corpus(args.dataset, args.pack_dir)
    preflight_contracts(layers)
    mappings = load_mapping()
    report: dict[str, object] = {
        "schema_version": 1,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "description": "same corpus and scorer; tools run with their documented defaults",
        "harness_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
        "harness_dirty": bool(subprocess.check_output(
            ["git", "status", "--porcelain"], cwd=REPO, text=True
        ).strip()),
        "scorer_sha256": digest_file(BENCH / "gaze_bench_score.py"),
        "mapping_sha256": digest_file(MAP_PATH),
        "contracts": {
            **{
                version: runner.load_scored_label_contract(REPO, path).sha256
                for version, path in CONTRACTS.items()
            },
            "agentic": agentic.load_contract(REPO).sha256,
        },
        "hardware": platform.platform(), "device": "cpu", "corpus": corpus,
        "tools": {}, "skipped": {}, "gaze": {},
    }
    for version in CONTRACTS:
        path = getattr(args, f"gaze_scorecard_{version}")
        if path is not None:
            report["gaze"][version] = gaze_row(path, version, corpus)
    gaze_revisions = {row["gaze_revision"] for row in report["gaze"].values()}
    if len(gaze_revisions) > 1:
        raise ValueError("Gaze scorecards for the three contracts use different revisions")
    with tempfile.TemporaryDirectory(prefix="gaze-comparison-") as temporary:
        for name in selected:
            backend = None
            if name == "opf" and args.tool == "all" and (
                args.opf_python is None or args.opf_checkpoint is None
            ):
                report["skipped"]["opf"] = "local OPF runtime or checkpoint not configured"
                continue
            if name.startswith("presidio"):
                if args.en_model is None or (name == "presidio-en-de" and args.de_model is None):
                    raise ValueError("Presidio needs --en-model and multilingual needs --de-model")
                backend = Presidio(args.en_model, args.de_model if name == "presidio-en-de" else None)
                provenance = {
                    "analyzer_version": package_version("presidio-analyzer"),
                    "anonymizer_version": package_version("presidio-anonymizer"),
                    "spacy_version": package_version("spacy"),
                    "en_model": model_info(args.en_model),
                    **({"de_model": model_info(args.de_model)} if name == "presidio-en-de" else {}),
                    "supported_languages": backend.languages,
                    "recognizers": "Presidio built-in registry defaults for supported languages",
                    "anonymizer": "default replace, default conflict and whitespace resolution",
                }
                mapping = mappings["presidio"]
            elif name == "gliner":
                if args.gliner_model is None:
                    raise ValueError("GLiNER needs --gliner-model")
                backend = Gliner(args.gliner_model, tuple(mappings["gliner"]))
                provenance = {"gliner_version": package_version("gliner"),
                              "model_sha256": digest_tree(args.gliner_model),
                              "model_snapshot": args.gliner_model.name,
                              "labels": list(mappings["gliner"]),
                              "threshold": 0.5, "flat_ner": True,
                              "threshold_source": "predict_entities library default"}
                mapping = mappings["gliner"]
            else:
                if args.opf_python is None or args.opf_checkpoint is None:
                    raise ValueError("OPF needs --opf-python and --opf-checkpoint")
                backend = Opf(args.opf_python, args.opf_checkpoint, Path(temporary))
                provenance = {"checkpoint_sha256": digest_tree(args.opf_checkpoint),
                              "runtime": opf_runtime_info(args.opf_python),
                              "decode": "default viterbi, typed output, cpu"}
                mapping = mappings["opf"]
            try:
                # Warm the model outside the measured per-document latency.
                backend.predict(score.Document("warmup", "alice@example.invalid", "en", "", "synthetic", ()))
                load_before = os.getloadavg()
                measured = measure(name, backend.predict, layers, mapping)
                measured["host_load_1m_before_after"] = [round(load_before[0], 2), round(os.getloadavg()[0], 2)]
                measured["provenance"] = provenance
                report["tools"][name] = measured
                write_report(args.output, report)
                print(f"COMPARISON_DONE {name}", file=sys.stderr, flush=True)
            finally:
                if isinstance(backend, Opf):
                    backend.close()
    write_report(args.output, report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
