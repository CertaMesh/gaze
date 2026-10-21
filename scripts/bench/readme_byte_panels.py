"""One receipt-bound byte-micro panel per README dataset."""
from __future__ import annotations

import hashlib
import json
import argparse
from pathlib import Path
import shutil
import gzip
from xml.sax.saxutils import escape

import benchmark_charts as charts
from tagged_gaze import tag_commit

DATASETS = {"own": "Own corpus", "sealed": "Sealed OpenPII/Gretel",
            "presidio-research": "Presidio Research", "piibench-commercial": "PIIBench-commercial",
            "pii-trace": "PII-TRACE"}
VERSIONS = ("v0.16.0", "v0.17.0")
METRIC = "Byte-micro F2 (label-agnostic)"
ALT = "One byte-micro F2 panel per dataset, Gaze v0.16 and v0.17 with measured competitors; leaked PII bytes under each bar."
POPULATIONS = {"own": {"holdout": 2910}, "sealed": {"openpii/all": 9990, "gretel/en": 2891,
               "gretel/de": 453, "gretel/fr": 443}, "presidio-research": {"test": 1500},
               "piibench-commercial": {"test": 5000}, "pii-trace": {"test": 4500}}


def baselines(directory: Path):
    """Reuse v0.16's already-recorded byte counts, preserving their provenance."""
    from readme_byte_measurements import summarize, totals
    import external_release_panels

    external_release_panels.load(directory)
    version = "v0.16.0"
    revision = tag_commit(version)
    result = {}

    def row(kind, sets, inputs, source, provenance):
        return {"schema_version": 1, "tag": version, "revision": revision, "dataset": kind,
                "contract": "v4" if kind == "own" else "all-gold", "method": "retained-byte-counts",
                "sets": sets, "totals": totals(sets), "inputs_sha256": inputs,
                "source": {"file": source, "sha256": hashlib.sha256((directory / source).read_bytes()).hexdigest()},
                "original_provenance": provenance,
                "command": ["python", "scripts/bench/readme_byte_panels.py", "--reuse-v016",
                            "--capture-dir", "<private-capture-directory>"]}

    source = "scorecard-v0.16.0-scored-labels-v4.json.gz"
    card = json.loads(gzip.decompress((directory / source).read_bytes()))
    if card["gaze"]["revision"] != revision:
        raise ValueError("retained own-corpus baseline is not the release tag")
    provenance = card["runner_provenance"]
    result["own"] = row("own", {"holdout": summarize(card["runs"][0])},
                        {key: card["dataset"]["integrity"]["component_sha256"][key]
                         for key in ("dataiku", "negative_corpus")}, source, provenance)
    for kind in ("sealed", "piibench-commercial", "pii-trace"):
        source = f"release-v0.17.0/{kind}-gate.json"
        base = json.loads((directory / source).read_text())["base"]
        provenance = base["receipts"]
        if provenance["runtime_revision"] != revision:
            raise ValueError("retained baseline is not the release tag")
        sets = {name: {"documents": value["attempted_documents"], "processed": value["completed_documents"],
                       "refused": value["failed_closed_documents"], "gold_bytes": value["gold_bytes"],
                       "leaked_bytes": value["leaked_bytes"], "false_positive_bytes": value["false_positive_bytes"],
                       "restore_exact": value["restore_exact_documents"], "manifest_valid": value["manifest_valid_documents"]}
                for name, value in base["sets"].items() if name != "openpii/en-de-fr"}
        pins = provenance["inputs_sha256"]
        inputs = pins if kind == "sealed" else {"dataset": pins["train.parquet"]} if kind == "pii-trace" else {
            "dataset": pins["data/test_5k.jsonl"], "manifest": pins["manifest.json"]}
        result[kind] = row(kind, sets, inputs, source, provenance)
    source = "their-benchmarks.json"
    bench = json.loads((directory / source).read_text())["benchmarks"]["presidio-research"]
    provenance = bench["provenance"]["gaze-v0.16.0"]["release"]
    if provenance["commit"] != revision or provenance["tag"] != version:
        raise ValueError("retained Presidio baseline is not the release tag")
    block = bench["rows"]["gaze-v0.16.0"]["product_coverage"]
    sets = {"test": {"documents": block["documents"], "processed": block["documents"], "refused": 0,
                     "gold_bytes": block["true_positive_bytes"] + block["leaked_bytes"],
                     "leaked_bytes": block["leaked_bytes"], "false_positive_bytes": block["false_positive_bytes"]}}
    result["presidio-research"] = row("presidio-research", sets, {"dataset": bench["identity"]["sha256"]}, source, provenance)
    return result


