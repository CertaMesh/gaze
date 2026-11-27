import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('changelog', ROOT / 'scripts/release/changelog.py')
changelog = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(changelog)

BASE = """# Changelog

## [Unreleased]

### Changed

- Legacy changed entry.

### Added

- Legacy added entry.

## [0.17.0] - 2026-10-19

### Fixed

- Old fix.

[Unreleased]: https://github.com/CertaMesh/gaze/compare/v0.17.0...HEAD
[0.17.0]: https://github.com/CertaMesh/gaze/compare/v0.16.0...v0.17.0
"""


class ChangelogFragmentsTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        (self.root / 'changelog.d').mkdir()
        (self.root / 'changelog.d/README.md').write_text('# Fragments\n')
        (self.root / 'CHANGELOG.md').write_text(BASE)

    def tearDown(self):
        self.tmp.cleanup()

    def fragment(self, name, text):
        (self.root / 'changelog.d' / name).write_text(text)

    def test_assemble_merges_into_existing_sections_in_order(self):
        self.fragment('b-pr.added.md', '- Second added,\n  wrapped.\n')
        self.fragment('a-pr.added.md', '- First added.\n')
        self.fragment('c-pr.security.md', '- A security fix.\n')
        self.assertEqual(changelog.assemble(self.root, '1.0.0', '2026-12-01'), 3)
        text = (self.root / 'CHANGELOG.md').read_text()
        self.assertIn(
            '## [Unreleased]\n\n## [1.0.0] - 2026-12-01\n\n### Changed\n\n- Legacy changed entry.\n\n'
            '### Added\n\n- Legacy added entry.\n- First added.\n- Second added,\n  wrapped.\n\n'
            '### Security\n\n- A security fix.\n\n## [0.17.0] - 2026-10-19\n', text)
        self.assertIn('[Unreleased]: https://github.com/CertaMesh/gaze/compare/v1.0.0...HEAD\n'
                      '[1.0.0]: https://github.com/CertaMesh/gaze/compare/v0.17.0...v1.0.0\n'
                      '[0.17.0]:', text)
        self.assertEqual([p.name for p in (self.root / 'changelog.d').iterdir()], ['README.md'])

    def test_assemble_refuses_existing_version_and_keeps_fragments(self):
        self.fragment('pr.fixed.md', '- Fix.\n')
        with self.assertRaisesRegex(ValueError, 'already has a section'):
            changelog.assemble(self.root, '0.17.0', '2026-12-01')
        self.assertTrue((self.root / 'changelog.d/pr.fixed.md').exists())
        self.assertEqual((self.root / 'CHANGELOG.md').read_text(), BASE)

    def test_new_section_keeps_keep_a_changelog_order(self):
        self.fragment('pr.deprecated.md', '- Deprecated thing.\n')
        changelog.assemble(self.root, '1.0.0', '2026-12-01')
        text = (self.root / 'CHANGELOG.md').read_text()
        # Legacy order is Changed, Added; Deprecated goes before neither, after both.
        self.assertIn('- Legacy added entry.\n\n### Deprecated\n\n- Deprecated thing.\n\n## [0.17.0]', text)
        self.fragment('pr.added.md', '- Added thing.\n')
        legacy = BASE.replace('### Changed', '### Security').replace(
            '### Added\n\n- Legacy added entry.\n\n', '')
        (self.root / 'CHANGELOG.md').write_text(legacy)
        changelog.assemble(self.root, '1.0.0', '2026-12-01')
        text = (self.root / 'CHANGELOG.md').read_text()
        self.assertIn('## [1.0.0] - 2026-12-01\n\n### Added\n\n- Added thing.\n\n'
                      '### Security\n\n- Legacy changed entry.\n\n## [0.17.0]', text)

    def test_prerelease_keeps_fragments_for_the_final_release(self):
        self.fragment('pr.fixed.md', '- Fix.\n')
        changelog.assemble(self.root, '1.0.0-rc.1', '2026-12-01')
        self.assertTrue((self.root / 'changelog.d/pr.fixed.md').exists())
        changelog.assemble(self.root, '1.0.0', '2026-12-08')
        text = (self.root / 'CHANGELOG.md').read_text()
        final = text[text.index('## [1.0.0] -'):text.index('## [1.0.0-rc.1]')]
        for entry in ['- Fix.', '- Legacy changed entry.', '- Legacy added entry.']:
            self.assertIn(entry, final)
            self.assertEqual(text.count(entry), 2)
        self.assertTrue(text.startswith('# Changelog\n\n## [Unreleased]\n\n## [1.0.0] - 2026-12-08\n'))
        self.assertIn('[1.0.0]: https://github.com/CertaMesh/gaze/compare/v1.0.0-rc.1...v1.0.0', text)
        self.assertFalse((self.root / 'changelog.d/pr.fixed.md').exists())

    def test_release_check_refuses_pending_fragments_except_prereleases(self):
        self.fragment('pr.fixed.md', '- Fix.\n')
        original = changelog.ROOT
        changelog.ROOT = self.root
        try:
            self.assertEqual(changelog.main(['check', '--release', '1.0.0-rc.1']), 0)
            self.assertEqual(changelog.main(['check', '--release', '1.0.0']), 1)
        finally:
            changelog.ROOT = original

    def test_missing_compare_link_fails_before_writing(self):
        self.fragment('pr.fixed.md', '- Fix.\n')
        broken = BASE.replace('compare/v0.17.0...HEAD', 'compare/0.17.0...HEAD')
        (self.root / 'CHANGELOG.md').write_text(broken)
        with self.assertRaisesRegex(ValueError, 'compare'):
            changelog.assemble(self.root, '1.0.0', '2026-12-01')
        self.assertEqual((self.root / 'CHANGELOG.md').read_text(), broken)
        self.assertTrue((self.root / 'changelog.d/pr.fixed.md').exists())

    def test_empty_unreleased_directly_before_a_version_heading(self):
        tight = '# Changelog\n\n## [Unreleased]\n## [0.17.0] - 2026-10-19\n\n- Old.\n\n' \
            '[Unreleased]: https://github.com/CertaMesh/gaze/compare/v0.17.0...HEAD\n'
        (self.root / 'CHANGELOG.md').write_text(tight)
        self.fragment('pr.fixed.md', '- Fix.\n')
        changelog.assemble(self.root, '1.0.0', '2026-12-01')
        self.assertEqual((self.root / 'CHANGELOG.md').read_text(),
                         '# Changelog\n\n## [Unreleased]\n\n## [1.0.0] - 2026-12-01\n\n'
                         '### Fixed\n\n- Fix.\n\n## [0.17.0] - 2026-10-19\n\n- Old.\n\n'
                         '[Unreleased]: https://github.com/CertaMesh/gaze/compare/v1.0.0...HEAD\n'
                         '[1.0.0]: https://github.com/CertaMesh/gaze/compare/v0.17.0...v1.0.0\n')

    def test_wrapped_issue_reference_is_not_a_heading(self):
        self.fragment('pr.fixed.md', '- Fix the queue drop reported in\n#934.\n')
        changelog.fragments(self.root)

    def test_malformed_fragments_are_rejected(self):
        for name, text, message in [
            ('pr.misc.md', '- Entry.\n', 'section one of'),
            ('PR.added.md', '- Entry.\n', 'section one of'),
            ('pr.added.md', 'Entry without a bullet.\n', 'list item'),
            ('pr.added.md', '- Entry.\n\n### Added\n', 'headings'),
            ('pr.added.md', '\n', 'empty'),
        ]:
            with self.subTest(name=name, text=text):
                for path in (self.root / 'changelog.d').glob('pr*'):
                    path.unlink()
                for path in (self.root / 'changelog.d').glob('PR*'):
                    path.unlink()
                self.fragment(name, text)
                with self.assertRaisesRegex(ValueError, message):
                    changelog.fragments(self.root)

    def test_repository_fragments_are_valid(self):
        changelog.fragments(ROOT)


if __name__ == '__main__':
    unittest.main()
