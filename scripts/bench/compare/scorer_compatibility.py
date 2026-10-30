"""Admit one reviewed validation-only successor without rewriting measured provenance."""
import json
from pathlib import Path


def scorer_is_compatible(recorded: str, current: str) -> bool:
    if recorded == current:
        return True
    # Retain the measured scorer identity. Only this reviewed validation-only
    # successor is admissible without rerunning inference or relabelling rows.
    predecessor = "27bf262c5382e3d80adc8204357fb3939c4d634261d294ac1650b26ca22c2403"
    successor = "3a14e0780a612797312768df66cbee3a4b14b997cf988db10c1993c37c6c40a1"
    if recorded != predecessor or current != successor:
        return False
    proof = json.loads((Path(__file__).resolve().parents[3] /
                       "docs/reference/benchmarks/refusal-contract-v2-history-proof.json").read_text())
    return (proof.get("baseline_scorer_sha256") == recorded
            and proof.get("candidate_scorer_sha256") == current
            and proof.get("changed_scorer_definitions") == [
                "PIPELINE_FAILURE_REASONS", "_validate_final_protection_trace", "validate_response"]
            and set(proof.get("releases", {})) == {"v0.17.0", "v0.16.0"}
            and all(r.get("complete_scorecards_identical") is True and r.get("refusals") == 0
                    and set(r.get("contracts", {})) == {"1", "2", "3", "4"}
                    for r in proof["releases"].values()))
