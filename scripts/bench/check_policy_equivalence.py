#!/usr/bin/env python3
"""Compare the release CLI and policy-file benchmark on identical documents."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

import dataiku_en_de_gaze_bench as dataiku
import gaze_bench_score as score
import run_no_opf_benchmark as runner
from bench_subprocess import BenchSubprocess


TOKEN = re.compile(r"<[0-9a-f]{8}:[^>]+>")
SESSION_HEX = re.compile(r"<([0-9a-f]{8}):")
ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "scripts/bench/fixtures"


def normalize(text: str) -> str:
    return SESSION_HEX.sub("<hex:", text)


def cli_protected_spans(raw: str, clean: str, entries: list[dict]) -> set[tuple[int, int]]:
    values = {entry["token"]: entry["raw"] for entry in entries}
    restored = []
    spans = set()
    raw_bytes = 0
    cursor = 0
    for match in TOKEN.finditer(clean):
        prefix = clean[cursor : match.start()]
        restored.append(prefix)
        raw_bytes += len(prefix.encode("utf-8"))
        value = values.get(match.group())
        if value is None:
            raise ValueError("CLI token has no manifest entry")
        end = raw_bytes + len(value.encode("utf-8"))
        spans.add((raw_bytes, end))
        restored.append(value)
        raw_bytes = end
        cursor = match.end()
    restored.append(clean[cursor:])
    if "".join(restored) != raw:
        raise ValueError("CLI token restoration does not reproduce raw text")
    return spans


def documents(args: argparse.Namespace) -> list[tuple[str, str, list[str]]]:
    if args.self_check:
        result = []
        for line in (FIXTURES / "policy_equivalence.jsonl").read_text().splitlines():
            row = json.loads(line)
            locale = "en-US" if row["language"] == "en" else "de-DE"
            result.append((row["id"], row["text"], [locale, "global"]))
        return result
    dataset = args.dataset.resolve()
    dataiku.verify_dataset(dataset)
    positive, _ = dataiku.load_documents(dataset)
    negative, _ = runner.load_negative_documents(args.negative_corpus.resolve())
    selected, _ = score.stratified_sample(positive + negative, args.documents, seed=args.seed)
    if {doc.language for doc in selected} != {"en", "de"} or not any(
        doc.negative_category for doc in selected
    ):
        raise ValueError("sample must contain both languages and negative documents")
    return [(doc.uid, doc.text, doc.locale_chain) for doc in selected]


def compare(args: argparse.Namespace) -> int:
    target = Path(os.environ.get("CARGO_TARGET_DIR", str(ROOT / "target")))
    gaze = args.gaze or target / "debug/gaze"
    bench = args.bench or target / "debug/examples/clean_for_bench"
    policy = args.policy or FIXTURES / "policy_equivalence.toml"
    if not all(path.is_file() for path in (gaze, bench, policy)):
        raise FileNotFoundError("release CLI, benchmark binary, or policy is missing")
    selected = documents(args)
    env = dict(os.environ)
    env.pop("GAZE_NYM_MODEL_DIR", None)
    env["GAZE_BENCH_POLICY"] = str(policy.resolve())
    mismatches = []
    command = [str(bench), "--config", "policy-file"]
    with BenchSubprocess(command, cwd=ROOT, env=env) as process:
        for index, (uid, raw, locales) in enumerate(selected, 1):
            cli = subprocess.run(
                [str(gaze), "clean", "--policy", str(policy)],
                input=raw,
                text=True,
                capture_output=True,
                timeout=300,
                cwd=ROOT,
                env=env,
            )
            if cli.returncode != 0:
                mismatches.append({
                    "id": uid,
                    "cause": "CLI pipeline refused the document",
                    "cli_exit": cli.returncode,
                })
                continue
            try:
                result = json.loads(cli.stdout)
            except json.JSONDecodeError:
                mismatches.append({"id": uid, "cause": "CLI response is not JSON"})
                continue
            request = {"fixture_id": uid, "locale_chain": locales, "text": raw}
            # Nym scans clean text containing session tokens. Match the CLI's random
            # token prefix so both paths receive the same safety-net input.
            session = None
            for entry in result["entries"]:
                session = SESSION_HEX.match(entry["token"])
                if session:
                    break
            if session is None:
                session = SESSION_HEX.search(result["clean_text"])
            if session:
                request["session_hex"] = session.group(1)
            observed = process.exchange(request)
            if "pipeline_error_code" in observed:
                mismatches.append({
                    "id": uid,
                    "cause": "benchmark pipeline refused the document",
                    "bench_error": observed["pipeline_error_code"],
                })
                continue
            if normalize(result["clean_text"]) != normalize(observed["clean_text"]):
                mismatches.append({"id": uid, "cause": "normalized clean_text differs"})
            try:
                cli_spans = cli_protected_spans(raw, result["clean_text"], result["entries"])
            except ValueError as error:
                mismatches.append({"id": uid, "cause": str(error)})
                continue
            bench_spans = {
                (span["raw_start"], span["raw_end"])
                for span in observed["final_protection_trace"]
            }
            if cli_spans != bench_spans:
                mismatches.append({
                    "id": uid,
                    "cause": "protected raw byte spans differ",
                    "cli_count": len(cli_spans),
                    "bench_count": len(bench_spans),
                })
            if index % 25 == 0:
                print(f"compared {index}/{len(selected)} documents", file=sys.stderr, flush=True)
    print(json.dumps({
        "documents": len(selected),
        "seed": args.seed,
        "mismatches": mismatches,
    }, indent=2))
    return 1 if mismatches else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-check", action="store_true")
    parser.add_argument("--policy", type=Path)
    parser.add_argument(
        "--dataset", type=Path,
        default=ROOT / "target/bench-data/dataiku-en-de/test.parquet",
    )
    parser.add_argument("--negative-corpus", type=Path, default=ROOT / runner.NEGATIVE_CORPUS)
    parser.add_argument("--documents", type=int, default=220)
    parser.add_argument("--seed", type=int, default=20260710)
    parser.add_argument("--gaze", type=Path)
    parser.add_argument("--bench", type=Path)
    args = parser.parse_args()
    try:
        return compare(args)
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        print(f"equivalence check failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
