#!/usr/bin/env python3
"""Record Presidio's raw findings on the comparison corpus (no document text is written).

`pool --pass NAME` runs one recording pass of the declared search space in the
comparison environment (../requirements.lock): every recognizer of the pass for
every document, before any threshold, de-duplication or allow list, with each
result's score under all three context modes. `search.py` replays candidate
configurations from these records offline.

`authors` runs Presidio Research's tuned analyzer (notebook 5, verbatim, through
presidio_research_repro.custom_analyzer) in the vendor environment
(../theirbench/requirements-theirbench.lock), called exactly as its evaluator
calls it, and records its final findings.

Each pass writes `<pass>.validation.jsonl` and `<pass>.test.jsonl` (one row per
document, corpus order, character offsets) plus `<pass>.meta.json`. The halves
are separate files so that selection opens the validation file only.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import importlib.metadata
import json
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import corpus  # noqa: E402
import space  # noqa: E402

PASSES = ("base", "dslim", "davlan", "openmed", "gliner")
SPLITS = ("validation", "test")


def tree_sha256(path: Path) -> str:
    return corpus.compare.digest_tree(path)


def check_pin(name: str, path: Path, pins: dict[str, str]) -> str:
    digest = tree_sha256(path)
    if pins.get(name) != digest:
        raise SystemExit(f"{name} model tree {digest} is not the pinned {pins.get(name)}")
    return digest


def split_sinks(out: Path, name: str) -> dict[str, object]:
    out.mkdir(parents=True, exist_ok=True)
    return {split: (out / f"{name}.{split}.jsonl.partial").open("w", encoding="utf-8") for split in SPLITS}


def finish(out: Path, name: str, sinks: dict[str, object], meta: dict[str, object]) -> None:
    digests = {}
    for split, sink in sinks.items():
        sink.close()
        final = out / f"{name}.{split}.jsonl"
        (out / f"{name}.{split}.jsonl.partial").replace(final)
        digests[split] = hashlib.sha256(final.read_bytes()).hexdigest()
    meta["sha256"] = digests
    (out / f"{name}.meta.json").write_text(json.dumps(meta, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def git_state() -> dict[str, object]:
    repo = corpus.compare.REPO
    return {
        "harness_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip(),
        "harness_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=repo, text=True).strip()),
        "space_sha256": hashlib.sha256((HERE / "space.py").read_bytes()).hexdigest(),
        "pool_sha256": hashlib.sha256((HERE / "pool.py").read_bytes()).hexdigest(),
        "produce_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    }


def record_document(engine: object, units: dict[str, str], enhancers: dict[str, object], text: str,
                    language: str, needs_artifacts: bool) -> list[list]:
    """Every recognizer's raw results for one document, each with its score under every
    context mode: what AnalyzerEngine.analyze holds just before its threshold step."""
    from presidio_analyzer import RecognizerResult

    recognizers = engine.registry.get_recognizers(language=language, all_fields=True)
    entities = engine.get_supported_entities(language=language)
    artifacts = engine.nlp_engine.process_text(text, language) if needs_artifacts else None
    raw = []
    for recognizer in recognizers:
        found = recognizer.analyze(text=text, entities=entities, nlp_artifacts=artifacts) or []
        for result in found:
            # AnalyzerEngine.__add_recognizer_id_if_not_exists, in effect.
            result.recognition_metadata = result.recognition_metadata or {}
            result.recognition_metadata.setdefault(RecognizerResult.RECOGNIZER_IDENTIFIER_KEY, recognizer.id)
            result.recognition_metadata.setdefault(RecognizerResult.RECOGNIZER_NAME_KEY, recognizer.name)
        raw.extend(found)
    scored = {}
    for mode, context_enhancer in enhancers.items():
        engine.context_aware_enhancer = context_enhancer
        scored[mode] = (engine._enhance_using_context(text, copy.deepcopy(raw), artifacts, recognizers, None)
                        if needs_artifacts else copy.deepcopy(raw))
        if [(r.start, r.end, r.entity_type) for r in scored[mode]] != [(r.start, r.end, r.entity_type) for r in raw]:
            raise RuntimeError("context enhancement reordered results; cannot align scores")
    return [[result.start, result.end, result.entity_type,
             units[result.recognition_metadata[RecognizerResult.RECOGNIZER_IDENTIFIER_KEY]],
             *(scored[mode][index].score for mode in space.CONTEXT_MODES)]
            for index, result in enumerate(raw)]


def pool_pass(args: argparse.Namespace, layers: dict[str, list]) -> None:
    import pool
    from presidio_analyzer import AnalyzerEngine, RecognizerRegistry

    spacy_paths = {language: getattr(args, f"{language}_model") for language in space.LANGUAGES}
    paths = pool.ModelPaths(spacy=spacy_paths, dslim=args.dslim, davlan=args.davlan,
                            openmed=args.openmed, gliner=args.gliner)
    pins = json.loads((HERE / "model-pins.json").read_text(encoding="utf-8"))
    models = {language: corpus.compare.model_info(path, language) for language, path in spacy_paths.items()}
    if args.pass_name in ("dslim", "davlan", "openmed", "gliner"):
        models[args.pass_name] = check_pin(args.pass_name, getattr(args, args.pass_name), pins)
    if args.pass_name == "gliner":
        models["gliner_tokenizer"] = check_pin("gliner_tokenizer", args.gliner_tokenizer, pins)
    engines = pool.nlp_engines(paths, dslim=args.pass_name == "dslim")
    extras = pool.Extras(paths, pool.pool_entities()) if args.pass_name in pool.EXTRA_UNITS else None
    languages = ("en",) if args.pass_name == "dslim" else space.LANGUAGES
    analyzers, units = {}, {}
    for language in languages:
        if args.pass_name == "base":
            members = pool.base_recognizers(language)
        elif args.pass_name == "dslim":
            from presidio_analyzer.predefined_recognizers import SpacyRecognizer

            members = [(pool.DSLIM_UNIT, SpacyRecognizer(supported_language="en"))]
        else:
            members = [(pool.EXTRA_UNITS[args.pass_name], extras.recognizer(args.pass_name, language))]
        registry = RecognizerRegistry(supported_languages=[language])
        for unit, recognizer in members:
            registry.add_recognizer(recognizer)
            units[recognizer.id] = unit
        analyzers[language] = AnalyzerEngine(
            nlp_engine=engines[language], registry=registry, supported_languages=[language],
            default_score_threshold=0.0,
        )
    # The pool pass records results for every language the pass covers; GLiNER's
    # label set must not depend on the recorded entity list (see space.GLINER_LABELS).
    enhancers = {mode: pool.enhancer(mode) for mode in space.CONTEXT_MODES}
    needs_artifacts = args.pass_name in ("base", "dslim")
    sinks = split_sinks(args.out, args.pass_name)
    started, count = time.perf_counter(), 0
    for layer in corpus.LAYERS:
        for document in layers[layer]:
            row = {"uid": document.uid, "layer": layer, "r": []}
            engine = analyzers.get(document.language)
            if engine is not None:
                row["r"] = record_document(engine, units, enhancers, document.text, document.language,
                                           needs_artifacts)
            sinks[corpus.split_for_id(document.uid)].write(json.dumps(row, ensure_ascii=False) + "\n")
            count += 1
            if count % 500 == 0:
                print(f"{args.pass_name}: {count} documents, {time.perf_counter() - started:.0f} s",
                      file=sys.stderr, flush=True)
    finish(args.out, args.pass_name, sinks, {
        "pass": args.pass_name, "documents": count, "models": models, "smoke": args.smoke,
        "context_modes": list(space.CONTEXT_MODES), "seconds": round(time.perf_counter() - started, 1),
        "python_hash_seed": os.environ.get("PYTHONHASHSEED"),
        "versions": {name: importlib.metadata.version(name) for name in (
            "presidio-analyzer", "presidio-anonymizer", "spacy", "transformers", "torch", "gliner")},
        **git_state(),
    })


def authors_pass(args: argparse.Namespace, layers: dict[str, list]) -> None:
    """Presidio Research's tuned analyzer, unchanged. It supports English only, so every
    document is analyzed with language="en", as its evaluator calls it."""
    sys.path.insert(0, str(HERE.parent / "theirbench"))
    import presidio_research_repro as repro
    import tagged_gaze
    from presidio_evaluator.models import PresidioAnalyzerWrapper

    pinned = json.loads((HERE.parent / "theirbench/vendor-tuned.json").read_text(encoding="utf-8"))
    pinned = pinned["presidio-research"]["model"]
    receipt = {**pinned, "tree_sha256": tuned_presidio_tree(args.openmed)}
    try:
        tagged_gaze.check_model_receipt(receipt, pinned, "--openmed")
    except ValueError as error:
        raise SystemExit(str(error)) from error
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=args.checkout, text=True).strip()
    if head != repro.COMMIT:
        raise SystemExit(f"presidio-research checkout is at {head}, expected {repro.COMMIT}")
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=args.checkout, text=True).strip():
        raise SystemExit("the presidio-research checkout has local changes")
    engine, threshold = repro.custom_analyzer(str(args.openmed))
    wrapper = PresidioAnalyzerWrapper(analyzer_engine=engine, score_threshold=threshold, language="en")
    kwargs: dict[str, object] = {}
    wrapper._PresidioAnalyzerWrapper__update_kwargs(kwargs)
    sinks = split_sinks(args.out, "authors")
    started, count = time.perf_counter(), 0
    for layer in corpus.LAYERS:
        for document in layers[layer]:
            found = engine.analyze(text=document.text, **kwargs)
            rows = sorted([r.start, r.end, r.entity_type, float(r.score)] for r in found)
            sinks[corpus.split_for_id(document.uid)].write(
                json.dumps({"uid": document.uid, "layer": layer, "r": rows}, ensure_ascii=False) + "\n")
            count += 1
            if count % 500 == 0:
                print(f"authors: {count} documents, {time.perf_counter() - started:.0f} s", file=sys.stderr, flush=True)
    finish(args.out, "authors", sinks, {
        "pass": "authors", "documents": count, "smoke": args.smoke, "analyzer_kwargs": {k: kwargs[k] for k in ("language", "score_threshold")},
        "openmed": receipt, "model_verified_before_inference": True, "presidio_research_commit": head,
        "seconds": round(time.perf_counter() - started, 1),
        "versions": {name: importlib.metadata.version(name) for name in (
            "presidio-analyzer", "presidio-anonymizer", "presidio-evaluator", "spacy", "en-core-web-sm",
            "transformers", "torch")},
        **git_state(),
    })


def tuned_presidio_tree(path: Path) -> str:
    """The tree digest `theirbench/tuned_presidio.py` pins OpenMed with."""
    sys.path.insert(0, str(HERE.parent / "theirbench"))
    import tuned_presidio

    return tuned_presidio.model_sha256(path)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="mode", required=True)
    for mode in ("pool", "authors"):
        command = sub.add_parser(mode)
        command.add_argument("--dataset", type=Path, required=True)
        command.add_argument("--out", type=Path, required=True, help="directory outside the repository")
        command.add_argument("--smoke", type=int, help="first N validation documents per layer; never scored")
    pool_cmd = sub.choices["pool"]
    pool_cmd.add_argument("--pass", dest="pass_name", choices=PASSES, required=True)
    for language in space.LANGUAGES:
        pool_cmd.add_argument(f"--{language}-model", type=Path, required=True)
    for name in ("dslim", "davlan", "openmed", "gliner", "gliner-tokenizer"):
        pool_cmd.add_argument(f"--{name}", type=Path)
    authors_cmd = sub.choices["authors"]
    authors_cmd.add_argument("--checkout", type=Path, required=True)
    authors_cmd.add_argument("--openmed", type=Path, required=True)
    args = parser.parse_args()
    if args.out.resolve().is_relative_to(corpus.compare.REPO):
        raise SystemExit("raw findings must stay outside the repository")
    if os.environ.get("PYTHONHASHSEED") != "0":
        raise SystemExit("set PYTHONHASHSEED=0: Presidio de-duplicates through a set")
    layers = corpus.load_measured(args.dataset, corpus.read_comparison())
    if args.smoke:
        # Mechanics only, on a few validation documents per layer; tune.py refuses the output.
        layers = {layer: list(docs[: args.smoke]) for layer, docs in corpus.validation_only(layers).items()}
    (pool_pass if args.mode == "pool" else authors_pass)(args, layers)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
