#!/usr/bin/env python3
"""Held-out panels: two public corpora Gaze is never tuned on, report-only.

The generated layers measure the shapes the generator knows. These panels
measure real annotated text from outside the project, with the metrics of
`report_metrics`:

- PrivacyBench+ (English workplace email and documents), human-annotated
  split, held-out companies only: the two Tracer recommends for evaluation.
  Spans Tracer marked unresolved and its indirect identifiers are neutral.
- GraSCCo_PII v2 (German clinical case reports), the whole corpus. Labels
  Gaze does not target (generic dates, titles, professions, hospitals,
  countries, ages, organizations) are neutral; the rest are scored.

The panels stay outside the gain gate and the capture: this script writes
only its own report, never a record, scorecard or cache entry, and no gate
or capture module imports it. Never tune a rule against a panel; a panel that
is tuned on stops being held out.

Data is downloaded at run time and verified against the SHA-256 pins below
before it is read. Nothing is committed. Both corpora are CC BY 4.0; the
report carries their attribution.

    python scripts/bench/heldout_panels.py --binary target/release/gaze \\
        --policy target/gate/policy.toml --output panels.json
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import time
from typing import Mapping, Sequence
import urllib.request
import zipfile

import gaze_bench_score as score
import report_metrics as metrics

SCHEMA_VERSION = 1
ROOT = Path(__file__).resolve().parents[2]
DEFAULT_CACHE = ROOT / "target/heldout-panels"

PRIVACYBENCH_REVISION = "4c52bbf08fd7816b629c01438d11c6be9a653287"
PRIVACYBENCH_URL = ("https://huggingface.co/datasets/Tracer-ML/PrivacyBench-Plus/resolve/"
                    f"{PRIVACYBENCH_REVISION}/")
PRIVACYBENCH_MANIFEST_SHA256 = "42a6ead2a0927eefb7bef017087608634087da6b7cbe4658f1429deb2422369f"
PRIVACYBENCH_HELD_OUT = ("megan_donovan_eli_lilly", "nora_caterpillar")
PRIVACYBENCH_INDIRECT = "data/extensions/indirect_identifiers.jsonl"

GRASCCO_URL = "https://zenodo.org/records/15747389/files/grascco_pii_2_json.zip?download=1"
GRASCCO_SHA256 = "b7326c3b88fbfbd077d049703972c1fcc1323e27f162d337e0fd6bc54d913b95"

ATTRIBUTION = {
    "privacybench_plus": (
        "PrivacyBench+ by Tracer, CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/), "
        f"https://huggingface.co/datasets/Tracer-ML/PrivacyBench-Plus at revision {PRIVACYBENCH_REVISION}. "
        "A modified copy of PrivacyBench v2.5 by TonicAI, CC BY 4.0, "
        "https://huggingface.co/datasets/TonicAI/Privacy-Bench. Used unmodified; scored here."),
    "grascco_pii_v2": (
        "GraSCCo_PII v2, CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/), "
        "https://doi.org/10.5281/zenodo.15747389. Used unmodified; scored here."),
}

_LOCATION = frozenset({"location", "custom:postal_code", "custom:building_number"})
_NAME = frozenset({"name"})
_TENANT = "tenant identifier; its class is whatever the integrator declares"

PRIVACYBENCH_TYPES: dict[str, frozenset[str]] = {
    "NAME_GIVEN": _NAME, "NAME_FAMILY": _NAME,
    "ORGANIZATION": frozenset({"organization"}),
    "EMAIL_ADDRESS": frozenset({"email"}),
    "PHONE_NUMBER": frozenset({"custom:phone"}),
    "LOCATION_ADDRESS": _LOCATION,
    "USERNAME": frozenset({"custom:username"}),
    "URL": frozenset({"custom:url"}),
}
PRIVACYBENCH_UNTYPED = {"EMPLOYEE_ID": _TENANT, "ACCOUNT_NUMBER": _TENANT}

GRASCCO_TYPES: dict[str, frozenset[str]] = {
    "NAME_PATIENT": _NAME, "NAME_DOCTOR": _NAME, "NAME_RELATIVE": _NAME, "NAME_EXT": _NAME,
    "NAME_USERNAME": frozenset({"custom:username"}),
    "DATE_BIRTH": frozenset({"custom:birth_date", "custom:date"}),
    "LOCATION_STREET": _LOCATION, "LOCATION_CITY": _LOCATION, "LOCATION_ZIP": _LOCATION,
    "CONTACT_PHONE": frozenset({"custom:phone"}), "CONTACT_FAX": frozenset({"custom:phone"}),
    "CONTACT_EMAIL": frozenset({"email"}),
}
GRASCCO_UNTYPED = {"ID": "clinical case and record numbers; no built-in class"}
# Labels outside what Gaze targets; their characters are neither gold nor false positive.
GRASCCO_NEUTRAL = frozenset({"DATE", "NAME_TITLE", "PROFESSION", "AGE", "LOCATION_HOSPITAL",
                             "LOCATION_ORGANIZATION", "LOCATION_COUNTRY"})


class PanelError(Exception):
    pass


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def fetch(url: str, sha256: str, cache: Path, *, download: bool) -> bytes:
    """The pinned bytes, from the cache or the network; never unverified."""
    path = cache / sha256
    if path.is_file():
        data = path.read_bytes()
        if _sha256(data) == sha256:
            return data
    if not download:
        raise PanelError(f"{url} is not cached in {cache}; run without --no-download")
    with urllib.request.urlopen(url, timeout=120) as response:  # noqa: S310 (pinned https)
        data = response.read()
    if _sha256(data) != sha256:
        raise PanelError(f"{url}: sha256 {_sha256(data)} is not the pinned {sha256}")
    cache.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    return data


def _jsonl(data: bytes) -> list[dict]:
    return [json.loads(line) for line in data.decode("utf-8").splitlines() if line.strip()]


def privacybench_items(cache: Path, *, download: bool) -> list[metrics.Item]:
    manifest = json.loads(fetch(PRIVACYBENCH_URL + "MANIFEST.json", PRIVACYBENCH_MANIFEST_SHA256,
                                cache, download=download))

    def member(path: str) -> bytes:
        if path not in manifest:
            raise PanelError(f"PrivacyBench+ manifest lists no {path}")
        return fetch(PRIVACYBENCH_URL + path, manifest[path], cache, download=download)

    indirect: dict[str, list[tuple[int, int]]] = {}
    for row in _jsonl(member(PRIVACYBENCH_INDIRECT)):
        if row["file"].startswith("human_annotations/"):
            key = f"{Path(row['file']).name}:{row['line']}"
            indirect.setdefault(key, []).append((row["start"], row["end"]))
    items = []
    for company in PRIVACYBENCH_HELD_OUT:
        for name in (f"{company}.jsonl", f"{company}_documents.jsonl"):
            for line, row in enumerate(_jsonl(member(f"data/human_annotations/{name}")), 1):
                uid = f"{name}:{line}"
                unresolved = [(u["annotation"]["start"], u["annotation"]["end"])
                              for u in row.get("_tracer_review", {}).get("unknown_targets", [])]
                gold = [(s["start"], s["end"], s["label"]) for s in row["ground_truth_spans"]]
                items.append(metrics.Item(uid, company, row["text"], gold, (),
                                          neutral=unresolved + indirect.get(uid, [])))
    return items


def grascco_items(cache: Path, *, download: bool) -> list[metrics.Item]:
    archive = zipfile.ZipFile(io.BytesIO(fetch(GRASCCO_URL, GRASCCO_SHA256, cache, download=download)))
    items = []
    for name in sorted(n for n in archive.namelist() if n.endswith(".json")):
        structures = json.loads(archive.read(name))["%FEATURE_STRUCTURES"]
        text = next(x["sofaString"] for x in structures if x.get("%TYPE") == "uima.cas.Sofa")
        if any(ord(ch) >= 0x10000 for ch in text):
            # UIMA offsets are UTF-16 code units; they equal code points only in the BMP.
            raise PanelError(f"{name}: text outside the BMP; offsets would shift")
        gold, neutral = [], []
        for x in structures:
            if x.get("%TYPE") != "webanno.custom.PHI" or x["end"] <= x["begin"]:
                continue
            kind = x.get("kind", "UNKNOWN")
            if kind in GRASCCO_NEUTRAL:
                neutral.append((x["begin"], x["end"]))
            else:
                gold.append((x["begin"], x["end"], kind))
        uid = Path(name).name.split(".txt")[0]
        items.append(metrics.Item(uid, uid, text, gold, (), neutral=neutral))
    return items


def class_name(value: object) -> str:
    """Daemon class (`"Email"` or `{"Custom": "phone"}`) in canonical form."""
    if isinstance(value, str):
        return value.lower()
    if isinstance(value, Mapping) and isinstance(value.get("Custom"), str):
        return f"custom:{value['Custom']}"
    raise PanelError(f"unknown daemon class {value!r}")


def predict(items: Sequence[metrics.Item], binary: Path, policy: Path) -> tuple[list[metrics.Item], list[float], int]:
    """Run each document through `gaze daemon` in its own session."""
    process = subprocess.Popen([str(binary), "daemon", "--policy", str(policy)], stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, bufsize=1)
    assert process.stdin is not None and process.stdout is not None
    scored, latencies, errors = [], [], 0
    try:
        for item in items:
            started = time.perf_counter()
            process.stdin.write(json.dumps({"session_id": f"panel-{item.uid}", "text": item.text}) + "\n")
            process.stdin.flush()
            line = process.stdout.readline()
            if not line:
                raise PanelError("gaze daemon exited")
            latencies.append((time.perf_counter() - started) * 1000)
            response = json.loads(line)
            if "error" in response:
                # A refused document hides nothing: every gold span counts as missed.
                errors += 1
                scored.append(item)
                continue
            chars = {offset: index for index, offset in enumerate(score.char_to_byte_offsets(item.text))}
            predictions = [(chars[e["raw_span"]["start"]], chars[e["raw_span"]["end"]], class_name(e["class"]))
                           for e in response["manifest"]]
            scored.append(metrics.Item(item.uid, item.group, item.text, item.gold, predictions,
                                       neutral=item.neutral))
    finally:
        process.stdin.close()
        process.wait()
    return scored, latencies, errors


def _percentile(values: Sequence[float], q: float) -> float | None:
    ordered = sorted(values)
    return round(ordered[int(q * (len(ordered) - 1))], 1) if ordered else None


PANELS = {
    "privacybench_plus": (privacybench_items, PRIVACYBENCH_TYPES, PRIVACYBENCH_UNTYPED),
    "grascco_pii_v2": (grascco_items, GRASCCO_TYPES, GRASCCO_UNTYPED),
}


def run(binary: Path, policy: Path, *, panels: Sequence[str], cache: Path, download: bool) -> dict:
    result = {"schema_version": SCHEMA_VERSION, "report_only": True, "held_out": True,
              "binary_sha256": _sha256(binary.read_bytes()), "policy_sha256": _sha256(policy.read_bytes()),
              "panels": {}}
    for name in panels:
        load, types, untyped = PANELS[name]
        items = load(cache, download=download)
        metrics.check_labels((label for item in items for *_, label in item.gold), types, untyped)
        scored, latencies, errors = predict(items, binary, policy)
        result["panels"][name] = {
            "attribution": ATTRIBUTION[name],
            "untyped_labels": untyped,
            "daemon_errors": errors,
            "latency_ms": {"p50": _percentile(latencies, 0.5), "p95": _percentile(latencies, 0.95)},
            **metrics.score_items(scored, types, untyped),
        }
    return result


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", type=Path, required=True, help="gaze binary to measure")
    parser.add_argument("--policy", type=Path, required=True, help="policy the daemon runs")
    parser.add_argument("--panel", action="append", choices=sorted(PANELS),
                        help="panel to run (default: all)")
    parser.add_argument("--cache", type=Path, default=DEFAULT_CACHE, help="verified download cache")
    parser.add_argument("--no-download", action="store_true", help="read only the cache")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args(argv)
    try:
        value = run(args.binary, args.policy, panels=args.panel or sorted(PANELS),
                    cache=args.cache, download=not args.no_download)
    except (PanelError, metrics.MetricsError, OSError, KeyError, ValueError) as error:
        print(f"heldout panels: {error}", file=sys.stderr)
        return 2
    text = json.dumps(value, indent=1, sort_keys=True, ensure_ascii=False) + "\n"
    if args.output:
        args.output.write_text(text, encoding="utf-8")
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
