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
from comparison_metrics import ComparisonMetrics, split_for_id  # noqa: E402

MAP_PATH = Path(__file__).with_name("label-map.json")
MODEL_PINS_PATH = Path(__file__).with_name("model-wheels.json")
CONTRACTS = {
    "v1": None,
    "v2": Path("docs/reference/benchmarks/scored-labels-v2.json"),
    "v3": Path("docs/reference/benchmarks/scored-labels-v3.json"),
}
TOOLS = (
    "presidio-all", "presidio-en", "presidio-en-de", "presidio-strong",
    "presidio-strong-high-recall",
    "datafog-core", "datafog-regex", "datafog-spacy", "datafog-gliner",
    "scrubadub-base", "scrubadub-spacy", "gliner", "gliner-high-recall", "opf",
)
PRESIDIO_LANGUAGES = ("en", "de", "nl", "fr", "pt")
PRESIDIO_ANONYMIZER_VERSION = "2.2.364"
GERMAN_RECOGNIZERS = (
    "DeTaxId", "DeTaxNumber", "DePassport", "DeIdCard", "DeSocialSecurity",
    "DeHealthInsurance", "DeKfz", "DeHandelsregister", "DePlz",
)
GLINER_REPO = "urchade/gliner_multi_pii-v1"
SPACY_MODELS = {
    "en": "en_core_web_lg", "de": "de_core_news_lg", "nl": "nl_core_news_lg",
    "fr": "fr_core_news_lg", "pt": "pt_core_news_lg",
}


def presidio_languages(tool: str) -> tuple[str, ...]:
    configurations = {
        "presidio-all": PRESIDIO_LANGUAGES,
        "presidio-en": ("en",),
        "presidio-en-de": ("en", "de"),
        "presidio-strong": PRESIDIO_LANGUAGES,
        "presidio-strong-high-recall": PRESIDIO_LANGUAGES,
    }
    if tool not in configurations:
        raise ValueError(f"unknown Presidio configuration: {tool}")
    return configurations[tool]


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
    if set(raw) != {"presidio", "gliner", "opf", "gaze", "datafog-core", "datafog-python", "scrubadub"}:
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


def resolved_presidio_spans(anonymizer: object, text: str, found: Sequence[object]) -> list[score.Span]:
    """Use Presidio's pinned 2.2.364 resolver, whose offsets stay in raw text."""
    from presidio_anonymizer.entities import ConflictResolutionStrategy

    copied = anonymizer._copy_recognizer_results(found)
    copied.sort(key=lambda item: (item.start, item.end))
    resolved = anonymizer._remove_conflicts_and_get_text_manipulation_data(
        copied, ConflictResolutionStrategy.MERGE_SIMILAR_OR_CONTAINED
    )
    resolved = anonymizer._merge_entities_with_spaces_between(text, resolved)
    return byte_spans(text, [(item.start, item.end, item.entity_type) for item in resolved])


