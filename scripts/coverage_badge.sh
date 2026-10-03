#!/usr/bin/env bash
# Render the measured Rust line coverage from cargo-llvm-cov as an SVG badge.
set -euo pipefail

if (( $# != 2 )); then
    echo "usage: bash scripts/coverage_badge.sh coverage.json coverage.svg" >&2
    exit 2
fi

# jq validates the numeric counts before they are interpolated into XML.
if ! totals=$(jq -er '
    .data[0].totals.lines
    | select((.covered | type) == "number" and (.count | type) == "number")
    | select(.covered == (.covered | floor) and .count == (.count | floor))
    | select(.covered >= 0 and .count > 0 and .covered <= .count)
    | "\(.covered) \(.count)"
' "$1"); then
    echo "coverage report has invalid Rust line totals" >&2
    exit 1
fi
read -r covered total <<< "$totals"

export LC_ALL=C
value=$(awk -v covered="$covered" -v total="$total" 'BEGIN {printf "%.1f%%", 100 * covered / total}')
if awk -v covered="$covered" -v total="$total" 'BEGIN {exit (100 * covered / total >= 80) ? 0 : 1}'; then
    state_top='#34D058'
    state_bottom='#28A745'
else
    state_top='#E05252'
    state_bottom='#CB2431'
fi

cat > "$2" <<EOF
<svg xmlns="http://www.w3.org/2000/svg" width="146" height="20" role="img" aria-label="Rust line coverage: $value">
  <title>Rust line coverage: $covered of $total lines ($value)</title>
  <defs>
    <linearGradient id="label-fill" x1="50%" y1="0%" x2="50%" y2="100%">
      <stop stop-color="#444D56" offset="0%"/>
      <stop stop-color="#24292E" offset="100%"/>
    </linearGradient>
    <linearGradient id="state-fill" x1="50%" y1="0%" x2="50%" y2="100%">
      <stop stop-color="$state_top" offset="0%"/>
      <stop stop-color="$state_bottom" offset="100%"/>
    </linearGradient>
  </defs>
  <rect width="146" height="20" rx="3" fill="url(#label-fill)"/>
  <path d="M76 0h66.939C144.629 0 146 1.343 146 3v14c0 1.657-1.371 3-3.061 3H76z" fill="url(#state-fill)"/>
  <g font-family="DejaVu Sans,Verdana,Geneva,sans-serif" font-size="11" text-anchor="middle">
    <text x="38" y="15" fill="#010101" fill-opacity=".3" aria-hidden="true">coverage</text>
    <text x="38" y="14" fill="#FFFFFF">coverage</text>
    <text x="111" y="15" fill="#010101" fill-opacity=".3" aria-hidden="true">$value</text>
    <text x="111" y="14" fill="#FFFFFF">$value</text>
  </g>
</svg>
EOF
echo "Rust line coverage: $covered/$total = $value"
