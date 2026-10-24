import sys
from pathlib import Path
from http.client import IncompleteRead
import pytest
sys.path.insert(0, str(Path(__file__).resolve().parent))
import freshness as f
DAY = '2026-10-24'
ROW = {'key': 'pypi:synthetic-tool@1.0', 'kind': 'pypi', 'name': 'synthetic-tool',
       'pinned_version': '1.0', 'consumers': [], 'driver_constraint': None}

@pytest.mark.parametrize('case', ['truncated', 'malformed'])
def test_one_failed_source_is_reportable(case):
    def get(_):
        if case == 'truncated':
            raise IncompleteRead(b'', 10)
        return {'releases': []}
    result = f.check(ROW, DAY, get)
    assert result['status'] in {'network-failure', 'source-failure'}

def test_pep440_equivalent_pin_is_current():
    assert f.Version('1.0') == f.Version('1.0.0')
    result = f.check(ROW, DAY, lambda _: {'releases': {'1.0.0': [
        {'upload_time_iso_8601': '2026-01-01T00:00:00Z', 'yanked': False}]}})
    assert result['status'] == 'current'

def test_git_commit_equal_to_stable_tag_is_current():
    sha = 'a' * 40
    row = {**ROW, 'kind': 'github', 'name': 'synthetic/repo',
           'pinned_version': sha, 'commit_pin': True, 'tag_prefix': ''}
    def get(url):
        if '/commits/' in url:
            return {'sha': sha, 'commit': {'committer': {'date': '2026-01-01T00:00:00Z'}}}
        if '/git/' in url:
            return {'object': {'type': 'commit', 'sha': sha}}
        return [{'tag_name': 'v1.0.0', 'target_commitish': sha,
                 'published_at': '2026-01-01T00:00:00Z', 'draft': False, 'prerelease': False}]
    assert f.check(row, DAY, get)['status'] == 'current'


@pytest.mark.parametrize('releases', [
    {'1.0.0': [], '1.0': []},
    {'v1.0.0': [], '1.0': []},
])
def test_pep440_final_comparison_preserves_original_pin(releases):
    for version in releases:
        releases[version] = [{'upload_time_iso_8601': '2026-01-01T00:00:00Z'}]
    result = f.check(ROW, DAY, lambda _: {'releases': releases})
    assert result['status'] == 'current'
    assert result['pinned_version'] == '1.0'
    assert result['pinned']['release_date'] == '2026-01-01'


def git_row(commit=True):
    return {**ROW, 'kind': 'github', 'name': 'synthetic/repo',
            'pinned_version': 'a' * 40 if commit else 'v1.0',
            'commit_pin': commit, 'tag_prefix': ''}


def git_get(url):
    if '/commits/' in url:
        return {'sha': 'a' * 40, 'commit': {'committer': {'date': DAY}}}
    if '/git/ref/' in url:
        return {'object': {'type': 'tag', 'sha': 'b' * 40}}
    if '/git/tags/' in url:
        return {'object': {'type': 'commit', 'sha': 'a' * 40}}
    if '/releases/tags/' in url:
        return {'published_at': DAY}
    return [{'tag_name': 'v2.0', 'target_commitish': 'main', 'published_at': DAY,
             'draft': False, 'prerelease': False}]


@pytest.mark.parametrize('commit', [True, False])
def test_github_annotated_tag_identity(commit):
    result = f.check(git_row(commit), DAY, git_get)
    assert result['status'] == 'current'
    assert result['latest_stable'] == {'version': 'v2.0', 'release_date': DAY, 'commit': 'a' * 40}
    assert result['pinned']['commit'] == 'a' * 40
    assert result['pinned_version'] == git_row(commit)['pinned_version']


def test_github_different_commit_is_update():
    def get(url):
        if '/git/tags/' in url:
            return {'object': {'type': 'commit', 'sha': 'c' * 40}}
        return git_get(url)
    assert f.check(git_row(), DAY, get)['status'] == 'update-unverified'


@pytest.mark.parametrize('bad_type', ['tree', 'blob', None, []])
def test_github_unsupported_tag_object(bad_type):
    def get(url):
        if '/git/ref/' in url:
            return {'object': {'type': bad_type, 'sha': 'a' * 40}}
        return git_get(url)
    assert f.check(git_row(), DAY, get)['status'] == 'source-failure'


def test_github_tag_cycle_is_reportable():
    calls = []
    def get(url):
        calls.append(url)
        assert len(calls) <= 4, 'cyclic metadata must not loop'
        if '/git/tags/' in url:
            return {'object': {'type': 'tag', 'sha': 'b' * 40}}
        return git_get(url)
    assert f.check(git_row(), DAY, get)['status'] == 'source-failure'


def test_github_tag_depth_is_bounded():
    calls = []
    def get(url):
        calls.append(url)
        assert len(calls) <= 13, 'tag resolution must stay bounded'
        if '/git/' in url:
            return {'object': {'type': 'tag', 'sha': f'{len(calls):040x}'}}
        return git_get(url)
    assert f.check(git_row(), DAY, get)['status'] == 'source-failure'


def hf_row():
    return {**ROW, 'kind': 'huggingface', 'name': 'synthetic/model', 'pinned_version': 'v1.0'}