class Presidio:
    def __init__(self, models: dict[str, Path], transformer: Path | None = None,
                 threshold: float = 0.0) -> None:
        version = package_version("presidio-anonymizer")
        if version != PRESIDIO_ANONYMIZER_VERSION:
            raise RuntimeError(
                f"raw-coordinate resolution requires presidio-anonymizer=="
                f"{PRESIDIO_ANONYMIZER_VERSION}; found {version}"
            )
        from presidio_analyzer import AnalyzerEngine, RecognizerRegistry
        from presidio_analyzer.nlp_engine import NlpEngineProvider
        from presidio_analyzer import predefined_recognizers
        from presidio_anonymizer import AnonymizerEngine

        languages = list(models)
        configured = [{"lang_code": language, "model_name": str(path)} for language, path in models.items()]
        engine = NlpEngineProvider(nlp_configuration={"nlp_engine_name": "spacy", "models": configured}).create_engine()
        registry = RecognizerRegistry(supported_languages=languages)
        registry.load_predefined_recognizers(languages=languages)
        if "de" in languages:
            for name in GERMAN_RECOGNIZERS:
                registry.add_recognizer(getattr(predefined_recognizers, name + "Recognizer")())
        self.analyzer = AnalyzerEngine(nlp_engine=engine, registry=registry, supported_languages=languages)
        self.english_analyzer = None
        if transformer is not None:
            hf_engine = NlpEngineProvider(nlp_configuration={
                "nlp_engine_name": "transformers",
                "models": [{"lang_code": "en", "model_name": {
                    "spacy": str(models["en"]), "transformers": str(transformer),
                }}],
                "ner_model_configuration": {
                    "model_to_presidio_entity_mapping": {"PER": "PERSON", "ORG": "ORGANIZATION", "LOC": "LOCATION", "MISC": "NRP"},
                    "low_confidence_score_multiplier": 0.4,
                    "low_score_entity_names": ["ORG"],
                },
            }).create_engine()
            english_registry = RecognizerRegistry(supported_languages=["en"])
            english_registry.load_predefined_recognizers(languages=["en"])
            self.english_analyzer = AnalyzerEngine(
                nlp_engine=hf_engine, registry=english_registry, supported_languages=["en"],
            )
        self.anonymizer = AnonymizerEngine()
        self.languages = languages
        self.threshold = threshold

    def predict(self, document: score.Document) -> list[score.Span]:
        if document.language not in self.languages:
            return []
        english_analyzer = getattr(self, "english_analyzer", None)
        analyzer = english_analyzer if document.language == "en" and english_analyzer else self.analyzer
        found = analyzer.analyze(text=document.text, language=document.language,
                                 score_threshold=getattr(self, "threshold", 0.0))
        return resolved_presidio_spans(self.anonymizer, document.text, found)


class Gliner:
    def __init__(self, path: Path, labels: Sequence[str], threshold: float = 0.5) -> None:
        from gliner import GLiNER

        self.model = GLiNER.from_pretrained(str(path)).to("cpu")
        self.labels = list(labels)
        self.threshold = threshold

    def predict(self, document: score.Document) -> list[score.Span]:
        found = self.model.predict_entities(document.text, self.labels, threshold=self.threshold)
        return byte_spans(document.text, [(item["start"], item["end"], item["label"]) for item in found])


class DataFogCore:
    def predict(self, document: score.Document) -> list[score.Span]:
        import datafog_core
        found = datafog_core.scan(document.text)
        result = [score.Span(item.byte_range.start, item.byte_range.end, item.entity_type) for item in found]
        return result


class DataFogPython:
    def __init__(self, engine: str) -> None:
        self.engine = engine

    def predict(self, document: score.Document) -> list[score.Span]:
        import datafog
        result = datafog.scan(document.text, engine=self.engine,
                              locales=["de"] if document.language == "de" else None)
        return byte_spans(document.text, [(item.start, item.end, item.type) for item in result.entities])


class Scrubadub:
    def __init__(self, spacy_model: str | None = None) -> None:
        import scrubadub
        self.scrubbers = {}
        for language, locale in (("en", "en_US"), ("de", "de_DE")):
            scrubber = scrubadub.Scrubber(locale=locale)
            if spacy_model and language == "en":
                from scrubadub_spacy.detectors import SpacyEntityDetector
                scrubber.add_detector(SpacyEntityDetector(model=spacy_model, locale=locale))
            self.scrubbers[language] = scrubber

    def predict(self, document: score.Document) -> list[score.Span]:
        found = self.scrubbers[document.language].iter_filth(document.text)
        return byte_spans(document.text, [(item.beg, item.end, item.type) for item in found])


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
            conn.settimeout(600)
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


def crates_tree(revision: str) -> str:
    return subprocess.check_output(
        ["git", "rev-parse", f"{revision}:crates"], cwd=REPO, text=True,
    ).strip()


def model_info(path: Path, language: str) -> dict[str, str]:
    meta = json.loads((path / "meta.json").read_text(encoding="utf-8"))
    name = SPACY_MODELS[language]
    pin = json.loads(MODEL_PINS_PATH.read_text(encoding="utf-8"))[name]
    if meta["version"] != pin["version"]:
        raise ValueError(f"{name} installed version differs from pinned wheel")
    return {"version": meta["version"], "license": meta["license"],
            "sha256": digest_tree(path), "wheel_sha256": pin["sha256"],
            "wheel_source": pin["url"]}


