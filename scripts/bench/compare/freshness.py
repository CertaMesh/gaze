#!/usr/bin/env python3
"""Report upstream freshness without installing packages or changing measurement pins."""
from __future__ import annotations

import argparse
import ast
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
from functools import lru_cache
import hashlib
from http.client import HTTPException
import json
from pathlib import Path
import re
from urllib.error import HTTPError, URLError
from urllib.parse import quote
from urllib.request import Request, urlopen

from packaging.requirements import Requirement
from packaging.specifiers import SpecifierSet
from packaging.version import InvalidVersion, Version

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
SCHEMA = "gaze-competitor-freshness-v1"
STATUSES = {"current", "update-unverified", "incompatible-latest", "missing",
            "network-failure", "source-failure", "no-stable-release", "unversioned"}


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def literals(path: Path) -> dict:
    result = {}
    for node in ast.parse(path.read_text()).body:
        if isinstance(node, ast.Assign) and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name):
            try:
                result[node.targets[0].id] = ast.literal_eval(node.value)
            except (ValueError, TypeError):
                pass
    return result


def inventory() -> tuple[list[dict], dict[str, str]]:
    """Read every locked distribution, model wheel and declared model/setup pin."""
    rows = {}
    inputs = {}

    def bind(path):
        inputs[path.relative_to(ROOT).as_posix()] = digest(path)

    def add(kind, name, pin, path, **extra):
        key = f"{kind}:{name}@{pin}"
        row = rows.setdefault(key, {"key": key, "kind": kind, "name": name,
                                    "pinned_version": pin, "consumers": [], **extra})
        consumer = path.relative_to(ROOT).as_posix()
        if consumer not in row["consumers"]:
            row["consumers"].append(consumer)

    for lock in sorted(HERE.rglob("requirements*.lock")):
        bind(lock)
        for line in lock.read_text().splitlines():
            if not line or line[0].isspace() or line.startswith("#"):
                continue
            req = Requirement(line.removesuffix("\\").strip())
            if req.url:
                wheel = re.fullmatch(r"https://github.com/([^/]+/[^/]+)/releases/download/([^/]+)/[^/]+", req.url)
                git = re.fullmatch(r"git\+https://github.com/([^@]+)@([0-9a-f]{40})", req.url)
                if wheel:
                    repo, tag = wheel.groups()
                    add("github", repo, tag, lock, tag_prefix=tag.rsplit("-", 1)[0] + "-")
                elif git:
                    add("github", git[1], git[2], lock, tag_prefix="", commit_pin=True)
                else:
                    raise ValueError("unsupported locked source URL")
            else:
                specs = list(req.specifier)
                if len(specs) != 1 or specs[0].operator != "==":
                    raise ValueError("freshness requires exact package pins")
                name = re.sub(r"[-_.]+", "-", req.name).lower()
                add("pypi", name, specs[0].version, lock)

    wheels = HERE / "model-wheels.json"
    bind(wheels)
    for name, pin in json.loads(wheels.read_text()).items():
        add("github", "explosion/spacy-models", f"{name}-{pin['version']}", wheels, tag_prefix=f"{name}-")

    space = HERE / "tuned_presidio/space.py"
    bind(space)
    values = literals(space)
    for model in [values["DSLIM"], *values["EXTRA_NER"].values()]:
        add("huggingface", model["repo"], model["revision"], space)
    tracer = HERE / "pii_tracer.py"
    bind(tracer)
    values = literals(tracer)
    add("huggingface", values["MODEL_REPO"], values["REVISION"], tracer)
    vendor = HERE / "theirbench/vendor-tuned.json"
    bind(vendor)
    for setup in json.loads(vendor.read_text()).values():
        if isinstance(setup, dict):
            model = setup.get("model", {})
            if model:
                add("huggingface", model["model"], model["revision"], vendor)
            source = re.search(r"github.com/([^/]+/[^/]+)/", setup["source"])[1]
            add("github", source, setup["commit"], vendor, tag_prefix="", commit_pin=True)

    sources = HERE / "freshness-sources.json"
    bind(sources)
    config = json.loads(sources.read_text())
    for source in config["additional_sources"]:
        add(source["kind"], source["name"], source["pin"], sources, **source.get("options", {}))
    for row in rows.values():
        row["consumers"].sort()
        row["driver_constraint"] = config["driver_constraints"].get(f"{row['kind']}:{row['name']}")
    return sorted(rows.values(), key=lambda r: r["key"]), inputs


class MissingSource(Exception):
    pass


class SourceFailure(Exception):
    pass


