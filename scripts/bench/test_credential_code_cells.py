"""Credential generator and mechanism preparation checks; no models or scoring run."""
import hashlib
from pathlib import Path
import tempfile
import tomllib
import unittest
from unittest import mock

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

    def test_v21_is_reconstructed_without_any_changed_document(self):
        previous = {"dev": "e5dc19cc4d5fd9ea99864172a193a4cef076631db2964842511fc5affcc70265",
                    "test": "5f8d113cb46ad5591178d1814d09588e46b49b7e89fb78a7b00c608e6e836587"}
        for partition in agentic.PARTITIONS:
            old = agentic.records_as_of(21, agentic.generate(partition))
            self.assertEqual(hashlib.sha256(agentic.corpus_bytes(old)).hexdigest(), previous[partition])

    def test_review_contexts_measure_both_credential_and_metadata_boundaries(self):
        for partition in agentic.PARTITIONS:
            rows = cells.review_records(agentic, partition)
            benign = {row.family for row in rows if row.layer == "D"}
            self.assertTrue({"logprob", "tokenizer", "token_schema", "password_schema",
                             "validation", "language", "masked"} <= benign)
            positive = {row.family for row in rows if row.layer == "A"}
            self.assertTrue({"spaced_passphrase", "pipe_password", "weak_string", "weak_stars",
                             "generic_token", "sha256crypt", "sha512crypt", "md5crypt",
                             "django_pbkdf2_sha256", "django_pbkdf2_sha1", "django_scrypt"} <= positive)
            surfaces = {row.surface for row in rows if row.layer == "A"}
            self.assertTrue({"credentials_review_array_compact", "credentials_review_array_first",
                             "credentials_review_array_final", "credentials_review_array_nested"} <= surfaces)
            for row in rows:
                self.assertEqual(len(row.gold), 2 if row.layer == "R" else 0 if row.layer == "D" else 1)
                for span in (*row.gold, *row.decoys):
                    self.assertEqual(row.text.encode()[span.start:span.end].decode(), span.value)
            for family in ("password_schema", "validation", "language", "masked"):
                selected = [row for row in rows if row.layer == "D" and row.family == family]
                self.assertTrue(all("password" in row.text and not row.gold for row in selected))

    def test_review_partitions_share_no_texts_values_or_rendered_templates(self):
        parts, templates, values = {}, {}, {}
        for partition in agentic.PARTITIONS:
            with mock.patch.object(agentic, "_fill_with_decoys", wraps=agentic._fill_with_decoys) as render:
                parts[partition] = cells.review_records(agentic, partition)
            # Inspect actual renderer inputs, including non-gold D values. The
            # partition embedded in a template ID cannot prove a held-out split.
            templates[partition] = {call.args[0] for call in render.call_args_list}
            values[partition] = {field[0] for call in render.call_args_list for field in call.args[1].values()}

        def skeleton(row):
            text = row.text
            for value in sorted({span.value for span in (*row.gold, *row.decoys)}, key=len, reverse=True):
                text = text.replace(value, "{V}")
            return text

        dev, test = agentic.PARTITIONS
        overlaps = {
            "rendered_texts": len({row.text for row in parts[dev]} & {row.text for row in parts[test]}),
            "gold_normalised_templates": len({skeleton(row) for row in parts[dev]} & {skeleton(row) for row in parts[test]}),
            "all_value_normalised_templates": len(templates[dev] & templates[test]),
            "inserted_values": len(values[dev] & values[test]),
        }
        self.assertEqual(overlaps, dict.fromkeys(overlaps, 0))

    def test_arm_changes_only_the_new_rulepack_slot(self):
        source = '[policy.rulepacks]\nbundled=["core","secrets"]\npaths=[]\n\n[[rule]]\nkind="default"\naction="tokenize"\n'
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            project = root / "repo"
            pack = Path("crates/gaze-recognizers/embedded/secrets.toml")
            (project / pack).parent.mkdir(parents=True)
            (project / pack).write_bytes((arm.ROOT / pack).read_bytes())
            with mock.patch.object(arm, "ROOT", project):
                base, candidate = arm.prepare(source, root)
            paths = candidate["policy"]["rulepacks"].pop("paths")
            self.assertEqual(paths[:-1], base["policy"]["rulepacks"]["paths"])
            candidate["policy"]["rulepacks"]["paths"] = paths[:-1]
            self.assertEqual(base, candidate)
            old = tomllib.loads((root / "base-secrets.toml").read_text())
            new = tomllib.loads((project / arm.RULEPACK_PATH).read_text())
            self.assertEqual({r["id"] for r in old["recognizers"]}, {"password.field", "security_token.anchored"})
            self.assertEqual({r["id"] for r in new["recognizers"]}, arm.NEW_IDS)
            delta = tomllib.loads((root / "delta.toml").read_text())
            self.assertEqual(delta["mechanism_delta"]["append_rulepack_paths"], paths[-1:])