def normalized_policy_sha256(path: Path, expected_raw_sha256: str) -> str:
    raw = path.read_bytes()
    if hashlib.sha256(raw).hexdigest() != expected_raw_sha256:
        raise ValueError("Gaze policy differs from the scored policy")
    normalized = raw.replace(str(Path.home()).encode(), b"$HOME")
    return hashlib.sha256(normalized).hexdigest()


def opf_runtime_info(python: Path) -> dict[str, object]:
    raw = subprocess.check_output(
        [str(python), "-c",
         "import importlib.metadata as m, json; d=m.distribution('opf'); "
         "print(json.dumps({'version':d.version,'direct_url':d.read_text('direct_url.json')}))"],
        text=True,
    )
    info = json.loads(raw)
    python_version = subprocess.check_output([str(python), "--version"], text=True).strip()
    result: dict[str, object] = {"version": info["version"], "python_version": python_version}
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
    return {"scorecard": portable_path(path), "scorecard_sha256": digest_file(path),
            "gaze_revision": card["gaze"]["revision"],
            "policy_sha256": card["parameters"]["policy_sha256"],
            "hardware": card["runner_provenance"]["hardware"], "layers": table}


def main_revision_for_tree(tree: str) -> str:
    main = subprocess.check_output(["git", "rev-parse", "origin/main"], cwd=REPO, text=True).strip()
    if crates_tree(main) != tree:
        raise ValueError("main crates tree differs from the measured Gaze detection tree")
    return main


def common_claimed_labels(mappings: dict[str, dict[str, tuple[str, ...]]]) -> frozenset[str]:
    capabilities = []
    for name in ("gaze", "presidio", "gliner", "opf", "datafog-core", "datafog-python", "scrubadub"):
        mapping = mappings[name]
        if name == "datafog-python":
            mapping = {key: value for key, value in mapping.items() if key in
                       {"EMAIL", "PHONE", "SSN", "CREDIT_CARD", "IP_ADDRESS", "DATE", "ZIP_CODE",
                        "DE_VAT_ID", "DE_IBAN", "DE_TAX_ID", "DE_POSTAL_CODE", "DE_PASSPORT", "DE_RESIDENCE_PERMIT"}}
        if name == "scrubadub":
            mapping = {key: value for key, value in mapping.items() if key not in
                       {"name", "organization", "location"}}
        capabilities.append({label for labels in mapping.values() for label in labels})
    return frozenset.intersection(*(frozenset(labels) for labels in capabilities))