@lru_cache(maxsize=1024)
def fetch(url: str, timeout: float = 8) -> object:
    request = Request(url, headers={"User-Agent": SCHEMA, "Accept": "application/json"})
    try:
        with urlopen(request, timeout=timeout) as response:
            data = response.read(16 * 1024 * 1024 + 1)
            if len(data) > 16 * 1024 * 1024:
                raise SourceFailure("response too large")
            return json.loads(data)
    except HTTPError as exc:
        if exc.code == 404:
            raise MissingSource() from None
        raise SourceFailure(f"HTTP {exc.code}") from None
    except (HTTPException, ValueError, RecursionError):
        raise SourceFailure("invalid HTTP response or JSON") from None


def metadata(value, expected):
    """Reject unsupported upstream shapes at the adapter boundary."""
    if not isinstance(value, expected):
        raise SourceFailure("unsupported metadata shape")
    return value


def sha(value):
    value = metadata(value, str)
    if re.fullmatch(r"[0-9a-fA-F]{40}", value) is None:
        raise SourceFailure("unsupported commit identity")
    return value.lower()


def stable(value: str, prefix: str = "") -> Version | None:
    metadata(value, str)
    if not value.startswith(prefix):
        return None
    try:
        version = Version(value[len(prefix):])
    except InvalidVersion:
        return None
    return None if version.is_prerelease or version.is_devrelease else version


def date(value: str | None) -> str | None:
    if value is None:
        return None
    metadata(value, str)
    return datetime.fromisoformat(value.replace("Z", "+00:00")).date().isoformat()


def release(version: str, released: str | None) -> dict:
    return {"version": metadata(version, str), "release_date": date(released)}


def pypi(row, get):
    url = f"https://pypi.org/pypi/{quote(row['name'], safe='')}/json"
    data = get(url)
    releases = metadata(metadata(data, dict)["releases"], dict)
    candidates = []
    pinned = None
    for version, files in releases.items():
        metadata(files, list)
        for item in files:
            metadata(item, dict)
            metadata(item["upload_time_iso_8601"], str)
            metadata(item.get("yanked", False), bool)
            if item.get("requires_python") is not None:
                metadata(item["requires_python"], str)
        try:
            parsed = Version(version)
        except InvalidVersion:
            continue
        if parsed == Version(row["pinned_version"]):
            pinned = release(version, min((f["upload_time_iso_8601"] for f in files), default=None))
        parsed = stable(version)
        usable = [f for f in files if not f.get("yanked", False)]
        if parsed is not None and usable:
            candidates.append((parsed, version, usable))
    if not candidates:
        return pinned, None, "no-stable-release", url
    _, version, files = max(candidates)
    latest = release(version, min(f["upload_time_iso_8601"] for f in files))
    # Different files can support different interpreters. A candidate is compatible
    # with this locked Python protocol if at least one release file supports it.
    if all(f.get("requires_python") and not SpecifierSet(f["requires_python"]).contains("3.12") for f in files):
        latest["incompatible_reason"] = "latest release does not support the locked Python 3.12 protocol"
    return pinned, latest, None, url


def github(row, get):
    base = f"https://api.github.com/repos/{row['name']}"
    pin = row["pinned_version"]
    if row.get("commit_pin"):
        commit = metadata(get(f"{base}/commits/{quote(pin, safe='')}"), dict)
        committer = metadata(metadata(commit["commit"], dict)["committer"], dict)
        pinned = release(pin, committer["date"])
        pinned["commit"] = sha(commit["sha"])
    else:
        data = metadata(get(f"{base}/releases/tags/{quote(pin, safe='')}"), dict)
        pinned = release(pin, data["published_at"])
        pinned["commit"] = github_commit(base, pin, get)
    if row["tag_prefix"]:
        # Model repositories publish many unrelated families. Query this tag
        # namespace directly instead of exhausting the repository release list.
        refs = metadata(get(f"{base}/git/matching-refs/tags/{quote(row['tag_prefix'], safe='')}"), list)
        tags = [metadata(metadata(ref, dict)["ref"], str).removeprefix("refs/tags/") for ref in refs]
        candidates = [(stable(tag, row["tag_prefix"]), tag) for tag in tags
                      if stable(tag, row["tag_prefix"]) is not None]
        for _, tag in sorted(candidates, reverse=True)[:100]:
            try:
                item = metadata(get(f"{base}/releases/tags/{quote(tag, safe='')}"), dict)
            except MissingSource:
                continue
            if not metadata(item["draft"], bool) and not metadata(item["prerelease"], bool):
                latest = release(tag, item["published_at"])
                latest["commit"] = github_commit(base, tag, get)
                return pinned, latest, None, base
        if len(candidates) > 100:
            raise SourceFailure("release lookup limit reached")
        return pinned, None, "no-stable-release", base
    candidates = []
    # A bounded listing cannot certify a missing/older release beyond its end.
    for page in range(1, 11):
        batch = metadata(get(f"{base}/releases?per_page=100&page={page}"), list)
        for item in batch:
            metadata(item, dict)
            version = stable(item["tag_name"], row["tag_prefix"])
            if not metadata(item["draft"], bool) and not metadata(item["prerelease"], bool) and version is not None:
                candidates.append((version, item["tag_name"], item["published_at"]))
        if len(batch) < 100:
            break
    else:
        raise SourceFailure("release pagination limit reached")
    if not candidates:
        return pinned, None, "no-stable-release", base
    _, version, released = max(candidates)
    latest = release(version, released)
    latest["commit"] = github_commit(base, version, get)
    return pinned, latest, None, base


