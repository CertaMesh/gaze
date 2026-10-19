"""Contract tests for the per-mechanism benchmark arms (scripts/bench/mechanism_arms.py).

Stdlib only. The synthetic mechanism is built from the committed GLiNER base
record: its candidate drops one layer C prediction, so the expected leak change
is known exactly without running a model.
"""

from __future__ import annotations

import copy
import functools
import gzip
import json
import os
import shutil
import tempfile
import tarfile
import unittest
from pathlib import Path
from unittest import mock

import mechanism_arms as mech
import scorecard_record as record


LEDGER = mech.load_ledger()
SOURCE = mech.ROOT / LEDGER["mechanisms"][0]["measurements"][0]["records"]["base"]["file"]
BASE_POLICY = '[ner]\nmodel_dir = "/models/ner"\nthreshold = 0.3\n'
DELTA = "[synthetic_mechanism]\nenabled = true\n"


def _rows(path: Path) -> list[dict]:
    with gzip.open(path, "rt", encoding="utf-8") as stream:
        return [json.loads(line) for line in stream]


def _write_rows(path: Path, rows: list[dict]) -> None:
    with gzip.open(path, "wt", encoding="utf-8") as stream:
        for row in rows:
            stream.write(json.dumps(row, sort_keys=True) + "\n")


def _drop_one_prediction(source: Path, target: Path) -> tuple[str, int]:
    """Copy `source` with the first gold-covering layer C prediction removed."""
    rows = _rows(source)
    gold = {row["id"]: row["gold"] for row in rows[0]["documents"]}
    for row in rows[1:]:
        trace = row["response"].get("final_protection_trace") or []
        if row["layer"] != "C" or row["config"] != "policy-file" or not trace:
            continue
        for index, item in enumerate(trace):
            spans = [span for span in gold[row["document_id"]]
                     if span[0] == item["raw_start"] and span[1] == item["raw_end"]]
            others = [other for position, other in enumerate(trace) if position != index]
            overlapped = any(o["raw_start"] < item["raw_end"] and item["raw_start"] < o["raw_end"]
                             for o in others)
            if len(spans) == 1 and spans[0][2] == "EMAIL" and not overlapped:
                row["response"]["final_protection_trace"] = others
                evidence = row["response"].get("gold_gap_evidence")
                if evidence is not None:
                    row["response"]["gold_gap_evidence"] = [
                        entry for entry in evidence
                        if (entry[0], entry[1]) != (item["raw_start"], item["raw_end"])
                    ]
                _write_rows(target, rows)
                return spans[0][2], item["raw_end"] - item["raw_start"]
    raise AssertionError("no isolated EMAIL prediction to drop")


def _copy_pinned_files(ledger: dict, root: Path) -> None:
    """Copy every file the ledger pins (delta, latency, records) under `root`."""
    for entry in ledger["mechanisms"]:
        paths = [entry["policy_delta"]["file"]]
        if "latency" in entry:
            paths.append(entry["latency"]["file"])
        paths += [m["records"][arm]["file"]
                  for m in entry["measurements"] for arm in ("base", "candidate")]
        for path in paths:
            (root / path).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(mech.ROOT / path, root / path)



def _released(ledger):
    """The ledger as if every measurement were of a tagged release.

    The renderer shows tagged measurements only (tagged_gaze.py); these tests
    exercise the table shapes, which need rows to be shown.
    """
    shown = copy.deepcopy(ledger)
    for entry in shown["mechanisms"]:
        for index, measurement in enumerate(entry["measurements"]):
            measurement["release"] = f"v0.15.{index}"
    return shown


