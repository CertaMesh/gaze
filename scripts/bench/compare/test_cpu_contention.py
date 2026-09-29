"""The measured tool's own threads must not count as machine contention."""

from __future__ import annotations

import unittest

from cpu_contention import foreign_cpu


class ForeignCpuTest(unittest.TestCase):
    def test_excludes_measured_process_tree_and_idle_noise(self) -> None:
        table = """100 1 40.0
101 100 570.0
102 101 35.0
200 1 90.0
201 1 4.9
300 1 45.0
400 100 80.0
"""
        self.assertEqual(foreign_cpu(table, root_pid=100, ignored_pid=400), (135.0, 2))

    def test_threshold_is_strictly_more_than_one_core(self) -> None:
        table = "100 1 500.0\n200 1 100.0\n"
        cpu, count = foreign_cpu(table, root_pid=100)
        self.assertEqual((cpu, count), (100.0, 1))
        self.assertFalse(cpu > 100)


if __name__ == "__main__":
    unittest.main()