def github_commit(base, tag, get):
    """Resolve lightweight or nested annotated tags, never target_commitish branches."""
    ref = metadata(get(f"{base}/git/ref/tags/{quote(tag, safe='')}"), dict)
    obj = metadata(ref["object"], dict)
    seen = set()
    for _ in range(10):
        identity = sha(obj["sha"])
        if obj["type"] == "commit":
            return identity
        if obj["type"] != "tag" or identity in seen:
            raise SourceFailure("unsupported or cyclic tag object")
        seen.add(identity)
        annotated = metadata(get(f"{base}/git/tags/{identity}"), dict)
        obj = metadata(annotated["object"], dict)
    raise SourceFailure("annotated tag depth limit reached")


def huggingface(row, get):
    base = f"https://huggingface.co/api/models/{row['name']}"

    def snapshot(revision):
        commits = metadata(get(f"{base}/commits/{quote(revision, safe='')}"), list)
        commit = metadata(commits[0], dict)
        return release(sha(commit["id"]), commit["date"])

    pinned = snapshot(row["pinned_version"])
    tags = metadata(metadata(get(f"{base}/refs"), dict)["tags"], list)
    candidates = []
    for tag in tags:
        metadata(tag, dict)
        version = stable(tag["name"])
        if version is not None:
            sha(tag["targetCommit"])
            candidates.append((version, tag))
    if not candidates:
        # main is a moving branch, not evidence of a stable release.
        return pinned, None, "unversioned", base
    _, tag = max(candidates, key=lambda v: v[0])
    latest = snapshot(tag["targetCommit"])
    latest["tag"] = tag["name"]
    return pinned, latest, None, base


def opaque(row, get):
    # The OPF receipt retains a checkpoint digest, without a versioned upstream
    # release source. Make that limitation visible rather than omitting the model.
    return release(row["pinned_version"], None), None, "unversioned", None


def check(row: dict, checked_on: str, get=fetch) -> dict:
    result = {**row, "checked_on": checked_on, "pinned": release(row["pinned_version"], None),
              "latest_stable": None, "status": "source-failure", "finding": None, "source_url": None}
    try:
        pinned, latest, status, url = {"pypi": pypi, "github": github, "huggingface": huggingface, "opaque": opaque}[row["kind"]](row, get)
        result.update(pinned=pinned or result["pinned"], latest_stable=latest, source_url=url)
        if pinned is None:
            status = "missing"
        if status:
            result["status"] = status
            result["finding"] = {"missing": "pinned version absent upstream", "no-stable-release": "no published stable release found",
                                 "unversioned": "no stable version tags or release source; retain the exact snapshot and qualify updates separately"}[status]
            return result
        constraint = row.get("driver_constraint")
        incompatible = latest.get("incompatible_reason")
        if constraint and not SpecifierSet(constraint["specifier"]).contains(latest["version"].removeprefix(row.get("tag_prefix", ""))):
            incompatible = constraint["reason"]
        if row["kind"] == "pypi":
            same = Version(latest["version"]) == Version(pinned["version"])
        elif row["kind"] == "github":
            same = latest["commit"] == pinned["commit"]
        else:
            same = latest["version"] == pinned["version"]
        if incompatible and not same:
            result.update(status="incompatible-latest", finding=incompatible + "; measured row remains explicitly old")
        elif same:
            result["status"] = "current"
        else:
            result.update(status="update-unverified", finding="different stable release; retain old pin until a separate reviewed driver/pin change and new subject identity")
    except MissingSource:
        result.update(status="missing", finding="package, repository or pinned revision not found")
    except (URLError, TimeoutError, OSError):
        result.update(status="network-failure", finding="source check failed; freshness unknown")
    except (SourceFailure, HTTPException, ValueError, KeyError, IndexError, TypeError, RecursionError):
        result.update(status="source-failure", finding="source metadata unavailable or unsupported; freshness unknown")
    return result


