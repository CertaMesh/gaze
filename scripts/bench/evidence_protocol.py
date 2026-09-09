"""Closed aggregate evidence boundary. Private operands must never be rendered."""
from __future__ import annotations

import copy
import json
import math
import re
from pathlib import Path

import gaze_bench_score as legacy

ARM_IDS = frozenset(('base', 'candidate'))
ROUTE_IDS = frozenset(('mcp.rmcp.duplex.v1', 'text.clean_for_bench.v1', 'daemon.jsonl.v1', 'proxy.http.v1', 'structured.core.v1', 'stream.v1', 'session.episode.v1', 'ocr.document.v1'))
ROUTE_STATUSES = frozenset(('IMPLEMENTED', 'NOT_IMPLEMENTED'))
CELL_IDS = frozenset(('synthetic.mcp.core.v1', 'synthetic.mcp.controlled.v1'))
POLICY_IDENTITIES = frozenset(('core.rule_floor.v1', 'controlled.email_only.v1'))
CLAIM_SCOPES = frozenset(('synthetic_harness_capability_only',))
TABLE_IDS = frozenset(('class-commitments-v1',))
OUTCOME_STATES = ('NOT_STARTED', 'COMPLETED', 'FAILED_CLOSED_NO_EGRESS', 'ERROR_PROTOCOL', 'UNKNOWN_EGRESS')
DERIVATIONS = frozenset(('route_native', 'observer_native', 'egress_reconstructed', 'invariant_enforced_not_counted', 'not_applicable_by_construction', 'not_measured'))
COUNTING_GRADES = frozenset(('route_native', 'observer_native', 'egress_reconstructed'))
GATE_RESULTS = frozenset(('PASS', 'FAIL', 'NOT_EVALUABLE', 'BLOCKED'))
METHOD_IDS = frozenset(('grouped_paired_percentile_v1',))
PROOF_METHODS = frozenset(('private_membership_order_v1',))
STRATUM_IDS = frozenset(('synthetic_en', 'synthetic_de'))
FEATURE_GRAPH_IDS = frozenset(('workspace.all_features', 'workspace.default'))
TOOLCHAIN_IDS = frozenset(('rust.workspace_pinned',))
CONTROL_REFUSAL_CODES = frozenset(('invalid-args', 'not-found', 'limit-exceeded', 'backend-unavailable', 'backend-failure', 'internal', 'invalid-session-id', 'auth-denied', 'manifest-persistence-failed', 'redaction-failed', 'response-serialization-failed'))
ERROR_CODES = CONTROL_REFUSAL_CODES
BASIS_IDS = frozenset(('paired_completed', 'full_cell'))
WEIGHTING_IDS = frozenset(('inventory_group',))
MULTIPLICITY_IDS = frozenset(('synthetic_none',))
ATTESTATION_SOURCES = frozenset(('live_repository', 'test_seam'))
METRIC_IDS = frozenset('''gold_occurrences_planned gold_occurrences_surviving_egress gold_occurrences_partially_surviving_egress gold_occurrences_attribution_not_measured gold_bytes_planned gold_bytes_surviving_egress false_positive_occurrences false_positive_bytes protected_leaves leaf_restore_exact leaf_restore_decision_failures tool_invocations manifest_terminal_events unknown_egress_lower_bound_cases egress_token_restore_failures egress_raw_value_mismatches egress_overlapping_clean_spans egress_authorized_range_bounds_invalid egress_authorized_range_non_monotonic egress_clean_bounds_invalid manifest_span_monotonicity_enforced manifest_raw_entry_agreement_enforced observer_leaves_observed observer_leaves_unobserved observer_manifest_spans_observed observer_recognizer_source_events protection_trace_items'''.split())
GATE_IDS = frozenset('''receipt_path_allowlist vocabulary_closure stamped_field_separation outcome_identities planned_inventory_reconciliation rejection_is_not_protection no_payload_positive_observation unknown_egress_lower_bound string_byte_reversibility gold_survival_oracle false_positive_negative_control egress_integrity_analogues source_attribution_events declaration_gating paired_grouped_interval_arithmetic cross_language_vocabulary_equality build_attestation_clean_source local_membership_order_proof producer_membership_order_proof claim_scope_present class_commitment_schema_load manifest_integrity_six_counter_schema_v4 per_token_protection_trace class_commitment_completeness unknown_egress_route_fragment_observation'''.split())
REFUSAL_CODES = frozenset('''unknown_path wrong_type value_out_of_vocabulary non_finite_number missing_mandatory_key stamped_key_in_emitted_receipt outcome_identity_violation derivation_conflict counted_non_measurement handle_shape_invalid membership_order_proof_failed attestation_shape_invalid protocol_identity_mismatch gate_coverage_incomplete duplicate_json_key uncounted_measurable_metric derivation_coverage_incomplete interval_without_declaration lower_bound_reported_as_exact conditional_reported_as_full_cell inventory_conflict declaration_invalid input_limit malformed_json class_commitment_invalid observer_coverage_incomplete'''.split())
LEAK_FAMILY_METRIC_IDS = frozenset(('gold_occurrences_surviving_egress', 'gold_occurrences_partially_surviving_egress', 'gold_bytes_surviving_egress', 'gold_occurrences_attribution_not_measured'))
BLOCKED_GATES = frozenset(('producer_membership_order_proof', 'manifest_integrity_six_counter_schema_v4', 'per_token_protection_trace', 'class_commitment_completeness', 'unknown_egress_route_fragment_observation'))
STAMPED_KEYS = frozenset(('local_membership_order_verified', 'local_membership_order_proof_method', 'build_attestation'))
DECLARATION_FIELDS = frozenset(('confidence_level', 'resample_count', 'seed', 'strata', 'weighting', 'multiplicity_treatment', 'coverage_target', 'acceptance_limit'))

