"""Build compared tools exactly as the main comparison does, without editing it.

compare.py builds its backends inline in main(). This module repeats those
constructor calls, argument for argument, for the comparison revision it
names, and refuses to run against any other compare.py: a change to the
main comparison must be mirrored here and reviewed, never drift silently.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

# scripts/bench/compare is also a directory under scripts/bench: put the module's
# own directory first so `compare` is compare.py, not a namespace package.
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
if getattr(sys.modules.get("compare"), "__file__", "") is None:
    del sys.modules["compare"]
import compare  # noqa: E402

COMPARISON_REVISION = "154f3da6"
PINNED_SHA256 = {
    "compare.py": "becbfc3dcafa69bb42d645f2c80055a68908e23fdc75c252dd01fb4e96ededb6",
    "comparison_metrics.py": "bd29a2c0d508bd950be572f59e27ea6000af2bd4c984271a21a554668f1af1c5",
    "label-map.json": "2e88a1839a6ae271da78aed35616930d6feff32de029b2cc58a1aae5d66acc8e",
    "chart-configs.json": "3d986092b7970db18c48a9e5ec645b83f98ea3204f016939ffdd3ba91c102d55",
    "cpu_contention.py": "7916c3e39ba3d2ebd67b47f5406958df2713e52a8e506f17fe053db0788ae600",
}
COMPARE_DIR = Path(compare.__file__).resolve().parent


def verify_pinned_comparison() -> dict[str, str]:
    actual = {name: hashlib.sha256((COMPARE_DIR / name).read_bytes()).hexdigest() for name in PINNED_SHA256}
    changed = sorted(name for name, digest in actual.items() if digest != PINNED_SHA256[name])
    if changed:
        raise SystemExit(f"{changed} differ from comparison revision {COMPARISON_REVISION}; "
                         "mirror the change in theirbench/backends.py and re-pin")
    return actual


def chart_configs() -> dict[str, str]:
    """The comparison's declared chart configuration per tool family (STEER 2)."""
    return json.loads((COMPARE_DIR / "chart-configs.json").read_text(encoding="utf-8"))


def add_tool_arguments(parser: argparse.ArgumentParser) -> None:
    """compare.py's tool flags, same names and meaning."""
    for language in compare.PRESIDIO_LANGUAGES:
        parser.add_argument(f"--{language}-model", type=Path)
    for flag in ("--gaze-policy", "--gliner-model", "--gliner-tokenizer", "--transformer-model",
                 "--opf-python", "--opf-checkpoint"):
        parser.add_argument(flag, type=Path)


def build_backend(name: str, args: argparse.Namespace, mappings: dict, scratch: Path):
    """(backend, provenance, mapping) for one tool, mirroring compare.main() at COMPARISON_REVISION."""
    c = compare
    if name == "opf" and (args.opf_python is None or args.opf_checkpoint is None):
        return None, {"skipped": "local OPF runtime or checkpoint not configured"}, None
    if name.startswith("presidio"):
        requested = c.presidio_languages(name)
        model_paths = {lang: getattr(args, f"{lang}_model") for lang in requested}
        if any(path is None for path in model_paths.values()):
            raise ValueError(f"{name} needs model paths for {', '.join(requested)}")
        strong = name.startswith("presidio-strong")
        if strong and args.transformer_model is None:
            raise ValueError("Presidio strong needs --transformer-model")
        threshold = 0.0 if name.endswith("high-recall") else 0.3 if strong else 0.0
        backend = c.Presidio(model_paths, args.transformer_model if strong else None, threshold=threshold)
        provenance = {
            "analyzer_version": c.package_version("presidio-analyzer"),
            "anonymizer_version": c.package_version("presidio-anonymizer"),
            "spacy_version": c.package_version("spacy"),
            "models": {lang: c.model_info(path, lang) for lang, path in model_paths.items()},
            "supported_languages": backend.languages,
            "german_recognizers": list(c.GERMAN_RECOGNIZERS) if "de" in requested else [],
            "score_threshold": threshold,
        }
        if strong:
            provenance["transformer"] = {"repo": "dslim/bert-base-NER",
                                         "revision": "d1a3e8f13f8c3566299d95fcfc9a8d2382a9affc",
                                         "sha256": c.digest_tree(args.transformer_model)}
        return backend, provenance, mappings["presidio"]
    if name in {"gliner", "gliner-high-recall"}:
        if args.gliner_model is None:
            raise ValueError("GLiNER needs --gliner-model")
        threshold = 0.3 if name == "gliner-high-recall" else 0.5
        backend = c.Gliner(args.gliner_model, tuple(mappings["gliner"]), threshold)
        return backend, {"gliner_version": c.package_version("gliner"), "model_repo": c.GLINER_REPO,
                         "model_sha256": c.digest_tree(args.gliner_model),
                         "tokenizer_sha256": c.digest_tree(args.gliner_tokenizer),
                         "threshold": threshold}, mappings["gliner"]
    if name == "datafog-core":
        return c.DataFogCore(), {"version": c.package_version("datafog-core"),
                                 "mode": "built-in text recognizers only"}, mappings["datafog-core"]
    if name.startswith("datafog-"):
        engine = name.removeprefix("datafog-")
        return c.DataFogPython(engine), {
            "version": c.package_version("datafog"), "engine": engine,
            "model_sha256": c.digest_tree(args.gliner_model) if engine == "gliner" else None,
            "spacy_model": c.model_info(args.en_model, "en") if engine == "spacy" else None,
        }, mappings["datafog-python"]
    if name.startswith("scrubadub-"):
        if name == "scrubadub-spacy" and args.en_model is None:
            raise ValueError("scrubadub spaCy needs --en-model")
        backend = c.Scrubadub(c.SPACY_MODELS["en"] if name == "scrubadub-spacy" else None)
        return backend, {"version": c.package_version("scrubadub"),
                         "spacy_model": c.model_info(args.en_model, "en") if name == "scrubadub-spacy" else None,
                         }, mappings["scrubadub"]
    if name == "opf":
        backend = c.Opf(args.opf_python, args.opf_checkpoint, scratch)
        return backend, {"checkpoint_sha256": c.digest_tree(args.opf_checkpoint),
                         "runtime": c.opf_runtime_info(args.opf_python)}, mappings["opf"]
    raise ValueError(f"unknown tool {name}")
