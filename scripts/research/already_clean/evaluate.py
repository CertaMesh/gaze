"""Candidate classification only. Train/dev selection precedes any test scoring."""

import argparse
import hashlib
import json
import os
from pathlib import Path

os.environ.setdefault("OMP_NUM_THREADS", "2")
os.environ.setdefault("OPENBLAS_NUM_THREADS", "2")

import numpy as np
from scipy import sparse
from sklearn.feature_extraction.text import TfidfVectorizer
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import average_precision_score, roc_auc_score
from sklearn.preprocessing import StandardScaler

from fixtures import validate


def metrics(rows, scores, threshold):
    truth = np.asarray([r["label"] for r in rows], dtype=bool)
    predicted = np.asarray(scores) >= threshold
    weights = np.asarray([len(r["value"].encode()) for r in rows])
    tp, fp, fn = int((predicted & truth).sum()), int((predicted & ~truth).sum()), int((~predicted & truth).sum())
    protected = int(weights[predicted & truth].sum())
    leaked = int(weights[~predicted & truth].sum())
    false = int(weights[predicted & ~truth].sum())
    return {"documents": len(rows), "tp": tp, "fp": fp, "fn": fn,
            "span_precision": tp / (tp + fp) if tp + fp else None,
            "span_recall": tp / (tp + fn) if tp + fn else None,
            "protected_bytes": protected, "missed_bytes": leaked, "false_positive_bytes": false,
            "byte_precision": protected / (protected + false) if protected + false else None,
            "byte_recall": protected / (protected + leaked) if protected + leaked else None,
            "average_precision": float(average_precision_score(truth, scores)) if truth.any() and not truth.all() else None,
            "roc_auc": float(roc_auc_score(truth, scores)) if truth.any() and not truth.all() else None}


def thresholds(rows, scores):
    y = np.asarray([r["label"] for r in rows], dtype=bool)
    assert y.any() and (~y).any() and np.isfinite(scores).all()
    positive = np.sort(scores[y])
    return {"dev_zero_fp": float(np.nextafter(scores[~y].max(), np.inf)),
            "dev_recall_95": float(positive[len(positive) - int(np.ceil(0.95 * len(positive)))])}


def fit_best(x, y, train, dev, c_values=(0.01, 0.1, 1.0, 10.0)):
    best = None
    for c in c_values:
        classifier = LogisticRegression(C=c, solver="liblinear", max_iter=2000, random_state=20261016)
        classifier.fit(x[train], y[train])
        scores = classifier.predict_proba(x)[:, 1]
        criterion = float(average_precision_score(y[dev], scores[dev]))
        if best is None or criterion > best[0]:
            best = criterion, scores, c, classifier
    assert best is not None
    return best[1], {"C": best[2], "dev_average_precision": best[0]}, best[3]


def standardize(x, train):
    scaler = StandardScaler().fit(x[train])
    return scaler.transform(x)


def correlation_order(x, y, train):
    z = standardize(x, train)
    centered = y[train] - y[train].mean()
    importance = np.abs(z[train].T @ centered)
    return np.argsort(-importance, kind="stable"), z


def rotate(x, seed):
    # An orthogonal Walsh-Hadamard change of basis after zero padding. No new information.
    width = 1 << (x.shape[1] - 1).bit_length()
    rng = np.random.default_rng(seed)
    z = np.zeros((len(x), width), dtype=np.float32)
    z[:, :x.shape[1]] = x[:, rng.permutation(x.shape[1])] * rng.choice([-1, 1], x.shape[1])
    step = 1
    while step < width:
        blocks = z.reshape(len(x), -1, 2, step)
        a, b = blocks[:, :, 0].copy(), blocks[:, :, 1].copy()
        blocks[:, :, 0], blocks[:, :, 1] = a + b, a - b
        step *= 2
    return z / np.sqrt(width)


def lexical(rows, train):
    texts, shape = [], []
    for row in rows:
        a, b = row["span"]
        value = row["value"]
        # Context-only lexical evidence plus shape. Candidate spellings cannot be memorized.
        texts.append(row["text"][:a] + " CANDIDATE_" + row["target"] + " " + row["text"][b:])
        shape.append([len(value), len(value.encode()), sum(c.isupper() for c in value), sum(c.isdigit() for c in value),
                      sum(c.isspace() for c in value), value.count("-"), value.count("_"), row["target"] == "identifier"])
    vectorizer = TfidfVectorizer(analyzer="char", ngram_range=(2, 4), min_df=2, sublinear_tf=True)
    vectorizer.fit(np.asarray(texts)[train])
    return sparse.hstack([vectorizer.transform(texts), sparse.csr_matrix(standardize(np.asarray(shape), train))]).tocsr()


