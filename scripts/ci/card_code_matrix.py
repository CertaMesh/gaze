#!/usr/bin/env python3
"""Compare PAN/code coverage against pinned main, including visible-layout controls."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
BASE_SHA = "3fb02d70e756102c871c51471cfd76d7ce6de4bc"
SPEC = importlib.util.spec_from_file_location("clean_equality", ROOT / "scripts/verify_clean_equality.py")
EQUALITY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EQUALITY)


def pans():
    rows = [("amex-seed", "378282246310005")]
    for length in range(13, 20):
        for prefix in ("4", "37", "50"):
            body = prefix + "0" * (length - len(prefix) - 1)
            total = sum((int(ch) * 2 // 10 + int(ch) * 2 % 10) if i % 2 == 0 else int(ch)
                        for i, ch in enumerate(reversed(body)))
            rows.append(("prefix-" + prefix, body + str((10 - total % 10) % 10)))
    return rows


def separators():
    visible = [("visible", "space-u%04x" % cp, chr(cp))
               for cp in (0x20, 0xA0, 0x1680, *range(0x2000, 0x200B), 0x202F, 0x205F, 0x3000)]
    visible += [("visible", name, value) for name, value in
                (("hyphen", "-"), ("tab", "\t"), ("lf", "\n"), ("crlf", "\r\n"), ("nonbreaking-hyphen", "\u2011"))]
    removed = [("removed", "ignorable-u%04x" % cp, chr(cp)) for cp in
               (0xAD, 0x34F, 0x61C, 0x115F, 0x17B4, 0x180E, 0x200B, 0x200C, 0x200D,
                0x200E, 0x200F, 0x202A, 0x2060, 0x2067, 0x2069, 0x3164, 0xFE0F,
                0xFEFF, 0xFFA0, 0xFFF0, 0x1BCA0, 0x1D173, 0xE0067)]
    return visible + removed + [("removed", "mixed-ignorable-run", "\u200b\u2060\u00ad")]


def observe(binary, text, cwd):
    result = subprocess.run([str(binary), "clean", "--locale", "en-US"], input=text.encode(),
                            capture_output=True, cwd=cwd, timeout=60,
                            env=dict(os.environ, GAZE_DISABLE_OS_KEYSTORE="1"))
    if result.returncode:
        raise RuntimeError("matrix clean failed")
    response = EQUALITY.normalize(json.loads(result.stdout))
    tokens = {entry["token"]: entry["raw"] for entry in response["entries"]}
    digits = [i for i, ch in enumerate(text) if ch.isascii() and ch.isdigit()]
    mask = [False] * len(digits)
    restored = []
    cursor = output_at = 0
    pattern = re.compile("|".join(re.escape(token) for token in tokens)) if tokens else None
    for match in pattern.finditer(response["clean_text"]) if pattern else ():
        literal = response["clean_text"][output_at:match.start()]
        if text[cursor:cursor + len(literal)] != literal:
            raise RuntimeError("literal output lost source alignment")
        restored.append(literal)
        cursor += len(literal)
        raw = tokens[match[0]]
        if text[cursor:cursor + len(raw)] != raw:
            raise RuntimeError("manifest raw value lost source alignment")
        for index, at in enumerate(digits):
            mask[index] |= cursor <= at < cursor + len(raw)
        restored.append(raw)
        cursor += len(raw)
        output_at = match.end()
    restored.append(response["clean_text"][output_at:])
    if "".join(restored) != text:
        raise RuntimeError("matrix manifest substitution failed to round-trip")
    return response, "".join("1" if covered else "0" for covered in mask)


def contains(actual, required):
    return len(actual) == len(required) and all(a == "1" or r == "0" for a, r in zip(actual, required))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-bin", type=Path, required=True)
    parser.add_argument("--candidate-bin", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--characterize-only", action="store_true")
    args = parser.parse_args()
    binaries = {"base": args.base_bin.resolve(strict=True), "candidate": args.candidate_bin.resolve(strict=True)}
    baseline_build = json.loads((ROOT / "target/separator-checks/release-build.json").read_text())
    if baseline_build["base_sha"] != BASE_SHA or hashlib.sha256(binaries["base"].read_bytes()).hexdigest() != baseline_build["binary_sha256"]["base"]:
        raise RuntimeError("matrix base differs from the fresh pinned-main build")
    layouts = [(family, pan, code) for family, pan in pans() for code in ("123", "1234")]
    seps = separators()
    total = len(layouts) * len(seps)
    rows, regressions, space_losses, visible_differences = [], [], [], []
    with tempfile.TemporaryDirectory(prefix="gaze-card-code-matrix-") as cwd:
        for family, pan, code in layouts:
            reference_mask = None
            for kind, name, sep in seps:
                text = "Card: " + pan + sep + code + " done"
                before, main_mask = observe(binaries["base"], text, cwd)
                after, candidate_mask = observe(binaries["candidate"], text, cwd)
                case_id = f"{family}/pan-{len(pan)}/code-{len(code)}/{name}"
                if name == "space-u0020":
                    reference_mask = main_mask
                if not contains(candidate_mask, main_mask):
                    regressions.append(case_id)
                if kind == "removed" and not contains(candidate_mask, reference_mask):
                    space_losses.append(case_id)
                identity = before == after
                if kind == "visible" and not identity:
                    visible_differences.append(case_id)
                rows.append({"case_id": case_id, "family": family, "pan_digits": len(pan),
                             "code_digits": len(code), "separator_kind": kind, "separator": name,
                             "input_sha256": hashlib.sha256(text.encode()).hexdigest(),
                             "base_digit_coverage": main_mask, "candidate_digit_coverage": candidate_mask,
                             "base_space_digit_coverage": reference_mask,
                             "canonical_output_equal": identity, "manifest_substitution_round_trip": True})
                if len(rows) % 10 == 0 or len(rows) == total:
                    print(f"CARD CODE MATRIX {len(rows)}/{total}", flush=True)
    gaps = []
    for length in range(13, 20):
        for size in (3, 4):
            cells = [row for row in rows if row["pan_digits"] == length and row["code_digits"] == size and row["separator_kind"] == "visible"]
            missing = [row for row in cells if row["base_digit_coverage"][-size:] != "1" * size]
            gaps.append({"pan_digits": length, "code_digits": size, "visible_cells": len(cells),
                         "main_code_gap_cells": len(missing),
                         "separator_classes": sorted({row["separator"] for row in missing})})
    report = {"schema": "gaze-card-code-boundary-matrix-v1", "base_sha": BASE_SHA,
              "candidate_source_sha": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "binary_sha256": {side: hashlib.sha256(path.read_bytes()).hexdigest() for side, path in binaries.items()},
              "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), "cells": total,
              "coverage_regressions": regressions, "removed_space_losses": space_losses,
              "visible_output_differences": visible_differences, "preexisting_visible_code_gaps": gaps,
              "observations": rows}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({key: report[key] for key in ("cells", "coverage_regressions", "removed_space_losses", "visible_output_differences")}))
    if not args.characterize_only and (regressions or space_losses or visible_differences):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
