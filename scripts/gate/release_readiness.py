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
from pathlib import Path
from typing import Any, Callable, Mapping


# Reuse the shipped stdlib renderer's schema/projection; never execute benchmarks.
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "bench"))
import render_benchmark_doc as render


BENCH = "docs/reference/benchmarks/"
MANDATORY_ACCEPTANCE = frozenset({
    "release_preflight", "benchmark_gain_v1", "benchmark_gain_v2",
    "historical_comparisons", "competitors", "native", "restore", "manifest",
    "private_preview", "public_documentation", "timing_authority",
    "tag_namespace", "publisher_refs",
})
GITHUB_ACTIONS_APP_ID = 15368


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

    # GitHub's message includes signature armor; the verified payload does not.
    verification = readiness_tag["verification"]
    payload = verification.get("payload")
    signature = verification.get("signature")
    if not isinstance(payload, str) or not isinstance(signature, str) or not signature:
        raise ReadinessError("readiness tag is missing its verified payload or signature")
    headers, separator, receipt_message = payload.partition("\n\n")
    expected_headers = f"object {documentation_commit}\ntype commit\ntag release-readiness/v{version}\n"
    if not separator or not headers.startswith(expected_headers):
        raise ReadinessError("readiness tag has a malformed verified payload")
    if readiness_tag.get("message") != receipt_message + signature:
        raise ReadinessError("readiness tag message does not match the verified payload and signature")
    receipt = parse_receipt(receipt_message)
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

    document = json_object(document_bytes, "documentation readiness manifest")
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
        if trusted_check(check, documentation_commit)
    }
    missing = [check for check in required_checks if not isinstance(check, str) or check not in successful]
    if missing:
        raise ReadinessError(
            f"accepted documentation commit lacks successful required checks: {', '.join(map(str, missing))}",
        )
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


def trusted_check(check: Mapping[str, Any], commit: str) -> bool:
    return (
        check.get("head_sha") == commit
        and check.get("status") == "completed"
        and check.get("conclusion") == "success"
        and isinstance(check.get("app"), dict)
        and check["app"].get("slug") == "github-actions"
        and check["app"].get("id") == GITHUB_ACTIONS_APP_ID
    )


def workflow_run_id(check: Mapping[str, Any], repository: str) -> str:
    # Anchor the origin, repository and job identity, not just a URL substring.
    match = re.fullmatch(
        rf"https://github\.com/{re.escape(repository)}/actions/runs/([1-9][0-9]*)/job/([1-9][0-9]*)",
        check.get("details_url", "") if isinstance(check.get("details_url"), str) else "",
    )
    if match is None or type(check.get("id")) is not int or str(check["id"]) != match.group(2):
        raise ReadinessError("docs check has no trusted repository workflow-run/job URL")
    return match.group(1)


def verify_documentation_source(get, repository, release_commit, commit, checks):
    repo = get("")
    if repo.get("full_name") != repository or type(repo.get("id")) is not int:
        raise ReadinessError("repository identity is invalid")
    default = repo.get("default_branch")
    if not isinstance(default, str) or not default:
        raise ReadinessError("repository default branch is missing")
    branch = get(f"/branches/{urllib.parse.quote(default, safe='')}")
    tip = require_sha(branch.get("commit", {}).get("sha"), "default branch tip")
    if branch.get("name") != default or branch.get("protected") is not True:
        raise ReadinessError("documentation default branch is not protected")
    for base, head in ((release_commit, commit), (commit, tip)):
        compare = get(f"/compare/{base}...{head}")
        if (
            compare.get("status") not in {"ahead", "identical"}
            or compare.get("base_commit", {}).get("sha") != base
            or compare.get("merge_base_commit", {}).get("sha") != base
        ):
            raise ReadinessError("documentation must descend from R and belong to the protected default branch")
    # A commit may have both PR and main checks. Select one complete trusted
    # main run; use that same check for every join rather than mixing records.
    for docs_check in checks:
        if docs_check.get("name") != "docs" or not trusted_check(docs_check, commit):
            continue
        try:
            verify_docs_run(get, repository, repo, default, commit, docs_check)
        except ReadinessError:
            continue
        return
    raise ReadinessError("accepted documentation commit lacks a joined successful main docs run")