# Every node, including containers, has one rule. Dynamic map keys are closed.
RECEIPT_PATHS = {
    '$': ('object', None),
    '$.protocol_id': ('enum', frozenset(('gaze-evidence',))),
    '$.protocol_version': ('version', None),
    '$.arm_id': ('enum', ARM_IDS), '$.route_id': ('enum', ROUTE_IDS),
    '$.cell_id': ('enum', CELL_IDS), '$.policy_identity': ('enum', POLICY_IDENTITIES),
    '$.claim_scope': ('enum', CLAIM_SCOPES), '$.population_handle': ('handle', None),
    '$.route_status': ('object', ROUTE_IDS), '$.route_status.*': ('enum', ROUTE_STATUSES),
    '$.class_commitment_table': ('object', frozenset(('id', 'version'))),
    '$.class_commitment_table.id': ('enum', TABLE_IDS), '$.class_commitment_table.version': ('positive', None),
    '$.planned_case_count': ('int', None), '$.outcomes': ('object', frozenset(OUTCOME_STATES)), '$.outcomes.*': ('int', None),
    '$.asymmetric_outcome_table': ('object', frozenset(OUTCOME_STATES)),
    '$.asymmetric_outcome_table.*': ('object', frozenset(OUTCOME_STATES)), '$.asymmetric_outcome_table.*.*': ('int', None),
    '$.counts': ('object', METRIC_IDS), '$.counts.*': ('int', None),
    '$.derivations': ('object', METRIC_IDS), '$.derivations.*': ('enum', DERIVATIONS),
    '$.not_measured': ('object', frozenset(('metrics', 'blocked_gates'))),
    '$.not_measured.metrics': ('array', None), '$.not_measured.metrics.[]': ('enum', METRIC_IDS),
    '$.not_measured.blocked_gates': ('array', None), '$.not_measured.blocked_gates.[]': ('enum', GATE_IDS),
    '$.gate_results': ('object', GATE_IDS), '$.gate_results.*': ('enum', GATE_RESULTS),
    '$.error_codes': ('object', ERROR_CODES), '$.error_codes.*': ('int', None),
    '$.analysis_declaration': ('nullable_object', DECLARATION_FIELDS),
    '$.analysis_declaration.confidence_level': ('number', None), '$.analysis_declaration.resample_count': ('positive', None),
    '$.analysis_declaration.seed': ('int', None), '$.analysis_declaration.strata': ('array', None),
    '$.analysis_declaration.strata.[]': ('enum', STRATUM_IDS),
    '$.analysis_declaration.weighting': ('enum', WEIGHTING_IDS), '$.analysis_declaration.multiplicity_treatment': ('enum', MULTIPLICITY_IDS),
    '$.analysis_declaration.coverage_target': ('number', None), '$.analysis_declaration.acceptance_limit': ('number', None),
    '$.intervals': ('object', METRIC_IDS), '$.intervals.*': ('interval', frozenset(('point', 'low', 'high', 'method_id', 'conditional', 'basis'))),
    '$.intervals.*.point': ('number', None), '$.intervals.*.low': ('number', None), '$.intervals.*.high': ('number', None),
    '$.intervals.*.method_id': ('enum', METHOD_IDS), '$.intervals.*.conditional': ('bool', None), '$.intervals.*.basis': ('enum', BASIS_IDS),
}
MANDATORY_KEYS = frozenset(p[2:] for p in RECEIPT_PATHS if p.count('.') == 1) - {'asymmetric_outcome_table'}
VOCABULARIES = {k: v for k, v in dict(globals()).items() if k.endswith(('_IDS', '_CODES', '_STATES', '_STATUSES', '_GRADES', '_SOURCES')) or k in ('DERIVATIONS', 'GATE_RESULTS', 'POLICY_IDENTITIES', 'CLAIM_SCOPES', 'BLOCKED_GATES', 'STAMPED_KEYS', 'DECLARATION_FIELDS')}
VOCABULARIES['RECEIPT_PATHS'] = frozenset(RECEIPT_PATHS)