class SyntheticMechanism:
    """Four runner output directories plus policies, under one temp root."""

    def __init__(self, root: Path) -> None:
        root.mkdir(parents=True)
        self.root = root
        self.delta = root / "delta.toml"
        self.delta.write_text(DELTA, encoding="utf-8")
        self.policies = {"base": root / "base.toml", "candidate": root / "candidate.toml"}
        self.policies["base"].write_text(BASE_POLICY, encoding="utf-8")
        self.policies["candidate"].write_text(BASE_POLICY + "\n" + DELTA, encoding="utf-8")
        self.binary = root / "clean_for_bench"
        self.binary.write_bytes(b"synthetic binary")
        records = {"base": root / "base.jsonl.gz", "candidate": root / "candidate.jsonl.gz"}
        shutil.copyfile(SOURCE, records["base"])
        self.label, self.dropped = _drop_one_prediction(SOURCE, records["candidate"])
        self.runs: dict[str, Path] = {}
        for arm, source in records.items():
            # The rescore returns the header's scorecard provenance, so the
            # policy binding goes into the record, as a runner would write it.
            rows = _rows(source)
            self._bind_policy(rows[0]["scorecard"], self.policies[arm])
            _write_rows(source, rows)
            for version in (2, 1):
                directory = root / f"{arm}-v{version}"
                directory.mkdir()
                target = directory / "observations-v1.jsonl.gz"
                shutil.copyfile(source, target)
                card = record.rescore(target, mech._contract(version))
                (directory / "scorecard-v4.json").write_text(json.dumps(card), encoding="utf-8")
                self.runs[f"{arm}-v{version}"] = directory

    def clone(self, root: Path) -> "SyntheticMechanism":
        """A copy under `root`; policy paths inside the records still name the originals."""
        shutil.copytree(self.root, root, dirs_exist_ok=True)
        copy_ = object.__new__(SyntheticMechanism)
        copy_.__dict__.update(self.__dict__)
        copy_.root = root
        copy_.delta = root / self.delta.name
        copy_.binary = root / self.binary.name
        copy_.policies = {arm: root / path.name for arm, path in self.policies.items()}
        copy_.runs = {name: root / path.name for name, path in self.runs.items()}
        return copy_

    @staticmethod
    def _bind_policy(card: dict, policy: Path) -> None:
        digest = mech.sha256(policy)
        card["parameters"]["policy_sha256"] = digest
        card["runner_provenance"]["policy"] = {"path": str(policy), "sha256": digest}

    def edit(self, name: str, change) -> None:
        path = self.runs[name] / "scorecard-v4.json"
        card = json.loads(path.read_text(encoding="utf-8"))
        change(card)
        path.write_text(json.dumps(card), encoding="utf-8")

    def record(self, ledger: dict, **overrides) -> dict:
        arguments = dict(
            mechanism="synthetic-drop", title="Synthetic drop", added_in="v0.16",
            shipped="test only",
            delta=self.delta, runs=self.runs, binary=self.binary,
            machine="test host", release=None, crates_tree="d" * 40,
            evidence_dir=self.root / "evidence", root=self.root,
        )
        arguments.update(overrides)
        return mech.record_measurement(ledger, **arguments)


class MechanismArmsTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls._tmp = tempfile.TemporaryDirectory()
        cls.pristine = SyntheticMechanism(Path(cls._tmp.name).resolve() / "pristine")
        cls.fixture = cls.pristine.clone(Path(cls._tmp.name).resolve() / "recorded")
        cls.ledger = mech.empty_ledger()
        cls.measurement = cls.fixture.record(cls.ledger)

    @classmethod
    def tearDownClass(cls) -> None:
        cls._tmp.cleanup()

    def fresh(self) -> SyntheticMechanism:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        return self.pristine.clone(Path(tmp.name).resolve())

    def rewrite_runs(self, fixture, revision=None):
        """Valid runner receipts with a new revision or different gzip bytes."""
        for name, directory in fixture.runs.items():
            path = directory / "observations-v1.jsonl.gz"
            if revision is None:
                path.write_bytes(path.read_bytes() + b"\0")
            else:
                rows = _rows(path)
                rows[0]["scorecard"]["gaze"]["revision"] = revision
                _write_rows(path, rows)
            version = 2 if name.endswith("v2") else 1
            card = record.rescore(path, mech._contract(version))
            (directory / "scorecard-v4.json").write_text(json.dumps(card), encoding="utf-8")

    def assert_refusal_preserves(self, fixture, ledger, action, error=mech.MechanismError):
        path = fixture.root / "ledger.json"
        path.write_text(json.dumps(ledger), encoding="utf-8")
        before = path.read_bytes()
        in_memory = copy.deepcopy(ledger)
        pins = {row["file"]: mech.sha256(fixture.root / row["file"])
                for entry in ledger["mechanisms"] for measurement in entry["measurements"]
                for row in measurement["records"].values()}
        try:
            with self.assertRaises(error):
                action()
                path.write_text(json.dumps(ledger), encoding="utf-8")
        finally:
            self.assertEqual(path.read_bytes(), before)
            self.assertEqual({name: mech.sha256(fixture.root / name) for name in pins}, pins)
            self.assertEqual(ledger, in_memory)

    def publication_fixture(self):
        fixture = self.fresh()
        ledger = mech.empty_ledger()
        fixture.record(ledger, release="v0.16.0")
        path = fixture.root / "ledger.json"
        path.write_text(json.dumps(ledger), encoding="utf-8")
        before = path.read_bytes()
        self.rewrite_runs(fixture)
        return fixture, ledger, path, before

    def assert_persisted_pins_unchanged(self, fixture, path, before):
        self.assertEqual(path.read_bytes(), before)
        old = json.loads(path.read_text(encoding="utf-8"))
        for entry in old["mechanisms"]:
            pins = [entry["policy_delta"]]
            if "latency" in entry:
                pins.append(entry["latency"])
            pins += [row for measurement in entry["measurements"]
                     for row in measurement["records"].values()]
            for row in pins:
                self.assertEqual(mech.sha256(fixture.root / row["file"]), row["sha256"])
        mech.validate(old, fixture.root)

    @unittest.skipUnless(hasattr(os, "fork"), "crash boundary requires fork")
    def test_crash_after_evidence_publish_preserves_persisted_pins(self):
        fixture, ledger, path, before = self.publication_fixture()
        original = mech._publish_evidence
        child = os.fork()
        if child == 0:
            def publish_then_die(*args):
                original(*args)
                os._exit(77)
            with mock.patch.object(mech, "_publish_evidence", side_effect=publish_then_die):
                fixture.record(ledger, release="v0.16.0")
            os._exit(99)
        _, status = os.waitpid(child, 0)
        self.assertEqual(os.waitstatus_to_exitcode(status), 77)
        self.assert_persisted_pins_unchanged(fixture, path, before)

    def test_cli_ledger_write_failure_preserves_persisted_pins(self):
        fixture, ledger, path, before = self.publication_fixture()
        original_record = mech.record_measurement
        original_write = Path.write_text
        original_replace = os.replace
        def record_in_fixture(ledger, **kwargs):
            return original_record(ledger, **kwargs, root=fixture.root,
                                   evidence_dir=fixture.root / "evidence", crates_tree="d" * 40)
        def fail_write(target, *args, **kwargs):
            if target == path:
                raise OSError("synthetic ledger write failure")
            return original_write(target, *args, **kwargs)
        def fail_replace(source, target):
            if Path(target) == path:
                raise OSError("synthetic ledger write failure")
            return original_replace(source, target)
        args = ["record", "--mechanism", "synthetic-drop", "--title", "Synthetic drop",
                "--added-in", "v0.16", "--shipped", "test only", "--machine", "test host",
                "--policy-delta", str(fixture.delta), "--binary", str(fixture.binary),
                "--release", "v0.16.0"]
        for name, directory in fixture.runs.items():
            args += ["--" + name, str(directory)]
        with mock.patch.object(mech, "LEDGER", path), \
             mock.patch.object(mech, "load_ledger", return_value=ledger), \
             mock.patch.object(mech, "record_measurement", side_effect=record_in_fixture), \
             mock.patch.object(Path, "write_text", fail_write), \
             mock.patch.object(os, "replace", side_effect=fail_replace):
            with self.assertRaises(SystemExit) as raised:
                mech.main(args)
            self.assertEqual(raised.exception.code, 2)
        self.assert_persisted_pins_unchanged(fixture, path, before)

    def test_release_revision_conflict_preserves_ledger_and_evidence(self):
        fixture = self.fresh()
        ledger = mech.empty_ledger()
        fixture.record(ledger, release="v0.16.0")
        self.rewrite_runs(fixture, "e" * 40)
        self.assert_refusal_preserves(
            fixture, ledger, lambda: fixture.record(ledger, release="v0.16.0"))

    def test_same_target_failed_validation_preserves_ledger_and_evidence(self):
        fixture = self.fresh()
        ledger = mech.empty_ledger()
        fixture.record(ledger, release="v0.16.0")
        self.rewrite_runs(fixture)
        self.assert_refusal_preserves(
            fixture, ledger, lambda: fixture.record(ledger, release="v0.16.0", crates_tree="invalid"))

    def test_same_target_second_copy_failure_preserves_ledger_and_evidence(self):
        fixture = self.fresh()
        ledger = mech.empty_ledger()
        fixture.record(ledger)
        self.rewrite_runs(fixture)
        real_copy = shutil.copyfile
        calls = 0
        def fail_second(source, target, *args, **kwargs):
            nonlocal calls
            calls += 1
            if calls == 2:
                raise OSError("synthetic second copy failure")
            return real_copy(source, target, *args, **kwargs)
        with mock.patch.object(shutil, "copyfile", side_effect=fail_second):
            self.assert_refusal_preserves(fixture, ledger, lambda: fixture.record(ledger), OSError)

    def test_same_target_second_publish_failure_restores_ledger_and_evidence(self):
        fixture = self.fresh()
        ledger = mech.empty_ledger()
        fixture.record(ledger)
        self.rewrite_runs(fixture)
        real_replace = mech.os.replace
        calls = 0
        def fail_second(source, target):
            nonlocal calls
            calls += 1
            if calls == 2:
                raise OSError("synthetic second publish failure")
            return real_replace(source, target)
        with mock.patch.object(mech.os, "replace", side_effect=fail_second):
            self.assert_refusal_preserves(fixture, ledger, lambda: fixture.record(ledger), OSError)

    def test_duplicate_release_targets_are_rejected_on_load_and_render(self):
        ledger = _released(self.ledger)
        entry = ledger["mechanisms"][0]
        entry["measurements"].append(copy.deepcopy(entry["measurements"][0]))
        entry["measurements"][-1]["revision"] = "e" * 40
        path = self.fixture.root / "duplicate-ledger.json"
        path.write_text(json.dumps(ledger), encoding="utf-8")
        with self.assertRaises(mech.MechanismError):
            mech.load_ledger(path)
        with self.assertRaises(mech.MechanismError):
            mech.render(ledger, ["v0.16.0"])
        with self.assertRaises(mech.MechanismError):
            mech.release_cell(entry, "v0.16.0")

    def test_new_record_paths_fit_ustar_without_losing_full_hash(self):
        fixture = self.fresh()
        for mechanism in ("x", "nym-person-name-labels"):
            measurement = fixture.record(mech.empty_ledger(), mechanism=mechanism)
            for row in measurement["records"].values():
                path = Path(row["file"])
                self.assertLessEqual(len(path.name.encode("utf-8")), 100)
                self.assertIn(row["sha256"], path.name)
                if mechanism == "x":
                    self.assertEqual(path.parent, Path("evidence"))
                # The private producer snapshot permits USTAR, never PAX path extensions.
                tarfile.TarInfo(path.as_posix()).tobuf(format=tarfile.USTAR_FORMAT)
                self.assertEqual(mech.sha256(fixture.root / path), row["sha256"])

    def test_two_untagged_revisions_remain_distinct(self):
        fixture = self.fresh()
        ledger = mech.empty_ledger()
        first = fixture.record(ledger)
        self.rewrite_runs(fixture, "e" * 40)
        second = fixture.record(ledger)
        self.assertEqual(len(ledger["mechanisms"][0]["measurements"]), 2)
        self.assertNotEqual(first["records"], second["records"])
        mech.validate(ledger, fixture.root)

    def test_short_revision_prefix_alias_is_refused_without_overwrite(self):
        fixture = self.fresh()
        ledger = mech.empty_ledger()
        first = fixture.record(ledger)
        revision = first["revision"][:12] + "e" * 28
        self.rewrite_runs(fixture, revision)
        self.assert_refusal_preserves(fixture, ledger, lambda: fixture.record(ledger))

    def test_summary_and_detail_use_the_same_measurement_after_rerecord(self):
        fixture = self.fresh()
        ledger = mech.empty_ledger()
        fixture.record(ledger, release="v0.16.0")
        self.rewrite_runs(fixture)
        latest = fixture.record(ledger, release="v0.16.0")
        self.assertEqual(ledger["mechanisms"][0]["measurements"], [latest])
        body = mech.render(ledger, ["v0.16.0"], fixture.root)
        self.assertEqual(body.count("| Synthetic drop | v0.16.0 | v4 |"), 1)
        self.assertIn(f"| Synthetic drop | v4: leaked +{fixture.dropped} B, FP 0 B |", body)

    def test_a_second_mechanism_needs_only_a_delta_and_a_record_run(self) -> None:
        for version in ("4", "3", "2", "1"):
            row = self.measurement["contracts"][version]
            self.assertEqual(row["candidate"]["leaked"] - row["base"]["leaked"], self.fixture.dropped)
            self.assertEqual(row["leaked_by_label_delta"], {self.fixture.label: self.fixture.dropped})
        self.assertEqual(self.measurement["contracts"]["2"]["false_positive_by_class_delta"], {})
        self.assertNotIn("false_positive_by_class_delta", self.measurement["contracts"]["3"])
        self.assertEqual(set(self.measurement["gate"]), {"v2", "v1"})
        self.assertEqual(self.measurement["gate"]["v2"]["verdict"], "fail")
        mech.check_evidence(self.ledger, self.fixture.root)

    def test_render_names_every_contract_and_marks_older_releases(self) -> None:
        body = mech.render(_released(self.ledger), ["v0.16.0", "v0.15.1"])
        for version in ("v4", "v3", "v2", "v1"):
            self.assertIn("| Synthetic drop | v0.15.0 |", body)
            self.assertIn(f"| {version} |", body)
        self.assertIn("not available: mechanism added in v0.16", body)
        self.assertIn("not measured for this release", body)
        self.assertNotIn("|  |", body)

    def test_release_cell_uses_the_released_measurement(self) -> None:
        entry = copy.deepcopy(self.ledger["mechanisms"][0])
        entry["measurements"][0]["release"] = "v0.16.0"
        self.assertEqual(
            mech.release_cell(entry, "v0.16.0"), f"v4: leaked +{self.fixture.dropped} B, FP 0 B"
        )
        self.assertTrue(mech.release_cell(entry, "v0.15.1").startswith("not available"))

    def test_older_davlan_cell_names_setup_default(self) -> None:
        entry = next(item for item in LEDGER["mechanisms"] if item["id"] == "davlan-ner")
        self.assertEqual(
            mech.release_cell(entry, "v0.14.0"),
            "not available: setup default added in v0.15",
        )

    def test_tampered_ledger_number_fails_the_evidence_check(self) -> None:
        ledger = copy.deepcopy(self.ledger)
        ledger["mechanisms"][0]["measurements"][0]["contracts"]["2"]["base"]["leaked"] += 1
        with self.assertRaisesRegex(mech.MechanismError, "re-derive"):
            mech.check_evidence(ledger, self.fixture.root)

    def test_tampered_record_fails_validation(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name).resolve()
        shutil.copytree(self.fixture.root, root, dirs_exist_ok=True)
        base = root / self.measurement["records"]["base"]["file"]
        base.write_bytes(base.read_bytes() + b"\0")
        with self.assertRaisesRegex(mech.MechanismError, "differs from its SHA-256"):
            mech.validate(self.ledger, root)

    def test_candidate_policy_beyond_the_delta_is_refused(self) -> None:
        fixture = self.fresh()
        fixture.policies["candidate"].write_text(
            BASE_POLICY.replace("0.3", "0.4") + "\n" + DELTA, encoding="utf-8"
        )
        for name in ("candidate-v2", "candidate-v1"):
            fixture.edit(name, lambda card: SyntheticMechanism._bind_policy(card, fixture.policies["candidate"]))
        with self.assertRaisesRegex(mech.MechanismError, "not comparable"):
            fixture.record(mech.empty_ledger())

    def test_contract_must_match_the_slot(self) -> None:
        fixture = self.fresh()
        fixture.runs["base-v2"], fixture.runs["base-v1"] = fixture.runs["base-v1"], fixture.runs["base-v2"]
        with self.assertRaisesRegex(mech.MechanismError, "measured under v1, not v2"):
            fixture.record(mech.empty_ledger())

    def test_dirty_tree_and_other_revision_are_refused(self) -> None:
        fixture = self.fresh()
        fixture.edit("candidate-v1", lambda card: card["gaze"].update(dirty=True))
        with self.assertRaisesRegex(mech.MechanismError, "dirty tree"):
            fixture.record(mech.empty_ledger())
        fixture = self.fresh()
        fixture.edit("candidate-v2", lambda card: card["gaze"].update(revision="0" * 40))
        with self.assertRaisesRegex(mech.MechanismError, "differs from base-v2 in \\['revision'\\]"):
            fixture.record(mech.empty_ledger())

    def test_v1_run_must_equal_the_v1_rescore_of_the_v2_record(self) -> None:
        fixture = self.fresh()
        def bump(card: dict) -> None:
            card["runs"][0]["metrics"]["utf8_bytes"]["leaked"] += 1
        fixture.edit("base-v1", bump)
        with self.assertRaisesRegex(mech.MechanismError, "differs from the v1 rescore"):
            fixture.record(mech.empty_ledger())

    def test_record_that_does_not_match_its_scorecard_is_refused(self) -> None:
        fixture = self.fresh()
        path = fixture.runs["candidate-v2"] / "observations-v1.jsonl.gz"
        path.write_bytes(path.read_bytes() + b"\0")
        with self.assertRaisesRegex(mech.MechanismError, "pinned SHA-256"):
            fixture.record(mech.empty_ledger())

    def test_identifiers_are_validated(self) -> None:
        for field, value, message in (
            ("mechanism", "Synthetic Drop", "kebab case"),
            ("added_in", "0.16", "added_in"),
            ("release", "v0.16", "release"),
            ("machine", " ", "--machine"),
            ("shipped", " ", "--shipped"),
        ):
            with self.subTest(field=field), self.assertRaisesRegex(mech.MechanismError, message):
                self.fixture.record(mech.empty_ledger(), **{field: value})

    def test_same_mechanism_with_another_delta_is_refused(self) -> None:
        ledger = copy.deepcopy(self.ledger)
        with self.assertRaisesRegex(mech.MechanismError, "another delta or added_in"):
            self.fixture.record(ledger, added_in="v0.17")

    def test_gold_gap_block_is_keyed_on_contract_version(self) -> None:
        run = {"metrics": {"utf8_bytes": {"pii": 1, "leaked": 0, "false_positive": 2}},
               "pipeline_availability": {"failed_closed_documents": 0}}
        self.assertEqual(mech.headline(run, 2)["false_positive"], 2)
        with self.assertRaisesRegex(mech.MechanismError, "gold_gap block belongs"):
            mech.headline(run, 3)
        run["metrics"]["gold_gap"] = {
            "status": "diagnostic", "gold_gap_protected_bytes": 1,
            "false_positive_bytes_after_gold_gap": 1, "adjusted_precision": 0.5,
            "gold_gap_protected_bytes_by_label": {"EMAIL": 1},
        }
        self.assertEqual(mech.headline(run, 3)["false_positive"], 1)
        with self.assertRaisesRegex(mech.MechanismError, "gold_gap block belongs"):
            mech.headline(run, 2)
        run["metrics"]["gold_gap"]["gold_gap_protected_bytes_by_label"] = {"EMAIL": 2}
        with self.assertRaisesRegex(mech.MechanismError, "do not sum"):
            mech.headline(run, 3)

    def test_a_contract_without_a_file_is_a_typed_error(self) -> None:
        with self.assertRaisesRegex(mech.MechanismError, "contract v5 has no file"):
            mech._contract(5)

    def test_a_newer_contract_needs_refresh_not_a_new_run(self) -> None:
        # A v5 file makes check refuse v4-only rows by name; validate and render
        # still work, and refresh re-derives the row from the same records.
        ledger = copy.deepcopy(self.ledger)
        with mock.patch.object(mech, "required_contracts", lambda: (5, 4, 3, 2, 1)):
            mech.validate(ledger, self.fixture.root)
            self.assertIn("| v4 |", mech.render(_released(ledger), ["v0.16.0"]))
            with self.assertRaisesRegex(mech.MechanismError, "requires \\[5, 4, 3, 2, 1\\]; run .*refresh"):
                mech.check_evidence(ledger, self.fixture.root)
        row = ledger["mechanisms"][0]["measurements"][0]
        stub = {key: copy.deepcopy(value) for key, value in row.items()
                if key not in mech.NON_DERIVED_FIELDS}
        stub["contracts"]["5"] = stub["contracts"]["4"]
        with mock.patch.object(mech, "derive", lambda *_args, **_kwargs: copy.deepcopy(stub)):
            mech.refresh(ledger, self.fixture.root)
        self.assertEqual(mech.measured_contracts(ledger["mechanisms"][0]["measurements"][0]), [5, 4, 3, 2, 1])

    def test_refresh_preserves_delta_pin_after_file_changes(self) -> None:
        ledger = copy.deepcopy(self.ledger)
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        delta_row = ledger["mechanisms"][0]["policy_delta"]
        pinned = delta_row["sha256"]
        delta = root / delta_row["file"]
        delta.write_text(self.fixture.delta.read_text(encoding="utf-8") + "# changed after measurement\n", encoding="utf-8")
        with mock.patch.object(mech, "derive", return_value={}):
            mech.refresh(ledger, root)
        self.assertEqual(delta_row["sha256"], pinned)
        with self.assertRaisesRegex(mech.MechanismError, "missing or differs from its SHA-256"):
            mech.validate(ledger, root)

    def test_required_contracts_come_from_the_repository(self) -> None:
        self.assertEqual(mech.required_contracts(), (4, 3, 2, 1))
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        bench = Path(tmp.name)
        for name in ("scored-labels-v2.json", "scored-labels-v3.json", "scored-labels-v4.json", "scored-labels-agentic.json"):
            (bench / name).write_text("{}", encoding="utf-8")
        self.assertEqual(mech.required_contracts(bench), (4, 3, 2, 1))
        (bench / "scored-labels-v4.json").unlink()
        with self.assertRaisesRegex(mech.MechanismError, "disagree with headline v4"):
            mech.required_contracts(bench)

    def test_two_mechanisms_render_side_by_side(self) -> None:
        fixture = self.fresh()
        root = fixture.root
        ledger = copy.deepcopy(LEDGER)
        _copy_pinned_files(ledger, root)
        fixture.record(ledger)
        body = mech.render(_released(ledger), ["v0.15.1"], root)
        titles = [entry["title"] for entry in ledger["mechanisms"]]
        expected = [entry["title"] for entry in LEDGER["mechanisms"]] + ["Synthetic drop"]
        self.assertEqual(titles, expected)
        for entry in ledger["mechanisms"]:
            title = entry["title"]
            self.assertEqual(
                sum(body.count(f"| {title} | v0.15.{index} |")
                    for index in range(len(entry["measurements"]))),
                4 * len(entry["measurements"]),
                title,
            )
            self.assertIn(f"- **{title}** ships ", body)
        self.assertIn("| Synthetic drop | not available: mechanism added in v0.16 |", body)

    def test_ledger_validation_rejects_bad_shapes(self) -> None:
        cases = {
            "schema": lambda ledger: ledger.update(schema_version=2),
            "duplicate": lambda ledger: ledger["mechanisms"].append(copy.deepcopy(ledger["mechanisms"][0])),
            "no measurement": lambda ledger: ledger["mechanisms"][0].update(measurements=[]),
            "no shipped status": lambda ledger: ledger["mechanisms"][0].pop("shipped"),
            "contracts": lambda ledger: ledger["mechanisms"][0]["measurements"][0]["contracts"].pop("2"),
            "contract key": lambda ledger: ledger["mechanisms"][0]["measurements"][0]["contracts"].update(v9={}),
            "attested": lambda ledger: ledger["mechanisms"][0]["measurements"][0].pop("attested"),
            "binary": lambda ledger: ledger["mechanisms"][0]["measurements"][0]["attested"].update(binary_sha256="x"),
            "gate": lambda ledger: ledger["mechanisms"][0]["measurements"][0]["gate"].pop("v1"),
            "same policy": lambda ledger: ledger["mechanisms"][0]["measurements"][0].update(
                candidate_policy_sha256=ledger["mechanisms"][0]["measurements"][0]["base_policy_sha256"]),
        }
        for name, change in cases.items():
            with self.subTest(name=name):
                ledger = copy.deepcopy(self.ledger)
                change(ledger)
                with self.assertRaises(mech.MechanismError):
                    mech.validate(ledger, self.fixture.root)


