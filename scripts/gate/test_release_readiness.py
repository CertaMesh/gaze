import base64
import copy
import hashlib
import json
from pathlib import Path
import unittest

import release_readiness as gate


R = "a" * 40
D = "b" * 40
H = "c" * 40
P = "d" * 40
TAG = "e" * 40
E = "1" * 64
SIGNATURE = "-----BEGIN SSH SIGNATURE-----\nc3ludGhldGljLXNpZ25hdHVyZQ==\n-----END SSH SIGNATURE-----\n"


def signed_readiness_tag(receipt):
    # Model GitHub's verified API envelope; cryptographic verification is upstream.
    receipt = receipt.rstrip("\n") + "\n"
    return {
        "object": {"type": "commit", "sha": D},
        "message": receipt + SIGNATURE,
        "verification": {
            "verified": True,
            "signature": SIGNATURE,
            "payload": f"object {D}\ntype commit\ntag release-readiness/v0.16.0\n"
            "tagger Fixture <signer@example.invalid> 0 +0000\n\n" + receipt,
        },
    }


def signed_release_tag(name="v0.16.0"):
    # Synthetic API envelope: this exercises binding, not cryptography.
    message = name + "\n"
    return {
        "tag": name,
        "object": {"type": "commit", "sha": R},
        "message": message + SIGNATURE,
        "verification": {
            "verified": True, "signature": SIGNATURE,
            "payload": f"object {R}\ntype commit\ntag {name}\n"
            "tagger Fixture <signer@example.invalid> 0 +0000\n\n" + message,
        },
    }


def inputs():
    document = {
        "version": "0.16.0",
        "release_commit": R,
        "release_tag_object": TAG,
        "harness_commit": H,
        "renderer_commit": P,
        "evidence": {
            "manifest_path": "docs/reference/benchmarks/evidence/v0.16.0.json",
            "sha256": E,
        },
        "scorecards": {"docs/reference/benchmarks/scorecard-v0.16.0.json": "2" * 64},
        "successful_checks": ["docs", "test"],
    }
    raw = json.dumps(document, sort_keys=True).encode()
    receipt = "\n".join(
        [
            "gaze-release-readiness-v1",
            "version=0.16.0",
            f"release_commit={R}",
            f"release_tag_object={TAG}",
            f"documentation_commit={D}",
            f"harness_commit={H}",
            f"renderer_commit={P}",
            f"evidence_sha256={E}",
            f"documentation_manifest_sha256={hashlib.sha256(raw).hexdigest()}",
        ]
    )
    return {
        "version": "0.16.0",
        "expected_release_commit": R,
        "release_tag_object": {"type": "tag", "sha": TAG},
        "release_tag": signed_release_tag(),
        "readiness_tag_object": {"type": "tag", "sha": "f" * 40},
        "readiness_tag": signed_readiness_tag(receipt),
        "document_bytes": raw,
        "checks": [
            {
                "name": "docs",
                "head_sha": D,
                "status": "completed",
                "conclusion": "success",
                "app": {"id": 15368, "slug": "github-actions"},
            },
            {
                "name": "test",
                "head_sha": D,
                "status": "completed",
                "conclusion": "success",
                "app": {"id": 15368, "slug": "github-actions"},
            },
        ],
    }


ROOT = Path(__file__).resolve().parents[2]
ARGV = ["--repository", "CertaMesh/gaze", "--release-tag", "v0.16.0", "--release-commit", R]
PREFIX = "docs/reference/benchmarks/"
CARD_PATHS = {
    1: PREFIX + "scorecard-v0.16.0.json",
    2: PREFIX + "scorecard-v0.16.0-scored-labels-v2.json",
    3: PREFIX + "scorecard-v0.16.0-scored-labels-v3.json",
}
# Explicit acceptance categories; these fixtures attest synthetic results only.
GATES = (
    "release_preflight",
    "benchmark_gain_v1",
    "benchmark_gain_v2",
    "historical_comparisons",
    "competitors",
    "native",
    "restore",
    "manifest",
    "private_preview",
    "public_documentation",
    "timing_authority",
    "tag_namespace",
    "publisher_refs",
)


def raw_json(value):
    return json.dumps(value, sort_keys=True).encode() + b"\n"


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def card_identity(card):
    identity = {
        "dataset": {k: copy.deepcopy(v) for k, v in card["dataset"].items() if k in ("repository", "revision", "file", "integrity", "evaluated_population", "sampling")},
        "parameters": copy.deepcopy(card["parameters"]),
        "policy_sha256": card["parameters"]["policy_sha256"],
        "model_bundles": copy.deepcopy(card["runner_provenance"]["model_bundles"]),
        "scored_label_contract": card.get("scoring", {}).get("scored_label_contract"),
    }
    if isinstance(card.get("layers"), dict):
        identity["agentic_layers"] = copy.deepcopy({
            "generator": card["layers"].get("generator"),
            "scored_label_contract": card["layers"].get("scored_label_contract"),
            "binary_commit": card.get("binary_commit", card.get("gaze")),
            "binary_sha256": card.get("binary_sha256"),
        })
    return identity