def load(directory: Path) -> dict:
    from readme_byte_measurements import totals

    manifest = json.loads((directory / "readme-byte-panels.json").read_text())
    history_path = directory / "release-history.json"
    if history_path.exists():
        released = [row for row in json.loads(history_path.read_text())["releases"] if not row.get("provisional")]
        if not released or released[-1]["version"] != VERSIONS[-1]:
            raise ValueError("README panels must include the latest measured release on every dataset")
    retained = baselines(directory) if manifest.get("reused_v016") else None
    result = {}
    for kind in DATASETS:
        result[kind] = {}
        for version in VERSIONS:
            pin = manifest["receipts"][kind][version]
            payload = (directory / pin["file"]).read_bytes()
            if hashlib.sha256(payload).hexdigest() != pin["sha256"]:
                raise ValueError("README measurement receipt digest differs")
            row = json.loads(payload)
            if retained is not None and version == "v0.16.0" and row != retained[kind]:
                raise ValueError("README retained baseline differs from its original byte counts")
            if row["tag"] != version or row["dataset"] != kind or row["revision"] != tag_commit(version):
                raise ValueError("README measurement is not bound to its release tag")
            if row["contract"] != ("v4" if kind == "own" else "all-gold"):
                raise ValueError("README measurement contract differs")
            if row["totals"] != totals(row["sets"]):
                raise ValueError("README byte-micro counters do not reproduce")
            if {name: counts["documents"] for name, counts in row["sets"].items()} != POPULATIONS[kind]:
                raise ValueError("README dataset population differs from its pinned scope")
            for counts in row["sets"].values():
                if any(type(value) is not int or value < 0 for value in counts.values()):
                    raise ValueError("README counters must be nonnegative integers")
                if counts["processed"] + counts["refused"] != counts["documents"]:
                    raise ValueError("README processed/refused population differs")
                if counts["leaked_bytes"] > counts["gold_bytes"]:
                    raise ValueError("README leaked bytes exceed gold")
            result[kind][version] = {**row, "receipt": pin["file"]}
    for kind, versions in result.items():
        base, candidate = (versions[v] for v in VERSIONS)
        if base["inputs_sha256"] != candidate["inputs_sha256"]:
            raise ValueError(f"{kind}: release inputs differ")
    return result


def byte_f2(block: dict) -> float:
    tp, fp, fn = (block[k] for k in ("true_positive_bytes", "false_positive_bytes", "leaked_bytes"))
    denominator = 5 * tp + fp + 4 * fn
    return 5 * tp / denominator if denominator else 0.0


def competitors(kind, comparison, their, declared):
    """Only measured rows; a legacy receipt marker survives metric conversion."""
    if kind == "sealed":
        return []
    if kind == "own":
        result = []
        tuned = comparison.get("presidio_tuned")
        for family, name in declared.items():
            if family == "presidio" and tuned:
                cell = tuned["rows"][tuned["chart"]["row"]]["contracts"]["v4"]["C"]
                label = charts.TUNED_BAR_NAME
            elif name in comparison["tools"]:
                cell = comparison["tools"][name]["contracts"]["v4"]["C"]
                label = charts.SHORT_NAMES[family]
            else:
                continue
            block = cell["metrics"]["product_coverage"]["full"]
            result.append((label, block, False, cell["skipped_documents"]))
        return result
    bench = their[kind]
    names = [(family, name) for family, name in declared.items() if name in bench["rows"]]
    if kind == "pii-trace":
        names.append(("pii-tracer", "pii-tracer"))
    result = []
    for family, name in names:
        choice = bench.get("vendor_tuned", {}).get(family)
        if choice:
            name, label = choice["row"], choice["bar_name"]
        else:
            label = charts.SHORT_NAMES.get(family, "PII-Tracer")
        result.append((label, bench["rows"][name]["product_coverage"],
                       bool(charts.receipt_marker(bench, name)), 0))
    return result


def panels(measured, comparison, their, declared):
    result = []
    for kind, title in DATASETS.items():
        bars = []
        for version in VERSIONS:
            row = measured[kind][version]["totals"]
            bars.append(charts.Bar("Gaze " + version, row["byte_micro_f2"], row["leaked_bytes"], None, gaze=True))
        for label, block, legacy, _ in competitors(kind, comparison, their, declared):
            bars.append(charts.Bar(label, byte_f2(block), block["leaked_bytes"], None, receipt_unverified=legacy))
        documents = measured[kind][VERSIONS[-1]]["totals"]["documents"]
        result.append(charts.Panel(title, f"{documents:,} documents", "Scored labels v4" if kind == "own" else "All gold labels",
                                   tuple(bars), documents=documents,
                                   note="† Pre-receipt; regeneration pending" if any(b.receipt_unverified for b in bars) else ""))
    return result