class ReceiptRefused(ValueError):
    def __init__(self, code):
        # Never stringify the supplied object, including on invalid exception use.
        self.code = code if type(code) is str and code in REFUSAL_CODES else 'wrong_type'
        super().__init__(self.code)


class ClassCommitmentRefused(ReceiptRefused):
    pass


def require(ok, code):
    if not ok:
        raise ReceiptRefused(code)


def _number(v):
    return type(v) in (int, float) and math.isfinite(v)


def walk(node, path='$'):
    require(path in RECEIPT_PATHS, 'unknown_path')
    kind, allowed = RECEIPT_PATHS[path]
    if kind == 'nullable_object' and node is None:
        return
    if kind == 'interval' and node == 'NOT_EVALUABLE':
        return
    if kind in ('object', 'nullable_object', 'interval'):
        require(type(node) is dict, 'wrong_type')
        for key, value in node.items():
            require(type(key) is str, 'wrong_type')
            if allowed is not None:
                require(key in allowed, 'value_out_of_vocabulary')
            child = path + '.' + key
            if child not in RECEIPT_PATHS:
                child = path + '.*'
            walk(value, child)
    elif kind == 'array':
        require(type(node) is list, 'wrong_type')
        for value in node:
            walk(value, path + '.[]')
        require(len(node) == len(set(node)), 'value_out_of_vocabulary')
    elif kind == 'enum':
        require(type(node) is str, 'wrong_type')
        require(node in allowed, 'value_out_of_vocabulary')
    elif kind in ('int', 'positive', 'version'):
        require(type(node) is int, 'wrong_type')
        require(node >= (1 if kind == 'positive' else 0), 'wrong_type')
        if kind == 'version':
            require(node == 1, 'protocol_identity_mismatch')
    elif kind == 'bool':
        require(type(node) is bool, 'wrong_type')
    elif kind == 'number':
        require(type(node) in (int, float), 'wrong_type')
        require(math.isfinite(node), 'non_finite_number')
    elif kind == 'handle':
        require(type(node) is str and re.fullmatch('[0-9a-f]{32,64}', node) is not None, 'handle_shape_invalid')


def declaration_valid(d, strata=None):
    if type(d) is not dict or set(d) != DECLARATION_FIELDS:
        return False
    return (_number(d['confidence_level']) and 0 < d['confidence_level'] < 1
            and type(d['resample_count']) is int and d['resample_count'] > 0
            and type(d['seed']) is int and d['seed'] >= 0
            and type(d['strata']) is list and bool(d['strata'])
            and all(type(s) is str and s in STRATUM_IDS for s in d['strata'])
            and len(d['strata']) == len(set(d['strata']))
            and (strata is None or set(d['strata']) == set(strata))
            and d['weighting'] == 'inventory_group' and d['multiplicity_treatment'] == 'synthetic_none'
            and all(_number(d[k]) and 0 <= d[k] <= 1 for k in ('coverage_target', 'acceptance_limit')))