def validate(report: dict, comparison: Path | None = None) -> None:
    if report.get("schema") != SCHEMA or not isinstance(report.get("rows"), list):
        raise ValueError("unsupported freshness inventory")
    if comparison and report.get("comparison_sha256") != digest(comparison):
        raise ValueError("freshness inventory belongs to another comparison")
    if date(report["checked_on"]) != report["checked_on"]:
        raise ValueError("invalid checked-on date")
    for path, sha in report["inputs"].items():
        if not (ROOT / path).resolve().is_relative_to(HERE) or digest(ROOT / path) != sha:
            raise ValueError("freshness source pins changed; rerun the check")
    rows, inputs = inventory()
    if report["inputs"] != inputs:
        raise ValueError("freshness input inventory changed")
    selected = set(report.get("selection") or [r["key"] for r in rows])
    if [r["key"] for r in report["rows"]] != [r["key"] for r in rows if r["key"] in selected]:
        raise ValueError("freshness source inventory incomplete")
    known = {r["key"]: r for r in rows}
    seen = set()
    for row in report["rows"]:
        key = row["key"]
        if key in seen or key not in known or any(row.get(k) != v for k, v in known[key].items()):
            raise ValueError("freshness pin metadata differs from locked source")
        seen.add(key)
        if row["status"] not in STATUSES or row["checked_on"] != report["checked_on"]:
            raise ValueError("invalid freshness classification")
        for version in (row["pinned"], row["latest_stable"]):
            if version is not None and (not isinstance(version["version"], str) or (version["release_date"] is not None and date(version["release_date"]) != version["release_date"])):
                raise ValueError("invalid release metadata")


def read_for_comparison(path: Path, comparison: Path) -> dict:
    report = json.loads(path.read_text())
    validate(report, comparison)
    if report.get("selection"):
        raise ValueError("partial freshness check cannot describe the aggregate comparison")
    return report


def markdown(report: dict) -> str:
    lines = ["## Competitor source freshness", "", f"Checked on {report['checked_on']} (UTC). "
             "Pins and measurement results are unchanged. New pins require review and new subject identities.", "",
             "| Source | Pinned version | Pin release date | Current stable | Stable release date | Finding |",
             "| --- | --- | --- | --- | --- | --- |"]
    for row in report["rows"]:
        pin, latest = row["pinned"], row["latest_stable"]
        cells = [f"{row['kind']}:{row['name']}", pin["version"], pin["release_date"] or "unknown",
                 latest["version"] if latest else "unknown", latest["release_date"] or "unknown" if latest else "unknown",
                 row["status"] + (": " + row["finding"] if row["finding"] else "")]
        lines.append("| " + " | ".join(str(c).replace("|", "&#124;").replace("\n", " ") for c in cells) + " |")
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--report", type=Path, help="bind the sidecar to this comparison's exact bytes")
    parser.add_argument("--source", action="append", help="check only this source key (repeatable); partial reports cannot be published")
    parser.add_argument("--timeout", type=float, default=8)
    args = parser.parse_args()
    if not 0 < args.timeout <= 30:
        parser.error("timeout must be in (0, 30]")
    rows, inputs = inventory()
    if args.source:
        rows = [row for row in rows if row["key"] in args.source]
        if set(args.source) != {row["key"] for row in rows}:
            parser.error("unknown source key")
    checked = datetime.now(timezone.utc).date().isoformat()
    with ThreadPoolExecutor(max_workers=4) as executor:
        results = list(executor.map(lambda row: check(row, checked, lambda url: fetch(url, args.timeout)), rows))
    report = {"schema": SCHEMA, "checked_on": checked, "inputs": inputs, "selection": args.source,
              "comparison_sha256": digest(args.report) if args.report else None, "rows": results}
    validate(report, args.report)
    protected = {*(ROOT / path for path in inputs), HERE / "freshness.py", HERE / "freshness.schema.json"}
    if args.report:
        protected.add(args.report.resolve())
    if args.output.resolve() in protected:
        parser.error("output would overwrite a source or measurement")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(markdown(report), end="")


if __name__ == "__main__":
    main()
