"""Model-free checks for the custom collision-metadata benchmark arm."""
import hashlib
import json
from pathlib import Path
import sys
import tomllib
from unittest.mock import patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import agentic_layers as agentic
import family_precedence_arm as arm

PINS = {
    "dev": "d507203771c42e85a3e083ceb1b18e1c63878b9420bbced15e1932f2b59bf33a",
    "test": "e73f16c54bff6f078ac112928115c8342dd5a7cccea430019310a2a00d52ba90",
}


@pytest.mark.parametrize("partition", ["dev", "test"])
def test_positive_and_control_cells_are_reproducible_and_pinned(partition):
    rows = arm.generate(partition)
    assert hashlib.sha256(agentic.corpus_bytes(rows)).hexdigest() == PINS[partition]
    assert len(rows) == 140
    assert len({row.uid for row in rows}) == len(rows)
    for case, (layer, *_rest) in arm.CASES.items():
        cells = [row for row in rows if row.family == f"effective_family_{case}"]
        assert len(cells) == 20
        assert {row.layer for row in cells} == {layer}
        for row in cells:
            assert bool(row.gold) == (layer == "A")
            if row.gold:
                (gold,) = row.gold
                assert row.text[gold.start:gold.end] == gold.value
                assert gold.label == "IDCARDNUM"
                assert gold.value.startswith(f"zzref-{case}-0")


def test_partitions_have_no_shared_values_or_templates():
    dev, test = arm.generate("dev"), arm.generate("test")
    assert not {row.text for row in dev} & {row.text for row in test}
    assert not {row.template for row in dev} & {row.template for row in test}


def test_delta_contains_real_mixed_precedence_and_control_members():
    assert arm.DELTA.read_text() == arm.policy_delta()
    parsed = tomllib.loads(arm.DELTA.read_text())
    members = {r["name"]: r for r in parsed["policy"]["custom_recognizers"]}
    for case in ("false_tie_gold", "false_tie_benign", "missed_tie_gold"):
        a, a2, b = [members[f"bench.effective.{case}.{suffix}"] for suffix in ("alpha", "calibration", "beta")]
        assert a["collision"]["precedence"] == 10
        assert a2["collision"]["precedence"] == 5
        assert a2["collision"]["variant"] == a["collision"]["variant"] == "alpha"
        assert b["collision"]["variant"] == "beta"
        assert b["collision"]["precedence"] == (5 if case == "missed_tie_gold" else 10)
        # The lower-precedence member is compiled; it has no candidate here.
        assert a2["pattern"] != a["pattern"]
    assert "collision" not in members["bench.effective.absent_id.beta"]
    assert members["bench.effective.same_variant.beta"]["collision"]["variant"] == "alpha"
    for case in ("no_tie_gold", "no_tie_benign"):
        assert members[f"bench.effective.{case}.beta"]["collision"]["precedence"] == 20


def test_policy_extension_keeps_setup_settings_and_precedes_defaults():
    setup = '''[session]\nscope = "ephemeral"\n[policy.rulepacks]\nbundled = ["core", "core-extended"]\npaths = []\n[[rule]]\nkind = "default"\naction = "tokenize"\n'''
    extended = arm.extend_policy(setup, arm.DELTA.read_text())
    data = tomllib.loads(extended)
    assert data["session"] == tomllib.loads(setup)["session"]
    assert data["policy"]["rulepacks"] == tomllib.loads(setup)["policy"]["rulepacks"]
    assert data["rule"][-1] == tomllib.loads(setup)["rule"][0]
    with pytest.raises(ValueError, match="already extended"):
        arm.extend_policy(extended, arm.DELTA.read_text())
    with pytest.raises(ValueError, match="differs"):
        arm.extend_policy(setup, arm.DELTA.read_text().replace("precedence = 5", "precedence = 6"))


def test_full_arm_uses_ordinary_runner_gold_writer_and_same_scored_contract(tmp_path):
    setup = '[session]\nscope="ephemeral"\n[[rule]]\nkind="default"\naction="tokenize"\n'
    policy = tmp_path / "policy.toml"
    policy.write_text(arm.extend_policy(setup, arm.DELTA.read_text()))
    original = agentic.generate("test")
    original_hash = hashlib.sha256(agentic.corpus_bytes(original)).hexdigest()

    def capture(args):
        assert args.policy == policy
        records = agentic.generate("test")
        assert records[:len(original)] == original
        assert records[len(original):] == arm.generate("test")
        prepared = agentic.prepare(arm.ROOT)
        assert prepared.manifest["extension"]["documents"] == 140
        assert prepared.manifest["corpus_sha256"] != original_hash
        assert len(prepared.identifiers) == sum(row.layer == "A" for row in records)
        assert len(prepared.lookalikes) == sum(row.layer == "D" for row in records)
        assert prepared.contract.sha256 == agentic.load_contract(arm.ROOT).sha256
        return 0

    with patch.object(arm.runner, "run", capture):
        assert arm.main(["full", "--policy", str(policy)]) == 0
    assert hashlib.sha256(agentic.corpus_bytes(agentic.generate("test"))).hexdigest() == original_hash


def test_arm_rejects_blind_or_unextended_policy(tmp_path):
    with pytest.raises(ValueError, match="requires"):
        arm.main(["full"])
    policy = tmp_path / "setup.toml"
    policy.write_text('[[rule]]\nkind="default"\naction="tokenize"\n')
    with pytest.raises(ValueError, match="exact committed"):
        arm.main(["full", "--policy", str(policy)])
