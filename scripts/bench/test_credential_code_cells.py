"""Credential generator and mechanism preparation checks; no models or scoring run."""
import hashlib
from pathlib import Path
import tempfile
import tomllib
import unittest

import agentic_layers as agentic
import credential_code_arm as arm
import credential_code_cells as cells


class CredentialCellsTests(unittest.TestCase):
    def test_new_cells_cover_formats_code_negatives_and_repeats(self):
        for partition in agentic.PARTITIONS:
            rows = cells.records(agentic, partition)
            gold = [span for row in rows for span in row.gold]
            self.assertEqual({span.label for span in gold}, {"PASSWORD", "SECURITYTOKEN"})
            self.assertTrue(all(len(row.gold) == 2 for row in rows if row.layer == "R"))
            self.assertTrue(all(not row.gold for row in rows if row.layer == "D"))
            families = {row.family for row in rows if row.layer == "A"}
            self.assertTrue({"bcrypt", "argon2id", "scrypt_phc", "yescrypt", "sha512crypt",
                             "md5crypt", "phpass_h", "django_scrypt", "password", "api_token"} <= families)
            for row in rows:
                for span in (*row.gold, *row.decoys):
                    self.assertEqual(row.text.encode()[span.start:span.end].decode(), span.value)

    def test_v20_is_reconstructed_without_any_changed_document(self):
        previous = {"dev": "265f4941b904e5def1e315c85089aba8fbeae05837aaa319cb2de11866ad3cd4",
                    "test": "ca29328268932f4ffa4613a4470e2b0c47871ccbcb1377f2ee82ccfb751739c3"}
        for partition in agentic.PARTITIONS:
            old = agentic.records_as_of(20, agentic.generate(partition))
            self.assertEqual(hashlib.sha256(agentic.corpus_bytes(old)).hexdigest(), previous[partition])

    def test_arm_changes_only_the_new_rulepack_slot(self):
        source = '[policy.rulepacks]\nbundled=["core","secrets"]\npaths=[]\n\n[[rule]]\nkind="default"\naction="tokenize"\n'
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            base, candidate = arm.prepare(source, root)
            paths = candidate["policy"]["rulepacks"].pop("paths")
            self.assertEqual(paths[:-1], base["policy"]["rulepacks"]["paths"])
            candidate["policy"]["rulepacks"]["paths"] = paths[:-1]
            self.assertEqual(base, candidate)
            old = tomllib.loads((root / "base-secrets.toml").read_text())
            new = tomllib.loads((root / "password-code-credentials.toml").read_text())
            self.assertEqual({r["id"] for r in old["recognizers"]}, {"password.field", "security_token.anchored"})
            self.assertEqual({r["id"] for r in new["recognizers"]}, arm.NEW_IDS)
            delta = tomllib.loads((root / "delta.toml").read_text())
            self.assertEqual(delta["mechanism_delta"]["append_rulepack_paths"], paths[-1:])