@functools.cache
def _cached_derive(base: Path, candidate: Path, versions: tuple[int, ...]) -> str:
    return json.dumps(_REAL_DERIVE(base, candidate, versions))


_REAL_DERIVE = mech.derive


def _derive_once(base: Path, candidate: Path, versions) -> dict:
    """Re-scoring a full record takes seconds; each mutation needs the same result."""
    return json.loads(_cached_derive(base, candidate, tuple(versions)))


class CommittedLedgerTest(unittest.TestCase):
    def test_shipped_release_columns_exclude_provisional_candidates(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "history.json"
            path.write_text(json.dumps({"releases": [
                {"version": "v0.15.0"},
                {"version": "v0.16.0", "provisional": True},
                {"version": "v0.15.1", "provisional": False},
            ]}), encoding="utf-8")
            self.assertEqual(mech._released_versions(path), ["v0.15.1", "v0.15.0"])

    def test_every_stored_field_is_checked_against_the_records(self) -> None:
        mutations = {
            "revision": lambda m: m.update(revision="0" * 40),
            "corpus_sha256": lambda m: m.update(corpus_sha256="0" * 64),
            "sampling_seed": lambda m: m.update(sampling_seed=m["sampling_seed"] + 1),
            "base_policy_sha256": lambda m: m.update(base_policy_sha256="0" * 64),
            "candidate_policy_sha256": lambda m: m.update(candidate_policy_sha256="1" * 64),
            "gate": lambda m: m["gate"]["v2"].update(verdict="fail"),
            "gate reason": lambda m: m["gate"]["v1"].update(reason="made up"),
            "layers": lambda m: m["agentic_layers"]["A"]["base"].update(leaked=0),
            "drop headline contract": lambda m: m["contracts"].pop("3"),
            "drop v2 contract": lambda m: m["contracts"].pop("2"),
            "unexplained field": lambda m: m.update(note="hand-written"),
        }
        with mock.patch.object(mech, "derive", _derive_once):
            mech.check_evidence(LEDGER)
            for name, change in mutations.items():
                with self.subTest(field=name):
                    ledger = copy.deepcopy(LEDGER)
                    change(ledger["mechanisms"][0]["measurements"][0])
                    with self.assertRaisesRegex(
                        mech.MechanismError, "re-derive|neither derived|repository requires"
                    ):
                        mech.check_evidence(ledger)

    def test_committed_ledger_matches_its_records_and_document(self) -> None:
        mech.validate(LEDGER)
        mech.check_evidence(LEDGER)
        document = mech.DOC.read_text(encoding="utf-8")
        releases = mech._released_versions(mech.RELEASE_HISTORY)
        self.assertEqual(mech.apply(document, mech.render(LEDGER, releases)), document)

    def test_gliner_records_the_public_opt_in_command_and_pinned_cost(self) -> None:
        entry = next(item for item in LEDGER["mechanisms"] if item["id"] == "gliner-dob-judge")
        self.assertIn("opt-in", entry["shipped"])
        self.assertIn("`gaze setup --dob-judge`", entry["shipped"])
        body = mech.render(_released(LEDGER), [])
        self.assertIn("peak RSS +664 MiB", body)
        self.assertIn("p95 +22.5 ms", body)

    def test_latency_evidence_must_be_pinned_and_valid(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        ledger = copy.deepcopy(LEDGER)
        entry = ledger["mechanisms"][0]
        _copy_pinned_files(ledger, root)
        mech.validate(ledger, root)
        latency = root / entry["latency"]["file"]
        data = json.loads(latency.read_text(encoding="utf-8"))
        data["verdict"] = "timing invalid: host was not quiet for the whole window"
        latency.write_text(json.dumps(data), encoding="utf-8")
        with self.assertRaisesRegex(mech.MechanismError, "differs from its SHA-256"):
            mech.validate(ledger, root)
        entry["latency"]["sha256"] = mech.sha256(latency)
        with self.assertRaisesRegex(mech.MechanismError, "not a valid quiet-host run"):
            mech.validate(ledger, root)

    def test_every_shipped_release_predating_gliner_says_so(self) -> None:
        entry = next(item for item in LEDGER["mechanisms"] if item["id"] == "gliner-dob-judge")
        for version in mech._released_versions(mech.RELEASE_HISTORY):
            if mech._before(version, entry["added_in"]):
                self.assertEqual(
                    mech.release_cell(entry, version), "not available: mechanism added in v0.16"
                )


if __name__ == "__main__":
    unittest.main()
