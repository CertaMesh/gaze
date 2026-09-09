"""In-memory synthetic paired evaluator; no per-document serialization surface."""
from __future__ import annotations

import copy
from dataclasses import dataclass, fields
import math
import random
from types import MappingProxyType
from pathlib import Path

import evidence_protocol as ep
from gaze_bench_score import merge_intervals, interval_length


@dataclass(frozen=True, repr=False)
class PlannedCase:
    key: str
    group_id: str
    stratum: str
    weight: float
    gold_occurrences: int | None = None
    gold_bytes: int | None = None


class PlannedInventory:
    def __init__(self, cases):
        self._cases = tuple(cases)
        index, groups = {}, {}
        for c in self._cases:
            ep.require(type(c) is PlannedCase and type(c.key) is str and bool(c.key)
                       and type(c.group_id) is str and bool(c.group_id), 'inventory_conflict')
            ep.require(c.key not in index and type(c.stratum) is str and c.stratum in ep.STRATUM_IDS
                       and ep._number(c.weight) and c.weight > 0, 'inventory_conflict')
            ep.require(all(v is None or type(v) is int and v >= 0 for v in (c.gold_occurrences, c.gold_bytes)), 'inventory_conflict')
            group = (c.stratum, c.weight)
            ep.require(c.group_id not in groups or groups[c.group_id] == group, 'inventory_conflict')
            index[c.key] = c; groups[c.group_id] = group
        self._index = MappingProxyType(index)

    def keys(self):
        return tuple(c.key for c in self._cases)

    def case(self, key):
        ep.require(type(key) is str and key in self._index, 'inventory_conflict')
        return self._index[key]

    def __len__(self):
        return len(self._cases)


@dataclass(frozen=True, repr=False)
class DocRecord:
    observed_metrics: frozenset = frozenset()
    outcome: str = 'NOT_STARTED'
    gold_occurrences: int = 0
    gold_occurrences_surviving: int = 0
    gold_occurrences_partially_surviving: int = 0
    gold_occurrences_attribution_not_measured: int = 0
    gold_bytes: int = 0
    gold_bytes_surviving: int = 0
    false_positive_occurrences: int = 0
    false_positive_bytes: int = 0
    entities: int = 0
    entities_fully_covered: int = 0
    restore_exact: bool = False
    restore_decision_success: bool = False
    pii_class: str = 'Email'
    region: str = 'global'


METRIC_FIELDS = {
    'gold_occurrences_planned':'gold_occurrences',
    'gold_occurrences_surviving_egress':'gold_occurrences_surviving',
    'gold_occurrences_partially_surviving_egress':'gold_occurrences_partially_surviving',
    'gold_occurrences_attribution_not_measured':'gold_occurrences_attribution_not_measured',
    'gold_bytes_planned':'gold_bytes', 'gold_bytes_surviving_egress':'gold_bytes_surviving',
    'false_positive_occurrences':'false_positive_occurrences', 'false_positive_bytes':'false_positive_bytes',
}


PLAN_METRICS = frozenset(('gold_occurrences_planned', 'gold_bytes_planned'))
OBSERVED_METRICS = frozenset(METRIC_FIELDS) - PLAN_METRICS
EVALUATOR_DERIVATIONS = {m: ('planned_inventory' if m in PLAN_METRICS else
    'private_authored_records' if m in OBSERVED_METRICS or m == 'unknown_egress_lower_bound_cases' else
    'not_measured') for m in ep.METRIC_IDS}


def surviving_bytes(ranges):
    """Use the legacy merge convention without exporting coordinates."""
    ep.require(type(ranges) in (list, tuple), 'wrong_type')
    ep.require(all(type(r) in (list,tuple) and len(r) == 2 and all(type(v) is int and v >= 0 for v in r) and r[1] >= r[0] for r in ranges), 'wrong_type')
    return interval_length(merge_intervals(ranges))


def quantile(samples, probability):
    ep.require(bool(samples) and 0 <= probability <= 1, 'declaration_invalid')
    return sorted(samples)[min(len(samples)-1, max(0, math.ceil(probability * len(samples))-1))]


