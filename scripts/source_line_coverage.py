#!/usr/bin/env python3
"""Gate on distinct Rust source lines in an LLVM LCOV report.

LLVM's summary can count one generic source line in several compiled
instantiations. LCOV DA records identify physical file/line pairs, so these
are the stable unit for the repository's source-line coverage policy.
"""

import argparse
import json
import sys
from pathlib import Path


def measure(path: Path) -> tuple[int, int]:
    lines: dict[tuple[str, int], bool] = {}
    source: str | None = None
    for record in path.read_text(encoding="utf-8").splitlines():
        if record.startswith("SF:"):
            source = record[3:]
        elif record.startswith("DA:") and source is not None:
            fields = record[3:].split(",")
            if len(fields) < 2:
                raise ValueError(f"invalid LCOV line record: {record}")
            number, hits = int(fields[0]), int(fields[1])
            if number < 1 or hits < 0:
                raise ValueError(f"invalid LCOV line record: {record}")
            if Path(source).suffix == ".rs" and "src" in Path(source).parts:
                key = (source, number)
                lines[key] = lines.get(key, False) or hits > 0
        elif record == "end_of_record":
            source = None
    if not lines:
        raise ValueError("LCOV report has no Rust source lines")
    return sum(lines.values()), len(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("lcov", type=Path)
    parser.add_argument("summary", type=Path)
    parser.add_argument("--min-percent", type=float, default=95.0)
    args = parser.parse_args()
    try:
        covered, count = measure(args.lcov)
    except (OSError, ValueError) as error:
        parser.error(str(error))
    percent = 100 * covered / count
    args.summary.write_text(
        json.dumps(
            {
                "metric": "distinct Rust source lines",
                "lines": {"covered": covered, "count": count, "percent": percent},
                "minimum_percent": args.min_percent,
            },
            indent=2,
        ) + "\n",
        encoding="utf-8",
    )
    print(f"Rust source-line coverage: {covered}/{count} = {percent:.2f}%")
    if percent < args.min_percent:
        print(f"below {args.min_percent:.2f}% floor", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
