#!/usr/bin/env python3
"""Remove each admission binding on a clean committed tree and prove its regression fails.

Run from a prepared Anvil worktree. Logs and the exact baseline SHA are written
under target/. No detector, Cargo command, or benchmark capture is executed.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
MODULE = 'scripts/bench/test_benchmark_admission.py'
SPEC = 'scripts/bench/benchmark_spec.py'
CERT = 'scripts/bench/benchmark_certification.py'
RECORD = 'scripts/bench/scorecard_record.py'
CURRENT = 'scripts/bench/run_no_opf_benchmark.py'
RELEASE = 'scripts/bench/rescore_past_release.py'
HISTORY = 'scripts/bench/benchmark_history.py'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--only", action="append", help="run a named mutation; repeat to select several")
    args = parser.parse_args()
    if subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True).strip():
        raise SystemExit('mutation_requires_clean_committed_tree')
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    optional_source = (ROOT / HISTORY).read_text()
    optional_start = optional_source.index("    for version in set(state['releases']) - selected:")
    optional_end = optional_source.index('    for job in jobs:', optional_start)
    mutations = [
        ('qualified_runtime', SPEC, 'if actual != QUALIFIED_RUNTIME or actual != expected:', 'if False:',
         'test_runtime_drift_refuses_resolve_capture_replay_and_admission', 4),
        ('spec_runtime', SPEC, ' or actual != expected:', ':', 'test_unicode_reviewer_boundary_is_bound_to_runtime', 1),
        ('resolver_runtime', SPEC, '"measuring_runtime": require_runtime(),', '"measuring_runtime": runtime_identity(),',
         'test_runtime_drift_refuses_resolve_capture_replay_and_admission', 4),
        ('current_runtime', CURRENT, '    benchmark_spec.require_runtime()\n', '',
         'test_runtime_drift_refuses_resolve_capture_replay_and_admission', 4),
        ('release_runtime', RELEASE, '    benchmark_spec.require_runtime()\n', '',
         'test_runtime_drift_refuses_resolve_capture_replay_and_admission', 4),
        ('writer_runtime', RECORD, '            benchmark_spec.require_runtime(scorecard["benchmark_spec"])', '            pass',
         'test_writer_refuses_spec_runtime_drift', 1),
        ('replay_runtime', RECORD, '            benchmark_spec.require_runtime(card["benchmark_spec"])', '            pass',
         'test_runtime_drift_refuses_resolve_capture_replay_and_admission', 4),
        ('canonical_descriptors', CERT, "if populations[layer].get('descriptors_sha256') != specs.descriptor_digest(descriptors):",
         'if False:', 'test_canonical_descriptors_bind_every_arm', 28),
        ('canonical_gold_order', SPEC, '"gold": sorted(row["gold"])', '"gold": row["gold"]',
         'test_canonical_gold_order_survives_scored_excluded_partition', 1),
        ('capture_contract', CERT, "            or any(block.get(key) != canonical[key] for key in\n"
         "                   ('id', 'version', 'file_sha256', 'excluded_labels', 'neutral_prediction_classes'))", '',
         'test_reviewer_contract_specific_capture_refuses_even_if_repinned', 2),
        ('capture_header_mode', CERT, "header.get('capture_protocol') != specs.CANONICAL_CAPTURE", 'False',
         'test_missing_producer_mode_cannot_be_inferred_from_v1[header]', 1),
        ('capture_original_mode', CERT, "original.get('capture_protocol') != specs.CANONICAL_CAPTURE", 'False',
         'test_missing_producer_mode_cannot_be_inferred_from_v1[original]', 1),
        ('capture_artifact_mode', CERT, "captured.get('capture_protocol') != specs.CANONICAL_CAPTURE", 'False',
         'test_missing_producer_mode_cannot_be_inferred_from_v1[capture]', 1),
        ('current_record_mode', CURRENT, '        capture_protocol=benchmark_spec.capture_protocol(args),\n', '',
         'test_actual_producers_persist_and_enforce_capture_mode', 6, 'current'),
        ('release_record_mode', RELEASE, '        capture_protocol=benchmark_spec.capture_protocol(args),\n', '',
         'test_actual_producers_persist_and_enforce_capture_mode', 6, 'release'),
        ('current_card_mode', CURRENT, '    candidate["capture_protocol"] = benchmark_spec.capture_protocol(args)\n', '',
         'test_actual_producers_persist_and_enforce_capture_mode', 6, 'current'),
        ('release_card_mode', RELEASE, '    card["capture_protocol"] = benchmark_spec.capture_protocol(args)\n', '',
         'test_actual_producers_persist_and_enforce_capture_mode', 6, 'release'),
        ('cache_capture_mode', CURRENT, '        "capture_protocol": benchmark_spec.capture_protocol(args),\n', '',
         'scripts/bench/test_capture_reuse.py::CaptureIdentityTests::test_runner_key_binds_native_artifacts_and_capture_parameters', 1),
        ('reviewer_manifest_adapter', CERT, "    if (header.get('response_validation') != response_validation\n            or original.get('response_validation') != response_validation\n            or captured.get('response_validation') != response_validation):\n        raise CertificationError('response_validation_mismatch')\n", '', 'test_reviewer_legacy_manifest_runner_cannot_certify', 4),
        ('validation_header', CERT, "header.get('response_validation') != response_validation", 'False', 'test_validation_adapter_required_at_every_boundary[header]', 1),
        ('validation_original', CERT, "original.get('response_validation') != response_validation", 'False', 'test_validation_adapter_required_at_every_boundary[original]', 1),
        ('validation_capture', CERT, "captured.get('response_validation') != response_validation", 'False', 'test_validation_adapter_required_at_every_boundary[capture]', 1),
        ('current_record_validation', CURRENT, '        response_validation=benchmark_spec.response_validation(args),\n', '', 'test_actual_producers_bind_every_validation_flag', 4, 'current'),
        ('current_card_validation', CURRENT, '    candidate["response_validation"] = benchmark_spec.response_validation(args)\n', '', 'test_actual_producers_bind_every_validation_flag', 4, 'current'),
        ('release_record_validation', RELEASE, '        response_validation=benchmark_spec.response_validation(args),\n', '', 'test_actual_producers_bind_every_validation_flag', 5, 'release'),
        ('release_card_validation', RELEASE, '    card["response_validation"] = benchmark_spec.response_validation(args)\n', '', 'test_actual_producers_bind_every_validation_flag', 5, 'release'),
        ('generated_record_validation', 'scripts/bench/agentic_layers.py', '        response_validation=benchmark_spec.response_validation(args),\n', '', 'test_generated_runner_records_validation_flags', 2),
        ('generated_card_validation', 'scripts/bench/agentic_layers.py', '        "response_validation": benchmark_spec.response_validation(args),\n', '', 'test_generated_runner_records_validation_flags', 2),
        ('cache_response_validation', CURRENT, '        "response_validation": benchmark_spec.response_validation(args),\n', '', 'scripts/bench/test_capture_reuse.py::CaptureIdentityTests::test_runner_key_binds_native_artifacts_and_capture_parameters', 1),
        ('split_source_validation', SPEC, 'getattr(args, "split_composite_source_ids", False)', 'False', 'test_generated_runner_records_validation_flags', 1),
        ('release_protocol_declaration', HISTORY, "if expected is None or job.get('response_validation') != expected:", 'if False:', 'test_history_declares_release_protocol', 5),
        ('history_record_validation', HISTORY, "        certification.admit_capture(folder / 'capture.json', record, spec, past.HARNESS_ROOT,\n                                    response_validation=validation)", '        pass', 'test_history_binds_declared_adapter_to_actual_record', 2),
        ('history_cli_validation', HISTORY, 'if specs.response_validation(args) != validation:', 'if False:', 'test_history_measure_refuses_adapter_before_subject', 1),
        ('retained_history_adapter', HISTORY, "\n            row['response_validation'] = release_validation(job)\n", '\n', 'test_history_rows_rederive_validation_adapter[True]', 1),
        ('selected_history_adapter', HISTORY, "\n                row['response_validation'] = release_validation(job)\n", '\n', 'test_history_rows_rederive_validation_adapter[False]', 1),
        ('render_history_adapter', 'scripts/bench/render_benchmark_doc.py', "if validation != benchmark_spec.RELEASE_PROTOCOLS.get(version) or validation is None:", 'if False:', 'test_history_renderer_refuses_wrong_adapter', 1),
        ('optional_history', HISTORY, optional_source[optional_start:optional_end], '',
         'test_default_resume_revalidates_optional_rows_without_backfill', 3),
    ]
    if args.only:
        if set(args.only) - {row[0] for row in mutations}:
            raise SystemExit('unknown_mutation')
        mutations = [row for row in mutations if row[0] in args.only]
    receipts = []
    for name, filename, before, after, selector, failures, *expression in mutations:
        path = ROOT / filename
        original = path.read_bytes()
        assert original.decode().count(before) == 1, name
        if not selector.startswith('scripts/'):
            selector = MODULE + '::' + selector
        command = ['anvil', 'run', '--', 'uv', 'run', '--project', 'scripts/bench', 'python', '-m',
                   'pytest', selector, '-q', '--color=no']
        if expression:
            command += ['-k', expression[0]]
        log = ROOT / 'target' / f'mutation-admission-{name}.log'
        try:
            path.write_text(original.decode().replace(before, after))
            with log.open('w') as output:
                output.write(json.dumps({'baseline': head, 'mutation': name, 'command': command}) + '\n')
                output.flush()
                result = subprocess.run(command, cwd=ROOT, stdout=output, stderr=subprocess.STDOUT)
        finally:
            path.write_bytes(original)
        output = log.read_text()
        assert result.returncode == 1, (name, result.returncode)
        assert re.search(rf'\b{failures} failed\b', output), (name, output[-2000:])
        assert 'AssertionError' in output or 'DID NOT RAISE' in output, name
        assert 'ERROR collecting' not in output and 'no tests ran' not in output, name
        row = dict(mutation=name, expected_failures=failures, exit=result.returncode,
                   log=str(log.relative_to(ROOT)), sha256=hashlib.sha256(log.read_bytes()).hexdigest())
        receipts.append(row)
        print(json.dumps(row), flush=True)
    (ROOT / 'target/mutation-admission-receipts.json').write_text(
        json.dumps(dict(head=head, receipts=receipts), indent=2) + '\n')


if __name__ == '__main__':
    main()