class PrivateEvaluator:
    def __init__(self, inventory):
        ep.require(type(inventory) is PlannedInventory, 'inventory_conflict')
        self.inventory = inventory
        self._class_rows = ep.load_class_commitments(Path(__file__).resolve().parents[2] / "docs/reference/benchmarks/class-commitments-v1.json")
        self._records = {a:{} for a in ep.ARM_IDS}
        self._finalized = False

    def add(self, arm, key, record):
        ep.require(type(arm) is str and arm in ep.ARM_IDS, 'inventory_conflict')
        self.inventory.case(key)
        ep.require(not self._finalized and key not in self._records[arm], 'inventory_conflict')
        ep.require(type(record) is DocRecord, 'wrong_type')
        ep.require(type(record.outcome) is str and record.outcome in ep.OUTCOME_STATES, 'value_out_of_vocabulary')
        for f in fields(record):
            if f.name not in ('outcome','pii_class','region','restore_exact','restore_decision_success','observed_metrics'):
                v = getattr(record, f.name)
                ep.require(type(v) is int and v >= 0, 'wrong_type')
        ep.require(type(record.observed_metrics) is frozenset and record.observed_metrics <= OBSERVED_METRICS, 'wrong_type')
        ep.require(all(not getattr(record, METRIC_FIELDS[m]) or m in record.observed_metrics for m in OBSERVED_METRICS), 'derivation_conflict')
        ep.require(not record.observed_metrics or record.outcome in ('COMPLETED','UNKNOWN_EGRESS'), 'derivation_conflict')
        ep.require(type(record.restore_exact) is bool and type(record.restore_decision_success) is bool, 'wrong_type')
        ep.require((record.pii_class, record.region) in {('Email','global'),('custom:phone','de')}, 'class_commitment_invalid')
        ep.require(record.gold_occurrences_surviving + record.gold_occurrences_partially_surviving + record.gold_occurrences_attribution_not_measured <= record.gold_occurrences and record.gold_bytes_surviving <= record.gold_bytes and record.entities_fully_covered <= record.entities, 'outcome_identity_violation')
        if record.outcome != 'COMPLETED':
            ep.require(not record.entities_fully_covered and not record.restore_exact and not record.restore_decision_success, 'outcome_identity_violation')
        if record.outcome not in ('COMPLETED','UNKNOWN_EGRESS'):
            ep.require(not record.gold_bytes_surviving and not record.gold_occurrences_surviving and not record.gold_occurrences_partially_surviving and not record.false_positive_occurrences and not record.false_positive_bytes, 'outcome_identity_violation')
        ep.require(not record.restore_exact or record.restore_decision_success, 'outcome_identity_violation')
        self._records[arm][key] = record

    def finalize(self):
        for arm in ep.ARM_IDS:
            for key in self.inventory.keys():
                self._records[arm].setdefault(key, DocRecord())
        self._finalized = True
        self.reconcile()
        return self

    def outcomes(self, arm):
        return {s:sum(r.outcome == s for r in self._records[arm].values()) for s in ep.OUTCOME_STATES}

    def asymmetric_outcome_table(self):
        table = {a:dict.fromkeys(ep.OUTCOME_STATES, 0) for a in ep.OUTCOME_STATES}
        for key in self.inventory.keys():
            table[self._records['base'][key].outcome][self._records['candidate'][key].outcome] += 1
        return table

    def reconcile(self):
        ep.require(all(set(records) == set(self.inventory.keys()) for records in self._records.values()), 'inventory_conflict')
        t = self.asymmetric_outcome_table()
        ep.require(sum(sum(row.values()) for row in t.values()) == len(self.inventory), 'inventory_conflict')
        ep.require(all(sum(t[s].values()) == self.outcomes('base')[s] and sum(t[a][s] for a in ep.OUTCOME_STATES) == self.outcomes('candidate')[s] for s in ep.OUTCOME_STATES), 'inventory_conflict')

    def paired_completed_keys(self):
        self.finalize()
        return tuple(k for k in self.inventory.keys() if all(self._records[a][k].outcome == 'COMPLETED' for a in ep.ARM_IDS))

    def _groups(self):
        groups = {}
        for key in self.paired_completed_keys():
            groups.setdefault(self.inventory.case(key).group_id, []).append(key)
        return groups

    def resample_unit_count(self):
        return len(self._groups())

    def draw_resample(self, seed, index):
        groups = self._groups()
        rng = random.Random(seed + index)
        result = []
        for stratum in sorted({self.inventory.case(keys[0]).stratum for keys in groups.values()}):
            ids = [g for g,keys in groups.items() if self.inventory.case(keys[0]).stratum == stratum]
            for _ in ids:
                result.extend(groups[rng.choice(ids)])
        return result

    def _estimate(self, metric, keys):
        field = METRIC_FIELDS[metric]
        groups = self._groups()
        original_mass = {}
        sampled = {}
        # A group appears as its full record block, repeated once for each draw.
        for g, members in groups.items():
            c = self.inventory.case(members[0])
            original_mass[c.stratum] = original_mass.get(c.stratum, 0.) + c.weight
            multiplicity = keys.count(members[0])
            ep.require(all(keys.count(k) == multiplicity for k in members), 'inventory_conflict')
            if not multiplicity:
                continue
            delta = 0. if metric in PLAN_METRICS else sum(getattr(self._records['candidate'][k], field) - getattr(self._records['base'][k], field) for k in members) / len(members)
            numerator, denominator = sampled.get(c.stratum, (0., 0.))
            sampled[c.stratum] = (numerator + delta*c.weight*multiplicity, denominator+c.weight*multiplicity)
        ep.require(set(sampled) == set(original_mass), 'inventory_conflict')
        return sum(original_mass[s] * n/d for s,(n,d) in sampled.items()) / sum(original_mass.values())

    def paired_interval(self, metric, declaration):
        ep.require(metric in METRIC_FIELDS, 'value_out_of_vocabulary')
        self.finalize()
        strata = {self.inventory.case(k).stratum for k in self.inventory.keys()}
        if not ep.declaration_valid(declaration, strata):
            return 'NOT_EVALUABLE'
        keys = self.paired_completed_keys()
        if not keys or {self.inventory.case(k).stratum for k in keys} != strata:
            return 'NOT_EVALUABLE'
        if metric in ep.LEAK_FAMILY_METRIC_IDS and any(self.outcomes(a)['UNKNOWN_EGRESS'] for a in ep.ARM_IDS):
            return 'NOT_EVALUABLE'
        if metric in PLAN_METRICS and any(getattr(self.inventory.case(k), METRIC_FIELDS[metric]) is None for k in keys):
            return 'NOT_EVALUABLE'
        if metric in OBSERVED_METRICS and any(metric not in self._records[a][k].observed_metrics for a in ep.ARM_IDS for k in keys):
            return 'NOT_EVALUABLE'
        samples = [self._estimate(metric, self.draw_resample(declaration['seed'], i)) for i in range(declaration['resample_count'])]
        alpha = (1-declaration['confidence_level'])/2
        return dict(point=self._estimate(metric, list(keys)), low=quantile(samples, alpha), high=quantile(samples, 1-alpha),
                    method_id='grouped_paired_percentile_v1', basis='paired_completed',
                    conditional=any(self.outcomes(a)['COMPLETED'] != len(self.inventory) for a in ep.ARM_IDS))

    def protected_case_count(self, arm):
        self.finalize()
        return sum(r.outcome == 'COMPLETED' and r.entities > 0 and r.entities == r.entities_fully_covered for r in self._records[arm].values())

    def aggregate(self, arm):
        self.finalize()
        result = {}
        for metric, field in METRIC_FIELDS.items():
            if metric in PLAN_METRICS:
                values = [getattr(self.inventory.case(k), field) for k in self.inventory.keys()]
                if values and all(v is not None for v in values): result[metric] = sum(values)
            else:
                values = [getattr(r, field) for r in self._records[arm].values() if metric in r.observed_metrics]
                if values: result[metric] = sum(values)
        unknown = [r for r in self._records[arm].values() if r.outcome == 'UNKNOWN_EGRESS']
        if unknown and all(r.observed_metrics & ep.LEAK_FAMILY_METRIC_IDS for r in unknown):
            result['unknown_egress_lower_bound_cases'] = sum(r.gold_bytes_surviving > 0 or r.gold_occurrences_surviving > 0 or r.gold_occurrences_partially_surviving > 0 for r in unknown)
        return result

    def derivations(self, arm):
        result = EVALUATOR_DERIVATIONS.copy()
        counts = self.aggregate(arm)
        for m in result:
            if m not in counts: result[m] = 'not_measured'
            elif m in OBSERVED_METRICS and any(m not in r.observed_metrics or r.outcome != 'COMPLETED' for r in self._records[arm].values()):
                result[m] = 'observed_subset_lower_bound'
        return result

    def class_cross_tab(self, arm):
        self.finalize()
        result = {}
        for r in self._records[arm].values():
            pair = (r.pii_class,r.region)
            result[pair] = result.get(pair, 0) + r.gold_bytes_surviving
        return result

    def export_receipt(self, template, custody, *, arm='candidate', declaration=None, repo_root=None, attestation_probe=None):
        self.finalize()
        ep.require(type(custody) is dict and custody.get('scope') == 'synthetic_fixture_only' and bool(self.inventory.keys()) and bool(custody.get('membership_order')) and tuple(custody.get('membership_order', ())) == self.inventory.keys(), 'membership_order_proof_failed')
        ep.require(arm in ep.ARM_IDS, 'value_out_of_vocabulary')
        r = copy.deepcopy(template)
        r.update(route_id='evaluator.private.v1', cell_id='synthetic.evaluator.v1', policy_identity='authored.records.v1',
                 route_status={k:'IMPLEMENTED' if k == 'evaluator.private.v1' else 'NOT_IMPLEMENTED' for k in ep.ROUTE_IDS}, arm_id=arm, planned_case_count=len(self.inventory), outcomes=self.outcomes(arm), analysis_declaration=declaration)
        if arm == 'candidate': r['asymmetric_outcome_table'] = self.asymmetric_outcome_table()
        else: r.pop('asymmetric_outcome_table', None)
        r['counts'] = self.aggregate(arm)
        # These are evaluator-only synthetic observations, never route-native proof.
        r['derivations'] = self.derivations(arm)
        r['not_measured']['metrics'] = sorted(k for k,v in r['derivations'].items() if v == 'not_measured')
        r['gate_results'] = {k:'BLOCKED' if k in ep.BLOCKED_GATES else 'NOT_EVALUABLE' for k in ep.GATE_IDS}
        for k in ('receipt_path_allowlist','vocabulary_closure','stamped_field_separation','outcome_identities','planned_inventory_reconciliation','declaration_gating','claim_scope_present'):
            r['gate_results'][k] = 'PASS'
        if any(r.outcome != 'COMPLETED' for r in self._records[arm].values()):
            r['gate_results']['rejection_is_not_protection'] = 'PASS'
        if 'unknown_egress_lower_bound_cases' in r['counts']:
            r['gate_results']['unknown_egress_lower_bound'] = 'PASS'
        valid = ep.declaration_valid(declaration, {self.inventory.case(k).stratum for k in self.inventory.keys()})
        # Malformed typed declarations are represented as unavailable, never echoed.
        if not valid: r['analysis_declaration'] = None
        r['intervals'] = {m:self.paired_interval(m, declaration) for m in METRIC_FIELDS if m in r['counts']} if arm == 'candidate' else {}
        for m in r['intervals']:
            if r['derivations'][m] == 'observed_subset_lower_bound': r['intervals'][m] = 'NOT_EVALUABLE'
        r['error_codes'] = {}
        r = ep.validate_receipt(r, custody, repo_root=repo_root, attestation_probe=attestation_probe)
        r['local_membership_order_verified'] = True
        r['local_membership_order_proof_method'] = 'private_membership_order_v1'
        r['gate_results']['local_membership_order_proof'] = 'PASS'
        return r