def files(panel_set):
    # Two rows keep eight-bar panels readable at README width. Each dataset
    # appears once; both rows use the same metric.
    result = {}
    for theme, palette in charts.THEMES.items():
        height = 2 * charts.PANEL_H + 3 * charts.GAP
        output = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {charts.WIDTH} {height}" '
                  f'width="{charts.WIDTH}" height="{height}" font-family="{charts.FONT}" role="img" aria-label="{escape(ALT)}">',
                  f'<title>{escape(ALT)}</title>']
        for index, row in enumerate((panel_set[:2], panel_set[2:])):
            x = float(charts.GAP)
            available = charts.WIDTH - charts.GAP * (len(row) + 1)
            widths = [available * 2 / 3, available / 3] if index == 0 else [available / 3] * 3
            for panel, width in zip(row, widths):
                output += charts._panel_svg(palette, x, charts.GAP + index * (charts.PANEL_H + charts.GAP),
                                            width, panel, METRIC, "higher is better", "{:.3f}",
                                            lambda b: b.f2, 1.0, True)
                x += width + charts.GAP
        result[f"benchmark-panels-{theme}.svg"] = "\n".join([*output, "</svg>", ""])
    return result


def picture(root):
    prefix = "docs/assets/benchmarks/" if root else "../../assets/benchmarks/"
    return '\n'.join(['<picture>', f'  <source media="(prefers-color-scheme: dark)" srcset="{prefix}benchmark-panels-dark.svg">',
                      f'  <img alt="{ALT}" src="{prefix}benchmark-panels-light.svg">', '</picture>'])


def readme(measured):
    newest = measured["own"]["v0.17.0"]["totals"]
    return picture(True) + f"\n\nGaze v0.17.0 leaks {newest['leaked_bytes']:,} PII bytes on our holdout " \
        f"(byte-micro F2 {newest['byte_micro_f2']:.3f}). All five panels use byte-micro F2 and leaked bytes. " \
        "† marks competitor results awaiting receipt regeneration. " \
        "[Measurements, setups and limits](docs/reference/benchmarks/README.md#benchmark-panels)."


def markdown(measured, comparison, their, declared):
    lines = [picture(False), "", "Byte-micro F2 pools UTF-8 bytes across documents: `5*TP/(5*TP+4*FN+FP)`. "
             "Labels do not affect overlap. Own-corpus bars use contract v4, including credentials; external bars count all gold. "
             "False positives in F2 are raw, without gold-gap credit. Refused documents are reported separately; "
             "leaked-on-all-processed counts every successfully processed document, including partial protection.", "",
             "| Dataset | Tool | Byte-micro F2 | Leaked on all processed (bytes) | FP bytes | Refused | Evidence |",
             "| --- | --- | ---: | ---: | ---: | ---: | --- |"]
    for kind, title in DATASETS.items():
        for version in VERSIONS:
            measurement = measured[kind][version]
            row = measurement["totals"]
            lines.append(f"| {title} | Gaze {version} | {row['byte_micro_f2']:.6f} | {row['leaked_bytes']:,} | "
                         f"{row['false_positive_bytes']:,} | {row['refused']:,} | [Receipt]({measurement['receipt']}) |")
        for label, block, legacy, skipped in competitors(kind, comparison, their, declared):
            marker = "†" if legacy else ""
            source = "competitors.md" if kind == "own" else "their-benchmarks.json"
            lines.append(f"| {title} | {label}{marker} | {byte_f2(block):.6f} | {block['leaked_bytes']:,} | "
                         f"{block['false_positive_bytes']:,} | — | [Counts]({source}) |")
    setup = comparison.get("presidio_tuned")
    own_setup = charts.TUNED_CAPTIONS[setup["chart"]["row"]] if setup else "the declared comparison configuration"
    vendor = their["presidio-research"].get("vendor_tuned", {}).get("presidio")
    vendor_setup = vendor["caption"] if vendor else "the declared comparison configuration"
    lines += ["", charts.RECEIPT_NOTE, "", f"Own corpus: {own_setup}. Presidio Research: {vendor_setup}. "
              "PIIBench publishes no tuned setup, so its declared best configuration is shown. "
              "PII-Tracer on PII-TRACE is an upper bound because of possible training overlap. "
              "Competitor skips count their gold as leaked; a dash in Refused means the tool has no Gaze refusal contract.", "",
              "Committed evidence has no competitor row for the sealed combined set. PII-TRACE currently has only PII-Tracer's competitor measurement. "
              "OPF has no own-corpus bar after two 600-second socket timeouts. Unmeasured configurations have no bar or N/A slot. "
              "Legacy competitor byte counts keep their † status until regenerated; converting retained byte counts to F2 does not verify receipts.", "",
              "The existing [panel configuration choices](../../../scripts/bench/compare/chart-configs.json) select one configuration per compared tool. "
              "Other configuration variants remain in the linked comparison evidence.", "",
              "[Pinned receipt index](readme-byte-panels.json). Reproduce with "
              "[`readme_byte_measurements.py`](../../../scripts/bench/readme_byte_measurements.py): build each signed tag with Rust 1.96.0, "
              "using [`readme_release_build.py`](../../../scripts/bench/readme_release_build.py), "
              "use its own `gaze setup` policy, and run the receipt's command on the pinned inputs. "
              "Each new dataset measurement is captured once per binary and replayed from that private record. "
              "Offline checks verify receipt integrity and recompute aggregates; only capture/replay on the pinned inputs proves the underlying byte counts. "
              "The v0.16 rows reuse existing byte counts and retain their original provenance, including Presidio Research's Rust 1.95 build. "
              "Only aggregate receipts are published; "
              "external text, document identifiers and prediction spans stay private. This refresh covers v0.16.0 and v0.17.0 only; "
              "historical release tables retain their original benchmark specifications."]
    return "\n".join(lines)