def measure(
    name: str, predictor: Callable[[score.Document], list[score.Span]],
    layers: dict[str, list[score.Document]], mapping: dict[str, tuple[str, ...]],
    supported_languages: Sequence[str] | None = None,
    common_labels: frozenset[str] | None = None,
) -> dict[str, object]:
    if common_labels is None:
        common_labels = frozenset(label for labels in mapping.values() for label in labels)
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
        skipped_gold_bytes = {key: 0 for key in contracts}
        skipped_documents = 0
        accumulators = {key: score.MetricAccumulator() for key in contracts}
        detailed = {
            version: {
                view: {
                    split: ComparisonMetrics(mapping, common_labels if view == "common_intersection" else None)
                    for split in ("full", "validation", "test")
                }
                for view in ("product_coverage", "common_intersection")
            }
            for version in contracts
        }
        for index, document in enumerate(documents, 1):
            skipped = supported_languages is not None and document.language not in supported_languages
            if skipped:
                predictions = []
                skipped_documents += 1
            else:
                start = time.perf_counter()
                try:
                    predictions = predictor(document)
                except TimeoutError as error:
                    raise RuntimeError(
                        f"{name} timed out in layer {layer} at document {index}/{len(documents)}"
                    ) from error
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
                if skipped:
                    skipped_gold_bytes[version] += score.interval_length(
                        score.merge_intervals((span.start, span.end) for span in applied.spans)
                    )
                accumulators[version].add(mapped_document(applied, mapping), predictions)
                for view in detailed[version].values():
                    view["full"].add(applied, predictions)
                    view[split_for_id(document.uid)].add(applied, predictions)
            if index % 500 == 0:
                print(f"{name}: scored {index}/{len(documents)} in layer {layer}", file=sys.stderr, flush=True)
        latency = {"p50_ms": round(score.percentile(timers, 0.5), 3) if timers else None,
                   "p95_ms": round(score.percentile(timers, 0.95), 3) if timers else None,
                   "samples": len(timers)}
        for version, accumulator in accumulators.items():
            result = accumulator.result()
            output["contracts"][version][layer] = {
                "leaked_bytes": result["utf8_bytes"]["leaked"],
                "false_positive_bytes": result["utf8_bytes"]["false_positive"],
                "gold_gap_protected_bytes": result.get("gold_gap", {}).get("gold_gap_protected_bytes", 0),
                "false_positive_bytes_after_gold_gap": result.get("gold_gap", {}).get("false_positive_bytes_after_gold_gap"),
                "documents": result["documents"], "processed_documents": len(timers),
                "skipped_documents": skipped_documents,
                "skipped_gold_bytes": skipped_gold_bytes[version],
                "latency": latency,
                "metrics": {
                    view: {split: cell.result() for split, cell in splits.items()}
                    for view, splits in detailed[version].items()
                },
            }
    return output


def measure_gaze(
    name: str, binary: Path, policy: Path, model_dir: Path,
    layers: dict[str, list[score.Document]], mapping: dict[str, tuple[str, ...]],
    common_labels: frozenset[str], diagnostics: Path,
) -> dict[str, object]:
    contracts = {key: runner.load_scored_label_contract(REPO, path) for key, path in CONTRACTS.items()}
    agentic_contract = agentic.load_contract(REPO)
    output: dict[str, object] = {version: {} for version in contracts}
    for layer, documents in layers.items():
        byte = {version: score.MetricAccumulator() for version in contracts}
        detailed = {
            version: {
                view: {split: ComparisonMetrics(mapping, common_labels if view == "common_intersection" else None)
                       for split in ("full", "validation", "test")}
                for view in ("product_coverage", "common_intersection")
            }
            for version in contracts
        }
        def record(_config: str, document: score.Document, response: dict[str, object], _measurements: object) -> None:
            if "pipeline_error_code" in response:
                raise RuntimeError(f"{name} refused {document.uid}; compare refusal handling before publishing")
            predictions = score.final_trace_predictions(document, response)
            validate_labels(predictions, mapping)
            for version, contract in contracts.items():
                applied = score.apply_scored_label_contract(
                    [document], agentic_contract if layer in {"A", "D", "R"} else contract,
                )[0]
                byte[version].add(mapped_document(applied, mapping), predictions)
                for view in detailed[version].values():
                    view["full"].add(applied, predictions)
                    view[split_for_id(document.uid)].add(applied, predictions)
        run = score.run_config(
            REPO, binary, "policy-file", documents, model_dir,
            None, None, None, 0.5, diagnostics / name / layer,
            policy_path=policy, record_document=record,
        )
        latency = run["latency_ms"]["clean_ms"]
        for version, accumulator in byte.items():
            result = accumulator.result()
            output[version][layer] = {
                "leaked_bytes": result["utf8_bytes"]["leaked"],
                "false_positive_bytes": result["utf8_bytes"]["false_positive"],
                "gold_gap_protected_bytes": result.get("gold_gap", {}).get("gold_gap_protected_bytes", 0),
                "false_positive_bytes_after_gold_gap": result.get("gold_gap", {}).get("false_positive_bytes_after_gold_gap"),
                "documents": result["documents"], "processed_documents": result["documents"],
                "skipped_documents": 0, "skipped_gold_bytes": 0,
                "latency": {"p50_ms": round(latency["median"], 3),
                            "p95_ms": round(latency["p95"], 3), "samples": result["documents"]},
                "metrics": {view: {split: cell.result() for split, cell in splits.items()}
                            for view, splits in detailed[version].items()},
            }
    return output


