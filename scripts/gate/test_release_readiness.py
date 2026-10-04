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
        "readiness_tag": signed_readiness_tag(receipt),
        "document_bytes": raw,
        "checks": [
            {"name": "docs", "conclusion": "success", "app": {"slug": "github-actions"}},
            {"name": "test", "conclusion": "success", "app": {"slug": "github-actions"}},
        ],
    }


class ReleaseReadinessTests(unittest.TestCase):
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
            receipt.replace(hashlib.sha256(inputs()["document_bytes"]).hexdigest(), hashlib.sha256(raw).hexdigest())
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

    def test_main_accepts_wrapped_content_and_exact_docs_workflow_path(self):
        values = inputs()
        evidence_bytes = json.dumps(
            {"release_commit": R, "harness_commit": H, "renderer_commit": P}, sort_keys=True
        ).encode()
        document = json.loads(values["document_bytes"])
        document["evidence"]["sha256"] = hashlib.sha256(evidence_bytes).hexdigest()
        scorecard_bytes = json.dumps({"gaze": {"revision": R}, "leaked_bytes": 0}, sort_keys=True).encode() + b"\n"
        scorecard_path = next(iter(document["scorecards"]))
        document["scorecards"][scorecard_path] = hashlib.sha256(scorecard_bytes).hexdigest()
        document_bytes = json.dumps(document, sort_keys=True).encode()
        message = values["readiness_tag"]["verification"]["payload"].partition("\n\n")[2]
        message = message.replace(E, document["evidence"]["sha256"])
        message = message.replace(
            hashlib.sha256(values["document_bytes"]).hexdigest(), hashlib.sha256(document_bytes).hexdigest()
        )
        values["readiness_tag"] = signed_readiness_tag(message)

        def encoded(body):
            # GitHub Contents API wraps base64 at 60 columns and adds a final LF.
            payload = base64.b64encode(body).decode("ascii")
            self.assertGreater(len(payload), 60)
            return {
                "encoding": "base64",
                "content": "\n".join(payload[i:i + 60] for i in range(0, len(payload), 60)) + "\n",
            }

        docs_run = {"path": ".github/workflows/docs.yml", "head_sha": D, "conclusion": "success"}
        docs_check_app = "github-actions"

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
                            "app": {"slug": docs_check_app},
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
                return docs_run
            self.fail(f"unexpected API path: {path}")

        self.assertEqual(
            gate.main(["--repository", "CertaMesh/gaze", "--release-tag", "v0.16.0", "--release-commit", R], get),
            0,
        )
        for docs_workflow_path in (
            ".github/workflows/docs.yml.bak",
            f".github/workflows/docs.yml@{D}",
            ".github/workflows/docs-extra.yml",
            ".github/workflows/other/docs.yml",
        ):
            with self.subTest(docs_workflow_path=docs_workflow_path):
                docs_run["path"] = docs_workflow_path
                with self.assertRaisesRegex(gate.ReadinessError, "not a successful docs.yml run"):
                    gate.main(
                        ["--repository", "CertaMesh/gaze", "--release-tag", "v0.16.0", "--release-commit", R], get
                    )
        docs_run["path"] = ".github/workflows/docs.yml"
        for field, invalid_value in (("head_sha", R), ("conclusion", "failure")):
            with self.subTest(field=field):
                original_value = docs_run[field]
                docs_run[field] = invalid_value
                with self.assertRaisesRegex(gate.ReadinessError, "not a successful docs.yml run"):
                    gate.main(
                        ["--repository", "CertaMesh/gaze", "--release-tag", "v0.16.0", "--release-commit", R], get
                    )
                docs_run[field] = original_value
        docs_check_app = "other-app"
        with self.assertRaisesRegex(gate.ReadinessError, "lacks successful required checks: docs"):
            gate.main(["--repository", "CertaMesh/gaze", "--release-tag", "v0.16.0", "--release-commit", R], get)


if __name__ == "__main__":
    unittest.main()
