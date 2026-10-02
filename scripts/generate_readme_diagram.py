#!/usr/bin/env python3
"""Generate the README's source-to-output diagram using only Python's stdlib."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from html import escape
from pathlib import Path
from xml.etree import ElementTree


OUTPUT = Path(__file__).resolve().parents[1] / "assets" / "conversion-flow.svg"


@dataclass(frozen=True)
class Outcome:
    y: int
    filename: str
    detail: str
    command: str
    color: str
    icon: str = "lines"


OUTCOMES = (
    Outcome(104, "copy.bdf", "Every original byte preserved", "roundtrip", "#64dec2"),
    Outcome(225, "edited.bdf", "Only selected GRID fields change", "set-grid", "#75bbff"),
    Outcome(346, "mesh.vtu", "Geometry + original IDs", "convert --geometry-only", "#ff9f58", "mesh"),
    Outcome(467, "report.json", "Machine-readable inspection¹", "info model.bdf --json", "#c2a4ff", "json"),
)

PRESENTATION = {
    "title": 'fill="#f6f9ff" font-family="Arial, Helvetica, sans-serif" font-size="32" font-weight="700"',
    "brand": 'fill="#f6f9ff" font-family="Arial, Helvetica, sans-serif" font-size="34" font-weight="700"',
    "subtitle": 'fill="#aebed3" font-family="Arial, Helvetica, sans-serif" font-size="16"',
    "eyebrow": 'fill="#8ea9c6" font-family="Arial, Helvetica, sans-serif" font-size="13" font-weight="700" letter-spacing="2"',
    "filename": 'fill="#f5f8ff" font-family="Arial, Helvetica, sans-serif" font-size="21" font-weight="700"',
    "detail": 'fill="#d1deed" font-family="Arial, Helvetica, sans-serif" font-size="15"',
    "command": 'fill="#9cb0c8" font-family="Menlo, Consolas, monospace" font-size="13"',
    "body": 'fill="#d1deed" font-family="Arial, Helvetica, sans-serif" font-size="16"',
    "note": 'fill="#aebed3" font-family="Arial, Helvetica, sans-serif" font-size="14"',
}


def file_icon(x: int, y: int, color: str, kind: str) -> str:
    contents = {
        "lines": f'<path d="M{x + 10} {y + 26}h24 M{x + 10} {y + 35}h21" stroke="{color}" stroke-width="2.5" stroke-linecap="round"/>',
        "mesh": f'<path d="M{x + 10} {y + 38}l12-15 12 15z M{x + 10} {y + 38}l12-7 12 7 M{x + 22} {y + 23}v8" fill="none" stroke="{color}" stroke-width="1.8" stroke-linejoin="round"/>',
        "json": f'<text x="{x + 8}" y="{y + 37}" fill="{color}" font-family="monospace" font-size="18" font-weight="700">{{ }}</text>',
    }[kind]
    return (
        f'<path d="M{x + 4} {y + 3}h29l11 11v37a4 4 0 0 1-4 4H{x + 4}a4 4 0 0 1-4-4V{y + 7}a4 4 0 0 1 4-4z" '
        f'fill="#17283e" stroke="{color}" stroke-width="2"/>'
        f'<path d="M{x + 33} {y + 3}v11h11" fill="none" stroke="{color}" stroke-width="2"/>'
        + contents
    )


def output_card(item: Outcome) -> str:
    y = item.y
    return f"""
  <g>
    <rect x="842" y="{y}" width="338" height="104" rx="19" fill="#15263c" stroke="#34506b" stroke-width="1.5"/>
    <rect x="842" y="{y}" width="7" height="104" rx="3.5" fill="{item.color}"/>
    {file_icon(861, y + 22, item.color, item.icon)}
    <text x="923" y="{y + 32}" class="filename">{escape(item.filename)}</text>
    <text x="923" y="{y + 59}" class="detail">{escape(item.detail)}</text>
    <text x="923" y="{y + 82}" class="command">{escape(item.command)}</text>
  </g>"""


def render() -> str:
    branch_lines = "\n".join(
        f'<path d="M784 {item.y + 52}H829" stroke="{item.color}" stroke-width="3"/>'
        f'<path d="M829 {item.y + 46}l11 6-11 6z" fill="{item.color}"/>'
        for item in OUTCOMES
    )
    cards = "\n".join(output_card(item).strip() for item in OUTCOMES)
    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" width="1240" height="650" viewBox="0 0 1240 650" role="img" aria-labelledby="title description">
  <title id="title">caxifer: one BDF source, several explicit outputs</title>
  <desc id="description">A model.bdf file enters caxifer. It can become a byte-identical BDF copy, a BDF with selected GRID fields edited, a geometry-only VTU mesh, or JSON inspection output saved from stdout. VTU omits solver data and reports omissions.</desc>
  <defs>
    <linearGradient id="background" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#101b2c"/>
      <stop offset="1" stop-color="#0a1220"/>
    </linearGradient>
    <linearGradient id="accent" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#ffbd55"/>
      <stop offset="1" stop-color="#ff6e3c"/>
    </linearGradient>
  </defs>
  <style>
    .title {{ fill: #f6f9ff; font: 700 32px system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; }}
    .brand {{ fill: #f6f9ff; font: 700 34px system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; }}
    .subtitle {{ fill: #aebed3; font: 16px system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; }}
    .eyebrow {{ fill: #8ea9c6; font: 700 13px system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; letter-spacing: 2px; }}
    .filename {{ fill: #f5f8ff; font: 700 21px system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; }}
    .detail {{ fill: #d1deed; font: 15px system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; }}
    .command {{ fill: #9cb0c8; font: 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }}
    .body {{ fill: #d1deed; font: 16px system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; }}
    .note {{ fill: #aebed3; font: 14px system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; }}
  </style>

  <rect width="1240" height="650" rx="27" fill="url(#background)"/>
  <text x="58" y="58" class="title">One BDF, several explicit outcomes</text>
  <text x="59" y="85" class="subtitle">Preserve the source first. Choose when to edit or project.</text>

  <path d="M340 344H401" stroke="#ffae57" stroke-width="4"/>
  <path d="M401 337l13 7-13 7z" fill="#ffae57"/>
  <path d="M729 344H784 M784 156V519" fill="none" stroke="#58708f" stroke-width="3" stroke-linecap="round"/>
  <circle cx="784" cy="344" r="6" fill="#ffae57"/>
  {branch_lines}

  <g>
    <rect x="58" y="218" width="282" height="251" rx="22" fill="#15263c" stroke="#3a5572" stroke-width="1.5"/>
    <text x="85" y="252" class="eyebrow">SOURCE FILE</text>
    <path d="M90 279h91l25 25v72a7 7 0 0 1-7 7H90a7 7 0 0 1-7-7v-90a7 7 0 0 1 7-7z" fill="#1b3049" stroke="#ffae57" stroke-width="2"/>
    <path d="M181 279v25h25" fill="none" stroke="#ffae57" stroke-width="2"/>
    <path d="M102 357l29-46 30 46z M102 357l30-21 29 21 M131 311l1 25" fill="none" stroke="#ffae57" stroke-width="2" stroke-linejoin="round"/>
    <text x="270" y="322" class="filename" text-anchor="middle">model.bdf</text>
    <text x="270" y="349" class="detail" text-anchor="middle">Nastran deck</text>
    <text x="84" y="419" class="detail">Comments, unknown cards,</text>
    <text x="84" y="442" class="detail">line endings, original bytes</text>
  </g>

  <g>
    <rect x="423" y="185" width="306" height="315" rx="25" fill="#192b42" stroke="#ff9c55" stroke-width="2"/>
    <rect x="423" y="185" width="306" height="8" rx="4" fill="url(#accent)"/>
    <text x="452" y="227" class="eyebrow">DOCUMENT + GEOMETRY</text>
    <text x="452" y="274" class="brand">caxifer</text>
    <text x="452" y="304" class="body">Read and preserve the source</text>
    <path d="M452 326H698" stroke="#38516b" stroke-width="1"/>
    <circle cx="460" cy="357" r="4" fill="#64dec2"/>
    <text x="475" y="363" class="body">Roundtrip exactly</text>
    <circle cx="460" cy="404" r="4" fill="#75bbff"/>
    <text x="475" y="410" class="body">Edit selected GRID fields</text>
    <circle cx="460" cy="451" r="4" fill="#ff9f58"/>
    <text x="475" y="457" class="body">Project geometry by choice</text>
  </g>

  {cards}

  <path d="M58 604H1180" stroke="#30445e" stroke-width="1"/>
  <text x="59" y="628" class="note">¹ Redirect JSON stdout to save report.json. VTU omits solver data; caxifer reports omissions.</text>
</svg>
"""
    for name, attributes in PRESENTATION.items():
        svg = svg.replace(f'class="{name}"', f'class="{name}" {attributes}')
    return svg


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail if the committed SVG is stale")
    args = parser.parse_args()
    svg = render().encode("utf-8")
    ElementTree.fromstring(svg)
    if args.check:
        if not OUTPUT.exists() or OUTPUT.read_bytes() != svg:
            parser.error(f"{OUTPUT} is stale; run this script without --check")
        print(f"up to date: {OUTPUT}")
    else:
        OUTPUT.parent.mkdir(parents=True, exist_ok=True)
        OUTPUT.write_bytes(svg)
        print(f"wrote {OUTPUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
