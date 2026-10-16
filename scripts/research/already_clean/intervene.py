"""Local causal influence on model outputs, not proof of a PII circuit."""

import argparse
import json
from pathlib import Path

import numpy as np
import onnx
from onnx import helper, numpy_helper
from tokenizers import Tokenizer

from extract import MODEL_ROOT, SPECS, encode, session, softmax, taps, target_indices


def ablate(source, destination, tensor, width, coordinates):
    graph = onnx.load(str(source))
    mask = np.ones(width, dtype=np.float32)
    mask[coordinates] = 0
    mask_name, output_name = "research_coordinate_mask", tensor + "_ablated"
    graph.graph.initializer.append(numpy_helper.from_array(mask, mask_name))
    nodes = []
    found = False
    for node in graph.graph.node:
        for index, name in enumerate(node.input):
            if name == tensor:
                node.input[index] = output_name
        nodes.append(node)
        if tensor in node.output:
            assert not found
            nodes.append(helper.make_node("Mul", [tensor, mask_name], [output_name], name="ResearchCoordinateAblation"))
            found = True
    assert found
    del graph.graph.node[:]
    graph.graph.node.extend(nodes)
    onnx.checker.check_model(graph)
    onnx.save(graph, str(destination))


def run(directory, output):
    rows = [json.loads(line) for line in (directory / "fixtures.jsonl").read_text().splitlines()]
    frozen = json.loads((directory / "frozen-selections.json").read_text())
    # Deterministic 36-case test slice: one value in each template/language parent.
    sampled = [r for r in rows if r["evaluation"] == "test" and r["id"].split('/')[3] == "0"]
    results = {"scope": "36 synthetic held-out documents; global zero ablation of one layer, no production mutation; outputs only", "models": {}}
    for model, spec in SPECS.items():
        selection = frozen["selections"][f"{model}/native_8"]
        layer = int(selection["tensor"].split('_')[1])
        coordinates = selection["coordinates"]
        tensor, width = taps(model, [layer])[f"mlp_{layer}"]
        source = MODEL_ROOT / spec["directory"] / spec["file"]
        tokenizer = Tokenizer.from_file(str(MODEL_ROOT / spec["directory"] / "tokenizer.json"))
        tokenizer.no_truncation()
        tokenizer.no_padding()
        labels_dict = json.loads((MODEL_ROOT / spec["directory"] / "config.json").read_text())["id2label"]
        labels = [labels_dict[str(i)] for i in range(len(labels_dict))]
        data = np.load(directory / f"{model}-features.npz")
        train = np.asarray([r["split"] == "train" for r in rows])
        energy = np.mean(np.square(data[f"mlp_{layer}"][train]), axis=0)
        available = set(range(width)) - set(coordinates)
        matched = []
        for coordinate in coordinates:
            chosen = min(available, key=lambda i: abs(float(energy[i] - energy[coordinate])))
            matched.append(chosen)
            available.remove(chosen)
        groups = {"selected": coordinates, "magnitude_matched": matched}
        for seed in [7, 13, 23]:
            groups[f"random_{seed}"] = np.random.default_rng(seed).choice(sorted(set(range(width)) - set(coordinates)), len(coordinates), replace=False).tolist()
        original = session(source)
        reference = {}
        for row in sampled:
            encoding, feed = encode(tokenizer, row["text"], original)
            reference[row["id"]] = original.run(["logits"], feed)[0][0]
        del original
        model_result = {"layer": layer, "coordinates": coordinates, "groups": {}}
        for group, mask_coordinates in groups.items():
            graph_path = directory / f"{model}-{group}-ablation.onnx"
            ablate(source, graph_path, tensor, width, mask_coordinates)
            runtime = session(graph_path)
            records = []
            for row in sampled:
                encoding, feed = encode(tokenizer, row["text"], runtime)
                logits = runtime.run(["logits"], feed)[0][0]
                assert np.isfinite(logits).all()
                a, b = row["span"]
                positions = [i for i, (left, right) in enumerate(encoding.offsets) if left < b and right > a and left < right]
                indices = target_indices(labels, row["target"])
                baseline = reference[row["id"]]
                before = float(softmax(baseline[positions])[:, indices].sum(-1).mean()) if indices else 0.0
                after = float(softmax(logits[positions])[:, indices].sum(-1).mean()) if indices else 0.0
                records.append({"id": row["id"], "family": row["family"], "target": row["target"], "positive": row["label"], "target_mass_before": before, "target_mass_after": after, "mass_delta": after - before,
                                "token_argmax_changes": int((baseline.argmax(-1) != logits.argmax(-1)).sum()), "max_logit_delta": float(np.abs(baseline - logits).max())})
            del runtime
            graph_path.unlink()
            model_result["groups"][group] = {"coordinates": mask_coordinates, "pooled_train_energy": float(energy[mask_coordinates].sum()), "records": records}
        results["models"][model] = model_result
    output.write_text(json.dumps(results, indent=2))
    print(json.dumps({"interventions": len(sampled) * len(SPECS) * 5, "failures": 0, "output": str(output)}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    run(args.directory, args.output)