def sparse_probe(feature_sets, y, train, dev, size):
    best = None
    for name, x in feature_sets.items():
        order, z = correlation_order(x, y, train)
        for selected in ([order[:size]] if size > 1 else [np.asarray([i]) for i in order[:16]]):
            scores, selection, classifier = fit_best(z[:, selected], y, train, dev)
            criterion = selection["dev_average_precision"]
            if best is None or criterion > best[0]:
                best = criterion, scores, {"tensor": name, "coordinates": selected.tolist(), **selection}, classifier, z[:, selected]
    assert best is not None
    return best[1:]


def frontier(rows, scores):
    # TEST-label oracle curves are descriptive, never operational selections.
    choices = [metrics(rows, scores, t) for t in np.r_[np.inf, np.unique(scores)]]
    at_budget = {str(b): max((m for m in choices if m["false_positive_bytes"] <= b), key=lambda m: (m["protected_bytes"], -m["false_positive_bytes"])) for b in [0, 64, 256]}
    at_recall = {str(r): min((m for m in choices if m["span_recall"] >= r), key=lambda m: m["false_positive_bytes"]) for r in [0.9, 0.95, 1.0]}
    return {"oracle_at_fp_budget": at_budget, "oracle_at_recall": at_recall}


def bootstrap_delta(rows, a, ta, b, tb):
    # Group translations and derived variants together. This is NOT corpus-level uncertainty.
    parents = sorted({r["parent"] for r in rows})
    rng = np.random.default_rng(20261016)
    values = []
    for _ in range(500):
        picked = rng.choice(parents, len(parents), replace=True)
        indices = [i for p in picked for i, r in enumerate(rows) if r["parent"] == p]
        left = metrics([rows[i] for i in indices], a[indices], ta)
        right = metrics([rows[i] for i in indices], b[indices], tb)
        values.append([left["missed_bytes"] - right["missed_bytes"], left["false_positive_bytes"] - right["false_positive_bytes"]])
    return {"scope": "500 paired parent-block bootstrap resamples of this generator only; a minus b", "missed_byte_delta_percentiles": np.percentile(np.asarray(values)[:, 0], [2.5, 50, 97.5]).tolist(), "fp_byte_delta_percentiles": np.percentile(np.asarray(values)[:, 1], [2.5, 50, 97.5]).tolist()}


