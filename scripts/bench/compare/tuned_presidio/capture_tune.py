#!/usr/bin/env python3
"""Measure, privately replay, and render tuned rows with independently bound inputs."""
from __future__ import annotations
import sys
from pathlib import Path
HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))
import tune
from tune import *
import capture_produce
import capture_pool
import input_binding
import document_capture
import subject_identity
from capture_identity import digest


def prepare(args):
    comparison = corpus.read_comparison()
    layers = corpus.load_measured(args.dataset, comparison)
    plans = json.loads(args.production_plans.read_text())
    if set(plans) != {*POOL_PASSES, "authors"}:
        raise ValueError("incomplete independent production plans")
    subjects = {}
    specifications = []
    gliner_plan = None
    for name in (*POOL_PASSES, "authors"):
        production = capture_produce.parse_args(plans[name])
        actual = production.pass_name if production.mode == "pool" else "authors"
        if (actual != name or production.dataset.resolve() != args.dataset.resolve()
                or production.out.resolve() != (args.authors if name == "authors" else args.pool).resolve()):
            raise ValueError("production plan selects different inputs")
        subject = capture_produce.verify_output(production, layers)
        specifications.append(json.loads(production.benchmark_spec.read_text()))
        subjects[name] = subject
        if name == "gliner":
            gliner_plan = capture_produce.prepare(production, layers).gliner_plan
    if len({spec["id"] for spec in specifications}) != 1:
        raise ValueError("production specifications differ")
    paths = pool.ModelPaths(spacy={language: getattr(args, f"{language}_model") for language in space.LANGUAGES},
                            dslim=args.dslim, davlan=args.davlan, openmed=args.openmed, gliner=args.gliner)
    base = {name: subject_identity.model_digest(path) for name, path in paths.spacy.items()}
    for name in POOL_PASSES:
        expected = dict(base)
        if name != "base":
            expected[name] = subject_identity.model_digest(getattr(paths, name))
        if subjects[name]["models"] != expected:
            raise ValueError("live models differ from independent production plan")
    selection = json.loads(SELECTION.read_text())
    if selection["code_sha256"]["space.py"] != sha256(HERE / "space.py"):
        raise ValueError("selection uses a different search space")
    metas = pool_meta(args.pool)
    for name, meta in metas.items():
        if meta["sha256"]["validation"] != selection["pool"][name]["validation_sha256"]:
            raise ValueError("selection did not read these production inputs")
    with corpus.measured_agentic_contract(comparison):
        found = search.read_pool(args.pool, POOL_PASSES, "validation")
        found.update(search.read_pool(args.pool, POOL_PASSES, "test"))
        docs = search.make_docs(layers, found, contracts())
        mapping = tuned_mapping()
        replayer = search.Replayer(docs, mapping, compare.typed_mapping_for_contract(mapping, "v3"))
    files = [SELECTION, corpus.COMPARISON, args.production_plans,
             *[folder / f"{name}.{split}.jsonl" for name, folder in
               [(name, args.pool) for name in POOL_PASSES] + [("authors", args.authors)] for split in produce_splits()]]
    # The fixed live sample uses the same objects, config and model selectors as scoring.
    def live(config, selected_paths, replay):
        return verify_live(config, selected_paths, replay, gliner_plan)
    inputs = input_binding.Inputs(selection=selection, replayer=replayer, paths=paths, files=files,
        sample=in_live_sample, live_verify=live, rows={objective: ROWS[objective] for objective in space.OBJECTIVES},
        gliner_plan=gliner_plan, runtime_reader=subject_identity.runtime)
    inputs.layers = layers
    inputs.spec = specifications[0]
    return inputs


def verify_live(config, paths, replayer, gliner_plan):
    analyzers = capture_pool.live_analyzers(config, paths, gliner_plan=gliner_plan)
    differences = {}
    count = 0
    for index, doc in enumerate(replayer.docs):
        if not in_live_sample(doc.uid):
            continue
        count += 1
        def live():
            found = pool.live_analyze(analyzers, config, doc.document.text, doc.language)
            return compare.resolved_presidio_spans(replayer.anonymizer, doc.document.text, found)
        replayed = replayer.predict(index, config)
        first = live()
        if first != replayed:
            differences[doc.uid] = {"persistent": all(live() != replayed for _ in range(2)),
                                    "live": input_binding.canonical(first), "replay": input_binding.canonical(replayed)}
    # Per-document differences remain private.
    return {"documents": count, "differences": differences}


def public_report(report):
    import copy
    public = copy.deepcopy(report)
    for provenance in public["provenance"].values():
        if "live_verification" in provenance:
            live = provenance["live_verification"]
            provenance["live_verification"] = {"binding_sha256": digest(live), "documents": live["documents"],
                "differing_documents": len(live["differences"]),
                "persistent_differing_documents": sum(item["persistent"] for item in live["differences"].values())}
            config = provenance.pop("config")
            provenance["config_binding"] = {"sha256": digest(config), "allow_list_entries": len(config.get("allow_list", []))}
    return public


