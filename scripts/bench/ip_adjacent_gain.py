#!/usr/bin/env python3
"""Gate adjacent IP protection against a baseline clean_for_bench binary.

The synthetic corpus uses loopback, link-local and documentation addresses only.
Both binaries run the rule floor to isolate recognizer behavior from NER.
"""

import argparse
import json
import subprocess


def cases():
    rows = []
    for left, right in [
        ("::1", "fe80::1"),
        ("fe80::1", "::1"),
        ("::ffff:127.0.0.1", "fe80::1"),
        ("fe80::1", "::ffff:127.0.0.1"),
    ]:
        for separator in [" ", ",", "\t", "\u00a0"]:
            rows.append((f"host {left}{separator}{right} done", [left, right]))
    rows += [
        ("host ::1 fe80::1 ::ffff:127.0.0.1 done", ["::1", "fe80::1", "::ffff:127.0.0.1"]),
        ("host 127.0.0.1 fe80::1 done", ["127.0.0.1", "fe80::1"]),
        ("host fe80::1 127.0.0.1 done", ["fe80::1", "127.0.0.1"]),
        ("host 2001:db8::1 fe80::1 done", ["fe80::1"]),
        ("host fe80::1 2001:db8::1 done", ["fe80::1"]),
    ]
    return rows


def measure(binary):
    rows = cases()
    requests = [
        {"fixture_id": f"boundary-{i}", "locale_chain": ["en-US"], "text": text}
        for i, (text, _) in enumerate(rows)
    ]
    process = subprocess.run(
        [binary, "--config", "rule-floor-extended"],
        input="".join(json.dumps(row) + "\n" for row in requests),
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )
    responses = [json.loads(line) for line in process.stdout.splitlines()]
    assert len(responses) == len(rows)
    result = {"documents": len(rows), "address_spans": 0, "protected_spans": 0,
              "leaked_bytes": 0, "exact_restore": 0, "valid_manifests": 0,
              "refused": 0, "missing_cases": []}
    for i, ((raw, addresses), response) in enumerate(zip(rows, responses)):
        if "pipeline_error_stage" in response:
            result["refused"] += 1
            result["missing_cases"].append(i)
            continue
        result["exact_restore"] += bool(response["restore"]["exact"])
        result["valid_manifests"] += all(
            value == 0 for key, value in response["manifest_integrity"].items()
            if key != "spans"
        )
        spans = [
            (entry["raw_start"], entry["raw_end"])
            for entry in response["manifest_spans"]
        ]
        search_from = 0
        for address in addresses:
            start_char = raw.index(address, search_from)
            search_from = start_char + len(address)
            start = len(raw[:start_char].encode("utf-8"))
            end = start + len(address.encode("utf-8"))
            result["address_spans"] += 1
            if (start, end) in spans:
                result["protected_spans"] += 1
            else:
                result["missing_cases"].append(i)
            result["leaked_bytes"] += sum(
                not any(span_start <= byte < span_end for span_start, span_end in spans)
                for byte in range(start, end)
            )
    result["missing_cases"] = sorted(set(result["missing_cases"]))
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--base", required=True)
    parser.add_argument("--candidate", required=True)
    args = parser.parse_args()
    base = measure(args.base)
    candidate = measure(args.candidate)
    passed = (
        base["leaked_bytes"] > candidate["leaked_bytes"]
        and candidate["leaked_bytes"] == 0
        and candidate["protected_spans"] == candidate["address_spans"]
        and candidate["refused"] == 0
        and candidate["exact_restore"] == candidate["documents"]
        and candidate["valid_manifests"] == candidate["documents"]
    )
    print(json.dumps({"base": base, "candidate": candidate,
                      "gain_gate": "PASS" if passed else "FAIL"}, indent=2))
    if not passed:
        raise SystemExit(1)
