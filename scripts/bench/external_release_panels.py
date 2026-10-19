"""Receipt-bound byte-micro panels, kept separate from character/vendor scores."""
from __future__ import annotations

import hashlib
import json
import math
import subprocess
from pathlib import Path

import benchmark_charts as charts
from tagged_gaze import tag_commit
from external_release_gate import decide
from external_quality_receipt import validate_receipt

VERSION = "0.17.0"
TITLES = {"sealed": "Sealed OpenPII/Gretel", "piibench-commercial": "PIIBench-commercial",
          "pii-trace": "PII-TRACE"}
METRIC = "Byte-micro F2 (all gold, label-agnostic)"


def load(directory: Path, *, repo: Path | None = None) -> dict:
    """Admit only the committed receipts for the measured released crates tree."""
    receipt = json.loads((directory / f"release-measurement-receipts-v{VERSION}.json").read_text())
    release = receipt["versions"][VERSION]

    def checked(name: str) -> dict:
        pin = release["aggregate_evidence"][name]
        content = (directory / pin["file"]).read_bytes()
        if hashlib.sha256(content).hexdigest() != pin["sha256"]:
            raise ValueError(f"external panels: receipt digest differs for {name}")
        return json.loads(content)

    repo = repo or directory.parents[2]
    for commit in (tag_commit(f"v{VERSION}", repo), release["measured_source"]):
        tree = subprocess.check_output(["git", "rev-parse", f"{commit}:crates"], cwd=repo, text=True).strip()
        if tree != release["raw_crates_tree"]:
            raise ValueError("external panels: measured source and release crates trees differ")
    review = checked("external-f2-release-review.json")
    if review["criterion"] != "byte_micro_f2" or set(review["results"]) != set(TITLES):
        raise ValueError("external panels: unexpected scoring contract or benchmarks")
    for name, result in review["results"].items():
        gate = checked(f"external/{name}-gate.json")
        quality = checked(f"external/{name}-quality.json")
        validate_receipt(quality)
        if gate["gate"] != decide(name, gate["base"]["sets"], gate["candidate"]["sets"]):
            raise ValueError(f"{name}: original gate does not reproduce")
        for field in ("driver_sha256", "wrapper_sha256", "harness_revision", "ner_manifest_sha256",
                      "nym_manifest_sha256", "inputs_sha256"):
            if gate["base"]["receipts"][field] != gate["candidate"]["receipts"][field]:
                raise ValueError(f"{name}: paired measurement identities differ")
        base_tree = subprocess.check_output(
            ["git", "rev-parse", f"{gate['base']['receipts']['runtime_revision']}:crates"],
            cwd=repo, text=True).strip()
        tagged_base_tree = subprocess.check_output(
            ["git", "rev-parse", f"{tag_commit('v0.16.0', repo)}:crates"], cwd=repo, text=True).strip()
        if base_tree != tagged_base_tree:
            raise ValueError(f"{name}: baseline does not bind v0.16.0")
        pin = release["aggregate_evidence"][f"external/{name}-gate.json"]
        if result["aggregate_gate_sha256"] != pin["sha256"] or gate["contract"] != "all-gold":
            raise ValueError(f"{name}: review does not bind the all-gold gate")
        if result["legacy_gate"] != gate["gate"]:
            raise ValueError(f"{name}: original net-byte verdict differs")
        for side in ("base", "candidate"):
            if result["sets"][side] != gate[side]["sets"]:
                raise ValueError(f"{name}: review counters differ from gate")
            summed = result["summed_sets"]
            if summed != gate["gate"]["summed_sets"]:
                raise ValueError(f"{name}: summed population differs")
            totals = result[side]
            for field, source in (("gold_bytes", "gold_bytes"), ("false_negative_bytes", "leaked_bytes"),
                                  ("false_positive_bytes", "false_positive_bytes")):
                if totals[field] != sum(result["sets"][side][s][source] for s in summed):
                    raise ValueError(f"{name}: {field} does not reproduce")
            tp = totals["gold_bytes"] - totals["false_negative_bytes"]
            denominator = 5 * tp + 4 * totals["false_negative_bytes"] + totals["false_positive_bytes"]
            if totals["true_positive_bytes"] != tp or not math.isclose(totals["f2_percent"], 500 * tp / denominator):
                raise ValueError(f"{name}: byte-micro F2 does not reproduce")
        accepted = (not gate["gate"]["regressions"] and
                    result["candidate"]["f2_percent"] > result["base"]["f2_percent"])
        if result["f2_gate"] != ("pass" if accepted else "fail"):
            raise ValueError(f"{name}: accepted F2 verdict does not reproduce")
        measured = gate["candidate"]["receipts"]
        for field, source in (("runtime_revision", "measured_source"), ("binary_sha256", "binary_sha256"),
                              ("policy_sha256", "policy_sha256")):
            if measured[field] != release[source]:
                raise ValueError(f"{name}: measured {field} differs from release receipt")
        if quality["receipts"] != measured:
            raise ValueError(f"{name}: quality receipt differs from measured identity")
        for split, counters in quality["quality"].items():
            if any(gate["candidate"]["sets"][split][key] != value for key, value in counters.items()):
                raise ValueError(f"{name}: quality counters differ")
    for name in ("strict-gate-v1.json", "strict-gate-v2.json"):
        if checked(name)["verdict"] != "not_comparable":
            raise ValueError("external panels: strict identity disclosure changed")
    return review


