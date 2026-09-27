"""Contract tests for the per-mechanism benchmark arms (scripts/bench/mechanism_arms.py).

Stdlib only. The synthetic mechanism is built from the committed GLiNER base
record: its candidate drops one layer C prediction, so the expected leak change
is known exactly without running a model.
"""

from __future__ import annotations

import copy
import gzip
import json
import shutil
import tempfile
import unittest
from pathlib import Path

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
            delta=self.delta, runs=self.runs, binary=self.binary,
            machine="test host", release=None,
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

    def test_a_second_mechanism_needs_only_a_delta_and_a_record_run(self) -> None:
        for version in ("3", "2", "1"):
            row = self.measurement["contracts"][version]
            self.assertEqual(row["candidate"]["leaked"] - row["base"]["leaked"], self.fixture.dropped)
            self.assertEqual(row["leaked_by_label_delta"], {self.fixture.label: self.fixture.dropped})
        self.assertEqual(self.measurement["contracts"]["2"]["false_positive_by_class_delta"], {})
        self.assertNotIn("false_positive_by_class_delta", self.measurement["contracts"]["3"])
        self.assertEqual(set(self.measurement["gate"]), {"v2", "v1"})
        self.assertEqual(self.measurement["gate"]["v2"]["verdict"], "fail")
        mech.check_evidence(self.ledger, self.fixture.root)

    def test_render_names_every_contract_and_marks_older_releases(self) -> None:
        body = mech.render(self.ledger, ["v0.16.0", "v0.15.1"])
        for version in ("v3", "v2", "v1"):
            self.assertIn(f"| Synthetic drop | `", body)
            self.assertIn(f"| {version} |", body)
        self.assertIn("not available: mechanism added in v0.16", body)
        self.assertIn("not measured for this release", body)
        self.assertNotIn("|  |", body)

    def test_release_cell_uses_the_released_measurement(self) -> None:
        entry = copy.deepcopy(self.ledger["mechanisms"][0])
        entry["measurements"][0]["release"] = "v0.16.0"
        self.assertEqual(
            mech.release_cell(entry, "v0.16.0"), f"leaked +{self.fixture.dropped} B, FP 0 B"
        )
        self.assertTrue(mech.release_cell(entry, "v0.15.1").startswith("not available"))

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
        with self.assertRaisesRegex(mech.MechanismError, "v3 run lacks"):
            mech.headline(run, 3)
        run["metrics"]["gold_gap"] = {"false_positive_bytes_after_gold_gap": 1}
        self.assertEqual(mech.headline(run, 3)["false_positive"], 1)
        with self.assertRaisesRegex(mech.MechanismError, "v2 run carries"):
            mech.headline(run, 2)

    def test_ledger_validation_rejects_bad_shapes(self) -> None:
        cases = {
            "schema": lambda ledger: ledger.update(schema_version=2),
            "duplicate": lambda ledger: ledger["mechanisms"].append(copy.deepcopy(ledger["mechanisms"][0])),
            "no measurement": lambda ledger: ledger["mechanisms"][0].update(measurements=[]),
            "contracts": lambda ledger: ledger["mechanisms"][0]["measurements"][0]["contracts"].pop("3"),
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


class CommittedLedgerTest(unittest.TestCase):
    def test_committed_ledger_matches_its_records_and_document(self) -> None:
        mech.validate(LEDGER)
        mech.check_evidence(LEDGER)
        document = mech.DOC.read_text(encoding="utf-8")
        releases = mech._released_versions(mech.RELEASE_HISTORY)
        self.assertEqual(mech.apply(document, mech.render(LEDGER, releases)), document)

    def test_every_shipped_release_predating_gliner_says_so(self) -> None:
        entry = next(item for item in LEDGER["mechanisms"] if item["id"] == "gliner-dob-judge")
        for version in mech._released_versions(mech.RELEASE_HISTORY):
            if mech._before(version, entry["added_in"]):
                self.assertEqual(
                    mech.release_cell(entry, version), "not available: mechanism added in v0.16"
                )


if __name__ == "__main__":
    unittest.main()
