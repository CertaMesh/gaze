import base64
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
        "release_tag": {"object": {"type": "commit", "sha": R}, "verification": {"verified": True}},
        "readiness_tag_object": {"type": "tag", "sha": "f" * 40},
        "readiness_tag": {"object": {"type": "commit", "sha": D}, "verification": {"verified": True}, "message": receipt},
        "document_bytes": raw,
        "checks": [
            {"name": "docs", "conclusion": "success", "app": {"slug": "github-actions"}},
            {"name": "test", "conclusion": "success", "app": {"slug": "github-actions"}},
        ],
    }


class ReleaseReadinessTests(unittest.TestCase):
    def test_accepts_complete_exact_immutable_readiness(self):
        result = gate.validate(**inputs())
        self.assertEqual(result.release_commit, R)
        self.assertEqual(result.documentation_commit, D)

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
        values["readiness_tag"]["message"] = values["readiness_tag"]["message"].replace(
            hashlib.sha256(inputs()["document_bytes"]).hexdigest(), hashlib.sha256(raw).hexdigest()
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

    def test_main_accepts_exact_document_evidence_scorecards_and_docs_workflow(self):
        values = inputs()
        evidence_bytes = json.dumps(
            {"release_commit": R, "harness_commit": H, "renderer_commit": P}, sort_keys=True
        ).encode()
        document = json.loads(values["document_bytes"])
        document["evidence"]["sha256"] = hashlib.sha256(evidence_bytes).hexdigest()
        scorecard_bytes = b"scorecard"
        scorecard_path = next(iter(document["scorecards"]))
        document["scorecards"][scorecard_path] = hashlib.sha256(scorecard_bytes).hexdigest()
        document_bytes = json.dumps(document, sort_keys=True).encode()
        message = values["readiness_tag"]["message"]
        message = message.replace(E, document["evidence"]["sha256"])
        message = message.replace(
            hashlib.sha256(values["document_bytes"]).hexdigest(), hashlib.sha256(document_bytes).hexdigest()
        )
        values["readiness_tag"]["message"] = message
        encoded = lambda body: {"encoding": "base64", "content": base64.b64encode(body).decode()}

        def get(path):
            if path.endswith("tags%2Fv0.16.0"):
                return {"object": values["release_tag_object"]}
            if path.endswith("tags%2Frelease-readiness%2Fv0.16.0"):
                return {"object": values["readiness_tag_object"]}
            if path.endswith(TAG):
                return values["release_tag"]
            if path.endswith("f" * 40):
                return values["readiness_tag"]
            if path.startswith("/contents/docs/reference/benchmarks/release-readiness/"):
                return encoded(document_bytes)
            if path.startswith("/contents/docs/reference/benchmarks/evidence/"):
                return encoded(evidence_bytes)
            if path.startswith("/contents/docs/reference/benchmarks/scorecard-"):
                return encoded(scorecard_bytes)
            if path == f"/git/commits/{D}":
                return {"verification": {"verified": True}}
            if path.startswith("/commits/"):
                return {
                    "check_runs": [
                        {
                            "name": "docs",
                            "conclusion": "success",
                            "app": {"slug": "github-actions"},
                            "details_url": "https://github.com/CertaMesh/gaze/actions/runs/42/job/7",
                        },
                        {
                            "name": "test",
                            "conclusion": "success",
                            "app": {"slug": "github-actions"},
                        },
                    ]
                }
            if path == "/actions/runs/42":
                return {"path": f".github/workflows/docs.yml@{D}", "head_sha": D, "conclusion": "success"}
            self.fail(f"unexpected API path: {path}")

        self.assertEqual(
            gate.main(["--repository", "CertaMesh/gaze", "--release-tag", "v0.16.0", "--release-commit", R], get),
            0,
        )


if __name__ == "__main__":
    unittest.main()