def evaluate(directory, output):
    rows = [json.loads(line) for line in (directory / "fixtures.jsonl").read_text().splitlines()]
    validate(rows)
    y = np.asarray([r["label"] for r in rows])
    train = np.asarray([r["evaluation"] == "train" for r in rows])
    dev = np.asarray([r["evaluation"] == "dev" for r in rows])
    test = np.asarray([r["evaluation"] == "test" for r in rows])
    lex = lexical(rows, train)
    arms, selected, selected_features = {}, {}, {}
    arms["lexical_context"], selected["lexical_context"], _ = fit_best(lex, y, train, dev)
    for model in ["davlan", "nym"]:
        data = np.load(directory / f"{model}-features.npz")
        assert data["ids"].tolist() == [r["id"] for r in rows]
        arms[f"{model}/mass"] = data["mass"][:, 0]
        logit_features = standardize(data["logits"], train)
        arms[f"{model}/calibrated_output"], selected[f"{model}/calibrated_output"], _ = fit_best(logit_features, y, train, dev)
        baseline = sparse.hstack([lex, sparse.csr_matrix(logit_features)]).tocsr()
        arms[f"{model}/output_context"], selected[f"{model}/output_context"], _ = fit_best(baseline, y, train, dev)
        native = {key: data[key] for key in data.files if key.startswith("mlp_")}
        for size in [1, 8]:
            name = f"{model}/native_{size}"
            arms[name], selected[name], _, selected_features[name] = sparse_probe(native, y, train, dev, size)
        # Full dimensions in a different basis, same selection budget.
        rotated = {key: rotate(value, 20261016) for key, value in native.items()}
        arms[f"{model}/rotated_8"], selected[f"{model}/rotated_8"], _, _ = sparse_probe(rotated, y, train, dev, 8)
        best = None
        for key in [k for k in data.files if k.startswith("hidden_")]:
            z = standardize(data[key], train)
            scores, selection, _ = fit_best(z, y, train, dev)
            if best is None or selection["dev_average_precision"] > best[0]:
                best = selection["dev_average_precision"], key, z, scores, selection
        assert best is not None
        arms[f"{model}/hidden"] = best[3]
        selected[f"{model}/hidden"] = {"tensor": best[1], **best[4]}
        for key, z in [("hidden_context", best[2]), ("native_context", selected_features[f"{model}/native_8"])]:
            name = f"{model}/{key}"
            x = sparse.hstack([baseline, sparse.csr_matrix(z)]).tocsr()
            arms[name], selected[name], _ = fit_best(x, y, train, dev)
        # Null-label control: independent labels per parent, translations kept together.
        random = np.random.default_rng(20261016)
        labels = {parent: int(random.integers(2)) for parent in sorted({r["parent"] for r in rows if r["split"] == "train"})}
        control_y = y.copy()
        control_y[train] = [labels[r["parent"]] for r in rows if r["split"] == "train"]
        # One fixed C, no control tuning on dev/test labels.
        control, _, _ = fit_best(best[2], control_y, train, dev, c_values=(0.1,))
        arms[f"{model}/shuffled_train_labels"] = control
    independent = directory / "minilm-features.npz"
    if independent.exists():
        data = np.load(independent)
        assert data["ids"].tolist() == [r["id"] for r in rows]
        z = standardize(data["embedding"], train)
        for key, x in [("encoder", z), ("encoder_context", sparse.hstack([lex, sparse.csr_matrix(z)]).tocsr())]:
            arms[f"minilm/{key}"], selected[f"minilm/{key}"], _ = fit_best(x, y, train, dev)
    # Everything above this line trains/selects only on train/dev. Nothing below mutates a model.
    dev_rows = [r for r, keep in zip(rows, dev, strict=True) if keep]
    frozen = {name: thresholds(dev_rows, scores[dev]) for name, scores in arms.items()}
    for name in ["davlan/mass", "nym/mass"]:
        frozen[name]["raw_0.3"] = 0.3
    selections = {"selections": selected, "thresholds": frozen, "fixture_sha256": hashlib.sha256((directory / 'fixtures.jsonl').read_bytes()).hexdigest(), "selection_contract": "all choices use train/dev only; test and transfer/stress untouched"}
    (directory / "frozen-selections.json").write_text(json.dumps(selections, indent=2))
    np.savez_compressed(directory / "scores.npz", **{name: scores for name, scores in arms.items()})
    # Retain the frozen stress rows, but also expose only genuinely changed unique texts.
    seen = {(r["parent"], r["language"], r["text"]) for r in rows if r["evaluation"] != "stress"}
    unique_stress = []
    for row in rows:
        key = (row["parent"], row["language"], row["text"])
        changed = row["evaluation"] == "stress" and key not in seen
        unique_stress.append(changed)
        seen.add(key)
    groups = {"test": test, "language_holdout": np.asarray([r["evaluation"] == "language_holdout" for r in rows]), "stress": np.asarray([r["evaluation"] == "stress" for r in rows]), "changed_unique_stress": np.asarray(unique_stress)}
    for key in ["language", "target", "domain", "family"]:
        for value in sorted({r[key] for r in rows}):
            groups[f"{key}/{value}"] = test & np.asarray([r[key] == value for r in rows])
    results = {"contract": "custom candidate-level research rubric, oracle span+target supplied; NOT v1/v2 nor end-to-end Gaze", "counts": {key: int(mask.sum()) for key, mask in {"train": train, "dev": dev, **groups}.items()}, "arms": {}}
    for name, scores in arms.items():
        results["arms"][name] = {"groups": {}, "selection": selected.get(name, {})}
        for group, mask in groups.items():
            if not mask.any():
                continue
            group_rows = [r for r, keep in zip(rows, mask, strict=True) if keep]
            results["arms"][name]["groups"][group] = {key: metrics(group_rows, scores[mask], t) for key, t in frozen[name].items()}
        results["arms"][name]["frontier"] = frontier([r for r, keep in zip(rows, test, strict=True) if keep], scores[test])
    results["paired_uncertainty"] = {}
    for model in ["davlan", "nym"]:
        a, b = f"{model}/native_context", f"{model}/output_context"
        results["paired_uncertainty"][model] = bootstrap_delta([r for r, keep in zip(rows, test, strict=True) if keep], arms[a][test], frozen[a]["dev_zero_fp"], arms[b][test], frozen[b]["dev_zero_fp"])
    output.write_text(json.dumps(results, indent=2, allow_nan=False))
    for name in arms:
        m = results["arms"][name]["groups"]["test"]["dev_zero_fp"]
        print(json.dumps({"arm": name, "missed_bytes": m["missed_bytes"], "fp_bytes": m["false_positive_bytes"], "tp": m["tp"], "fp": m["fp"], "AP": m["average_precision"]}), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    evaluate(args.directory, args.output)
