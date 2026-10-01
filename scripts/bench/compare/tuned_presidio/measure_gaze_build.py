#!/usr/bin/env python3
"""Run a Gaze build on the tuned-Presidio comparison corpus and keep per-document spans.

Runs with the Gaze checkout's OWN harness modules (`--gaze-root`), because the
bench binary's protocol belongs to that checkout. The documents are the ones
`comparison.json` measured: layer C and the agentic layers of the generator
version it names, checked by document-id digest. Output (outside the
repository): `<out>/<layer>.jsonl`, one row per document with byte spans or a
refusal, plus `<out>/meta.json`. No text is written.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

COMPARISON = Path(__file__).resolve().parents[4] / "docs/reference/benchmarks/comparison.json"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--gaze-root", type=Path, required=True, help="clean Gaze checkout (its harness and binary)")
    parser.add_argument("--binary", type=Path, required=True, help="that checkout's clean_for_bench")
    parser.add_argument("--policy", type=Path, required=True, help="that checkout's `gaze setup` policy")
    parser.add_argument("--model-dir", type=Path, required=True)
    parser.add_argument("--dataset", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    root = args.gaze_root.resolve()
    if args.out.resolve().is_relative_to(root) or args.out.resolve().is_relative_to(COMPARISON.parents[3]):
        raise SystemExit("per-document spans must stay outside both repositories")
    if subprocess.check_output(["git", "status", "--porcelain", "--", "crates", "scripts"], cwd=root, text=True).strip():
        raise SystemExit("the Gaze checkout has local changes")
    sys.path.insert(0, str(root / "scripts/bench/compare"))
    sys.path.insert(0, str(root / "scripts/bench"))
    import compare  # the checkout's own modules

    comparison = json.loads(COMPARISON.read_text(encoding="utf-8"))
    layers, _identity = compare.load_corpus(args.dataset, None)
    agentic = compare.agentic
    version = comparison["corpus"]["agentic"]["generator_version"]
    records = agentic.records_as_of(version, agentic.generate(agentic.PUBLISHED_PARTITION))
    contract = agentic.load_contract(root, version=version)
    documents = agentic.apply_contract([record.to_document() for record in records], contract)
    corpus = {"C": layers["C"], **{layer: [d for d in documents if d.cell and d.cell.startswith(layer + "|")]
                                   for layer in ("A", "D", "R")}}
    for layer, docs in corpus.items():
        digest = compare.score.document_ids_digest([d.uid for d in docs])
        if digest != comparison["corpus"]["layers"][layer]["ids_sha256"]:
            raise SystemExit(f"layer {layer} documents differ from comparison.json")
    args.out.mkdir(parents=True, exist_ok=True)
    counts = {}
    for layer, docs in corpus.items():
        rows = {}

        def record(_config, document, response, _measurements) -> None:
            if "pipeline_error_code" in response:
                rows[document.uid] = {"uid": document.uid, "refused": response["pipeline_error_code"], "spans": []}
                return
            spans = compare.score.final_trace_predictions(document, response)
            rows[document.uid] = {"uid": document.uid, "refused": None,
                                  "spans": [[s.start, s.end, s.label] for s in spans]}

        compare.score.run_config(root, args.binary, "policy-file", docs, args.model_dir, None, None, None, 0.5,
                                 args.out / "diagnostics" / layer, policy_path=args.policy, record_document=record)
        if set(rows) != {d.uid for d in docs}:
            raise SystemExit(f"layer {layer}: not every document was recorded")
        with (args.out / f"{layer}.jsonl").open("w", encoding="utf-8") as sink:
            for document in docs:
                sink.write(json.dumps(rows[document.uid]) + "\n")
        counts[layer] = {"documents": len(docs), "refused": sum(1 for r in rows.values() if r["refused"])}
        print(f"{layer}: {counts[layer]}", file=sys.stderr, flush=True)
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()  # noqa: E731
    (args.out / "meta.json").write_text(json.dumps({
        "gaze_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
        "crates_tree": subprocess.check_output(["git", "rev-parse", "HEAD:crates"], cwd=root, text=True).strip(),
        "binary_sha256": sha(args.binary), "policy_sha256": sha(args.policy),
        "model_tree_sha256": compare.digest_tree(args.model_dir), "counts": counts,
        "spans_sha256": {layer: sha(args.out / f"{layer}.jsonl") for layer in corpus},
    }, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
