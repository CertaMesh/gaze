"""Private synthetic rmcp bridge. IPC observations are never evidence receipts."""
from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
import os
from pathlib import Path

from bench_subprocess import BenchSubprocess, ProducerFailure, producer_boundary
import evidence_eval as ee

FORMAT = 'synthetic_bridge_v1'
SCENARIO = 'phone_pair_v1'
MIN_FRAGMENT_BYTES = 8
GOLD_BYTES = 'gold_bytes_surviving_egress'
ATTRIBUTION = 'gold_occurrences_attribution_not_measured'
SURVIVAL = frozenset((GOLD_BYTES, 'gold_occurrences_surviving_egress',
                      'gold_occurrences_partially_surviving_egress'))
NEGATIVE = frozenset(('false_positive_occurrences', 'false_positive_bytes'))


class Coverage(Enum):
    COMPLETE = 'complete'
    PARTIAL = 'missing_slot'
    UNAVAILABLE = 'unavailable'
    ATTRIBUTION = 'unresolved_due_to_attribution'
    UNCOMPARED = 'uncompared'


@dataclass(frozen=True, repr=False)
class Slot:
    name: str
    byte_count: int


@dataclass(frozen=True, repr=False)
class Plan:
    ordinal: int
    key: str
    group: str
    weight: int
    gold: tuple[Slot, ...] = (Slot('a', 16), Slot('b', 16))
    negative: tuple[Slot, ...] = (Slot('n', 12),)
    restore: tuple[str, ...] = ('a', 'b', 'n')
    pii_class: str = 'custom:phone'
    region: str = 'de'
    stratum: str = 'synthetic_de'


PLANS = (Plan(0, 'case0', 'g0', 1), Plan(1, 'case1', 'g1', 3))


@dataclass(frozen=True, repr=False)
class Observation:
    gold_coverage: Coverage
    negative_coverage: Coverage
    restore_coverage: Coverage
    measured_gold_bytes: int
    full: int
    partial: int
    unknown: int
    false_positive_occurrences: int
    false_positive_bytes: int
    record: ee.DocRecord


@dataclass(frozen=True)
class BridgeSuccess:
    numeric_verified: bool
    groups_verified: bool


def require(condition):
    if not condition:
        raise ProducerFailure('protocol', 'payload')


def exact_keys(value, keys):
    require(type(value) is dict and set(value) == set(keys))


def uint(value):
    require(type(value) is int and 0 <= value <= (1 << 64) - 1)
    return value


def rows(value, planned, keys):
    require(type(value) is list and len(value) <= len(planned))
    result = {}
    for row in value:
        exact_keys(row, keys)
        slot = row['slot']
        require(type(slot) is str and slot in planned and slot not in result)
        result[slot] = row
    return result


def coverage(visited, planned):
    if not visited:
        return Coverage.UNAVAILABLE
    return Coverage.COMPLETE if set(visited) == set(planned) else Coverage.PARTIAL


def request_for(plan, arm):
    return dict(format=FORMAT, arm=arm, ordinal=plan.ordinal, scenario=SCENARIO)


def inventory(plans=PLANS):
    return ee.PlannedInventory(ee.PlannedCase(
        p.key, p.group, p.stratum, p.weight, len(p.gold), sum(s.byte_count for s in p.gold),
    ) for p in plans)


def declaration():
    # Existing test declaration constants, restricted to this synthetic stratum.
    return dict(confidence_level=.5, resample_count=16, seed=4, strata=['synthetic_de'],
                weighting='inventory_group', multiplicity_treatment='synthetic_none',
                coverage_target=.8, acceptance_limit=.1)


