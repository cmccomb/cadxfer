#!/usr/bin/env python3
"""Checks for the physical source-line coverage denominator."""

import tempfile
import unittest
from pathlib import Path

from source_line_coverage import measure


class SourceLineCoverageTests(unittest.TestCase):
    def test_duplicate_instantiations_count_one_physical_line(self) -> None:
        report = """SF:/repo/src/lib.rs
DA:1,1
DA:1,0
DA:2,0
LF:3
LH:1
end_of_record
SF:/repo/tests/integration.rs
DA:1,1
end_of_record
"""
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "coverage.lcov"
            path.write_text(report, encoding="utf-8")
            self.assertEqual(measure(path), (1, 2))

    def test_empty_rust_source_report_fails(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "coverage.lcov"
            path.write_text("SF:/repo/tests/test.rs\nDA:1,1\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "no Rust source lines"):
                measure(path)


if __name__ == "__main__":
    unittest.main()