def hf_get(url):
    if url.endswith('/refs'):
        return {'tags': [{'name': 'v2.0', 'targetCommit': 'a' * 40}]}
    return [{'id': 'a' * 40, 'date': DAY}]


def test_huggingface_compares_resolved_snapshot():
    result = f.check(hf_row(), DAY, hf_get)
    assert result['status'] == 'current'
    assert result['pinned_version'] == 'v1.0'
    assert result['latest_stable']['tag'] == 'v2.0'


@pytest.mark.parametrize('kind,getter', [('github', git_get), ('huggingface', hf_get)])
@pytest.mark.parametrize('bad_sha', ['', 'main', 'a' * 39, 123])
def test_invalid_commit_identity_is_reportable(kind, getter, bad_sha):
    def get(url):
        data = getter(url)
        if kind == 'github' and '/commits/' in url:
            data['sha'] = bad_sha
        elif kind == 'huggingface' and '/commits/' in url:
            data[0]['id'] = bad_sha
        return data
    assert f.check(git_row() if kind == 'github' else hf_row(), DAY, get)['status'] == 'source-failure'


@pytest.mark.parametrize('row', [ROW, git_row(), hf_row()])
@pytest.mark.parametrize('data', [None, [], {}, 'unexpected', 42])
def test_every_adapter_rejects_malformed_top_level(row, data):
    assert f.check(row, DAY, lambda _: data)['status'] == 'source-failure'


@pytest.mark.parametrize('data', [
    {'releases': []}, {'releases': {'1.0': {}}}, {'releases': {'1.0': [None]}},
    {'releases': {'1.0': [{'upload_time_iso_8601': []}]}},
    {'releases': {'1.0': [{'upload_time_iso_8601': DAY, 'yanked': 'false'}]}},
    {'releases': {'1.0': [{'upload_time_iso_8601': DAY, 'requires_python': []}]}},
])
def test_pypi_rejects_malformed_release_files(data):
    assert f.check(ROW, DAY, lambda _: data)['status'] == 'source-failure'


@pytest.mark.parametrize('url_fragment,data', [
    ('/refs', {'tags': {}}), ('/refs', {'tags': [None]}),
    ('/refs', {'tags': [{'name': []}]}),
    ('/refs', {'tags': [{'name': 'v2.0', 'targetCommit': 'main'}]}),
    ('/commits/', [None]), ('/commits/', [{'id': 'a' * 40, 'date': []}]),
])
def test_huggingface_rejects_malformed_nested_metadata(url_fragment, data):
    def get(url):
        return data if url_fragment in url else hf_get(url)
    assert f.check(hf_row(), DAY, get)['status'] == 'source-failure'


@pytest.mark.parametrize('url_fragment,data', [
    ('/commits/', {'sha': 'a' * 40, 'commit': []}),
    ('/commits/', {'sha': 'a' * 40, 'commit': {'committer': []}}),
    ('/git/ref/', {'object': []}), ('/git/tags/', {'object': []}),
    ('/releases?', [None]),
    ('/releases?', [{'tag_name': [], 'draft': False, 'prerelease': False}]),
    ('/releases?', [{'tag_name': 'v2.0', 'draft': [], 'prerelease': False}]),
])
def test_github_rejects_malformed_nested_metadata(url_fragment, data):
    def get(url):
        return data if url_fragment in url else git_get(url)
    assert f.check(git_row(), DAY, get)['status'] == 'source-failure'


@pytest.mark.parametrize('failure', ['protocol', 'json', 'deep-json', 'oversize'])
def test_fetch_translates_protocol_and_parse_errors(monkeypatch, failure):
    class Response:
        def __enter__(self):
            return self
        def __exit__(self, *_):
            pass
        def read(self, limit):
            if failure == 'protocol':
                raise IncompleteRead(b'private diagnostic', 10)
            if failure == 'json':
                return b'{'
            if failure == 'deep-json':
                return b'[' * 2000
            return b' ' * (limit - 2) + b'{}'
    monkeypatch.setattr(f, 'urlopen', lambda *a, **kw: Response())
    f.fetch.cache_clear()
    with pytest.raises(f.SourceFailure):
        f.fetch('https://example.invalid/metadata')
    f.fetch.cache_clear()


@pytest.mark.parametrize('case', ['protocol', 'shape'])
def test_main_writes_aggregate_with_failed_and_healthy_rows(tmp_path, monkeypatch, case):
    other = {**ROW, 'key': 'pypi:other@1.0', 'name': 'other'}
    monkeypatch.setattr(f, 'inventory', lambda: ([ROW, other], {}))
    def get(url, timeout):
        if '/synthetic-tool/' in url:
            if case == 'protocol':
                raise IncompleteRead(b'private diagnostic', 10)
            return {'releases': []}
        return {'releases': {'1.0': [{'upload_time_iso_8601': DAY}]}}
    monkeypatch.setattr(f, 'fetch', get)
    output = tmp_path / 'inventory.json'
    monkeypatch.setattr(sys, 'argv', ['freshness.py', '--output', str(output)])
    f.main()
    report = __import__('json').loads(output.read_text())
    assert [row['status'] for row in report['rows']] == ['source-failure', 'current']
    assert 'private diagnostic' not in output.read_text()
