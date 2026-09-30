from pathlib import Path
import tempfile
import unittest

import validator_probe_gate as gate


class ProbeVersions(unittest.TestCase):
    def test_current_probe_versions_match(self):
        gate.check_versions(gate.ROOT)

    def test_direct_and_inherited_versions_reject_drift(self):
        for declaration in ('version = "1.2.3"', 'version.workspace = true'):
            with self.subTest(declaration=declaration), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                (root / "Cargo.toml").write_text(
                    '[workspace]\nmembers = ["crates/example"]\n'
                    '[workspace.package]\nversion = "1.2.3"\n'
                )
                member = root / "crates/example"
                member.mkdir(parents=True)
                (member / "Cargo.toml").write_text(
                    f'[package]\nname = "example"\n{declaration}\n'
                )
                probe = root / "scripts/bench/validator_recall_probe"
                probe.mkdir(parents=True)
                lock = probe / "Cargo.lock"
                lock.write_text('[[package]]\nname = "example"\nversion = "1.2.3"\n')
                gate.check_versions(root)
                lock.write_text('[[package]]\nname = "example"\nversion = "1.2.2"\n')
                with self.assertRaisesRegex(ValueError, "example: probe Cargo.lock=1.2.2, workspace=1.2.3"):
                    gate.check_versions(root)

    def test_missing_manifest_fails_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            with self.assertRaises(FileNotFoundError):
                gate.check_versions(Path(temp))


if __name__ == "__main__":
    unittest.main()