def admit(args, inputs, private_path, submitted):
    private = json.loads(private_path.read_text())
    original = private["report"]
    def reproduce(objective):
        uid_index = {doc.uid: index for index, doc in enumerate(inputs.replayer.docs)}
        with corpus.measured_agentic_contract(corpus.read_comparison()):
            return strip_latency(compare.measure(ROWS[objective], replay_predictor(inputs.replayer, inputs.config(objective), uid_index),
                inputs.layers, tuned_mapping(), None, frozenset(corpus.read_comparison()["common_intersection_labels"])))
    inputs.admit(original, private_path, reproduce)
    documents = {d.uid: d for docs in inputs.layers.values() for d in docs}
    predictions = authors_predictions(args.authors, documents)
    with corpus.measured_agentic_contract(corpus.read_comparison()):
        authors = strip_latency(compare.measure(ROWS["authors"], lambda doc: predictions[doc.uid], inputs.layers,
            tuned_mapping(), None, frozenset(corpus.read_comparison()["common_intersection_labels"])))
    if digest(authors) != digest(original["rows"][ROWS["authors"]]):
        raise ValueError("private authors replay differs from submitted computation")
    if digest(public_report(original)) != digest(submitted):
        raise ValueError("public tuned projection differs from private computation")
    return submitted


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("dataset", "pool", "authors", "production-plans", "private-dir", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    for language in space.LANGUAGES:
        parser.add_argument(f"--{language}-model", type=Path, required=True)
    for name in ("dslim", "davlan", "openmed", "gliner"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--budget-source-revision", required=True)
    parser.add_argument("--verify-report", type=Path)
    parser.add_argument("--render", type=Path, help="render admitted aggregate bytes as Markdown")
    args = parser.parse_args()
    args.skip_live, args.allow_anchor_mismatch = False, False
    require_hash_seed()
    root = document_capture.private_root(args.private_dir, compare.REPO)
    private = root / "tuned-report.json"
    inputs = prepare(args)
    if args.verify_report:
        report = admit(args, inputs, private, json.loads(args.verify_report.read_text()))
    else:
        report = public_report(measure(args, inputs, private))
        admit(args, inputs, private, report)
        compare.write_report(args.output, report)
    if args.render:
        # Rendering is downstream of independent input reconstruction and replay admission.
        lines = ["| Tool | Layer | Leaked bytes | False-positive bytes |", "| --- | --- | ---: | ---: |"]
        for name, row in report["rows"].items():
            for layer, cell in row["contracts"]["v3"].items():
                lines.append(f"| {name} | {layer} | {cell['leaked_bytes']} | {cell['false_positive_bytes']} |")
        args.render.write_text("\n".join(lines) + "\n")



def measure(args, inputs, private_path):
    require_hash_seed()
    measured_budget = budget(args.budget_source_revision)
    state = git_state()
    comparison = corpus.read_comparison()
    selection = inputs.selection
    metas = pool_meta(args.pool)
    authors_meta = json.loads((args.authors / "authors.meta.json").read_text())
    layers = inputs.layers
    documents = {d.uid: d for docs in layers.values() for d in docs}
    mapping = tuned_mapping()
    common = frozenset(comparison["common_intersection_labels"])
    report_rows, provenance = {}, {}
    with corpus.measured_agentic_contract(comparison):
        replayer = inputs.replayer
        docs = replayer.docs
        uid_index = {doc.uid: index for index, doc in enumerate(docs)}
        anchor_row = compare.measure("presidio-anchor", replay_predictor(replayer, search.start_config("presidio-default"), uid_index),
                                     layers, mapping, None, common)
        anchored = anchor(anchor_row, comparison)
        print(f"anchor: {anchored['equal']}", file=sys.stderr, flush=True)
        if not anchored["equal"] and not args.allow_anchor_mismatch:
            raise SystemExit("replay does not reproduce presidio-all: " + "; ".join(anchored["mismatches"][:10]))
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
                                "config": config, "live_verification": inputs.verify_live(objective),
                                "live_inputs": inputs.public()}
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
        "benchmark_spec": inputs.spec,
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
        "budget": {**measured_budget, "candidates_evaluated": selection["candidates_evaluated"],
                   "validation_documents": selection["validation_documents"]},
        "code_sha256": code_digests(),
        "compare_sha256": sha256(Path(compare.__file__)),
        "comparison_metrics_sha256": sha256(HERE.parent / "comparison_metrics.py"),
        "scorer_sha256": sha256(REPO / "scripts/bench/gaze_bench_score.py"),
        "python_hash_seed": os.environ["PYTHONHASHSEED"],
        **state,
    }
    inputs.seal(report, private_path)
    return report



if __name__ == "__main__":
    main()
