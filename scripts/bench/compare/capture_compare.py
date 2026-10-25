#!/usr/bin/env python3
"""Capture or reuse competitor documents under an independent measurement plan."""
from __future__ import annotations

import argparse
import json
import sys
from contextlib import nullcontext
from pathlib import Path

import compare
import document_capture as captures
import subject_identity as identity
from capture_identity import digest, file_digest
from capture_launch import initialize

initialize()


def measure(session, name, predictor, layers, mapping, languages, common, predictions_dir=None, *, receipt=None):
    stored, elapsed = {}, {}
    for layer, documents in layers.items():
        elapsed[layer] = []
        for document in documents:
            if languages is not None and document.language not in languages:
                stored[document.uid] = []
            else:
                spans, milliseconds = session.capture(document, predictor, receipt=receipt)
                stored[document.uid] = spans
                elapsed[layer].append(milliseconds)
    result = compare.measure(name, lambda document: stored[document.uid], layers, mapping,
                             languages, common, predictions_dir)
    for contract in result["contracts"].values():
        for layer, cell in contract.items():
            timers = elapsed[layer]
            cell["latency"] = {
                "p50_ms": round(compare.score.percentile(timers, 0.5), 3) if timers else None,
                "p95_ms": round(compare.score.percentile(timers, 0.95), 3) if timers else None,
                "samples": len(timers)}
    result["capture_binding"] = session.finish()
    return result


def configuration(args, name, mappings):
    models = {}
    config = {"tool": name, "mapping": compare.mapping_for_tool(name, mappings),
              "device": "cpu", "warmups": 1}
    if name.startswith("presidio"):
        config["languages"] = list(compare.presidio_languages(name))
        models = {language: getattr(args, f"{language}_model") for language in config["languages"]}
        config["threshold"] = 0.0 if name.endswith("high-recall") else 0.3 if name.startswith("presidio-strong") else 0.0
        if name.startswith("presidio-strong"):
            models["transformer"] = args.transformer_model
    elif name in {"gliner", "gliner-high-recall"}:
        models = {"model": args.gliner_model, "tokenizer": args.gliner_tokenizer}
        config.update(labels=list(mappings["gliner"]), flat_ner=True,
                      threshold=0.3 if name.endswith("high-recall") else 0.5)
    elif name.startswith("datafog-"):
        config["engine"] = name.removeprefix("datafog-")
    elif name.startswith("scrubadub-"):
        config["spacy_model"] = compare.SPACY_MODELS["en"] if name.endswith("spacy") else None
    else:
        raise ValueError("external OPF capture is not qualified")
    if any(path is None for path in models.values()):
        raise ValueError("missing selected competitor model")
    return config, models


def backend(name, config, models, plan):
    if name.startswith("presidio"):
        return compare.Presidio({key: value for key, value in models.items() if key != "transformer"},
                                models.get("transformer"), config["threshold"]), None
    if name in {"gliner", "gliner-high-recall"}:
        with identity.gliner_identity.observe(plan) as observed:
            result = compare.Gliner(models["model"], config["labels"], config["threshold"])
        return result, observed.receipt(result.model)
    if name == "datafog-core":
        return compare.DataFogCore(), None
    if name.startswith("datafog-"):
        return compare.DataFogPython(config["engine"]), None
    return compare.Scrubadub(config["spacy_model"]), None


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tool", choices=[name for name in compare.TOOLS if name != "opf"], required=True)
    parser.add_argument("--dataset", type=Path, required=True)
    parser.add_argument("--pack-dir", type=Path, default=compare.REPO / "docs/reference/benchmarks/variant-packs")
    parser.add_argument("--benchmark-spec", type=Path, required=True)
    parser.add_argument("--cache-dir", type=Path, required=True)
    parser.add_argument("--predictions-dir", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--freshness-report", type=Path)
    parser.add_argument("--verify-report", type=Path, help="admit a public report against private captures; no inference")
    for name in ("en-model", "de-model", "nl-model", "fr-model", "pt-model", "transformer-model",
                 "gliner-model", "gliner-tokenizer"):
        parser.add_argument("--" + name, type=Path)
    return parser.parse_args(argv)


def run(args):
    import benchmark_spec
    spec = benchmark_spec.validate(json.loads(args.benchmark_spec.read_text()))
    benchmark_spec.require_current(spec, compare.REPO, dataset=args.dataset)
    root = captures.private_root(args.cache_dir, compare.REPO)
    if args.predictions_dir:
        captures.private_root(args.predictions_dir, compare.REPO)
    layers, corpus = compare.load_corpus(args.dataset, args.pack_dir)
    identity.require_population(spec, layers)
    compare.preflight_contracts(layers)
    mappings = compare.load_mapping()
    name = args.tool
    config, models = configuration(args, name, mappings)
    runtime = identity.runtime()
    plan = None
    if name in identity.GLINER:
        request = (identity.gliner_identity.Request.datafog() if name == "datafog-gliner" else
                   identity.gliner_identity.Request("standalone", str(args.gliner_model), {}))
        plan = identity.plan_gliner(request, runtime)
    read = lambda: identity.subject(name=name, config=config, models=models,
        runtime_identity=identity.runtime(), adapter=Path(__file__), gliner_plan=plan)
    read = identity.freeze_spec(args.benchmark_spec, compare.REPO, args.dataset, read)
    session = captures.Session(root, spec, read, compare.mapping_for_tool(name, mappings),
                               gliner_plan=plan, read_only=bool(args.verify_report))
    loaded = {}
    def predictor(document):
        if args.verify_report:
            raise ValueError("private admission requires a complete capture")
        if not loaded:
            instance, receipt = backend(name, config, models, plan)
            warmup = compare.score.Document("warmup", "alice@example.invalid", "en", "", "synthetic", ())
            with identity.gliner_identity.observe(plan) if name == "datafog-gliner" else nullcontext() as observed:
                instance.predict(warmup)
            if observed is not None:
                import datafog.engine as engine
                receipt = observed.receipt(engine._get_gliner_annotator().model)
            loaded.update(instance=instance, receipt=receipt)
        return loaded["instance"].predict(document)
    receipt = lambda: loaded.get("receipt")
    languages = (config["languages"] if name.startswith("presidio") else
                 ("en", "de", "nl", "fr", "pt", "es") if name.startswith("scrubadub") else None)
    with compare.ForeignCpuSampler() as sampler:
        row = measure(session, name, predictor, layers, compare.mapping_for_tool(name, mappings),
                      languages, compare.common_claimed_labels(mappings), args.predictions_dir, receipt=receipt)
    row["provenance"] = {"config_sha256": digest(config), "subject_sha256": digest(session.subject)}
    row, contention = session.retain_row(row, sampler.result())
    report = {"schema_version": 1, "benchmark_spec": spec, "corpus": corpus,
              "tools": {name: row}, "contention_samples": {name: contention}}
    if args.freshness_report:
        # Freshness is an independent aggregate sidecar; it never alters subject execution.
        report["freshness"] = json.loads(args.freshness_report.read_text())
        report["freshness_sha256"] = file_digest(args.freshness_report)
    if args.verify_report:
        if digest(json.loads(args.verify_report.read_text())) != digest(report):
            raise ValueError("private captures do not reproduce submitted report")
        return report
    compare.write_report(args.output, report)
    print(f"CAPTURE_REUSE {name} hits={session.hits} fresh={session.misses}", file=sys.stderr)
    return report


if __name__ == "__main__":
    run(parse_args())