def synthetic_card(contract, contract_bytes):
    run = {
        "config": "policy-file",
        "metrics": {
            "utf8_bytes": {"pii": 10, "leaked": 1, "leak_rate": .1, "false_positive": 0, "precision": 1.0},
            "zero_leak_document_rate": 0.5,
        },
        "pipeline_contract": {"restore_exact_rate": 1.0, "manifest_valid_document_rate": 1.0},
        "pipeline_availability": {"completion_rate": 1.0, "failed_closed_documents": 0},
        "latency_ms": {"clean_ms": {"p95": 1.0}},
    }
    if contract == 3:
        run["metrics"]["gold_gap"] = {
            "status": "diagnostic",
            "gold_gap_protected_bytes": 0,
            "false_positive_bytes_after_gold_gap": 0,
            "adjusted_precision": 1.0,
            "gold_gap_protected_bytes_by_label": {},
        }
    card = {
        "schema_version": 4,
        "gaze": {"revision": R, "dirty": False},
        "dataset": {
            "repository": "fixture/aggregate",
            "revision": "fixture-v1",
            "file": "fixture.jsonl",
            "integrity": {"sha256": "3" * 64, "component_sha256": {"fixture": "3" * 64}},
            "evaluated_population": {"documents": 2, "entities": 2},
        },
        "parameters": {
            "profile": "full",
            "sampling_seed": 20260710,
            "ner_threshold": .3,
            "configs": ["policy-file"],
            "policy_sha256": digest(b"fixture policy\n"),
        },
        "runner_provenance": {
            "entry_point": "scripts/bench/run_no_opf_benchmark.py",
            "policy": {"path": "fixture-policy.toml", "sha256": digest(b"fixture policy\n")},
            "model_bundles": [{"model_id": "fixture-model", "expected_sha256": "4" * 64, "observed_sha256": "4" * 64}],
        },
        "runs": [run],
    }
    if contract != 1:
        card["scoring"] = {
            "scored_label_contract": {
                "id": f"scored-labels-v{contract}",
                "version": contract,
                "file_sha256": digest(contract_bytes),
                "excluded_labels": [],
            },
        }
    return card


class ApiFixture:
    def __init__(self):
        # Same field layout as observed main docs run and its check, sanitized IDs.
        repo = {"id": 1, "full_name": "CertaMesh/gaze"}
        self.repo = {**repo, "default_branch": "main"}
        self.branch = {"name": "main", "protected": True, "commit": {"sha": "9" * 40}}
        self.on_main = {"status": "ahead", "merge_base_commit": {"sha": D}, "base_commit": {"sha": D}}
        self.descendant = {"status": "ahead", "merge_base_commit": {"sha": R}, "base_commit": {"sha": R}}
        self.workflow = {"id": 100, "path": ".github/workflows/docs.yml", "state": "active"}
        self.run = {
            "id": 42,
            "path": ".github/workflows/docs.yml",
            "head_sha": D,
            "status": "completed",
            "conclusion": "success",
            "workflow_id": 100,
            "check_suite_id": 8,
            "repository": repo,
            "head_repository": repo,
            "head_branch": "main",
            "event": "push",
        }
        self.checks = [
            {
                "id": 7,
                "name": "docs",
                "head_sha": D,
                "status": "completed",
                "conclusion": "success",
                "app": {"id": 15368, "slug": "github-actions"},
                "check_suite": {"id": 8},
                "details_url": "https://github.com/CertaMesh/gaze/actions/runs/42/job/7",
            },
            {
                "name": "test",
                "head_sha": D,
                "status": "completed",
                "conclusion": "success",
                "app": {"id": 15368, "slug": "github-actions"},
            },
        ]
        self.contract_bytes = {i: raw_json({"schema_version": 1, "version": i}) for i in (2, 3)}
        self.layer_contract_path = PREFIX + "scored-labels-agentic.json"
        self.layer_contract_bytes = (ROOT / self.layer_contract_path).read_bytes()
        self.cards = {CARD_PATHS[i]: synthetic_card(i, self.contract_bytes.get(i)) for i in (1, 2, 3)}
        self.evidence = {
            "schema_version": 1,
            "format": "gaze-release-evidence-v1",
            "version": "0.16.0",
            "release_commit": R,
            "measured_commit": R,
            "harness_commit": H,
            "renderer_commit": P,
            "status": "PASS",
            "scorecards": {},
            "acceptance": {"authority": "signed-maintainer", "gates": {}},
        }
        self.receipts = {}
        for name in GATES:
            self.receipts[name] = raw_json({"schema_version": 1, "gate": name, "status": "PASS", "release_commit": R})
            self.evidence["acceptance"]["gates"][name] = {
                "status": "PASS",
                "receipt": {"path": PREFIX + f"evidence/v0.16.0/{name}.json", "sha256": digest(self.receipts[name])},
            }
        self.document = json.loads(inputs()["document_bytes"])
        self.document["schema_version"] = 1
        self.document["policy"] = {"path": PREFIX + "evidence/v0.16.0/policy.toml", "sha256": digest(b"fixture policy\n")}
        self.history = {"schema_version": 1, "releases": []}
        self.seal(refresh_history=True)

    def seal(self, refresh_history=False, refresh_identities=True):
        self.card_bytes = {p: c if isinstance(c, bytes) else raw_json(c) for p, c in self.cards.items()}
        self.document["scorecards"] = {p: digest(c) for p, c in self.card_bytes.items()}
        if refresh_identities:
            self.evidence["scorecards"] = {p: {
                "sha256": digest(self.card_bytes[p]),
                "identities": card_identity(c) if isinstance(c, dict) and "dataset" in c else {},
            } for p, c in self.cards.items()}
        if refresh_history:
            # Existing renderer projection; no scores or fixture PII are synthesized by the gate.
            import sys
            sys.path.insert(0, str(ROOT / "scripts/bench"))
            import render_benchmark_doc as render
            row = render.history_entry_from_scorecard(
                self.cards[CARD_PATHS[1]],
                version="v0.16.0",
                machine="fixture",
                scorecard_filename="scorecard-v0.16.0.json",
                scorecard_sha256=self.document["scorecards"][CARD_PATHS[1]],
                shipped_arm="policy-file",
            )
            row["contract_results"] = [render.contract_result_from_scorecard(
                self.cards[CARD_PATHS[i]],
                row,
                scorecard_filename=Path(CARD_PATHS[i]).name,
                scorecard_sha256=self.document["scorecards"][CARD_PATHS[i]],
            ) for i in (2,3)]
            historical = copy.deepcopy(row)
            historical["version"] = "v0.15.1"
            historical["commit"] = "8" * 40
            historical["scorecard"] = "scorecard-v0.15.1.json"
            historical.pop("contract_results")
            self.history["releases"] = [historical, row]
        self.history_bytes = raw_json(self.history)
        self.document["history"] = {"path": PREFIX + "release-history.json", "sha256": digest(self.history_bytes)}
        self.evidence["history"] = copy.deepcopy(self.document["history"])
        self.evidence_bytes = raw_json(self.evidence)
        self.document["evidence"]["sha256"] = digest(self.evidence_bytes)
        self.document_bytes = raw_json(self.document)
        message = inputs()["readiness_tag"]["verification"]["payload"].partition("\n\n")[2]
        message = message.replace(E, digest(self.evidence_bytes)).replace(
            digest(inputs()["document_bytes"]),
            digest(self.document_bytes),
        )
        self.tag = signed_readiness_tag(message)

    def get(self, path):
        if path == "":
            return self.repo
        if path == "/branches/main":
            return self.branch
        if path == f"/compare/{D}...{'9' * 40}":
            return self.on_main
        if path == f"/compare/{R}...{D}":
            return self.descendant
        if path == "/actions/workflows/docs.yml":
            return self.workflow
        if path.endswith("tags%2Fv0.16.0"):
            return {"object": inputs()["release_tag_object"]}
        if path.endswith("tags%2Frelease-readiness%2Fv0.16.0"):
            return {"object": inputs()["readiness_tag_object"]}
        if path == f"/git/tags/{TAG}":
            return inputs()["release_tag"]
        if path == f"/git/tags/{'f' * 40}":
            return self.tag
        if path == f"/git/commits/{D}":
            return {"sha": D, "verification": {"verified": True}}
        if path == f"/commits/{D}/check-runs?per_page=100":
            return {"check_runs": self.checks, "total_count": len(self.checks)}
        if path == "/actions/runs/42":
            return self.run
        if path.startswith("/contents/"):
            from urllib.parse import unquote
            file, ref = unquote(path.removeprefix("/contents/")).split("?ref=")
            files = {
                PREFIX + "release-readiness/v0.16.0.json": self.document_bytes,
                self.document["evidence"]["manifest_path"]: self.evidence_bytes,
                self.document["history"]["path"]: self.history_bytes,
                self.document["policy"]["path"]: b"fixture policy\n",
                **self.card_bytes,
                **{self.evidence["acceptance"]["gates"][n]["receipt"]["path"]: raw for n, raw in self.receipts.items() if n in self.evidence["acceptance"]["gates"]},
            }
            if file.startswith(PREFIX + "scored-labels-v"):
                assert ref == H
                raw = self.contract_bytes[int(file[-6])]
            elif file == self.layer_contract_path:
                assert ref == H
                raw = self.layer_contract_bytes
            else:
                assert ref == D
                raw = files[file]
            payload = base64.b64encode(raw).decode("ascii")
            return {
                "encoding": "base64",
                "content": "\n".join(payload[i:i + 60] for i in range(0, len(payload), 60)) + "\n",
            }
        raise AssertionError(f"unexpected API path: {path}")


