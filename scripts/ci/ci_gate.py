"""Conservative PR selection and fail-closed integration status, using only stdlib."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]
FULL_JOBS = {'msrv', 'test-support', 'workspace-shards', 'test-coverage',
             'benchmark-model-free', 'xtask-feature-gates', 'workspace-default', 'dylint'}
ALL_JOBS = FULL_JOBS | {'plan', 'fast', 'test', 'xtask-gates'}
EVENTS = {'pull_request', 'merge_group', 'push', 'workflow_dispatch'}
# Audited prose only. Docs, root Markdown and external fixtures can be Rust
# inputs (including the release scrubber's dynamically followed doc links).
PROSE_ONLY = {'CODE_OF_CONDUCT.md', 'LICENSE-APACHE', 'LICENSE-MIT'}


def workspace():
    manifest = tomllib.loads((ROOT / 'Cargo.toml').read_text())
    members = manifest['workspace']['members']
    manifests = {member: tomllib.loads((ROOT / member / 'Cargo.toml').read_text())
                 for member in members}
    names = {member: data['package']['name'] for member, data in manifests.items()}
    reverse = {name: set() for name in names.values()}
    shared = manifest['workspace']['dependencies']
    for member, data in manifests.items():
        tables = [data] + list(data.get('target', {}).values())
        for table in tables:
            for kind in ['dependencies', 'dev-dependencies', 'build-dependencies']:
                for alias, dependency in table.get(kind, {}).items():
                    if isinstance(dependency, dict):
                        if dependency.get('workspace'):
                            dependency = shared[alias]
                        name = dependency.get('package', alias) if isinstance(dependency, dict) else alias
                    else:
                        name = alias
                    if name in reverse:
                        reverse[name].add(names[member])
    return names, reverse


def make_plan(event, paths):
    if event not in EVENTS:
        raise ValueError(f'unsupported event: {event}')
    names, reverse = workspace()
    paths = list(paths)
    if not all(isinstance(path, str) and path for path in paths):
        raise ValueError('changed paths must be nonempty strings')
    integration = event != 'pull_request'
    wide = integration or not paths
    selected = set()
    bench = dylint = msrv = integration
    for path in paths:
        if (path.startswith(('.github/', '.cargo/', 'scripts/ci/', 'lint/'))
                or Path(path).name in {'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'}):
            wide = True
            continue
        if path.startswith(('docs/', 'scripts/bench/fixtures/')):
            wide = True
            continue
        if path.startswith('scripts/bench/'):
            bench = True
            if path.endswith('.rs'):
                wide = True
            continue
        member = next((member for member in names if path.startswith(member + '/')), None)
        if member:
            selected.add(names[member])
            msrv = dylint = True
            if '/examples/' in path:
                bench = True
            continue
        if path in PROSE_ONLY:
            continue
        wide = True
    if wide:
        selected = set(names.values())
        msrv = bench = dylint = True
    pending = list(selected)
    while pending:
        for dependent in reverse[pending.pop()] - selected:
            selected.add(dependent)
            pending.append(dependent)
    return dict(integration=integration, wide=wide, packages=sorted(selected),
                msrv=msrv, bench=bench, dylint=dylint)


def outputs(plan):
    return {key: json.dumps(value, separators=(',', ':')) for key, value in plan.items()}


def required_jobs(plan, context):
    if context == 'test':
        return {'test-support', 'workspace-shards', 'test-coverage'} if plan['integration'] else {'fast'}
    if context == 'xtask':
        return {'test', 'xtask-feature-gates', 'workspace-default'} if plan['integration'] else {'test', 'fast'}
    if context != 'integration':
        raise ValueError(f'unknown aggregate: {context}')
    if plan['integration']:
        return FULL_JOBS | {'test', 'xtask-gates'}
    jobs = {'fast', 'test', 'xtask-gates'}
    for flag, job in [('msrv', 'msrv'), ('bench', 'benchmark-model-free'), ('dylint', 'dylint')]:
        if plan[flag]:
            jobs.add(job)
    return jobs


def check(event, needs, context):
    if event not in EVENTS or needs.get('plan', {}).get('result') != 'success':
        raise ValueError('the lane plan is missing or unsuccessful')
    raw = needs['plan'].get('outputs', {})
    plan = {}
    for key in ['integration', 'wide', 'msrv', 'bench', 'dylint', 'packages']:
        try:
            value = json.loads(raw[key])
        except (KeyError, TypeError, ValueError) as error:
            raise ValueError(f'missing or invalid plan output: {key}') from error
        if key == 'packages':
            if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
                raise ValueError('invalid package selection')
        elif type(value) is not bool:
            raise ValueError(f'invalid boolean: {key}')
        plan[key] = value
    if plan['integration'] != (event != 'pull_request'):
        raise ValueError('plan does not match the triggering event')
    required = required_jobs(plan, context) | {'plan'}
    for job in sorted(required | set(needs)):
        result = needs.get(job, {}).get('result')
        if result != 'success' and not (job not in required and result == 'skipped'):
            raise ValueError(f'{context}: {job} must succeed, got {result!r}')
    lane = 'FULL INTEGRATION' if plan['integration'] else 'PR FAST LANE ONLY; full integration has not run'
    return f'{lane}: all mandatory {context} owners succeeded.'


def changed_paths(base, head):
    # A three-dot diff includes the entire stacked branch. Deleted and renamed paths
    # are included; disabling rename detection checks both sides of a rename.
    result = subprocess.check_output(
        ['git', 'diff', '--name-only', '--no-renames', '-z', f'{base}...{head}'], cwd=ROOT)
    return [path.decode('utf-8', errors='strict') for path in result.split(b'\0') if path]


def run_fast(packages):
    known = set(workspace()[0].values())
    if not isinstance(packages, list) or not set(packages) <= known:
        raise ValueError('unknown package in fast-lane selection')
    subprocess.run(['cargo', 'fmt', '--all', '--', '--check'], cwd=ROOT, check=True)
    for pattern in ['test_ci_gate.py', 'test_test_shards.py']:
        subprocess.run(['python3', '-m', 'unittest', 'discover', '-s', 'scripts/ci', '-p', pattern],
                       cwd=ROOT, check=True)
    if not packages:
        print('Documentation-only change: no affected Rust packages for clippy or tests.')
        return
    selection = [arg for package in packages for arg in ['-p', package]]
    subprocess.run(['cargo', 'clippy', '--locked', *selection, '--all-features', '--all-targets',
                    '--', '-D', 'warnings'], cwd=ROOT, check=True)
    subprocess.run(['cargo', 'test', '--locked', *selection, '--all-features',
                    '--no-fail-fast'], cwd=ROOT, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    plan = sub.add_parser('plan')
    plan.add_argument('--event', required=True, choices=sorted(EVENTS))
    plan.add_argument('--base')
    plan.add_argument('--head')
    plan.add_argument('--paths-json', help='Synthetic path sample, using the PR selector')
    aggregate = sub.add_parser('check')
    aggregate.add_argument('--context', choices=['test', 'xtask', 'integration'], required=True)
    sub.add_parser('fast')
    args = parser.parse_args()
    if args.command == 'plan':
        paths = []
        if args.event == 'pull_request':
            paths = json.loads(args.paths_json) if args.paths_json else changed_paths(args.base, args.head)
            if not isinstance(paths, list):
                raise ValueError('changed paths must be an array')
        result = make_plan(args.event, paths)
        print(json.dumps(result, indent=2))
        with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
            for key, value in outputs(result).items():
                output.write(f'{key}={value}\n')
    elif args.command == 'check':
        message = check(os.environ['EVENT_NAME'], json.loads(os.environ['NEEDS_JSON']), args.context)
        print(message)
        if os.environ.get('GITHUB_STEP_SUMMARY'):
            with open(os.environ['GITHUB_STEP_SUMMARY'], 'a') as summary:
                summary.write(message + '\n')
    else:
        run_fast(json.loads(os.environ['PACKAGES_JSON']))


if __name__ == '__main__':
    main()