def publish(capture_dir: Path, directory: Path, hardware: Path):
    """Copy exact aggregate receipts and pin their inputs before rendering."""
    from readme_byte_measurements import SEALED_PINS
    import dataiku_en_de_gaze_bench as dataiku
    import openpii_gaze_bench as openpii

    manifest = {"schema_version": 1, "receipts": {}, "reused_v016": True, "hardware": json.loads(hardware.read_text()),
                "inputs": {
                    "own": {"repository": dataiku.DATASET_REPO, "revision": dataiku.DATASET_REVISION,
                            "file": dataiku.DATASET_FILE, "sha256": dataiku.DATASET_SHA256},
                    "sealed": {"openpii": {"repository": openpii.DATASET_REPO,
                               "revision": openpii.DATASET_REVISION, "file": openpii.DATASET_FILE},
                               "gretel": {"repository": "gretelai/synthetic_pii_finance_multilingual",
                               "revision": "7b844d16738527a04264f50214cb426a4cea0897",
                               "files": [f"data/{name}_test-00000-of-00001.parquet" for name in ("English", "German", "France")]},
                               "sha256": SEALED_PINS},
                }}
    their = json.loads((directory / "their-benchmarks.json").read_text())["benchmarks"]
    manifest["inputs"].update({kind: their[kind]["identity"] for kind in
                               ("presidio-research", "piibench-commercial", "pii-trace")})
    destination = directory / "readme-byte-measurements"
    destination.mkdir(exist_ok=True)
    for kind in DATASETS:
        manifest["receipts"][kind] = {}
        for version in VERSIONS:
            source = capture_dir / ("v0" + version.split(".")[1]) / kind / "receipt.json"
            target = destination / f"{version}-{kind}.json"
            shutil.copyfile(source, target)
            manifest["receipts"][kind][version] = {
                "file": target.relative_to(directory).as_posix(),
                "sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
            }
    (directory / "readme-byte-panels.json").write_text(json.dumps(manifest, indent=2) + "\n")
    load(directory)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Pin aggregate README measurement receipts, then run render_benchmark_doc.py.")
    parser.add_argument("--capture-dir", type=Path, required=True)
    parser.add_argument("--hardware", type=Path)
    parser.add_argument("--reuse-v016", action="store_true")
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parents[2] / "docs/reference/benchmarks")
    args = parser.parse_args()
    if args.reuse_v016:
        for kind, receipt in baselines(args.output).items():
            target = args.capture_dir / "v016" / kind / "receipt.json"
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(json.dumps(receipt, indent=2) + "\n")
    else:
        if args.hardware is None:
            parser.error("--hardware is required when publishing")
        publish(args.capture_dir, args.output, args.hardware)
