"""Run isolated freshness guard mutations; restores the exact source even on failure.

Run with the locked benchmark Python environment from the repository root.
Each probe must fail an executed behavior test; logs include the tested commit.
"""
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
SOURCE = HERE / 'freshness.py'
PROBES = [
    ('prerelease', 'return None if version.is_prerelease or version.is_devrelease else version', 'return version', 'test_freshness.py::test_prerelease_only_newer_ignored'),
    ('yanked', 'if not f.get("yanked", False)', 'if True', 'test_freshness.py::test_yanked_release_ignored'),
    ('missing', 'result.update(status="missing", finding="package, repository or pinned revision not found")', 'result.update(status="current", finding=None)', 'test_freshness.py::test_missing_package'),
    ('missing-pin', 'if pinned is None:', 'if False:', 'test_freshness.py::test_missing_pinned_release'),
    ('network', 'result.update(status="network-failure", finding="source check failed; freshness unknown")', 'result.update(status="current", finding=None)', 'test_freshness.py::test_network_failure_visible_nonfatal'),
    ('source-failure', 'result.update(status="source-failure", finding="source metadata unavailable or unsupported; freshness unknown")', 'result.update(status="current", finding=None)', 'test_freshness.py::test_invalid_source_visible_nonfatal'),
    ('incompatible', 'if incompatible and not same:', 'if False:', 'test_freshness.py::test_incompatible_latest_driver_keeps_old_row'),
    ('python', 'if all(f.get("requires_python")', 'if False and all(f.get("requires_python")', 'test_freshness.py::test_incompatible_python'),
    ('github-prerelease', 'if not metadata(item["draft"], bool) and not metadata(item["prerelease"], bool):', 'if True:', 'test_freshness.py::test_github_model_family_and_prerelease'),
    ('model-stability', 'return pinned, None, "unversioned", base', 'return pinned, pinned, None, base', 'test_freshness.py::test_untagged_model_does_not_call_main_stable'),
    ('current', 'elif same:', 'elif False:', 'test_freshness.py::test_prerelease_only_newer_ignored'),
    ('update', 'status="update-unverified", finding=', 'status="current", finding=', 'test_freshness.py::test_stable_newer'),
    ('no-stable', '"no-stable-release", url', '"current", url', 'test_freshness.py::test_no_stable'),
    ('metadata-shape', 'if not isinstance(value, expected):', 'if False:', 'test_freshness_regressions.py::test_pypi_rejects_malformed_release_files'),
    ('commit-identity-shape', 'if re.fullmatch(r"[0-9a-fA-F]{40}", value) is None:', 'if False:', 'test_freshness_regressions.py::test_invalid_commit_identity_is_reportable'),
    ('fetch-protocol', 'except (HTTPException, ValueError, RecursionError):', 'except (ValueError, RecursionError):', 'test_freshness_regressions.py::test_fetch_translates_protocol_and_parse_errors[protocol]'),
    ('fetch-json', 'except (HTTPException, ValueError, RecursionError):', 'except (HTTPException, RecursionError):', 'test_freshness_regressions.py::test_fetch_translates_protocol_and_parse_errors[json]'),
    ('fetch-depth', 'except (HTTPException, ValueError, RecursionError):', 'except (HTTPException, ValueError):', 'test_freshness_regressions.py::test_fetch_translates_protocol_and_parse_errors[deep-json]'),
    ('fetch-size', 'if len(data) > 16 * 1024 * 1024:', 'if False:', 'test_freshness_regressions.py::test_fetch_translates_protocol_and_parse_errors[oversize]'),
    ('injected-protocol', 'SourceFailure, HTTPException, ValueError,', 'SourceFailure, ValueError,', 'test_freshness_regressions.py::test_main_writes_aggregate_with_failed_and_healthy_rows[protocol]'),
    ('pep440-lookup', 'if parsed == Version(row["pinned_version"]):', 'if version == row["pinned_version"]:', 'test_freshness_regressions.py::test_pep440_equivalent_pin_is_current'),
    ('pep440-decision', 'same = Version(latest["version"]) == Version(pinned["version"])', 'same = latest["version"] == pinned["version"]', 'test_freshness_regressions.py::test_pep440_final_comparison_preserves_original_pin'),
    ('github-decision', 'same = latest["commit"] == pinned["commit"]', 'same = latest["version"] == row["pinned_version"]', 'test_freshness_regressions.py::test_github_annotated_tag_identity'),
    ('tag-kind', 'if obj["type"] != "tag" or identity in seen:', 'if identity in seen:', 'test_freshness_regressions.py::test_github_unsupported_tag_object'),
    ('tag-cycle', 'if obj["type"] != "tag" or identity in seen:', 'if obj["type"] != "tag":', 'test_freshness_regressions.py::test_github_tag_cycle_is_reportable'),
    ('tag-depth', 'for _ in range(10):', 'for _ in range(100):', 'test_freshness_regressions.py::test_github_tag_depth_is_bounded'),
    ('hf-decision', 'same = latest["version"] == pinned["version"]', 'same = latest["version"] == row["pinned_version"]', 'test_freshness_regressions.py::test_huggingface_compares_resolved_snapshot'),
]


def main():
    original = SOURCE.read_bytes()
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    if subprocess.check_output(['git', 'status', '--porcelain', '--', str(SOURCE)], cwd=ROOT, text=True):
        raise RuntimeError('commit source before probing')
    logs = ROOT / 'target/freshness-review-probes'
    logs.mkdir(parents=True, exist_ok=True)
    summary = [f'commit {head}']
    try:
        for name, old, new, test in PROBES:
            source = original.decode()
            if old not in source:
                raise RuntimeError(f'probe target missing: {name}')
            SOURCE.write_text(source.replace(old, new, 1))
            run = subprocess.run([sys.executable, '-m', 'pytest', '--color=no', '-q', str(HERE / test)],
                                 cwd=ROOT, capture_output=True, text=True)
            output = run.stdout + run.stderr
            (logs / f'{name}.log').write_text(f'commit {head}\n' + output)
            if run.returncode != 1 or 'FAILED ' not in output or 'ERROR collecting' in output:
                raise AssertionError(f'{name}: mutation not killed by an executed test; see log')
            summary.append(f'{name}: executed behavior test failed as required')
            print(summary[-1], flush=True)
    finally:
        SOURCE.write_bytes(original)
        (logs / 'summary.log').write_text('\n'.join(summary) + '\n')
    print(f'{len(PROBES)}/{len(PROBES)} mutations killed at {head}')


if __name__ == '__main__':
    main()
