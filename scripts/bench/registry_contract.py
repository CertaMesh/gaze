#!/usr/bin/env python3
"""Compare ordered assembly census and construction-only SDK admission evidence.

Build the registry_contract example on each revision. For the baseline, copy
the committed example and apply registry-census-base.patch: it only exposes
scan order and reports the absent try_build API. Run the example with each
runtime's own setup policy, then its #800 extended policy, then --admission.
Invalid configurations are never part of a corpus leak/refusal score.
"""
import argparse
import hashlib
import json
from pathlib import Path


def read(path):
    return json.loads(path.read_text())


def compare(base_default, candidate_default, base_arm, candidate_arm, base_admission, candidate_admission):
    for name, base, candidate in (("default", base_default, candidate_default),
                                   ("family_precedence", base_arm, candidate_arm)):
        if read(base) != read(candidate):
            raise ValueError(f"{name} ordered registration census changed")
        rows = read(base)["ordered_registrations"]
        if not rows or len({row["id"] for row in rows}) != len(rows):
            raise ValueError(f"{name} census is empty or contains duplicates")
    base = {(r["case"], r["entrypoint"]): r["observed"] for r in read(base_admission)["rows"]}
    candidate = {(r["case"], r["entrypoint"]): r["observed"] for r in read(candidate_admission)["rows"]}
    expected = {(case, entry) for case in ("valid", "forward", "orphan", "duplicate")
                for entry in ("build", "try_build")}
    if set(base) != expected or set(candidate) != expected:
        raise ValueError("admission table has incomplete scenarios")
    for key in expected:
        if base[key]["construction_scans"] != 0 or candidate[key]["construction_scans"] != 0:
            raise ValueError("construction scanned a document")
        case, entry = key
        if base[key]["status"] != ("admitted" if entry == "build" else "entrypoint_unavailable"):
            raise ValueError("baseline admission differs from the observed old API")
        if case in ("valid", "forward"):
            if candidate[key] != base[(case, "build")]:
                raise ValueError("valid registration changed")
        else:
            status = "fixed_panic" if entry == "build" else (
                "orphan_membership" if case == "orphan" else "duplicate_id")
            if candidate[key]["status"] != status:
                raise ValueError("invalid registration was not rejected as specified")
    if base[("valid", "build")]["first_wins"] is not True or base[("orphan", "build")]["first_wins"] is not False:
        raise ValueError("baseline orphan did not demonstrate precedence corruption")
    duplicate = base[("duplicate", "build")]
    if duplicate["scan_sources"] == duplicate["lookup_sources"]:
        raise ValueError("baseline duplicate did not demonstrate scan/lookup disagreement")
    return {"schema_version": 1, "acceptance": "P census plus construction admission",
            "census": {name: {"identical": True, "registrations": len(read(path)["ordered_registrations"])}
                       for name, path in (("default", base_default), ("family_precedence", base_arm))},
            "admission": {"base": read(base_admission), "candidate": read(candidate_admission)},
            "inputs": [{"name": path.name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
                       for path in (base_default, candidate_default, base_arm, candidate_arm,
                                    base_admission, candidate_admission)]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("base-default", "candidate-default", "base-arm", "candidate-arm",
                 "base-admission", "candidate-admission", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    report = compare(args.base_default, args.candidate_default, args.base_arm, args.candidate_arm,
                     args.base_admission, args.candidate_admission)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print("ordered registration census identical; SDK admission contract passed")


if __name__ == "__main__":
    main()
