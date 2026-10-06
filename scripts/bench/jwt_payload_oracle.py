#!/usr/bin/env python3
"""Inactive opt-in JWT supplemental corpus. No PII scoring-contract credit."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import re
import subprocess
from pathlib import Path

VERSION = "jwt-payload-supplement-v1"
CLASS = "custom:security_token"
SOURCE = "security_token.anchored"
FIXTURES = Path(__file__).parent / "fixtures" / "jwt_payload"
PINNED_SOURCES = ("jwt_payload_oracle.py", "test_jwt_payload_oracle.py",
                  "fixtures/jwt_payload/policy.toml",
                  "../../crates/gaze-recognizers/examples/jwt_payload_probe.rs")
CUES = (
    "securitytoken", "security token", "security_token", "security-token",
    "sicherheitstoken", "zugangstoken", "zugangsschlussel", "zugangsschlüssel",
    "apikey", "api key", "api_key", "api-key", "accesstoken", "access token",
    "access_token", "access-token", "authtoken", "auth token", "auth_token",
    "auth-token", "authorizationtoken", "authorization token", "authorization_token",
    "authorization-token", "bearer", "token", "BEARER", "SICHERHEITSTOKEN",
)
FAMILIES = CUES + ("bare", "json_token", "json_api_key", "multiline_fallback", "prefixed_hyphen", "prefixed_dot", "jwt_tail", "jwt_ellipsis", "cued_prefixed_hyphen", "cued_prefixed_dot")
CONTROL_FAMILIES = ("bare_nonpersonal",)
HEADER = "eyJhbGciOiJIUzI1NiJ9"
PAYLOAD = "eyJzdWIiOiJhbGljZUBleGFtcGxlLmludmFsaWQifQ"
PUBLIC_PAYLOAD = "eyJzdWIiOiJleGFtcGxlIn0"
DELIMITERS = (": ", " = ", " ", " lautet = ")
SUFFIXES = (". Next.", ", next", ";next", "\n")


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def encode(value: str) -> str:
    return base64.urlsafe_b64encode(value.encode()).decode().rstrip("=")


def placement(partition: str, family: str, variant: int) -> tuple[str, str]:
    if family.startswith("cued_prefixed_"):
        return partition + " " + family + " token: ", SUFFIXES[variant]
    if family == "prefixed_hyphen":
        return partition + " prefix-", SUFFIXES[variant]
    if family == "prefixed_dot":
        return partition + " prefix.", SUFFIXES[variant]
    if family == "jwt_tail":
        return partition + " token: ", ".extra" + SUFFIXES[variant]
    if family == "jwt_ellipsis":
        return partition + " token: ", "... Next."
    if family in ("bare", "bare_nonpersonal"):
        return partition + (" value ", ' "', " (", "\n")[variant], ('', '"', ")", ".")[variant]
    if family.startswith("json_"):
        return partition + ' {"' + family[5:] + '":"', '"}' + SUFFIXES[variant]
    if family == "multiline_fallback":
        return partition + " token:\n", SUFFIXES[variant]
    return partition + " " + family + DELIMITERS[variant], SUFFIXES[variant]


def inserted(partition: str, family: str, variant: int, kind: str, value: str,
             reason: str, personal: bool = False, protect: bool = False) -> dict:
    prefix, suffix = placement(partition, family, variant)
    start = len(prefix.encode())
    row = {
        "version": VERSION, "id": f"{partition}/{family}/{variant}/{kind}",
        "partition": partition, "family": family, "variant": variant,
        "pair": f"{partition}/{family}/{variant}", "kind": kind,
        "text": prefix + value + suffix, "value": value,
        "value_span": [start, start + len(value.encode())],
        "payload": PAYLOAD if personal else None,
        "payload_span": [start + value.index(PAYLOAD), start + value.index(PAYLOAD) + len(PAYLOAD)] if personal else None,
        "protect": protect, "reason": reason, "class": CLASS, "source": SOURCE,
    }
    row["input_sha256"] = digest(row["text"].encode())
    return row


def generate() -> list[dict]:
    rows = []
    # These shapes have no personal claims. Expectations come from source insertion,
    # never from executing a detector or decoding a prediction.
    malformed = (
        "eyJabc.x.y", "eyJabc.x", "eyJabc..sig", "eyJabc.x.y.z",
        "build.more", "order_ref.v2", "code_fn.more", "prefix-eyJabc",
        "prefix.eyJabc", "eYJabc.x.y", "akiaexample", "AKIAEXAMPLE",
        "eyJabc..", "build_id..", "order-ref.v2", "code_fn.x",
    )
    for partition in ("dev", "test"):
        for index, family in enumerate(FAMILIES):
            for variant in range(4):
                signature = encode(f"synthetic-signature-{partition}-{index}-{variant}")
                jwt = HEADER + "." + PAYLOAD + "." + signature
                if family == "cued_prefixed_hyphen":
                    jwt = "prefix-" + jwt
                elif family == "cued_prefixed_dot":
                    jwt = "prefix." + jwt
                rows.append(inserted(partition, family, variant, "positive", jwt,
                                     "Whole JWT owns encoded personal sub; punctuation stays outside.", True, True))
                bad = malformed[(index * 4 + variant) % len(malformed)]
                rows.append(inserted(partition, family, variant, "benign", bad,
                                     "Nonpersonal malformed/build/order/code value must remain byte-identical."))
        # Opt-in credential controls are intentionally separate from benign FP gold.
        for variant, value in enumerate(("synthetic_opaque_value", "AKIAIOSFODNN7EXAMPLE",
                                        "ASIAIOSFODNN7EXAMPLE", HEADER + "." + PUBLIC_PAYLOAD + ".synthetic_signature")):
            family = "bare_nonpersonal" if variant == 3 else "token"
            rows.append(inserted(partition, family, variant, "control", value,
                                 "Valid opt-in credential stays protected; absence of PII does not make it FP.", protect=True))
        for variant, value in enumerate(("akiaiosfodnn7example", "ASIAIOSFODNN7EXAMPL",
                                        "tokenization_helper_registry", "api_key_rotation_schedule_v2")):
            rows.append(inserted(partition, "bare", variant, "control", value,
                                 "Noncredential issuer/code control stays byte-identical."))
    return rows


def validate_fixture(row: dict) -> None:
    required = {"version", "id", "partition", "family", "variant", "pair", "kind", "text",
                "value", "value_span", "payload", "payload_span", "protect", "reason", "class",
                "source", "input_sha256"}
    if not isinstance(row, dict) or set(row) != required:
        raise ValueError("fixture schema mismatch")
    if row["version"] != VERSION or row["partition"] not in ("dev", "test") or row["family"] not in FAMILIES + CONTROL_FAMILIES:
        raise ValueError("fixture identity mismatch")
    if row["kind"] not in ("positive", "benign", "control") or type(row["variant"]) is not int or row["variant"] not in range(4):
        raise ValueError("fixture kind/variant mismatch")
    if row["family"] in CONTROL_FAMILIES and row["kind"] != "control":
        raise ValueError("control-only family")
    prefix, suffix = placement(row["partition"], row["family"], row["variant"])
    start = len(prefix.encode())
    if row["text"] != prefix + row["value"] + suffix or row["value_span"] != [start, start + len(row["value"].encode())]:
        raise ValueError("insertion gold mismatch")
    if row["input_sha256"] != digest(row["text"].encode()) or row["class"] != CLASS or row["source"] != SOURCE:
        raise ValueError("fixture provenance mismatch")
    if type(row["protect"]) is not bool or not isinstance(row["reason"], str) or not row["reason"]:
        raise ValueError("fixture expectation mismatch")
    identity = f'{row["partition"]}/{row["family"]}/{row["variant"]}'
    if row["pair"] != identity or row["id"] != identity + "/" + row["kind"]:
        raise ValueError("fixture pair/id mismatch")
    personal = row["kind"] == "positive"
    if personal:
        value = row["value"]
        expected = [start + value.index(PAYLOAD), start + value.index(PAYLOAD) + len(PAYLOAD)]
        if not row["protect"] or row["payload"] != PAYLOAD or row["payload_span"] != expected:
            raise ValueError("personal payload gold mismatch")
        jwt = value
        if row["family"] == "cued_prefixed_hyphen":
            jwt = value.removeprefix("prefix-")
        elif row["family"] == "cued_prefixed_dot":
            jwt = value.removeprefix("prefix.")
        if jwt.split(".")[:2] != [HEADER, PAYLOAD] or len(jwt.split(".")) != 3:
            raise ValueError("positive JWT shape mismatch")
        if base64.urlsafe_b64decode(PAYLOAD + "==") != b'{"sub":"alice@example.invalid"}':
            raise ValueError("personal payload mismatch")
    elif row["payload"] is not None or row["payload_span"] is not None or (row["kind"] == "benign" and row["protect"]):
        raise ValueError("nonpersonal expectation mismatch")


METRICS = ("leaked_value_bytes", "leaked_payload_bytes", "benign_fp_bytes",
           "punctuation_fp_bytes", "refusals", "restore_failures", "invalid_manifests",
           "exact_restores", "valid_manifests", "whole_values", "control_failures")


def byte_span(span, text: bytes) -> tuple[int, int]:
    if (not isinstance(span, list) or len(span) != 2 or
            any(type(v) is not int for v in span) or not 0 <= span[0] < span[1] <= len(text)):
        raise ValueError("invalid byte span")
    for at in span:
        try:
            text[:at].decode("utf-8")
        except UnicodeDecodeError as error:
            raise ValueError("span splits UTF-8") from error
    return span[0], span[1]


def score_record(row: dict, record: dict, policy_sha256: str) -> dict:
    """Score actual clean bytes through validated raw-to-clean manifest mapping."""
    validate_fixture(row)
    required = {"id", "input_sha256", "policy_sha256", "refused", "error", "clean_text",
                "restored", "imported_restored", "manifest", "trace"}
    if not isinstance(record, dict) or set(record) != required:
        raise ValueError("native record schema mismatch")
    if (record["id"] != row["id"] or record["input_sha256"] != row["input_sha256"] or
            record["policy_sha256"] != policy_sha256 or type(record["refused"]) is not bool):
        raise ValueError("native record provenance mismatch")
    result = dict.fromkeys(METRICS, 0)
    raw = row["text"].encode()
    value = set(range(*row["value_span"]))
    payload = set(range(*row["payload_span"])) if row["payload_span"] else set()
    if record["refused"]:
        if (not isinstance(record["error"], str) or not record["error"] or
                any(record[k] is not None for k in ("clean_text", "restored", "imported_restored")) or
                record["manifest"] != [] or record["trace"] != []):
            raise ValueError("invalid refusal record")
        result.update(refusals=1, leaked_value_bytes=len(value) if row["kind"] == "positive" else 0,
                      leaked_payload_bytes=len(payload), control_failures=int(row["kind"] == "control"))
        return result
    if record["error"] is not None or not isinstance(record["clean_text"], str):
        raise ValueError("native clean/error mismatch")
    if not isinstance(record["manifest"], list) or not isinstance(record["trace"], list):
        raise ValueError("native spans must be lists")
    clean = record["clean_text"].encode()
    traces = []
    for trace in record["trace"]:
        if (not isinstance(trace, dict) or set(trace) != {"raw_span", "class", "sources"} or
                trace["class"] != CLASS or trace["sources"] != [SOURCE]):
            raise ValueError("native source/class mismatch")
        byte_span(trace["raw_span"], raw)
        traces.append((trace["raw_span"], trace["class"]))
    protected = set()
    raw_at = clean_at = 0
    observed = []
    for entry in record["manifest"]:
        if not isinstance(entry, dict) or set(entry) != {"raw_span", "clean_span", "class", "token_restore"} or entry["class"] != CLASS:
            raise ValueError("native manifest schema/class mismatch")
        start, end = byte_span(entry["raw_span"], raw)
        cs, ce = byte_span(entry["clean_span"], clean)
        if start < raw_at or cs < clean_at or raw[raw_at:start] != clean[clean_at:cs]:
            raise ValueError("native mapping/gap mismatch")
        token = clean[cs:ce]
        # No redaction or arbitrary safe-looking replacement can earn reversible credit.
        if not re.fullmatch(rb"<[0-9a-f]{8}:Custom:security_token_[0-9]+>", token):
            raise ValueError("native replacement is not a credential token")
        if entry["token_restore"] != raw[start:end].decode():
            raise ValueError("native token restore mismatch")
        observed.append((entry["raw_span"], entry["class"]))
        protected.update(range(start, end))
        raw_at, clean_at = end, ce
    if raw[raw_at:] != clean[clean_at:] or observed != traces:
        raise ValueError("native tail/trace mismatch")
    restored = record["restored"] == row["text"] and record["imported_restored"] == row["text"]
    result.update(valid_manifests=1, exact_restores=int(restored), restore_failures=int(not restored))
    if row["kind"] == "positive":
        result["leaked_value_bytes"] = len(value - protected)
        result["leaked_payload_bytes"] = len(payload - protected)
        result["whole_values"] = int(any(e["raw_span"] == row["value_span"] for e in record["manifest"]))
    allowed = value if row["protect"] else set()
    fp = protected - allowed
    result["benign_fp_bytes"] = len(fp)
    result["punctuation_fp_bytes"] = sum(chr(raw[i]) in ".,;:!?()[]{}\"'" for i in fp if raw[i] < 128)
    if row["kind"] == "control":
        result["control_failures"] = int((row["protect"] and not any(e["raw_span"] == row["value_span"] for e in record["manifest"])) or
                                        (not row["protect"] and (clean != raw or bool(protected))))
    return result


def strict_json(text: str):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError("duplicate JSON key")
            result[key] = value
        return result
    try:
        return json.loads(text, object_pairs_hook=pairs,
                          parse_constant=lambda value: (_ for _ in ()).throw(ValueError("nonfinite JSON")))
    except (TypeError, json.JSONDecodeError) as error:
        raise ValueError("malformed JSON") from error


def score_stream(rows: list[dict], output: str, policy_sha256: str, commit: str) -> dict:
    if not rows or not re.fullmatch(r"[0-9a-f]{40}", commit) or not re.fullmatch(r"[0-9a-f]{64}", policy_sha256):
        raise ValueError("empty corpus or invalid build commit")
    lines = output.splitlines()
    if len(lines) != len(rows) + 1 or any(not line for line in lines):
        raise ValueError("zero/missing/extra executed native records")
    handshake = strict_json(lines[0])
    if (handshake != {"protocol": VERSION, "build_commit": commit,
                      "policy_sha256": policy_sha256, "model_free": True} or
            type(handshake.get("model_free")) is not bool):
        raise ValueError("native handshake provenance mismatch")
    seen = set()
    metrics = dict.fromkeys(METRICS, 0)
    cells = {}
    details = []
    for row, line in zip(rows, lines[1:]):
        if row["id"] in seen:
            raise ValueError("duplicate fixture")
        seen.add(row["id"])
        record = strict_json(line)
        score = score_record(row, record, policy_sha256)
        cell = cells.setdefault(row["partition"] + "/" + row["kind"], dict.fromkeys(METRICS, 0))
        for key in METRICS:
            metrics[key] += score[key]
            cell[key] += score[key]
        details.append({"id": row["id"], "score": score, "native": record})
    return {"executed": len(rows), "metrics": metrics, "cells": cells, "records": details}


def load_corpus(directory: Path = FIXTURES) -> list[dict]:
    pin = strict_json((directory / "pins.json").read_text())
    if (set(pin) != {"version", "counts", "files", "sources"} or
            pin.get("version") != VERSION or pin.get("counts") != {"positive": 152, "benign": 152, "control": 8} or
            set(pin["files"]) != {"dev.jsonl", "test.jsonl"} or
            set(pin["sources"]) != set(PINNED_SOURCES)):
        raise ValueError("corpus pin schema mismatch")
    rows = []
    for partition in ("dev", "test"):
        data = (directory / (partition + ".jsonl")).read_bytes()
        if digest(data) != pin["files"][partition + ".jsonl"]:
            raise ValueError("corpus bytes do not match pin")
        selected = [strict_json(line) for line in data.decode().splitlines()]
        for row in selected:
            validate_fixture(row)
        rows.extend(selected)
    # The checked-in corpus is independently reviewable; reject rewritten gold,
    # dropped families, duplicate rows and drift from the deterministic recipe.
    if rows != generate():
        raise ValueError("corpus differs from insertion recipe")
    for name, sha in pin["sources"].items():
        if digest((Path(__file__).parent / name).read_bytes()) != sha:
            raise ValueError("supplemental source pin drift")
    return rows


def run_native(binary: Path, expected_sha256: str, commit: str, rows: list[dict],
               policy: Path, policy_sha256: str) -> dict:
    """Explicit invocation only. Never compiles, loads models or supplies setup policy."""
    if not rows or not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("zero executed native records")
    for row in rows:
        validate_fixture(row)
    if len({row["id"] for row in rows}) != len(rows):
        raise ValueError("duplicate native request")
    if digest(binary.read_bytes()) != expected_sha256 or digest(policy.read_bytes()) != policy_sha256:
        raise ValueError("native binary/policy hash mismatch")
    request = "".join(json.dumps({"id": r["id"], "text": r["text"]}) + "\n" for r in rows)
    try:
        completed = subprocess.run([str(binary.resolve()), str(policy.resolve())], input=request,
                                   text=True, capture_output=True, timeout=120, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ValueError("native invocation failed") from error
    if completed.returncode != 0 or completed.stderr:
        raise ValueError(f"native error exit={completed.returncode}: {completed.stderr[:1000]}")
    if digest(binary.read_bytes()) != expected_sha256 or digest(policy.read_bytes()) != policy_sha256:
        raise ValueError("native bytes changed during invocation")
    return score_stream(rows, completed.stdout, policy_sha256, commit)


def write_corpus() -> None:
    rows = generate()
    for row in rows:
        validate_fixture(row)
    for partition in ("dev", "test"):
        data = "".join(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n"
                       for row in rows if row["partition"] == partition).encode()
        (FIXTURES / (partition + ".jsonl")).write_bytes(data)
    pin = {"version": VERSION, "counts": {"positive": 152, "benign": 152, "control": 8},
           "files": {part + ".jsonl": digest((FIXTURES / (part + ".jsonl")).read_bytes())
                     for part in ("dev", "test")},
           "sources": {name: digest((Path(__file__).parent / name).read_bytes()) for name in PINNED_SOURCES}}
    (FIXTURES / "pins.json").write_text(json.dumps(pin, sort_keys=True, indent=2) + "\n")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["generate", "validate", "measure"])
    parser.add_argument("--base-bin", type=Path)
    parser.add_argument("--candidate-bin", type=Path)
    parser.add_argument("--base-sha256")
    parser.add_argument("--candidate-sha256")
    parser.add_argument("--base-commit")
    parser.add_argument("--candidate-commit")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.command == "generate":
        write_corpus()
        return 0
    rows = load_corpus()
    if args.command == "validate":
        print(json.dumps({"version": VERSION, "documents": len(rows), "native": "NOTRUN"}))
        return 0
    if any(getattr(args, field) is None for field in
           ("base_bin", "candidate_bin", "base_sha256", "candidate_sha256", "base_commit", "candidate_commit", "output")):
        parser.error("measure needs exact base/candidate binary hashes, build commits and output")
    if args.output.exists():
        raise ValueError("existing report cannot be reused")
    if args.base_commit == args.candidate_commit or args.base_sha256 == args.candidate_sha256:
        raise ValueError("base and candidate must be distinct fresh builds")
    policy = FIXTURES / "policy.toml"
    policy_hash = digest(policy.read_bytes())
    result = {"version": VERSION, "acceptance": "UNDECIDED", "policy_sha256": policy_hash,
              "pins_sha256": digest((FIXTURES / "pins.json").read_bytes())}
    for side in ("base", "candidate"):
        binary = getattr(args, side + "_bin")
        sha = getattr(args, side + "_sha256")
        commit = getattr(args, side + "_commit")
        result[side] = {"binary_sha256": sha, "build_commit": commit,
                        **run_native(binary, sha, commit, rows, policy, policy_hash)}
    with args.output.open("x", encoding="utf-8") as output:
        output.write(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    return int(any(result[side]["metrics"][key] for side in ("base", "candidate")
                   for key in ("refusals", "restore_failures", "invalid_manifests")))


if __name__ == "__main__":
    raise SystemExit(main())