def verify_docs_run(get, repository, repo, default, commit, docs_check):
    run_id = workflow_run_id(docs_check, repository)
    run = get(f"/actions/runs/{run_id}")
    workflow = get("/actions/workflows/docs.yml")
    suite_id = docs_check.get("check_suite", {}).get("id")
    if (
        type(suite_id) is not int or suite_id <= 0
        or run.get("check_suite_id") != suite_id
        or type(run.get("id")) is not int or str(run["id"]) != run_id
        or type(workflow.get("id")) is not int
        or run.get("workflow_id") != workflow["id"]
        or workflow.get("path") != ".github/workflows/docs.yml"
        or run.get("path") != ".github/workflows/docs.yml"
        or run.get("head_sha") != commit
        or run.get("status") != "completed" or run.get("conclusion") != "success"
        or run.get("head_branch") != default or run.get("event") != "push"
        or any(run.get(key, {}).get("id") != repo["id"] or run.get(key, {}).get("full_name") != repository
               for key in ("repository", "head_repository"))
    ):
        raise ReadinessError(
            "docs check is not a successful docs.yml run joined to the protected documentation source",
        )


def json_object(raw: bytes, description: str) -> dict[str, Any]:
    def unique_pairs(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ReadinessError(f"{description} contains duplicate JSON keys")
            result[key] = value
        return result

    def reject_constant(value):
        raise ReadinessError(f"{description} contains a non-finite JSON value")

    try:
        value = json.loads(raw, object_pairs_hook=unique_pairs, parse_constant=reject_constant)
    except (ValueError, UnicodeDecodeError) as exc:
        raise ReadinessError(f"{description} is not JSON") from exc
    if not isinstance(value, dict):
        raise ReadinessError(f"{description} must be an object")
    return value


def bound_content(get, binding, commit, description):
    if not isinstance(binding, dict):
        raise ReadinessError(f"{description} binding is missing")
    path = binding.get("path")
    if (not isinstance(path, str) or not path.startswith(BENCH)
            or any(part in {"", ".", ".."} for part in path.split("/"))):
        raise ReadinessError(f"{description} path is unsafe")
    expected = require_sha(binding.get("sha256"), f"{description} digest", 64)
    raw = content_bytes(get(f"/contents/{urllib.parse.quote(path, safe='/')}?ref={commit}"), description)
    if hashlib.sha256(raw).hexdigest() != expected:
        raise ReadinessError(f"{description} bytes do not match their binding")
    return raw


def scorecard_identities(card):
    dataset, parameters = card.get("dataset"), card.get("parameters")
    provenance = card.get("runner_provenance")
    if not all(isinstance(v, dict) for v in (dataset, parameters, provenance)):
        raise ReadinessError("scorecard input/policy/model provenance is incomplete")
    if not all(isinstance(dataset.get(key), str) and dataset[key] for key in ("repository", "revision", "file")):
        raise ReadinessError("scorecard frozen input identity is incomplete")
    population = dataset.get("evaluated_population")
    if not isinstance(population, dict) or type(population.get("documents")) is not int or population["documents"] <= 0:
        raise ReadinessError("scorecard must describe an actually evaluated population")
    policy = require_sha(parameters.get("policy_sha256"), "scorecard policy digest", 64)
    if provenance.get("policy", {}).get("sha256") != policy:
        raise ReadinessError("scorecard policy provenance does not match parameters")
    models = provenance.get("model_bundles")
    if not isinstance(models, list):
        raise ReadinessError("scorecard model bundle inventory is missing")
    seen = set()
    for model in models:
        if not isinstance(model, dict) or not isinstance(model.get("model_id"), str) or not model["model_id"] or model["model_id"] in seen:
            raise ReadinessError("scorecard model identity is invalid or duplicated")
        seen.add(model["model_id"])
        expected = require_sha(model.get("expected_sha256"), "model expected digest", 64)
        if model.get("observed_sha256") != expected:
            raise ReadinessError("scorecard model observed digest does not match expected digest")
    input_identity = {key: dataset[key] for key in (
        "repository", "revision", "file", "integrity", "evaluated_population", "sampling"
    ) if key in dataset}
    if parameters.get("profile") != "full":
        raise ReadinessError("release scorecards must record the full benchmark profile")
    return {"dataset": input_identity, "parameters": parameters, "policy_sha256": policy,
            "model_bundles": models, "scored_label_contract": card.get("scoring", {}).get("scored_label_contract")}


def verify_evidence(get, document, evidence, commit):
    # A signed maintainer attests execution and acceptance. This verifier joins
    # identities, schema, completeness and PASS receipts; it does not rerun gates.
    release = document["release_commit"]
    if type(document.get("schema_version")) is not int or document["schema_version"] != 1 or type(evidence.get("schema_version")) is not int or evidence["schema_version"] != 1 or evidence.get("format") != "gaze-release-evidence-v1":
        raise ReadinessError("unknown release documentation/evidence contract")
    if set(evidence) != {
        "schema_version", "format", "version", "release_commit", "measured_commit",
        "harness_commit", "renderer_commit", "status", "scorecards", "acceptance", "history",
    }:
        raise ReadinessError("release evidence has missing or unknown fields")
    for key in ("version", "release_commit", "harness_commit", "renderer_commit"):
        if evidence.get(key) != document[key]:
            raise ReadinessError(f"immutable evidence manifest {key} does not match the readiness manifest")
    if evidence.get("measured_commit") != release or evidence.get("status") != "PASS":
        raise ReadinessError("evidence does not accept the actual measured release commit")
    acceptance = evidence.get("acceptance")
    if not isinstance(acceptance, dict) or acceptance.get("authority") != "signed-maintainer":
        raise ReadinessError("explicit signed maintainer acceptance is required")
    gates = acceptance.get("gates")
    if not isinstance(gates, dict) or set(gates) != MANDATORY_ACCEPTANCE:
        raise ReadinessError("mandatory gate acceptance inventory is incomplete or unknown")
    for name, result in gates.items():
        if not isinstance(result, dict) or result.get("status") != "PASS":
            raise ReadinessError(f"mandatory gate {name} is not accepted PASS")
        receipt = json_object(bound_content(get, result.get("receipt"), commit, f"{name} acceptance receipt"), name)
        if type(receipt.get("schema_version")) is not int or receipt["schema_version"] != 1 or receipt.get("gate") != name or receipt.get("status") != "PASS" or receipt.get("release_commit") != release:
            raise ReadinessError(f"mandatory gate {name} receipt does not accept R")
    version = "v" + document["version"]
    paths = {
        1: BENCH + f"scorecard-{version}.json",
        **{n: BENCH + f"scorecard-{version}-scored-labels-v{n}.json" for n in (2, 3)},
    }
    if set(document["scorecards"]) != set(paths.values()) or not isinstance(evidence.get("scorecards"), dict) or set(evidence["scorecards"]) != set(paths.values()):
        raise ReadinessError("release must bind the complete version-owned v1/v2/v3 scorecard inventory")
    if evidence.get("history") != document.get("history"):
        raise ReadinessError("evidence/history digest binding does not match documentation")
    if not isinstance(document.get("history"), dict) or document["history"].get("path") != BENCH + "release-history.json":
        raise ReadinessError("current release history binding is missing")
    history = json_object(bound_content(get, document["history"], commit, "release history"), "release history")
    policy_digest = hashlib.sha256(bound_content(get, document.get("policy"), commit, "frozen policy")).hexdigest()
    try:
        render.validate_history(history)
        rows = [r for r in history["releases"] if r["version"] == version]
        if len(rows) != 1 or history["releases"][-1] != rows[0]:
            raise ReadinessError("release history must end with exactly one current release row")
        row = rows[0]
        if row.get("commit") != release or row.get("provisional") is not False:
            raise ReadinessError("current history row must record the actual non-provisional R")
        results = row.get("contract_results")
        if not isinstance(results, list) or {r["scored_label_contract"]["version"] for r in results} != {2, 3} or len(results) != 2:
            raise ReadinessError("current history must join contract v1/v2/v3 scorecards")
        shared_identity = None
        for number, path in paths.items():
            digest = document["scorecards"][path]
            card = json_object(bound_content(get, {"path": path, "sha256": digest}, commit, "scorecard"), "scorecard")
            if card.get("gaze", {}).get("revision") != release or card.get("gaze", {}).get("dirty") is not False:
                raise ReadinessError("current scorecard must record clean measured R")
            identities = scorecard_identities(card)
            if evidence["scorecards"][path] != {"sha256": digest, "identities": identities}:
                raise ReadinessError("scorecard frozen input/policy/model identities do not match evidence")
            common = {key: value for key, value in identities.items() if key != "scored_label_contract"}
            if shared_identity is not None and common != shared_identity:
                raise ReadinessError("current scorecards do not share frozen inputs/policy/models")
            shared_identity = common
            if identities["policy_sha256"] != policy_digest:
                raise ReadinessError("scorecard policy differs from frozen policy bytes")
            harness_revision = card["runner_provenance"].get("harness_revision")
            if harness_revision is not None and harness_revision != document["harness_commit"]:
                raise ReadinessError("scorecard measured harness differs from frozen H")
            if number > 1:
                contract = identities["scored_label_contract"]
                if not isinstance(contract, dict) or contract.get("version") != number:
                    raise ReadinessError("scorecard scored-label contract does not match its inventory slot")
                bound_content(
                    get,
                    {
                        "path": BENCH + f"scored-labels-v{number}.json",
                        "sha256": contract.get("file_sha256"),
                    },
                    document["harness_commit"],
                    "frozen scored-label contract",
                )
                projected = render.contract_result_from_scorecard(
                    card,
                    row,
                    scorecard_filename=Path(path).name,
                    scorecard_sha256=digest,
                )
                actual = next(r for r in results if r["scored_label_contract"]["version"] == number)
            else:
                if render._scored_label_contract(card) is not None:
                    raise ReadinessError("primary scorecard must use scored-label contract v1")
                projected = render.history_entry_from_scorecard(
                    card,
                    version=version,
                    machine=row["machine"],
                    scorecard_filename=Path(path).name,
                    scorecard_sha256=digest,
                    shipped_arm=render.shipped_default_arm(row),
                )
                actual = row
            for key, value in projected.items():
                if key not in {"note", "date"} and actual.get(key) != value:
                    raise ReadinessError(f"current history {key} does not match scorecard projection")
    except (render.RenderError, KeyError, TypeError, AttributeError) as exc:
        raise ReadinessError(f"invalid release scorecard/history schema: {exc}") from exc


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
    if not isinstance(checks, list) or not all(isinstance(check, dict) for check in checks):
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
    document_json = json_object(document_bytes, "documentation readiness manifest")
    documentation_commit_object = get(f"/git/commits/{documentation_commit}")
    if documentation_commit_object.get("sha") != documentation_commit or documentation_commit_object.get("verification", {}).get("verified") is not True:
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
    evidence_manifest = json_object(evidence_bytes, "immutable evidence manifest")
    verify_documentation_source(get, args.repository, ready.release_commit, documentation_commit, checks)
    verify_evidence(get, document_json, evidence_manifest, documentation_commit)
    print(
        f"release readiness accepted: release={ready.release_commit} documentation={ready.documentation_commit}",
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ReadinessError, urllib.error.URLError, json.JSONDecodeError, ValueError) as exc:
        print(f"release readiness rejected: {exc}", file=sys.stderr)
        raise SystemExit(1) from exc