def api_fixture():
    return ApiFixture()


def layered_fixture(explicit_binary=True):
    fixture = api_fixture()
    contract = json.loads(fixture.layer_contract_bytes)
    for card in fixture.cards.values():
        if explicit_binary:
            card["binary_commit"] = {"revision": R, "dirty": False}
            card["binary_sha256"] = "7" * 64
        # Actual producer manifest/report layout; generated gold is separate
        # from the primary corpus and uses its own H contract in every slot.
        card["layers"] = {
            "schema_version": 1,
            "generator": {
                "generator": contract["corpus"]["generator"],
                "generator_version": contract["corpus"]["generator_version"],
                "partition": contract["corpus"]["published_partition"],
                "seed": 20260710, "docs_per_family": 1, "documents": 6,
                "documents_by_layer": {"A": 2, "D": 2, "R": 2},
                "corpus_sha256": "5" * 64, "synthetic_only": True,
            },
            "scored_label_contract": {
                "id": contract["contract"], "version": contract["contract_version"],
                "file": fixture.layer_contract_path, "file_sha256": digest(fixture.layer_contract_bytes),
                "excluded_labels": [], "neutral_prediction_classes": [],
                "scored_gold_entities": 4, "scored_gold_utf8_bytes": 20,
                "excluded_gold_entities": 0, "excluded_gold_utf8_bytes": 0,
                "scored_gold_digest": "6" * 64,
            },
            "C": {"source": "runs", "description": "Primary corpus"},
        }
        for name in ("A", "D", "R"):
            run = copy.deepcopy(card["runs"][0])
            run["per_label_recall"] = {}
            card["layers"][name] = {"description": "Synthetic generated layer",
                                     "population": {"documents": 2, "entities": 2}, "runs": [run]}
    fixture.seal(refresh_history=True)
    return fixture