def check_outcome_identities(r):
    o = r['outcomes']
    require(set(o) == set(OUTCOME_STATES), 'outcome_identity_violation')
    require(sum(o.values()) == r['planned_case_count'], 'outcome_identity_violation')
    require(('asymmetric_outcome_table' in r) == (r['arm_id'] == 'candidate'), 'outcome_identity_violation')
    if r['arm_id'] == 'candidate':
        t = r['asymmetric_outcome_table']
        require(set(t) == set(OUTCOME_STATES) and all(set(row) == set(OUTCOME_STATES) for row in t.values()), 'outcome_identity_violation')
        require(all(sum(t[a][b] for a in OUTCOME_STATES) == o[b] for b in OUTCOME_STATES), 'outcome_identity_violation')


def validate_structure(r):
    require(type(r) is dict, 'wrong_type')
    require(not STAMPED_KEYS.intersection(r), 'stamped_key_in_emitted_receipt')
    walk(r)
    require(MANDATORY_KEYS <= set(r), 'missing_mandatory_key')
    require(set(r['class_commitment_table']) == {'id', 'version'}, 'missing_mandatory_key')
    require(set(r['not_measured']) == {'metrics', 'blocked_gates'}, 'missing_mandatory_key')
    require(set(r['route_status']) == ROUTE_IDS, 'missing_mandatory_key')
    require(r['route_status']['mcp.rmcp.duplex.v1'] == 'IMPLEMENTED' and all(v == 'NOT_IMPLEMENTED' for k,v in r['route_status'].items() if k != 'mcp.rmcp.duplex.v1'), 'value_out_of_vocabulary')
    require(set(r['gate_results']) == GATE_IDS, 'gate_coverage_incomplete')
    check_outcome_identities(r)
    d, c = r['derivations'], r['counts']
    require(set(d) == METRIC_IDS, 'derivation_coverage_incomplete')
    require(all(d[k] in COUNTING_GRADES for k in c), 'counted_non_measurement')
    require(all(k in c for k,v in d.items() if v in COUNTING_GRADES), 'uncounted_measurable_metric')
    require(set(r['not_measured']['metrics']) == {k for k,v in d.items() if v == 'not_measured'}, 'derivation_conflict')
    require(set(r['not_measured']['blocked_gates']) == {k for k,v in r['gate_results'].items() if v == 'BLOCKED'}, 'derivation_conflict')
    require(all(r['gate_results'][k] == 'BLOCKED' for k in BLOCKED_GATES), 'derivation_conflict')
    if c.get('observer_leaves_unobserved', 0):
        require(r['gate_results']['source_attribution_events'] != 'PASS', 'observer_coverage_incomplete')
    for metric, interval in r['intervals'].items():
        require(metric in c, 'counted_non_measurement')
        if interval == 'NOT_EVALUABLE':
            continue
        require(declaration_valid(r['analysis_declaration']), 'interval_without_declaration')
        require(set(interval) == {'point','low','high','method_id','conditional','basis'}, 'missing_mandatory_key')
        require(interval['low'] <= interval['high'], 'wrong_type')
        require(not (r['outcomes']['UNKNOWN_EGRESS'] and metric in LEAK_FAMILY_METRIC_IDS), 'lower_bound_reported_as_exact')
        if interval['basis'] == 'full_cell':
            require(not interval['conditional'] and not r['outcomes']['UNKNOWN_EGRESS'] and not r['outcomes']['NOT_STARTED'], 'conditional_reported_as_full_cell')
        elif r['outcomes']['COMPLETED'] != r['planned_case_count']:
            require(interval['conditional'], 'conditional_reported_as_full_cell')
    return copy.deepcopy(r)


