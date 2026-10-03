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
    state_top, state_bottom = ("#34D058", "#28A745") if percent >= 80 else ("#E05252", "#CB2431")
    badge = f'''<svg xmlns="http://www.w3.org/2000/svg" width="146" height="20" role="img" aria-label="Rust line coverage: {value}">
  <title>Rust line coverage: {covered} of {total} lines ({value})</title>
  <defs>
    <linearGradient id="label-fill" x1="50%" y1="0%" x2="50%" y2="100%">
      <stop stop-color="#444D56" offset="0%"/>
      <stop stop-color="#24292E" offset="100%"/>
    </linearGradient>
    <linearGradient id="state-fill" x1="50%" y1="0%" x2="50%" y2="100%">
      <stop stop-color="{state_top}" offset="0%"/>
      <stop stop-color="{state_bottom}" offset="100%"/>
    </linearGradient>
  </defs>
  <rect width="146" height="20" rx="3" fill="url(#label-fill)"/>
  <path d="M76 0h66.939C144.629 0 146 1.343 146 3v14c0 1.657-1.371 3-3.061 3H76z" fill="url(#state-fill)"/>
  <g font-family="DejaVu Sans,Verdana,Geneva,sans-serif" font-size="11" text-anchor="middle">
    <text x="38" y="15" fill="#010101" fill-opacity=".3" aria-hidden="true">coverage</text>
    <text x="38" y="14" fill="#FFFFFF">coverage</text>
    <text x="111" y="15" fill="#010101" fill-opacity=".3" aria-hidden="true">{value}</text>
    <text x="111" y="14" fill="#FFFFFF">{value}</text>
  </g>
</svg>
'''
    Path(sys.argv[2]).write_text(badge, encoding="utf-8")
    print(f"Rust line coverage: {covered}/{total} = {value}")


if __name__ == "__main__":
    main()