class ReleaseReadinessTests(unittest.TestCase):
    def test_accepts_complete_current_layers_with_distinct_frozen_generated_input(self):
        for explicit in (True, False):
            fixture = layered_fixture(explicit_binary=explicit)
            primary = fixture.cards[CARD_PATHS[1]]
            self.assertNotEqual(primary["dataset"]["integrity"]["sha256"], primary["layers"]["generator"]["corpus_sha256"])
            historical = fixture.history["releases"][0]["agentic_layers"]
            historical["binary_commit"] = {"revision": "8" * 40, "dirty": False}
            historical["harness_commit"] = {"revision": "9" * 40, "dirty": False}
            fixture.seal()
            self.assertEqual(gate.main(ARGV, fixture.get), 0)

    def test_rejects_resealed_explicit_layer_binary_contradicting_clean_R(self):
        for slot in (1, 2, 3):
            for identity in ({"revision": D, "dirty": False}, {"revision": R, "dirty": True},
                             {"revision": D, "dirty": True}, {"revision": R}, None):
                with self.subTest(slot=slot, identity=identity):
                    fixture = layered_fixture()
                    fixture.cards[CARD_PATHS[slot]]["binary_commit"] = identity
                    fixture.seal(refresh_history=True)
                    self.assertEqual(fixture.cards[CARD_PATHS[slot]]["gaze"], {"revision": R, "dirty": False})
                    with self.assertRaisesRegex(gate.ReadinessError, "layer binary must record clean measured R"):
                        gate.main(ARGV, fixture.get)

    def test_rejects_generated_identity_replacement_against_frozen_evidence(self):
        for slot in (1, 2, 3):
            for key, field, replacement in (("generator", "corpus_sha256", "0" * 64),
                                            ("generator", "seed", 123),
                                            ("scored_label_contract", "file_sha256", "0" * 64)):
                with self.subTest(slot=slot, field=field):
                    fixture = layered_fixture()
                    frozen = copy.deepcopy(fixture.evidence["scorecards"][CARD_PATHS[slot]]["identities"])
                    fixture.cards[CARD_PATHS[slot]]["layers"][key][field] = replacement
                    fixture.seal(refresh_history=True)
                    fixture.evidence["scorecards"][CARD_PATHS[slot]]["identities"] = frozen
                    fixture.seal(refresh_identities=False)
                    self.assertEqual(fixture.document["scorecards"][CARD_PATHS[slot]], fixture.evidence["scorecards"][CARD_PATHS[slot]]["sha256"])
                    with self.assertRaisesRegex(gate.ReadinessError, "frozen input/policy/model identities"):
                        gate.main(ARGV, fixture.get)

    def test_rejects_resealed_generated_contract_not_matching_own_H_file(self):
        for mutation in ("digest", "id", "version", "generator_version", "partition", "unsafe_file", "missing_file", "empty_D"):
            with self.subTest(mutation=mutation):
                fixture = layered_fixture()
                for card in fixture.cards.values():
                    layers = card["layers"]
                    contract = layers["scored_label_contract"]
                    if mutation == "digest":
                        contract["file_sha256"] = "0" * 64
                    elif mutation == "id":
                        contract["id"] = "other-contract"
                    elif mutation == "version":
                        contract["version"] += 1
                    elif mutation == "generator_version":
                        layers["generator"]["generator_version"] += 1
                    elif mutation == "partition":
                        layers["generator"]["partition"] = "train"
                    elif mutation == "unsafe_file":
                        contract["file"] = PREFIX + "../scored-labels-agentic.json"
                    elif mutation == "missing_file":
                        del contract["file"]
                    else:
                        layers["D"]["runs"] = []
                fixture.seal(refresh_history=True)
                error = ("frozen generated-layer contract bytes" if mutation == "digest" else
                         "path is unsafe" if mutation == "unsafe_file" else
                         "path is unsafe" if mutation == "missing_file" else
                         "layer D must have measured runs" if mutation == "empty_D" else
                         "identity does not match its frozen H contract")
                with self.assertRaisesRegex(gate.ReadinessError, error):
                    gate.main(ARGV, fixture.get)

    def test_rejects_release_tag_alias_and_inconsistent_signed_envelope(self):
        values = inputs()
        values["release_tag"] = signed_release_tag("v0.15.1")
        with self.assertRaisesRegex(gate.ReadinessError, "own name"):
            gate.validate(**values)
        fixture = api_fixture()
        def get(path):
            return signed_release_tag("v0.15.1") if path == f"/git/tags/{TAG}" else fixture.get(path)
        with self.assertRaisesRegex(gate.ReadinessError, "own name"):
            gate.main(ARGV, get)
        for field, value in (("tag", "v0.15.1"), ("payload", None), ("signature", None),
                             ("signature", "different signature"), ("message", "extra message")):
            with self.subTest(field=field):
                values = inputs()
                target = values["release_tag"] if field in ("tag", "message") else values["release_tag"]["verification"]
                target[field] = value
                with self.assertRaises(gate.ReadinessError):
                    gate.validate(**values)
        for old, new in (("tag v0.16.0\n", "tag v0.15.1\n"),
                         (f"object {R}\n", f"object {D}\n"), ("type commit\n", "type tree\n")):
            values = inputs()
            values["release_tag"]["verification"]["payload"] = values["release_tag"]["verification"]["payload"].replace(old, new)
            with self.assertRaisesRegex(gate.ReadinessError, "verified payload"):
                gate.validate(**values)

    def test_json_rejects_nested_overflow_and_retains_finite_numbers(self):
        for number in ("1e999", "-1e999", "NaN", "Infinity", "-Infinity"):
            with self.subTest(number=number):
                with self.assertRaisesRegex(gate.ReadinessError, "non-finite"):
                    gate.json_object(('{"outer":[{"value":' + number + '}]}').encode(), "fixture")
        self.assertEqual(gate.json_object(b'{"values":[1e308,-1e308,0.5,1e-999,17]}', "fixture"),
                         {"values": [1e308, -1e308, .5, 0.0, 17]})

    def test_rejects_fully_resealed_overflow_scorecards(self):
        original = raw_json
        try:
            globals()["raw_json"] = lambda value: original(value).replace(b"Infinity", b"1e999")
            fixture = api_fixture()
            for card in fixture.cards.values():
                card["runs"][0]["metrics"]["utf8_bytes"]["leak_rate"] = float("inf")
            fixture.seal(refresh_history=True)
            self.assertIn(b"1e999", fixture.card_bytes[CARD_PATHS[1]])
            with self.assertRaisesRegex(gate.ReadinessError, "non-finite"):
                gate.main(ARGV, fixture.get)
        finally:
            globals()["raw_json"] = original

    def test_rejects_actual_only_semantic_fields_in_both_history_projections(self):
        fixture = api_fixture()
        validator = {"custom:fixture": {
            "validator_kinds": ["synthetic"], "gold_spans": 2, "validator_failed_gold_spans": 1,
            "validator_backed_full_coverage_recall": 1.0, "shape_only_full_coverage_recall": 1.0,
            "leaked_utf8_bytes_validator_passed_gold": 0, "leaked_utf8_bytes_validator_failed_gold": 0,
        }}
        observation = {"format": "gzip-jsonl", "schema_version": 2, "file": "observations.jsonl.gz",
                       "sha256": "6" * 64, "bytes": 100, "observations": 2,
                       "corpus_sha256": fixture.history["releases"][-1]["dataset"]["integrity"]["sha256"]}
        measurement = {"method": "past release, today's harness", "harness_revision": H,
                       "binary_sha256": "7" * 64, "manifest_replacing_actions": ["tokenize"]}
        additions = {"validator_recall": validator, "agentic_layers": {"arms": {}},
                     "observation_record": observation, "measurement": measurement}
        for slot in (1, 2, 3):
            for key, value in additions.items():
                with self.subTest(slot=slot, key=key):
                    fixture = api_fixture()
                    row = fixture.history["releases"][-1]
                    actual = row if slot == 1 else row["contract_results"][slot - 2]
                    actual[key] = copy.deepcopy(value)
                    fixture.seal(refresh_history=False)
                    # The normal renderer accepts these shapes; rejection must
                    # be the measured semantic join, not a stale digest/schema.
                    gate.render.validate_history(fixture.history)
                    with self.assertRaisesRegex(gate.ReadinessError, f"current history {key}"):
                        gate.main(ARGV, fixture.get)
        fixture = api_fixture()
        fixture.history["releases"][-1]["scored_label_contract"] = None
        fixture.seal()
        with self.assertRaisesRegex(gate.ReadinessError, "current history scored_label_contract"):
            gate.main(ARGV, fixture.get)

    def test_accepts_history_annotations_and_historical_measurements(self):
        fixture = api_fixture()
        row, historical = fixture.history["releases"][-1], fixture.history["releases"][0]
        row["note"], row["date"] = "Maintainer annotation", "2026-10-04"
        for result in row["contract_results"]:
            result["note"], result["date"] = "Contract annotation", "2026-10-04"
        historical["measurement"] = {"harness_revision": "8" * 40, "binary_sha256": "8" * 64}
        historical["note"] = "Historical source retained"
        fixture.seal()
        self.assertEqual(gate.main(ARGV, fixture.get), 0)

    def test_accepts_measured_optional_history_fields_and_rejects_their_drift(self):
        fixture = layered_fixture()
        for card in fixture.cards.values():
            card["observation_record"] = {
                "format": "gzip-jsonl", "schema_version": 2, "file": "observations.jsonl.gz",
                "sha256": "6" * 64, "bytes": 100, "observations": 2,
                "corpus_sha256": card["dataset"]["integrity"]["sha256"],
            }
            card["runner_provenance"].update({
                "entry_point": gate.render.PAST_RELEASE_ENTRY_POINT,
                "harness_revision": H, "binary_sha256": "7" * 64,
                "manifest_replacing_actions": ["tokenize"],
            })
            card["runs"][0]["validator_recall_by_label"] = {"custom:fixture": {
                "applicability": "applicable", "validator_kinds": ["synthetic"],
                "gold_spans": 2, "validator_failed_gold_spans": 1,
                "validator_backed_recall": {"full_coverage_recall": 1.0},
                "shape_only_recall": {"full_coverage_recall": 1.0},
                "production_recall_by_gold_validity": {
                    "validator_passed_gold": {"leaked_utf8_bytes": 0},
                    "validator_failed_gold": {"leaked_utf8_bytes": 0},
                },
            }}
        fixture.seal(refresh_history=True)
        self.assertEqual(gate.main(ARGV, fixture.get), 0)
        self.assertIn("`custom:fixture`", gate.render.render_current_release(fixture.history))
        original = copy.deepcopy(fixture.history)
        for slot, key in ((1, "validator_recall"), (1, "observation_record"),
                          (2, "measurement"), (2, "observation_record"), (3, "agentic_layers")):
            for mutation in ("delete", "change"):
                with self.subTest(slot=slot, key=key, mutation=mutation):
                    fixture.history = copy.deepcopy(original)
                    row = fixture.history["releases"][-1]
                    target = row if slot == 1 else row["contract_results"][slot - 2]
                    if mutation == "delete":
                        del target[key]
                    elif key == "validator_recall":
                        target[key]["custom:fixture"]["gold_spans"] += 1
                    elif key == "observation_record":
                        target[key]["bytes"] += 1
                    elif key == "measurement":
                        target[key]["binary_sha256"] = "8" * 64
                    else:
                        target[key]["binary_commit"] = {"revision": D, "dirty": True}
                    fixture.seal()
                    with self.assertRaisesRegex(gate.ReadinessError, f"current history {key}"):
                        gate.main(ARGV, fixture.get)

    def test_content_bytes_accepts_unwrapped_base64(self):
        raw = b"manifest\n"
        response = {"encoding": "base64", "content": base64.b64encode(raw).decode("ascii")}
        self.assertEqual(gate.content_bytes(response, "manifest"), raw)

    def test_content_bytes_accepts_wrapped_base64_without_changing_bytes(self):
        raw = bytes(range(256))
        encoded = base64.b64encode(raw).decode("ascii")
        for newline in ("\n", "\r\n"):
            with self.subTest(newline=newline):
                wrapped = newline.join(encoded[i:i + 60] for i in range(0, len(encoded), 60)) + newline
                response = {"encoding": "base64", "content": wrapped}
                self.assertEqual(gate.content_bytes(response, "manifest"), raw)

    def test_content_bytes_rejects_malformed_base64_even_when_wrapped(self):
        for encoded in (
            "bWFu\naWZl%c3Q=\n",  # Invalid alphabet character.
            "bWFu\naWZlc3Q\n",  # Missing padding.
            "bWFu\naWZlc3Q===\n",  # Excess padding.
            "bWFu\naWZlc3Q=AAAA\n",  # Data after padding.
            "bWFu\naWZlc3Q=\u00e9\n",  # Non-ASCII character.
            "bWFu\naWZl c3Q=\n",  # Space is not a line separator.
            "bWFu\naWZl\tc3Q=\n",  # Tab is not a line separator.
        ):
            with self.subTest(encoded=encoded):
                with self.assertRaisesRegex(gate.ReadinessError, "manifest has invalid base64 content"):
                    gate.content_bytes({"encoding": "base64", "content": encoded}, "manifest")

    def test_content_bytes_rejects_invalid_content_response(self):
        for response in (
            {"encoding": "none", "content": "bWFuaWZlc3Q="},
            {"encoding": "base64", "content": None},
            {"encoding": "base64", "content": b"bWFuaWZlc3Q="},
            {"encoding": "base64"},
        ):
            with self.subTest(response=response):
                with self.assertRaisesRegex(gate.ReadinessError, "not a base64 GitHub content response"):
                    gate.content_bytes(response, "manifest")

    def test_accepts_complete_exact_immutable_readiness(self):
        result = gate.validate(**inputs())
        self.assertEqual(result.release_commit, R)
        self.assertEqual(result.documentation_commit, D)

    def test_rejects_malformed_data_in_signed_receipt(self):
        receipt = inputs()["readiness_tag"]["verification"]["payload"].partition("\n\n")[2]
        for malformed in (
            "unexpected prefix\n" + receipt,
            receipt + "malformed line\n",
            receipt + "version=0.16.0\n",
        ):
            with self.subTest(receipt=malformed):
                values = inputs()
                values["readiness_tag"] = signed_readiness_tag(malformed)
                with self.assertRaises(gate.ReadinessError):
                    gate.validate(**values)

    def test_rejects_message_data_outside_verified_receipt_payload(self):
        values = inputs()
        tag = values["readiness_tag"]
        tag["message"] = tag["message"].replace(SIGNATURE, "malformed line\n" + SIGNATURE)
        with self.assertRaisesRegex(gate.ReadinessError, "does not match the verified payload"):
            gate.validate(**values)

    def test_rejects_missing_or_malformed_signed_receipt_envelope(self):
        payload = inputs()["readiness_tag"]["verification"]["payload"]
        for field, invalid_value in (
            ("payload", None),
            ("payload", "gaze-release-readiness-v1\nversion=0.16.0\n"),
            ("payload", payload.replace(f"object {D}\n", f"object {R}\n")),
            ("payload", payload.replace("type commit\n", "type tree\n")),
            ("payload", payload.replace("tag release-readiness/v0.16.0\n", "tag other/v0.16.0\n")),
            ("signature", None),
            ("signature", ""),
            ("signature", "different signature"),
        ):
            with self.subTest(field=field, value=invalid_value):
                values = inputs()
                values["readiness_tag"]["verification"][field] = invalid_value
                with self.assertRaisesRegex(gate.ReadinessError, "verified payload"):
                    gate.validate(**values)

    def test_rejects_wrong_version_or_missing_field_in_signed_receipt(self):
        receipt = inputs()["readiness_tag"]["verification"]["payload"].partition("\n\n")[2]
        for malformed, error in (
            (receipt.replace("version=0.16.0", "version=0.15.1"), "version does not match"),
            (receipt.replace(f"harness_commit={H}\n", ""), "missing harness_commit"),
        ):
            with self.subTest(receipt=malformed):
                values = inputs()
                values["readiness_tag"] = signed_readiness_tag(malformed)
                with self.assertRaisesRegex(gate.ReadinessError, error):
                    gate.validate(**values)

    def test_rejects_missing_readiness_signature(self):
        values = inputs()
        values["readiness_tag"]["verification"]["verified"] = False
        with self.assertRaisesRegex(gate.ReadinessError, "signature"):
            gate.validate(**values)

    def test_rejects_mismatched_release_target(self):
        values = inputs()
        values["release_tag"]["object"]["sha"] = D
        with self.assertRaisesRegex(gate.ReadinessError, "tag target"):
            gate.validate(**values)

    def test_rejects_stale_document_manifest(self):
        values = inputs()
        values["document_bytes"] += b" "
        with self.assertRaisesRegex(gate.ReadinessError, "manifest bytes"):
            gate.validate(**values)

    def test_rejects_missing_or_failed_required_checks(self):
        values = inputs()
        values["checks"] = values["checks"][:1]
        with self.assertRaisesRegex(gate.ReadinessError, "lacks successful"):
            gate.validate(**values)

    def test_rejects_later_document_harness_replacement(self):
        values = inputs()
        document = json.loads(values["document_bytes"])
        document["harness_commit"] = D
        raw = json.dumps(document, sort_keys=True).encode()
        values["document_bytes"] = raw
        receipt = values["readiness_tag"]["verification"]["payload"].partition("\n\n")[2]
        values["readiness_tag"] = signed_readiness_tag(
            receipt.replace(
                hashlib.sha256(inputs()["document_bytes"]).hexdigest(),
                hashlib.sha256(raw).hexdigest(),
            )
        )
        with self.assertRaisesRegex(gate.ReadinessError, "harness_commit"):
            gate.validate(**values)

    def test_both_publication_workflows_require_the_same_gate(self):
        root = Path(__file__).resolve().parents[2]
        release = (root / ".github/workflows/release.yml").read_text()
        crates = (root / ".github/workflows/publish-crates.yml").read_text()
        invocation = "python3 scripts/gate/release_readiness.py"
        self.assertIn(invocation, release)
        self.assertIn(invocation, crates)
        self.assertLess(release.index(invocation), release.index("softprops/action-gh-release"))
        self.assertLess(crates.index(invocation), crates.index("Authenticate to crates.io via OIDC"))
        self.assertIn("Reject manual crate publication", crates)
        self.assertIn("inputs.dry_run != true", crates)

    def test_rejects_ambiguous_json_and_unknown_contract_fields(self):
        for raw in (b'{"status":"PASS","status":"FAIL"}', b'{"value":NaN}', b'[]'):
            with self.subTest(raw=raw):
                with self.assertRaises(gate.ReadinessError):
                    gate.json_object(raw, "evidence")
        for key, value in (("schema_version", True), ("restore", "FAIL")):
            fixture = api_fixture()
            fixture.evidence[key] = value
            fixture.seal()
            with self.assertRaises(gate.ReadinessError):
                gate.main(ARGV, fixture.get)

    def test_rejects_incomplete_frozen_input_identity(self):
        for field in ("repository", "revision", "file"):
            with self.subTest(field=field):
                fixture = api_fixture()
                for card in fixture.cards.values():
                    del card["dataset"][field]
                fixture.seal(refresh_history=True)
                with self.assertRaises(gate.ReadinessError):
                    gate.main(ARGV, fixture.get)
        fixture = api_fixture()
        for card in fixture.cards.values():
            card["dataset"]["evaluated_population"]["documents"] = 0
        fixture.seal(refresh_history=True)
        with self.assertRaises(gate.ReadinessError):
            gate.main(ARGV, fixture.get)

    def test_rejects_model_policy_and_input_changes_even_with_resealed_identities(self):
        for mutation in ("model", "policy", "dataset", "profile"):
            fixture = api_fixture()
            card = fixture.cards[CARD_PATHS[2]]
            if mutation == "model":
                card["runner_provenance"]["model_bundles"][0]["observed_sha256"] = "0" * 64
            elif mutation == "policy":
                card["runner_provenance"]["policy"]["sha256"] = "0" * 64
            elif mutation == "profile":
                card["parameters"]["profile"] = "smoke"
            else:
                card["dataset"]["revision"] = "replacement"
            fixture.seal()
            with self.assertRaises(gate.ReadinessError):
                gate.main(ARGV, fixture.get)

    def test_main_accepts_wrapped_content_and_exact_docs_workflow_path(self):
        fixture = api_fixture()
        self.assertEqual(gate.main(ARGV, fixture.get), 0)

    def test_rejects_hash_consistent_invalid_scorecards(self):
        for mutation in (
            "non_json",
            "old",
            "dirty",
            "missing_dirty",
            "wrong_schema",
            "no_runs",
            "wrong_revision",
            "wrong_contract",
        ):
            with self.subTest(mutation=mutation):
                fixture = api_fixture()
                path = CARD_PATHS[1]
                card = fixture.cards[path]
                if mutation == "non_json":
                    card = b"scorecard"
                elif mutation == "old":
                    card = json.loads((ROOT / "docs/reference/benchmarks/scorecard-v0.15.1.json").read_text())
                elif mutation == "dirty":
                    card["gaze"]["dirty"] = True
                elif mutation == "missing_dirty":
                    del card["gaze"]["dirty"]
                elif mutation == "wrong_schema":
                    card["schema_version"] = 3
                elif mutation == "no_runs":
                    card["runs"] = []
                elif mutation == "wrong_revision":
                    card["gaze"]["revision"] = D
                else:
                    card["scoring"] = {
                        "scored_label_contract": {
                            "version": 2,
                            "id": "scored-labels-v2",
                            "file_sha256": "2" * 64,
                            "excluded_labels": [],
                        },
                    }
                fixture.cards[path] = card
                fixture.seal()
                with self.assertRaises(gate.ReadinessError):
                    gate.main(ARGV, fixture.get)

    def test_rejects_failed_unknown_or_incomplete_evidence(self):
        for field, value in (("schema_version", 99), ("measured_commit", D), ("status", "BLOCKED")):
            with self.subTest(field=field):
                fixture = api_fixture()
                fixture.evidence[field] = value
                fixture.seal()
                with self.assertRaises(gate.ReadinessError):
                    gate.main(ARGV, fixture.get)
        for status in (None, "FAIL", "HOLD", "NOT_RUN", "UNKNOWN", "accepted"):
            with self.subTest(status=status):
                fixture = api_fixture()
                fixture.evidence["acceptance"]["gates"]["native"]["status"] = status
                fixture.seal()
                with self.assertRaises(gate.ReadinessError):
                    gate.main(ARGV, fixture.get)
        for missing in (
            "native",
            "benchmark_gain_v1",
            "benchmark_gain_v2",
            "timing_authority",
            "tag_namespace",
            "publisher_refs",
        ):
            with self.subTest(missing=missing):
                fixture = api_fixture()
                del fixture.evidence["acceptance"]["gates"][missing]
                fixture.seal()
                with self.assertRaises(gate.ReadinessError):
                    gate.main(ARGV, fixture.get)

    def test_rejects_incomplete_inventory_or_wrong_current_history(self):
        for mutation in (
            "missing_card",
            "missing_current",
            "wrong_commit",
            "wrong_digest",
            "missing_contract",
            "provisional",
            "wrong_metrics",
        ):
            with self.subTest(mutation=mutation):
                fixture = api_fixture()
                row = fixture.history["releases"][-1]
                if mutation == "missing_card":
                    del fixture.cards[CARD_PATHS[2]]
                elif mutation == "missing_current":
                    fixture.history["releases"] = fixture.history["releases"][:-1]
                elif mutation == "wrong_commit":
                    row["commit"] = D
                elif mutation == "wrong_digest":
                    row["scorecard_sha256"] = "0" * 64
                elif mutation == "missing_contract":
                    row["contract_results"] = row["contract_results"][:1]
                elif mutation == "provisional":
                    row["provisional"] = True
                else:
                    row["arms"]["policy-file"]["surviving_pii_utf8_bytes"] += 1
                fixture.seal(refresh_history=False)
                with self.assertRaises(gate.ReadinessError):
                    gate.main(ARGV, fixture.get)

    def test_rejects_replaced_frozen_inputs_and_receipts(self):
        for mutation in (
            "corpus",
            "policy",
            "models",
            "harness",
            "contract",
            "receipt_status",
            "receipt_commit",
            "receipt_digest",
        ):
            with self.subTest(mutation=mutation):
                fixture = api_fixture()
                if mutation in ("corpus", "policy", "models"):
                    identity = fixture.evidence["scorecards"][CARD_PATHS[1]]["identities"]
                    if mutation == "corpus":
                        identity["dataset"]["integrity"]["sha256"] = "0" * 64
                    elif mutation == "policy":
                        identity["policy_sha256"] = "0" * 64
                    else:
                        identity["model_bundles"][0]["observed_sha256"] = "0" * 64
                elif mutation == "harness":
                    fixture.evidence["harness_commit"] = D
                elif mutation == "contract":
                    fixture.contract_bytes[2] += b" "
                elif mutation == "receipt_digest":
                    fixture.receipts["native"] += b" "
                else:
                    receipt = json.loads(fixture.receipts["native"])
                    receipt["status" if mutation == "receipt_status" else "release_commit"] = "NOT_RUN" if mutation == "receipt_status" else D
                    fixture.receipts["native"] = raw_json(receipt)
                    fixture.evidence["acceptance"]["gates"]["native"]["receipt"]["sha256"] = digest(fixture.receipts["native"])
                fixture.seal(refresh_identities=False)
                with self.assertRaises(gate.ReadinessError):
                    gate.main(ARGV, fixture.get)

    def test_selects_same_trusted_check_even_with_untrusted_first_match(self):
        fixture = api_fixture()
        decoy = copy.deepcopy(fixture.checks[0])
        decoy["app"] = {"id": 99, "slug": "other-app"}
        decoy["details_url"] = "https://evil.example.invalid/actions/runs/99"
        fixture.checks.insert(0, decoy)
        self.assertEqual(gate.main(ARGV, fixture.get), 0)

    def test_accepts_main_docs_run_when_successful_pr_check_is_first(self):
        fixture = api_fixture()
        pr_check = copy.deepcopy(fixture.checks[0])
        pr_check["id"] = 9
        pr_check["check_suite"] = {"id": 10}
        pr_check["details_url"] = "https://github.com/CertaMesh/gaze/actions/runs/43/job/9"
        fixture.checks.insert(0, pr_check)
        pr_run = {**fixture.run, "id": 43, "check_suite_id": 10, "event": "pull_request"}
        def get(path):
            return pr_run if path == "/actions/runs/43" else fixture.get(path)
        self.assertEqual(gate.main(ARGV, get), 0)

    def test_rejects_wrong_or_unsigned_documentation_commit(self):
        for wrong in ({"sha": R, "verification": {"verified": True}}, {"sha": D, "verification": {"verified": False}}):
            fixture = api_fixture()
            def get(path):
                return wrong if path == f"/git/commits/{D}" else fixture.get(path)
            with self.assertRaises(gate.ReadinessError):
                gate.main(ARGV, get)

    def test_rejects_unmerged_or_unprotected_documentation(self):
        for mutation in ("unmerged", "not_descendant", "unprotected", "pull_request"):
            with self.subTest(mutation=mutation):
                fixture = api_fixture()
                if mutation == "unmerged":
                    fixture.on_main["status"] = "diverged"
                elif mutation == "not_descendant":
                    fixture.descendant["status"] = "behind"
                elif mutation == "unprotected":
                    fixture.branch["protected"] = False
                else:
                    fixture.run["event"] = "pull_request"
                with self.assertRaises(gate.ReadinessError):
                    gate.main(ARGV, fixture.get)

    def test_rejects_wrong_check_run_workflow_and_repository_joins(self):
        mutations = [
            ("check", "head_sha", R), ("check", "status", "in_progress"),
            ("check", "conclusion", "failure"), ("check", "app", {"id": 99, "slug": "github-actions"}),
            ("check", "check_suite", {"id": 9}),
            ("check", "details_url", "https://github.com/Other/gaze/actions/runs/42/job/7"),
            ("check", "details_url", "https://evil.example.invalid/CertaMesh/gaze/actions/runs/42/job/7"),
            ("check", "details_url", "https://github.com/CertaMesh/gaze/actions/runs/42/job/999"),
            ("run", "id", 43), ("run", "head_sha", R), ("run", "status", "queued"),
            ("run", "conclusion", "failure"), ("run", "workflow_id", 99),
            ("run", "check_suite_id", 99), ("run", "head_branch", "feature"),
            ("run", "repository", {"id": 2, "full_name": "Other/gaze"}),
            ("run", "head_repository", {"id": 2, "full_name": "Other/gaze"}),
            ("workflow", "path", ".github/workflows/other.yml"),
        ]
        mutations += [("run", "path", p) for p in (
            ".github/workflows/docs.yml.bak",
            f".github/workflows/docs.yml@{D}",
            ".github/workflows/docs-extra.yml",
            ".github/workflows/other/docs.yml",
        )]
        for target, field, value in mutations:
            with self.subTest(target=target, field=field, value=value):
                fixture = api_fixture()
                obj = fixture.checks[0] if target == "check" else getattr(fixture, target)
                obj[field] = value
                with self.assertRaises(gate.ReadinessError):
                    gate.main(ARGV, fixture.get)


if __name__ == "__main__":
    unittest.main()