def write_report(path: Path, report: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def select_thresholds(report: dict[str, object]) -> None:
    choices = {
        "presidio-strong": ("presidio-strong", "presidio-strong-high-recall"),
        "gliner": ("gliner", "gliner-high-recall"),
    }
    for family, names in choices.items():
        if not all(name in report["tools"] for name in names):
            continue
        def validation_key(name: str) -> tuple[int, int]:
            cells = report["tools"][name]["contracts"]["v3"]
            metrics = [cell["metrics"]["product_coverage"]["validation"] for cell in cells.values()]
            return (sum(item["leaked_bytes"] for item in metrics),
                    sum(item["false_positive_bytes"] for item in metrics))
        report.setdefault("selected_threshold_rows", {})[family] = min(names, key=validation_key)
        report.setdefault("threshold_validation", {})[family] = {
            name: {"leaked_bytes": validation_key(name)[0],
                   "false_positive_bytes": validation_key(name)[1]} for name in names
        }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tool", choices=[*TOOLS, "all"], default="all")
    parser.add_argument("--dataset", type=Path, required=True)
    parser.add_argument("--en-model", type=Path)
    parser.add_argument("--de-model", type=Path)
    parser.add_argument("--nl-model", type=Path)
    parser.add_argument("--fr-model", type=Path)
    parser.add_argument("--pt-model", type=Path)
    parser.add_argument("--gaze-policy", type=Path)
    parser.add_argument("--gliner-model", type=Path)
    parser.add_argument("--gliner-tokenizer", type=Path)
    parser.add_argument("--transformer-model", type=Path)
    parser.add_argument("--gaze-binary", type=Path)
    parser.add_argument("--gaze-model-dir", type=Path)
    parser.add_argument("--gaze-policy-rules", type=Path)
    parser.add_argument("--gaze-policy-rules-ner", type=Path)
    parser.add_argument("--measure-gaze", action="store_true")
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--opf-python", type=Path)
    parser.add_argument("--opf-checkpoint", type=Path)
    parser.add_argument(
        "--pack-dir", type=Path,
        default=REPO / "docs/reference/benchmarks/variant-packs",
    )
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--validate-args-only", action="store_true")
    for version in CONTRACTS:
        parser.add_argument(f"--gaze-scorecard-{version}", type=Path)
    args = parser.parse_args()
    selected = TOOLS if args.tool == "all" else (args.tool,)
    if args.validate_args_only:
        required = tuple(dict.fromkeys(
            language
            for name in selected if name.startswith("presidio")
            for language in presidio_languages(name)
        ))
        missing = [f"--{lang}-model" for lang in required if getattr(args, f"{lang}_model") is None]
        if any(name in {"gliner", "gliner-high-recall", "datafog-gliner"} for name in selected) and args.gliner_model is None:
            missing.append("--gliner-model")
        if any(name in {"gliner", "gliner-high-recall", "datafog-gliner"} for name in selected) and args.gliner_tokenizer is None:
            missing.append("--gliner-tokenizer")
        if any(name.startswith("presidio-strong") for name in selected) and args.transformer_model is None:
            missing.append("--transformer-model")
        if any(getattr(args, f"gaze_scorecard_{version}") for version in CONTRACTS) and args.gaze_policy is None:
            missing.append("--gaze-policy")
        if missing:
            parser.error("missing comparison inputs: " + ", ".join(missing))
        print("COMPARISON_ARGS_OK")
        return 0
    if args.pack_dir != REPO / "docs/reference/benchmarks/variant-packs" and not args.pack_dir.is_dir():
        raise FileNotFoundError(f"variant pack directory missing: {args.pack_dir}")
    layers, corpus = load_corpus(args.dataset, args.pack_dir)
    preflight_contracts(layers)
    mappings = load_mapping()
    latest_release = json.loads(
        (REPO / "docs/reference/benchmarks/release-history.json").read_text(encoding="utf-8")
    )["releases"][-1]
    report: dict[str, object] = {
        "schema_version": 2,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "description": "same corpus and scorer; tools run with documented configurations",
        "harness_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
        "harness_dirty": bool(subprocess.check_output(
            ["git", "status", "--porcelain"], cwd=REPO, text=True
        ).strip()),
        "gaze_crates_tree": crates_tree("HEAD"),
        "runner_sha256": digest_file(BENCH / "run_no_opf_benchmark.py"),
        "dataset_loader_sha256": digest_file(BENCH / "dataiku_en_de_gaze_bench.py"),
        "latest_release_at_measurement": {
            key: latest_release[key] for key in ("version", "scorecard_sha256")
        },
        "scorer_sha256": digest_file(BENCH / "gaze_bench_score.py"),
        "compare_sha256": digest_file(Path(__file__)),
        "opf_adapter_sha256": digest_file(BENCH / "opf_daemon.py"),
        "requirements_sha256": digest_file(Path(__file__).with_name("requirements.lock")),
        "scrubadub_requirements_sha256": digest_file(Path(__file__).with_name("requirements-scrubadub.lock")),
        "comparison_metrics_sha256": digest_file(Path(__file__).with_name("comparison_metrics.py")),
        "mapping_sha256": digest_file(MAP_PATH),
        "model_pins_sha256": digest_file(MODEL_PINS_PATH),
        "contracts": {
            **{
                version: runner.load_scored_label_contract(REPO, path).sha256
                for version, path in CONTRACTS.items()
            },
            "agentic": agentic.load_contract(REPO).sha256,
        },
        "hardware": platform.platform(), "device": "cpu", "corpus": corpus,
        "tools": {}, "skipped": {}, "gaze": {}, "gaze_ablations": {},
    }
    if args.resume:
        previous = json.loads(args.output.read_text(encoding="utf-8"))
        for key in ("corpus", "mapping_sha256", "model_pins_sha256", "compare_sha256",
                    "comparison_metrics_sha256", "gaze_crates_tree", "contracts"):
            if previous[key] != report[key]:
                raise ValueError(f"resume input differs in {key}")
        report = previous
    for version in CONTRACTS:
        path = getattr(args, f"gaze_scorecard_{version}")
        if path is not None and not args.resume:
            report["gaze"][version] = gaze_row(path, version, corpus)
    if args.measure_gaze and not report["gaze"]:
        previous = json.loads((REPO / "docs/reference/benchmarks/comparison.json").read_text(encoding="utf-8"))
        if previous["corpus"] != corpus or previous["contracts"] != report["contracts"]:
            raise ValueError("committed Gaze comparison corpus or contracts differ")
        report["gaze"] = previous["gaze"]
    for key in ("gaze_revision", "policy_sha256", "hardware"):
        if len({row[key] for row in report["gaze"].values()}) > 1:
            raise ValueError(f"Gaze scorecards for the three contracts use different {key}")
    if not args.measure_gaze and any(crates_tree(row["gaze_revision"]) != report["gaze_crates_tree"] for row in report["gaze"].values()):
        raise ValueError("Gaze scorecards were not measured on this detection tree")
    report["gaze_main_revision"] = main_revision_for_tree(report["gaze_crates_tree"])
    if report["gaze"]:
        if args.gaze_policy is None:
            raise ValueError("Gaze scorecards require --gaze-policy for portable provenance")
        report["policy_sha256_home_normalized"] = normalized_policy_sha256(
            args.gaze_policy, next(iter(report["gaze"].values()))["policy_sha256"]
        )
    common_labels = common_claimed_labels(mappings)
    report["common_intersection_labels"] = sorted(common_labels)
    report["heldout_split"] = {
        "rule": "validation iff first byte of SHA-256(UTF-8 document id) < 128; test otherwise",
        "implementation_sha256": digest_file(Path(__file__).with_name("comparison_metrics.py")),
        "layers": {
            layer: {
                split: {
                    "documents": len(selected),
                    "ids_sha256": score.document_ids_digest([doc.uid for doc in selected]),
                }
                for split in ("validation", "test")
                for selected in ([doc for doc in documents if split_for_id(doc.uid) == split],)
            }
            for layer, documents in layers.items()
        },
    }
    if args.measure_gaze:
        if args.resume or args.gaze_binary is None or args.gaze_model_dir is None:
            raise ValueError("Gaze measurement needs a fresh report, binary and model directory")
        expected = json.loads((REPO / "docs/reference/benchmarks/comparison.json").read_text(encoding="utf-8"))
        for name, policy in (("rules-only", args.gaze_policy_rules),
                             ("rules-ner", args.gaze_policy_rules_ner),
                             ("full", args.gaze_policy)):
            if policy is None:
                raise ValueError(f"missing Gaze {name} policy")
            measured = measure_gaze(name, args.gaze_binary, policy, args.gaze_model_dir,
                                    layers, mappings["gaze"], common_labels,
                                    args.output.parent / "diagnostics")
            if name == "full":
                for version in CONTRACTS:
                    for layer in layers:
                        new = measured[version][layer]
                        old = expected["gaze"][version]["layers"][layer]
                        for field in ("leaked_bytes", "false_positive_bytes",
                                      "gold_gap_protected_bytes", "false_positive_bytes_after_gold_gap"):
                            if new[field] != old[field]:
                                raise ValueError(f"Gaze byte mismatch: {version}/{layer}/{field}: {new[field]} != {old[field]}")
                        report["gaze"][version]["layers"][layer] = new
                    report["gaze"][version]["prior_scorecard_revision"] = report["gaze"][version]["gaze_revision"]
                    report["gaze"][version]["gaze_revision"] = report["gaze_main_revision"]
                    report["gaze"][version]["scorecard"] = "re-inferred by compare.py"
                report["gaze_inference"] = {
                    "binary_sha256": digest_file(args.gaze_binary),
                    "main_revision": report["gaze_main_revision"],
                    "policy_sha256": digest_file(policy),
                    "model_sha256": digest_tree(args.gaze_model_dir),
                    "byte_counts_equal_prior_report": True,
                }
            else:
                report["gaze_ablations"][name] = measured
            write_report(args.output, report)
    with tempfile.TemporaryDirectory(prefix="gaze-comparison-") as temporary:
        for name in selected:
            backend = None
            if name == "opf" and (args.opf_python is None or args.opf_checkpoint is None):
                report["skipped"]["opf"] = "local OPF runtime or checkpoint not configured"
                write_report(args.output, report)
                continue
            if name.startswith("presidio"):
                requested = presidio_languages(name)
                model_paths = {lang: getattr(args, f"{lang}_model") for lang in requested}
                if any(path is None for path in model_paths.values()):
                    raise ValueError(f"{name} needs model paths for {', '.join(requested)}")
                strong = name.startswith("presidio-strong")
                if strong and args.transformer_model is None:
                    raise ValueError("Presidio strong needs --transformer-model")
                threshold = 0.0 if name.endswith("high-recall") else 0.3 if strong else 0.0
                backend = Presidio(model_paths, args.transformer_model if strong else None,
                                   threshold=threshold)
                provenance = {
                    "analyzer_version": package_version("presidio-analyzer"),
                    "anonymizer_version": package_version("presidio-anonymizer"),
                    "spacy_version": package_version("spacy"),
                    "models": {lang: model_info(path, lang) for lang, path in model_paths.items()},
                    "supported_languages": backend.languages,
                    "recognizers": "Presidio built-in defaults plus nine documented German recognizers when de is enabled",
                    "german_recognizers": list(GERMAN_RECOGNIZERS) if "de" in requested else [],
                    "anonymizer": "Presidio 2.2.364 raw-coordinate resolution with default conflict and whitespace rules",
                    "score_threshold": threshold,
                    "context_enhancer": "LemmaContextAwareEnhancer (AnalyzerEngine default)",
                }
                if strong:
                    provenance["transformer"] = {
                        "repo": "dslim/bert-base-NER", "revision": "d1a3e8f13f8c3566299d95fcfc9a8d2382a9affc",
                        "sha256": digest_tree(args.transformer_model),
                    }
                mapping = mappings["presidio"]
            elif name in {"gliner", "gliner-high-recall"}:
                if args.gliner_model is None:
                    raise ValueError("GLiNER needs --gliner-model")
                threshold = 0.3 if name == "gliner-high-recall" else 0.5
                backend = Gliner(args.gliner_model, tuple(mappings["gliner"]), threshold)
                provenance = {"gliner_version": package_version("gliner"),
                              "model_repo": GLINER_REPO,
                              "model_sha256": digest_tree(args.gliner_model),
                              "model_snapshot": args.gliner_model.name,
                              "tokenizer_revision": args.gliner_tokenizer.name,
                              "tokenizer_sha256": digest_tree(args.gliner_tokenizer),
                              "labels": list(mappings["gliner"]),
                              "threshold": threshold, "flat_ner": True,
                              "threshold_source": "library default" if threshold == 0.5 else "predeclared high-recall sweep"}
                mapping = mappings["gliner"]
            elif name == "datafog-core":
                backend = DataFogCore()
                provenance = {"version": package_version("datafog-core"), "mode": "built-in text recognizers only"}
                mapping = mappings["datafog-core"]
            elif name.startswith("datafog-"):
                engine = name.removeprefix("datafog-")
                backend = DataFogPython(engine)
                provenance = {"version": package_version("datafog"), "engine": engine,
                              "model_sha256": digest_tree(args.gliner_model) if engine == "gliner" else None,
                              "tokenizer_revision": args.gliner_tokenizer.name if engine == "gliner" else None,
                              "tokenizer_sha256": digest_tree(args.gliner_tokenizer) if engine == "gliner" else None,
                              "spacy_model": model_info(args.en_model, "en") if engine == "spacy" else None}
                mapping = mappings["datafog-python"]
            elif name.startswith("scrubadub-"):
                if name == "scrubadub-spacy" and args.en_model is None:
                    raise ValueError("scrubadub spaCy needs --en-model")
                backend = Scrubadub(SPACY_MODELS["en"] if name == "scrubadub-spacy" else None)
                provenance = {"version": package_version("scrubadub"),
                              "plugin_version": package_version("scrubadub-spacy") if name == "scrubadub-spacy" else None,
                              "spacy_model": model_info(args.en_model, "en") if name == "scrubadub-spacy" else None,
                              "mode": "autoloaded built-in detectors plus spaCy" if name == "scrubadub-spacy" else "autoloaded built-in detectors"}
                mapping = mappings["scrubadub"]
            else:
                if args.opf_python is None or args.opf_checkpoint is None:
                    raise ValueError("OPF needs --opf-python and --opf-checkpoint")
                backend = Opf(args.opf_python, args.opf_checkpoint, Path(temporary))
                provenance = {"checkpoint_sha256": digest_tree(args.opf_checkpoint),
                              "runtime": opf_runtime_info(args.opf_python),
                              "decode": "default viterbi, typed output, cpu",
                              "socket_timeout_seconds": 600}
                mapping = mappings["opf"]
            try:
                # Warm the model outside the measured per-document latency.
                backend.predict(score.Document("warmup", "alice@example.invalid", "en", "", "synthetic", ()))
                load_before = os.getloadavg()
                measured = measure(
                    name, backend.predict, layers, mapping,
                    backend.languages if isinstance(backend, Presidio) else None,
                    common_labels,
                )
                measured["host_load_1m_before_after"] = [round(load_before[0], 2), round(os.getloadavg()[0], 2)]
                measured["provenance"] = provenance
                report["tools"][name] = measured
                select_thresholds(report)
                write_report(args.output, report)
                print(f"COMPARISON_DONE {name}", file=sys.stderr, flush=True)
            finally:
                if isinstance(backend, Opf):
                    backend.close()
    write_report(args.output, report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