def panels(review: dict) -> list[charts.Panel]:
    return [charts.Panel(
        title, "Fresh paired release observations", "All gold; historical harness retained",
        tuple(charts.Bar(f"Gaze {version}", result[side]["f2_percent"] / 100,
                        result[side]["false_negative_bytes"], None, gaze=side == "candidate")
              for side, version in (("base", "v0.16.0"), ("candidate", "v0.17.0"))),
        note=f"Original net-byte gate: {result['legacy_gate']['verdict'].upper()}",
    ) for name, title in TITLES.items() for result in (review["results"][name],)]


def files(review: dict) -> dict[str, str]:
    result = {}
    panel_set = panels(review)
    for theme in charts.THEMES:
        alt = "Fresh v0.16.0 and v0.17.0 external byte-micro F2; leaked PII bytes below each bar."
        output = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {charts.WIDTH} {charts.PANEL_H + 2 * charts.GAP}" '
                  f'font-family="{charts.FONT}" role="img" aria-label="{alt}">', f"<title>{alt}</title>"]
        x = float(charts.GAP)
        for panel, width in zip(panel_set, charts.panel_widths(panel_set)):
            output += charts._panel_svg(charts.THEMES[theme], x, charts.GAP, width, panel,
                                        METRIC, "higher is better", "{:.5f}", lambda bar: bar.f2, 1.0, True)
            x += width + charts.GAP
        result[f"external-release-panels-{theme}.svg"] = "\n".join([*output, "</svg>", ""])
    return result


def markdown(review: dict, *, root: bool) -> str:
    prefix = "docs/assets/benchmarks/" if root else "../../assets/benchmarks/"
    link = "docs/reference/benchmarks/README.md#v017-final-candidate-evidence" if root else "release-v0.17.0/external-f2-release-review.json"
    lines = ["Fresh external release measurements use byte-micro F2, `5*TP/(5*TP+4*FN+FP)`. "
             "These are separate from the character-level and vendor-evaluator scores below.", "", "<picture>",
             f'  <source media="(prefers-color-scheme: dark)" srcset="{prefix}external-release-panels-dark.svg">',
             f'  <img alt="Fresh external byte-micro F2 for Gaze v0.16.0 and v0.17.0, with leaked bytes and original net-byte verdicts." src="{prefix}external-release-panels-light.svg">',
             "</picture>", ""]
    if not root:
        lines += ["| Benchmark | Version | Byte-micro F2 % | Leaked bytes | FP bytes | Original net-byte gate | Accepted F2 gate |",
                  "| --- | --- | ---: | ---: | ---: | --- | --- |"]
        for name, title in TITLES.items():
            result = review["results"][name]
            for side, version in (("base", "v0.16.0"), ("candidate", "v0.17.0")):
                totals = result[side]
                verdict = result["legacy_gate"]["verdict"].upper() if side == "candidate" else "baseline"
                accepted = result["f2_gate"].upper() if side == "candidate" else "baseline"
                lines.append(f"| {title} | {version} | {totals['f2_percent']:.10f} | "
                             f"{totals['false_negative_bytes']:,} | {totals['false_positive_bytes']:,} | {verdict} | {accepted} |")
        lines.append("")
    lines += [f"[Evidence and limits]({link}): all three accepted external F2 comparisons pass. "
              "TRACE's original net-byte gate fails. Strict own-policy identity remains `not_comparable`; "
              "zero-leak readiness remains unmet."]
    return "\n".join(lines)
