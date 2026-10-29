#!/usr/bin/env python3
"""Re-run focused bridge/evidence regressions against removed boundary guards."""
from pathlib import Path
import re
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]


def run(command):
    if command[0] == 'cargo':
        while True:
            pressure = subprocess.check_output(['memory_pressure'], text=True)
            free = re.search(r'free percentage: (\d+)%', pressure)
            if free and int(free[1]) >= 30:
                break
            time.sleep(15)
        command = ['anvil', 'run', '--', 'env', 'CARGO_BUILD_JOBS=4', 'nice', '-n', '10', *command]
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    output = result.stdout + result.stderr
    if command[0] == 'anvil' and not re.search(r'running [1-9]\d* test', output):
        raise RuntimeError('Rust probe did not execute a test:\n' + output[-4000:])
    return result.returncode, output


def main():
    subprocess.run(['git', 'diff', '--exit-code', 'HEAD'], cwd=ROOT, check=True)
    bridge = ['cargo', 'test', '-p', 'gaze-mcp-bridge']
    evidence = ['cargo', 'test', '-p', 'gaze-mcp-rmcp', '--test', 'evidence_route']
    python = ['uv', 'run', '--project', 'scripts/bench', '--python', '3.13', '--locked',
              'python', '-m', 'unittest', 'discover', '-s', 'scripts/bench',
              '-p', 'test_evidence_protocol.py', '-k', 'test_untransformable_refusal_code_is_accepted']
    probes = [
        ('argument-scalar-scan', 'crates/gaze-mcp-bridge/src/bridge.rs',
         '    reject_sensitive_scalars(pipeline, locale_chain, raw_args)?;', '',
         [*bridge, 'labelled_scalar_args_refuse_before_downstream_and_unlabelled_pass']),
        ('result-scalar-scan', 'crates/gaze-mcp-bridge/src/bridge.rs',
         '    reject_sensitive_scalars(pipeline, locale_chain, value)?;', '',
         [*bridge, 'labelled_scalar_results_refuse_and_unlabelled_results_preserve_types']),
        ('typed-redaction-error', 'crates/gaze-mcp-bridge/src/error.rs',
         'gaze::Error::Protection(error) => Self::Protection(error)',
         'gaze::Error::Protection(error) => Self::Redaction(error.to_string())',
         [*bridge, 'redaction_preserves_closed_untransformable_reason']),
        ('two-block-refusal', 'crates/gaze-mcp-rmcp/tests/support/evidence_harness.rs',
         '["untransformable", reason] =>', '["removed-untransformable", reason] =>',
         [*evidence, 'untransformable_refusal_has_closed_value_free_shape']),
        ('evidence-code-vocabulary', 'scripts/bench/evidence_protocol.py',
         "'untransformable', ", '', python),
    ]
    for name, filename, original, mutation, command in probes:
        path = ROOT / filename
        source = path.read_text()
        if source.count(original) != 1:
            raise RuntimeError(f'{name}: mutation anchor must occur exactly once')
        try:
            path.write_text(source.replace(original, mutation, 1))
            code, output = run(command)
            if code == 0:
                raise RuntimeError(f'{name}: mutation survived')
            if 'FAILED' not in output:
                raise RuntimeError(f'{name}: no behavioural failure:\n{output[-4000:]}')
            print(f'KILLED {name}', flush=True)
        finally:
            path.write_text(source)
    for command in ([*bridge, 'labelled_scalar'],
                    [*bridge, 'redaction_preserves_closed_untransformable_reason'],
                    [*evidence, 'untransformable_refusal_has_closed_value_free_shape'], python):
        code, output = run(command)
        if code:
            raise RuntimeError('Restored source failed:\n' + output[-4000:])
    subprocess.run(['git', 'diff', '--exit-code', 'HEAD'], cwd=ROOT, check=True)
    print('PASS: five mutations killed; restored-source regressions pass', flush=True)


if __name__ == '__main__':
    main()
