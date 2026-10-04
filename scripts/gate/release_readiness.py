#!/usr/bin/env python3
"""Fail-closed verification for the post-tag release-publication hold."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import re
import sys
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Any, Callable, Mapping


SHA = re.compile(r"^[0-9a-f]{40}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")


class ReadinessError(ValueError):
    """A required, immutable release-readiness binding is missing or invalid."""


@dataclass(frozen=True)
class ReadyRelease:
    release_commit: str
    documentation_commit: str


def require_sha(value: object, name: str, length: int = 40) -> str:
    pattern = SHA if length == 40 else SHA256
    if not isinstance(value, str) or not pattern.fullmatch(value):
        raise ReadinessError(f"{name} must be a lowercase {length}-character SHA")
    return value


def parse_receipt(message: str) -> Mapping[str, str]:
    lines = message.splitlines()
    if not lines or lines[0] != "gaze-release-readiness-v1":
        raise ReadinessError("readiness tag has an unknown receipt format")
    receipt: dict[str, str] = {}
    for line in lines[1:]:
        if not line:
            continue
        key, separator, value = line.partition("=")
        if not separator or not key or not value or key in receipt:
            raise ReadinessError("readiness tag receipt is malformed")
        receipt[key] = value
    return receipt


def required(receipt: Mapping[str, str], name: str, *, length: int = 40) -> str:
    try:
        return require_sha(receipt[name], name, length)
    except KeyError as exc:
        raise ReadinessError(f"readiness tag receipt is missing {name}") from exc


def validate(
    *,
    version: str,
    expected_release_commit: str,
    release_tag_object: Mapping[str, Any],
    release_tag: Mapping[str, Any],
    readiness_tag: Mapping[str, Any],
    readiness_tag_object: Mapping[str, Any],
    document_bytes: bytes,
    checks: list[Mapping[str, Any]],
) -> ReadyRelease:
    """Validate the immutable tag, documentation, measurement, and CI bindings."""
    expected_release_commit = require_sha(expected_release_commit, "expected release commit")
    if release_tag_object.get("type") != "tag":
        raise ReadinessError("release tag must be annotated")
    release_tag_oid = require_sha(release_tag_object.get("sha"), "release tag object")
    if release_tag.get("verification", {}).get("verified") is not True:
        raise ReadinessError("release tag signature is not verified")
    if release_tag.get("object", {}).get("type") != "commit":
        raise ReadinessError("release tag must target a commit")
    if release_tag["object"].get("sha") != expected_release_commit:
        raise ReadinessError("release tag target does not equal the measured release commit")

    if readiness_tag_object.get("type") != "tag":
        raise ReadinessError("readiness tag must be annotated")
    if readiness_tag.get("verification", {}).get("verified") is not True:
        raise ReadinessError("readiness tag signature is not verified")
    if readiness_tag.get("object", {}).get("type") != "commit":
        raise ReadinessError("readiness tag must target the accepted documentation commit")
    documentation_commit = require_sha(readiness_tag["object"].get("sha"), "documentation commit")

    receipt = parse_receipt(str(readiness_tag.get("message", "")))
    if receipt.get("version") != version:
        raise ReadinessError("readiness receipt version does not match the release tag")
    if required(receipt, "release_commit") != expected_release_commit:
        raise ReadinessError("readiness receipt release commit does not match the tag target")
    if required(receipt, "release_tag_object") != release_tag_oid:
        raise ReadinessError("readiness receipt does not bind the real annotated release tag")
    if required(receipt, "documentation_commit") != documentation_commit:
        raise ReadinessError("readiness receipt documentation commit does not match its tag target")
    harness_commit = required(receipt, "harness_commit")
    renderer_commit = required(receipt, "renderer_commit")
    evidence_sha256 = required(receipt, "evidence_sha256", length=64)
    manifest_sha256 = required(receipt, "documentation_manifest_sha256", length=64)
    if hashlib.sha256(document_bytes).hexdigest() != manifest_sha256:
        raise ReadinessError("documentation readiness manifest bytes do not match the signed receipt")

    try:
        document = json.loads(document_bytes)
    except json.JSONDecodeError as exc:
        raise ReadinessError("documentation readiness manifest is not JSON") from exc
    if not isinstance(document, dict):
        raise ReadinessError("documentation readiness manifest must be an object")
    expected_document = {
        "version": version,
        "release_commit": expected_release_commit,
        "release_tag_object": release_tag_oid,
        "harness_commit": harness_commit,
        "renderer_commit": renderer_commit,
    }
    for key, value in expected_document.items():
        if document.get(key) != value:
            raise ReadinessError(f"documentation readiness manifest {key} does not match the signed receipt")
    evidence = document.get("evidence")
    if not isinstance(evidence, dict) or evidence.get("sha256") != evidence_sha256:
        raise ReadinessError("documentation readiness manifest evidence hash does not match the signed receipt")
    if not isinstance(evidence.get("manifest_path"), str) or not evidence["manifest_path"].startswith(
        "docs/reference/benchmarks/evidence/"
    ):
        raise ReadinessError("documentation readiness manifest must name its immutable evidence manifest")
    scorecards = document.get("scorecards")
    if not isinstance(scorecards, dict) or not scorecards:
        raise ReadinessError("documentation readiness manifest must bind published scorecards")
    for name, digest in scorecards.items():
        if not isinstance(name, str) or not name or not SHA256.fullmatch(str(digest)):
            raise ReadinessError("documentation readiness manifest has an invalid scorecard digest")

    required_checks = document.get("successful_checks")
    if not isinstance(required_checks, list) or not required_checks or "docs" not in required_checks:
        raise ReadinessError("documentation readiness manifest must require the successful docs check")
    successful = {
        check.get("name")
        for check in checks
        if check.get("conclusion") == "success" and check.get("app", {}).get("slug") == "github-actions"
    }
    missing = [check for check in required_checks if not isinstance(check, str) or check not in successful]
    if missing:
        raise ReadinessError(f"accepted documentation commit lacks successful required checks: {', '.join(map(str, missing))}")
    return ReadyRelease(expected_release_commit, documentation_commit)


def github_get(repository: str, path: str, token: str) -> Mapping[str, Any]:
    request = urllib.request.Request(
        f"https://api.github.com/repos/{repository}{path}",
        headers={"Accept": "application/vnd.github+json", "Authorization": f"Bearer {token}"},
    )
    with urllib.request.urlopen(request) as response:  # noqa: S310 -- fixed GitHub API origin
        body = json.load(response)
    if not isinstance(body, dict):
        raise ReadinessError(f"GitHub API returned an unexpected object for {path}")
    return body


def content_bytes(content: Mapping[str, Any], description: str) -> bytes:
    if content.get("encoding") != "base64" or not isinstance(content.get("content"), str):
        raise ReadinessError(f"{description} is not a base64 GitHub content response")
    # GitHub wraps base64 with line breaks; keep every other character strict.
    encoded = content["content"].replace("\r", "").replace("\n", "")
    try:
        return base64.b64decode(encoded, validate=True)
    except ValueError as exc:
        raise ReadinessError(f"{description} has invalid base64 content") from exc


def workflow_run_id(check: Mapping[str, Any]) -> str:
    details_url = check.get("details_url")
    match = re.search(r"/actions/runs/(\d+)(?:/|$)", details_url) if isinstance(details_url, str) else None
    if match is None:
        raise ReadinessError("docs check has no GitHub Actions workflow-run URL")
    return match.group(1)


def main(argv: list[str] | None = None, get: Callable[[str], Mapping[str, Any]] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--release-commit", required=True)
    args = parser.parse_args(argv)
    if not re.fullmatch(r"v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", args.release_tag):
        parser.error("--release-tag must be a release tag such as v0.16.0")
    version = args.release_tag.removeprefix("v")
    token = os.environ.get("GITHUB_TOKEN")
    if get is None:
        if not token:
            raise ReadinessError("GITHUB_TOKEN is required to verify immutable GitHub evidence")
        get = lambda path: github_get(args.repository, path, token)

    tag_ref = urllib.parse.quote(f"tags/{args.release_tag}", safe="")
    readiness_ref = urllib.parse.quote(f"tags/release-readiness/{args.release_tag}", safe="")
    release_tag_object = get(f"/git/ref/{tag_ref}").get("object", {})
    readiness_tag_object = get(f"/git/ref/{readiness_ref}").get("object", {})
    release_tag = get(f"/git/tags/{release_tag_object.get('sha', '')}")
    readiness_tag = get(f"/git/tags/{readiness_tag_object.get('sha', '')}")
    documentation_commit = readiness_tag.get("object", {}).get("sha", "")
    document_path = urllib.parse.quote(
        f"docs/reference/benchmarks/release-readiness/{args.release_tag}.json", safe="/"
    )
    document = get(f"/contents/{document_path}?ref={documentation_commit}")
    document_bytes = content_bytes(document, "documentation readiness manifest")
    checks = get(f"/commits/{documentation_commit}/check-runs?per_page=100").get("check_runs")
    if not isinstance(checks, list):
        raise ReadinessError("GitHub check-runs response is malformed")
    ready = validate(
        version=version,
        expected_release_commit=args.release_commit,
        release_tag_object=release_tag_object,
        release_tag=release_tag,
        readiness_tag_object=readiness_tag_object,
        readiness_tag=readiness_tag,
        document_bytes=document_bytes,
        checks=checks,
    )
    document_json = json.loads(document_bytes)
    documentation_commit_object = get(f"/git/commits/{documentation_commit}")
    if documentation_commit_object.get("verification", {}).get("verified") is not True:
        raise ReadinessError("accepted documentation commit signature is not verified")
    evidence = document_json["evidence"]
    evidence_path = evidence["manifest_path"]
    if evidence_path.startswith("/") or ".." in evidence_path.split("/"):
        raise ReadinessError("documentation readiness manifest evidence path is unsafe")
    evidence_bytes = content_bytes(
        get(f"/contents/{urllib.parse.quote(evidence_path, safe='/')}?ref={documentation_commit}"),
        "immutable evidence manifest",
    )
    if hashlib.sha256(evidence_bytes).hexdigest() != evidence["sha256"]:
        raise ReadinessError("immutable evidence manifest bytes do not match the readiness manifest")
    try:
        evidence_manifest = json.loads(evidence_bytes)
    except json.JSONDecodeError as exc:
        raise ReadinessError("immutable evidence manifest is not JSON") from exc
    for identity in ("release_commit", "harness_commit", "renderer_commit"):
        if evidence_manifest.get(identity) != document_json[identity]:
            raise ReadinessError(f"immutable evidence manifest {identity} does not match the readiness manifest")
    docs_check = next(
        (check for check in checks if check.get("name") == "docs" and check.get("conclusion") == "success"), None
    )
    if docs_check is None:
        raise ReadinessError("accepted documentation commit has no successful docs check")
    docs_run = get(f"/actions/runs/{workflow_run_id(docs_check)}")
    if (
        docs_run.get("path") != ".github/workflows/docs.yml"
        or docs_run.get("head_sha") != documentation_commit
        or docs_run.get("conclusion") != "success"
    ):
        raise ReadinessError("docs check is not a successful docs.yml run for the accepted documentation commit")
    for path, expected_digest in document_json["scorecards"].items():
        if path.startswith("/") or ".." in path.split("/"):
            raise ReadinessError("documentation readiness manifest scorecard path is unsafe")
        scorecard = get(f"/contents/{urllib.parse.quote(path, safe='/')}?ref={documentation_commit}")
        actual_digest = hashlib.sha256(content_bytes(scorecard, f"scorecard {path}")).hexdigest()
        if actual_digest != expected_digest:
            raise ReadinessError(f"documentation scorecard bytes do not match the readiness manifest: {path}")
    print(f"release readiness accepted: release={ready.release_commit} documentation={ready.documentation_commit}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ReadinessError, urllib.error.URLError, json.JSONDecodeError, ValueError) as exc:
        print(f"release readiness rejected: {exc}", file=sys.stderr)
        raise SystemExit(1) from exc