def validate_receipt(payload, custody, *, repo_root=None, attestation_probe=None):
    r = validate_structure(payload)
    require(type(custody) is dict and custody.get('population_handle') == r['population_handle'], 'membership_order_proof_failed')
    if repo_root is not None:
        try:
            meta = legacy.git_metadata(Path(repo_root))
        except Exception:
            raise ReceiptRefused('attestation_shape_invalid') from None
        source = 'live_repository'
    else:
        require(attestation_probe is not None, 'attestation_shape_invalid')
        meta, source = attestation_probe, 'test_seam'
    require(type(meta) is dict and set(meta) == {'revision', 'dirty'}, 'attestation_shape_invalid')
    require(type(meta['revision']) is str and re.fullmatch('[0-9a-f]{40}', meta['revision']) is not None and type(meta['dirty']) is bool, 'attestation_shape_invalid')
    r['build_attestation'] = dict(source_revision=meta['revision'], dirty=meta['dirty'], source=source,
                                  declared_feature_graph_id='workspace.default', declared_toolchain_id='rust.workspace_pinned')
    r['gate_results']['build_attestation_clean_source'] = 'FAIL' if meta['dirty'] else ('PASS' if source == 'live_repository' else 'NOT_EVALUABLE')
    r['gate_results']['local_membership_order_proof'] = 'NOT_EVALUABLE'
    return r


def overall(gates):
    require(type(gates) is dict and set(gates) == GATE_IDS, 'gate_coverage_incomplete')
    require(all(type(v) is str and v in GATE_RESULTS for v in gates.values()), 'value_out_of_vocabulary')
    if 'FAIL' in gates.values():
        return 'FAIL'
    if any(v != 'PASS' for v in gates.values()):
        return 'NOT_EVALUABLE'
    return 'PASS'


def loads_receipt(text):
    require(type(text) is str, 'wrong_type')
    require(len(text) <= 262144, 'input_limit')
    def pairs(items):
        result = {}
        for k,v in items:
            require(k not in result, 'duplicate_json_key')
            result[k] = v
        return result
    try:
        return json.loads(text, object_pairs_hook=pairs, parse_constant=lambda _: (_ for _ in ()).throw(ReceiptRefused('non_finite_number')))
    except ReceiptRefused:
        raise
    except (ValueError, RecursionError):
        raise ReceiptRefused('malformed_json') from None


def load_class_commitments(path):
    try:
        data = loads_receipt(Path(path).read_text())
        schema, rows = data['schema'], data['fixture_rows']
        required = {'taxonomy_id','taxonomy_version','gold_label','region','canonical_class','commitment','partial_scope','designation','rationale','source_ids','model_coverage','validator_applicability','normalization_scope','malformed_rule','stage_veto_attribution'}
        require(data['id'] == 'class-commitments-v1' and data['version'] == 1 and data['scope'] == 'fixture_only_not_a_corpus_commitment', 'class_commitment_invalid')
        require(set(schema['required']) == required and type(rows) is list, 'class_commitment_invalid')
        seen = set()
        for row in rows:
            require(type(row) is dict and set(row) == required, 'class_commitment_invalid')
            pair = (row['gold_label'], row['region'])
            require(pair in {('EMAIL','global'), ('PHONE','de')} and pair not in seen, 'class_commitment_invalid')
            seen.add(pair)
            require(row['canonical_class'] == {'EMAIL':'Email','PHONE':'custom:phone'}[pair[0]], 'class_commitment_invalid')
            for key, values in {'commitment':{'supported','partial','unsupported'}, 'model_coverage':{'covered','gap','unknown'}, 'designation':{'direct','contextual'}, 'validator_applicability':{'not_applicable','not_measured'}, 'normalization_scope':{'exact_utf8'}, 'malformed_rule':{'D1_held'}, 'stage_veto_attribution':{'unknown_not_measured'}}.items():
                require(type(row[key]) is str and row[key] in values, 'class_commitment_invalid')
            require(row['taxonomy_id'] == 'synthetic.v1' and type(row['taxonomy_version']) is int and row['taxonomy_version'] == 1, 'class_commitment_invalid')
            require(all(type(row[k]) is str and row[k] for k in ('partial_scope','rationale')), 'class_commitment_invalid')
            require(type(row['source_ids']) is list and bool(row['source_ids']) and all(type(v) is str and v in {'email.controlled.v1','fixture.phone.owned.v1'} for v in row['source_ids']), 'class_commitment_invalid')
        return rows
    except Exception:
        raise ClassCommitmentRefused('class_commitment_invalid') from None
