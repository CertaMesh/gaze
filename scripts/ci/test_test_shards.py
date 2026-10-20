import contextlib
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

import test_shards as shards


class ShardContracts(unittest.TestCase):
    def test_every_workspace_test_binary_has_exactly_one_owner(self):
        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=shards.ROOT
        ))
        packages = {package["id"]: package["name"] for package in metadata["packages"]}
        binaries = set()
        for package in metadata["packages"]:
            if package["id"] not in metadata["workspace_members"]:
                continue
            for target in package["targets"]:
                if target["test"] and target["kind"][0] in ("lib", "bin", "test"):
                    binaries.add(shards.cargo_binary_id(
                        {"target": target, "package_id": package["id"]}, packages
                    ))
        assigned = {binary for values in shards.load_roster().values() for binary in values}
        self.assertEqual(binaries, assigned, "new binaries need an explicit shard assignment")

    def test_missing_duplicate_and_stale_assignments_fail(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "roster.json"
            path.write_text(json.dumps({"cli": ["a"], "runtime": ["a"], "gates": []}))
            with self.assertRaisesRegex(ValueError, "exactly one owner"):
                shards.load_roster(path)
        roster = {"cli": ["a"], "runtime": ["b"], "gates": ["c"]}
        inventory = {"rust-suites": {name: {"status": "listed"} for name in "abc"}}
        shards.validate_inventory(inventory, roster)
        for binaries in ("ab", "abcd"):
            with self.assertRaisesRegex(ValueError, "unassigned binaries"):
                shards.validate_inventory(
                    {"rust-suites": {name: {"status": "listed"} for name in binaries}}, roster
                )
        inventory["rust-suites"]["a"]["status"] = "skipped"
        with self.assertRaisesRegex(ValueError, "successfully listed"):
            shards.validate_inventory(inventory, roster)

    def test_execution_report_rejects_failure_skip_and_duplicate(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "junit.xml"
            test = '<testcase name="one" time="1.25">{}</testcase>'
            for child in ("<skipped/>", "<failure/>", "<error/>"):
                path.write_text('<testsuites><testsuite name="binary">'
                                + test.format(child) + '</testsuite></testsuites>')
                with self.assertRaises(ValueError):
                    shards.executed_tests(path)
            path.write_text('<testsuites><testsuite name="binary">'
                            + test.format("") * 2 + '</testsuite></testsuites>')
            with self.assertRaisesRegex(ValueError, "more than once"):
                shards.executed_tests(path)

    def test_proof_requires_all_shards_and_exact_test_names(self):
        roster = shards.load_roster()
        baseline = {binary: {} for values in roster.values() for binary in values}
        for values in roster.values():
            baseline[values[0]] = {"one": False, "helper": True}
        with tempfile.TemporaryDirectory() as temp:
            evidence = Path(temp)
            for shard in shards.SHARDS:
                directory = evidence / shard
                directory.mkdir()
                for filename in ("before.json", "after.json"):
                    (directory / filename).write_text(json.dumps(baseline))
                (directory / "ownership.json").write_text(json.dumps(roster))
                (directory / "junit.xml").write_text(
                    f'<testsuites><testsuite name="{roster[shard][0]}">'
                    '<testcase name="one" time="1.25"/></testsuite></testsuites>'
                )
            with contextlib.redirect_stdout(io.StringIO()):
                shards.prove(evidence, evidence / "report")
            self.assertEqual(len(json.loads((evidence / "report/timings.json").read_text())), 3)
            path = evidence / "cli/junit.xml"
            original = path.read_text()
            for change in (original.replace('name="one"', 'name="wrong"'), "<testsuites/>"):
                path.write_text(change)
                with self.assertRaisesRegex(ValueError, "missing"):
                    shards.prove(evidence, evidence / "report")
            path.unlink()
            with self.assertRaises(FileNotFoundError):
                shards.prove(evidence, evidence / "report")


if __name__ == "__main__":
    unittest.main()
