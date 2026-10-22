"""Keep tracked gzip evidence free of absolute home-directory paths.

`check` decompresses every tracked `.gz` file and fails when one contains a
`/Users/`, `/home/` or `C:\\Users\\` path. `verify-receipts` proves that each
scrubbed archive listed in the receipts file equals its original capture (read
from git history) record by record and field by field, except the named
provenance path fields, and that both sides match their pinned SHA-256.
"""
from __future__ import annotations

import argparse
import copy
import gzip
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

import export_portable_record

ROOT = Path(__file__).resolve().parents[2]
RECEIPTS = ROOT / "docs/reference/benchmarks/portable-evidence-receipts.json"

# Raw text and JSON-escaped spellings: `C:\Users\` appears as `C:\\Users\\` in JSON.
HOME_PATH = re.compile(
    rb"/Users/|/home/|"
    rb"(?:[A-Za-z]:|(?:\\{2,}|//)(?:\?(?:\\+|/)(?i:UNC)(?:\\+|/))?"
    rb"[^\\/\r\n]+)(?:\\+|/)(?i:Users)(?:\\+|/)"
)
MAX_JSON_STRING_DEPTH = 4

# Intentional fixtures that must carry a home path (for example scrubber tests).
# Repo-relative POSIX paths; keep empty unless a fixture genuinely needs one.
ALLOWED_FIXTURES: frozenset[str] = frozenset()


class EvidenceError(Exception):
    pass


def tracked_gz(root: Path = ROOT) -> list[str]:
    output = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z", "--", "*.gz"],
        check=True, capture_output=True,
    ).stdout
    return sorted(item.decode() for item in output.split(b"\0") if item)


def _decoded_hits(value, depth: int = 0) -> int:
    if isinstance(value, str):
        hits = len(HOME_PATH.findall(value.encode("utf-8")))
        if not value.lstrip().startswith(('{', '[', '"')):
            return hits
        if depth >= MAX_JSON_STRING_DEPTH:
            return max(1, hits)  # Opaque serialized JSON cannot prove publication safe.
        try:
            nested = json.loads(value)
        except json.JSONDecodeError:
            return hits
        return max(hits, _decoded_hits(nested, depth + 1))
    if isinstance(value, dict):
        return sum(_decoded_hits(key, depth) + _decoded_hits(item, depth) for key, item in value.items())
    if isinstance(value, list):
        return sum(_decoded_hits(item, depth) for item in value)
    return 0


def home_path_hits(data: bytes) -> int:
    raw = gzip.decompress(data)
    raw_hits = len(HOME_PATH.findall(raw))
    if b"\\" not in raw:
        return raw_hits  # Without JSON escapes, decoding cannot reveal another home path.
    try:
        decoded_hits = _decoded_hits(json.loads(raw))
    except (json.JSONDecodeError, UnicodeDecodeError):
        decoded_hits = 0
        for line in raw.splitlines():
            try:
                decoded_hits += _decoded_hits(json.loads(line))
            except (json.JSONDecodeError, UnicodeDecodeError):
                continue  # Non-JSON evidence is still covered by the raw scan.
    # Raw and decoded scans usually see the same path; don't count it twice.
    return max(raw_hits, decoded_hits)


def scan(root: Path = ROOT, files: list[str] | None = None) -> dict[str, int]:
    """Map each offending tracked archive to its number of home-path matches."""
    hits = {}
    for name in tracked_gz(root) if files is None else files:
        if name in ALLOWED_FIXTURES:
            continue
        count = home_path_hits((root / name).read_bytes())
        if count:
            hits[name] = count
    return hits


def _rows(data: bytes) -> list[dict]:
    return [json.loads(line) for line in gzip.decompress(data).decode("utf-8").splitlines()]


def _get(row: dict, dotted: str):
    value = row
    for key in dotted.split("."):
        value = value[key]
    return value


def _set(row: dict, dotted: str, value) -> None:
    *parents, last = dotted.split(".")
    for key in parents:
        row = row[key]
    row[last] = value