def map_observation(frame, plan, arm):
    exact_keys(frame, ('format', 'kind', 'arm', 'ordinal', 'scenario', 'outcome',
                       'gold', 'negative', 'restore'))
    require(frame['format'] == FORMAT and frame['kind'] == 'observation'
            and type(frame['arm']) is str and frame['arm'] == arm
            and type(frame['ordinal']) is int and frame['ordinal'] == plan.ordinal
            and frame['scenario'] == SCENARIO)
    state = frame['outcome']
    require(type(state) is str and state in ('COMPLETED', 'FAILED_CLOSED_NO_EGRESS', 'UNKNOWN_EGRESS'))
    gold_plan = {s.name: s.byte_count for s in plan.gold}
    negative_plan = {s.name: s.byte_count for s in plan.negative}
    gold = rows(frame['gold'], gold_plan, ('slot', 'verdict', 'surviving_bytes'))
    negatives = rows(frame['negative'], negative_plan, ('slot', 'verdict', 'false_positive_bytes'))
    restores = rows(frame['restore'], plan.restore, ('slot', 'decision_success', 'exact'))
    full = partial = unknown = measured = fp_count = fp_bytes = 0
    for slot, row in gold.items():
        verdict, count = row['verdict'], uint(row['surviving_bytes'])
        require(type(verdict) is str and verdict in ('full', 'partial', 'protected', 'unknown'))
        require((verdict == 'full' and count == gold_plan[slot])
                or (verdict == 'partial' and MIN_FRAGMENT_BYTES <= count < gold_plan[slot])
                or (verdict in ('protected', 'unknown') and count == 0))
        full += verdict == 'full'
        partial += verdict == 'partial'
        unknown += verdict == 'unknown'
        measured += count
    uncompared = False
    for slot, row in negatives.items():
        verdict, count = row['verdict'], uint(row['false_positive_bytes'])
        require(type(verdict) is str and verdict in ('full', 'protected', 'uncompared'))
        require(count == (negative_plan[slot] if verdict == 'protected' else 0))
        uncompared |= verdict == 'uncompared'
        fp_count += verdict == 'protected'
        fp_bytes += count
    for row in restores.values():
        require(type(row['decision_success']) is bool and type(row['exact']) is bool)
        require(not row['exact'] or row['decision_success'])
        require(state == 'COMPLETED' or not (row['exact'] or row['decision_success']))
    require(state == 'COMPLETED' or not (gold or negatives or restores))
    gc = coverage(gold, gold_plan)
    nc = coverage(negatives, negative_plan)
    rc = coverage(restores, plan.restore)
    observed = set()
    if gc == Coverage.COMPLETE:
        observed.add(ATTRIBUTION)
        if unknown:
            gc = Coverage.ATTRIBUTION
        else:
            observed.update(SURVIVAL)
    if nc == Coverage.COMPLETE:
        if uncompared:
            nc = Coverage.UNCOMPARED
        else:
            observed.update(NEGATIVE)
    restore_resolved = rc == Coverage.COMPLETE and bool(plan.restore) and state == 'COMPLETED'
    record = ee.DocRecord(
        outcome=state, observed_metrics=frozenset(observed),
        gold_bytes_surviving=measured if GOLD_BYTES in observed else 0,
        gold_occurrences_surviving=full if GOLD_BYTES in observed else 0,
        gold_occurrences_partially_surviving=partial if GOLD_BYTES in observed else 0,
        gold_occurrences_attribution_not_measured=unknown if ATTRIBUTION in observed else 0,
        false_positive_occurrences=fp_count if NEGATIVE <= observed else 0,
        false_positive_bytes=fp_bytes if NEGATIVE <= observed else 0,
        restore_exact=restore_resolved and all(row['exact'] for row in restores.values()),
        restore_decision_success=restore_resolved and all(row['decision_success'] for row in restores.values()),
        pii_class=plan.pii_class, region=plan.region,
    )
    return Observation(gc, nc, rc, measured, full, partial, unknown, fp_count, fp_bytes, record)


def validate_numeric(evaluator, observations):
    expected = {('base', 0): (0, 0, 0, True), ('base', 1): (0, 0, 0, True),
                ('candidate', 0): (16, 1, 0, True), ('candidate', 1): (8, 0, 1, False)}
    require(set(observations) == set(expected))
    for identity, (byte_count, full, partial, exact) in expected.items():
        side = observations[identity]
        r = side.record
        require((side.measured_gold_bytes, side.full, side.partial, side.unknown) == (byte_count, full, partial, 0))
        require(side.gold_coverage == side.negative_coverage == side.restore_coverage == Coverage.COMPLETE)
        require(r.observed_metrics == ee.OBSERVED_METRICS and r.gold_bytes_surviving == byte_count)
        require(r.restore_exact == exact and r.restore_decision_success)
        require(r.false_positive_bytes == r.false_positive_occurrences == 0)
        require(r.gold_bytes is None and r.gold_occurrences is None)
    interval = evaluator.paired_interval(GOLD_BYTES, declaration())
    require(type(interval) is dict and interval['point'] == 10 and not interval['conditional'])
    require(evaluator.resample_unit_count() == 2)
    require(tuple((evaluator.inventory.case(p.key).group_id, evaluator.inventory.case(p.key).weight,
                   evaluator.inventory.case(p.key).stratum) for p in PLANS)
            == (('g0', 1, 'synthetic_de'), ('g1', 3, 'synthetic_de')))
    return BridgeSuccess(numeric_verified=True, groups_verified=True)


@producer_boundary
def run(binary, *, _owner=None, _order=None):
    """Run only the fixed synthetic cell; publish success after child cleanup."""
    require(isinstance(binary, (str, Path)) and os.path.isfile(binary) and os.access(binary, os.X_OK))
    evaluator = ee.PrivateEvaluator(inventory())
    observations = {}
    owner = _owner if _owner is not None else BenchSubprocess([str(binary)])
    order = _order if _order is not None else tuple((a, p) for p in PLANS for a in ('base', 'candidate'))
    with owner as child:
        require(child.receive_handshake() == {'format': FORMAT, 'kind': 'ready'})
        child.check_message_deadline()
        for arm, plan in order:
            require(arm in ('base', 'candidate') and plan in PLANS and (arm, plan.ordinal) not in observations)
            frame = child.exchange(request_for(plan, arm))
            side = map_observation(frame, plan, arm)
            child.check_message_deadline()
            evaluator.add(arm, plan.key, side.record)
            observations[arm, plan.ordinal] = side
            child.check_deadline()
        staged = validate_numeric(evaluator, observations)
        child.check_deadline()
        child.finish()
    return staged
