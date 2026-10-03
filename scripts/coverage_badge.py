#!/usr/bin/env python3
"""Render the measured Rust line coverage from cargo-llvm-cov as an SVG badge."""

import json
import sys
from pathlib import Path


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: coverage_badge.py coverage.json coverage.svg")

    report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    lines = report["data"][0]["totals"]["lines"]
    covered, total = lines["covered"], lines["count"]
    if not isinstance(covered, int) or not isinstance(total, int) or not 0 <= covered <= total or total == 0:
        raise SystemExit("coverage report has invalid Rust line totals")

    # The label comes from covered/total, never from the workflow's pass threshold.
    percent = 100 * covered / total
    value = f"{percent:.1f}%"
    color = "#2da44e" if percent >= 80 else "#d73a49"
    badge = f'''<svg xmlns="http://www.w3.org/2000/svg" width="146" height="20" role="img" aria-label="Rust line coverage: {value}">
  <title>Rust line coverage: {covered} of {total} lines ({value})</title>
  <rect width="146" height="20" rx="3" fill="#555"/>
  <path d="M76 0h67a3 3 0 0 1 3 3v14a3 3 0 0 1-3 3H76z" fill="{color}"/>
  <g fill="#fff" text-anchor="middle" font-family="Verdana,Arial,sans-serif" font-size="11">
    <text x="38" y="14">coverage</text>
    <text x="111" y="14">{value}</text>
  </g>
</svg>
'''
    Path(sys.argv[2]).write_text(badge, encoding="utf-8")
    print(f"Rust line coverage: {covered}/{total} = {value}")


if __name__ == "__main__":
    main()