def compare_records(original: bytes, published: bytes, fields: list[str]) -> list[str]:
    """Return every difference outside `fields` (header paths); empty means equal."""
    left, right = _rows(original), _rows(published)
    problems = []
    if len(left) != len(right):
        return [f"row count {len(left)} != {len(right)}"]
    if not left:
        return ["empty record"]
    header = copy.deepcopy(left[0])
    for field in fields:
        try:
            _set(header, field, _get(right[0], field))
        except (KeyError, TypeError):
            problems.append(f"header field {field} missing")
    if header != right[0]:
        keys = sorted(set(header) | set(right[0]))
        problems.extend(f"header field {key} differs" for key in keys
                        if header.get(key) != right[0].get(key))
    for index, (old, new) in enumerate(zip(left[1:], right[1:]), start=1):
        if old != new:
            problems.append(f"observation row {index} differs")
    if home_path_hits(published):
        problems.append("published record still carries a home path")
    for field in fields:
        try:
            value = _get(right[0], field)
        except (KeyError, TypeError):
            continue
        if value == _get(left[0], field):
            problems.append(f"header field {field} is unchanged")
    return problems


def git_blob(commit: str, path: str, root: Path = ROOT) -> bytes:
    result = subprocess.run(["git", "-C", str(root), "show", f"{commit}:{path}"],
                            capture_output=True)
    if result.returncode != 0:
        raise EvidenceError(f"original {path} at {commit} is not in git history")
    return result.stdout


def verify_receipts(receipts_path: Path = RECEIPTS, root: Path = ROOT) -> int:
    receipts = json.loads(receipts_path.read_text(encoding="utf-8"))
    if receipts.get("schema_version") != 1 or not receipts.get("archives"):
        raise EvidenceError("unknown or empty receipts file")
    for item in receipts["archives"]:
        original = git_blob(receipts["source_commit"], item["file"], root)
        published = (root / item["file"]).read_bytes()
        if hashlib.sha256(original).hexdigest() != item["source_sha256"]:
            raise EvidenceError(f"{item['file']}: original SHA-256 differs from receipt")
        if hashlib.sha256(published).hexdigest() != item["exported_sha256"]:
            raise EvidenceError(f"{item['file']}: published SHA-256 differs from receipt")
        problems = compare_records(original, published, item["changed_fields"])
        if problems:
            raise EvidenceError(f"{item['file']}: " + "; ".join(problems[:5]))
        rows = len(_rows(published)) - 1
        if rows != item["observation_rows"]:
            raise EvidenceError(f"{item['file']}: {rows} observation rows, receipt says "
                                f"{item['observation_rows']}")
    return len(receipts["archives"])


def scrub(files: list[str], source_commit: str, scratch: Path) -> dict:
    """Rewrite each archive's policy path via export_portable_record; return receipts."""
    archives = []
    for name in files:
        original = scratch / Path(name).name
        original.write_bytes(git_blob(source_commit, name))
        receipt = export_portable_record.export_record(original, ROOT / name)
        archives.append({
            "file": name,
            "source_sha256": receipt["source_sha256"],
            "exported_sha256": receipt["exported_sha256"],
            "metadata_normalization": receipt["metadata_normalization"],
            "changed_fields": ["scorecard.runner_provenance.policy.path"],
            "observation_rows": len(_rows((ROOT / name).read_bytes())) - 1,
        })
    return {
        "schema_version": 1,
        "comment": ("Original -> public hashes for evidence archives whose policy path named a "
                    "home directory. Checked by scripts/bench/evidence_home_paths.py "
                    "verify-receipts against the originals in git history."),
        "source_commit": source_commit,
        "archives": archives,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("check", help="fail on any tracked .gz carrying a home path")
    commands.add_parser("verify-receipts", help="prove scrubbed archives equal their originals")
    rewrite = commands.add_parser("scrub", help="rewrite archives and write receipts")
    rewrite.add_argument("--source-commit", required=True)
    rewrite.add_argument("--scratch", type=Path, required=True)
    rewrite.add_argument("files", nargs="+")
    args = parser.parse_args(argv)
    try:
        if args.command == "check":
            hits = scan(ROOT)
            for name, count in sorted(hits.items()):
                print(f"{name}: {count} home path(s)", file=sys.stderr)
            if hits:
                return 1
            print("EVIDENCE_HOME_PATHS_OK")
        elif args.command == "verify-receipts":
            print(f"PORTABLE_EVIDENCE_RECEIPTS_OK {verify_receipts()} archives")
        else:
            args.scratch.mkdir(parents=True, exist_ok=True)
            receipts = scrub(args.files, args.source_commit, args.scratch)
            RECEIPTS.write_text(json.dumps(receipts, indent=2) + "\n", encoding="utf-8")
            print(f"wrote {RECEIPTS.relative_to(ROOT)}")
    except EvidenceError as error:
        print(f"evidence_home_paths: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
