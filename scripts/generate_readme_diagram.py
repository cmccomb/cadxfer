#!/usr/bin/env python3
"""Generate the README's mesh comparison SVG using only Python's stdlib."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from html import escape
from pathlib import Path
from xml.etree import ElementTree


OUTPUT = Path(__file__).resolve().parents[1] / "assets" / "conversion-flow.svg"
BACKGROUND = "#0b1423"
PANEL = "#17283e"
WHITE = "#f5f8ff"
MUTED = "#b6c7d9"
COPY = "#69dfc0"
EDIT = "#77baff"
VTU = "#ffa468"
RECORDS = ("PSHELL", "MAT1", "FORCE", "SPC1")


@dataclass(frozen=True)
class Panel:
    x: int
    step: str
    filename: str
    color: str
    variant: str
    summary: str


PANELS = (
    Panel(50, "01  EXACT COPY", "copy.bdf", COPY, "copy", "Same mesh. Every source byte retained."),
    Panel(437, "02  GRID EDIT", "edited.bdf", EDIT, "edited", "One node moved. Other fields retained."),
    Panel(824, "03  VTU PROJECTION", "mesh.vtu", VTU, "vtu", "Same geometry and IDs; omissions reported."),
)


def text(x: int, y: int, value: str, *, size: int = 16, color: str = WHITE,
         weight: int = 400, mono: bool = False, spacing: int = 0) -> str:
    family = "Menlo, Consolas, monospace" if mono else "Arial, Helvetica, sans-serif"
    return (f'<text x="{x}" y="{y}" fill="{color}" font-family="{family}" '
            f'font-size="{size}" font-weight="{weight}" letter-spacing="{spacing}">'
            f'{escape(value)}</text>')


def mesh(x: int, y: int, variant: str) -> str:
    """Draw one schematic quad mesh; the edited view displaces one GRID node."""
    points: dict[tuple[int, int], tuple[int, int]] = {}
    for row in range(4):
        for col in range(5):
            px, py = x + col * 43 + row * 8, y + row * 34 - col * 4
            if variant == "edited" and (col, row) == (4, 1):
                px += 25
                py -= 11
            points[col, row] = px, py

    color = VTU if variant == "vtu" else (EDIT if variant == "edited" else COPY)
    parts = ['<g aria-hidden="true">']
    for row in range(3):
        for col in range(4):
            corners = (points[col, row], points[col + 1, row],
                       points[col + 1, row + 1], points[col, row + 1])
            coordinates = " ".join(f"{px},{py}" for px, py in corners)
            fill = "#b65d35" if variant == "vtu" and (row + col) % 2 else "#2f4b62"
            opacity = "0.45" if variant == "vtu" else "0.35"
            parts.append(f'<polygon points="{coordinates}" fill="{fill}" fill-opacity="{opacity}" '
                         f'stroke="{color}" stroke-width="1.7" stroke-linejoin="round"/>')
    for (col, row), (px, py) in points.items():
        moved = variant == "edited" and (col, row) == (4, 1)
        parts.append(f'<circle cx="{px}" cy="{py}" r="{6 if moved else 2.8}" '
                     f'fill="{VTU if moved else color}"/>')
    if variant == "edited":
        old_x, old_y = x + 4 * 43 + 8, y + 34 - 4 * 4
        new_x, new_y = points[4, 1]
        parts.append(f'<circle cx="{old_x}" cy="{old_y}" r="7" fill="none" '
                     'stroke="#a9bdd1" stroke-width="1.5" stroke-dasharray="3 3"/>')
        parts.append(f'<path d="M{old_x + 9} {old_y - 2}L{new_x - 8} {new_y + 2}" '
                     f'stroke="{VTU}" stroke-width="2.5" stroke-linecap="round"/>')
        parts.append(f'<circle cx="{new_x}" cy="{new_y}" r="9" fill="none" '
                     f'stroke="{VTU}" stroke-opacity="0.65" stroke-width="2"/>')
    parts.append("</g>")
    return "\n".join(parts)


def record_pills(x: int, y: int, *, color: str, omitted: bool) -> str:
    parts = []
    for index, label in enumerate(RECORDS):
        left = x + index * 78
        stroke = "#71839a" if omitted else color
        parts.append(f'<rect x="{left}" y="{y}" width="70" height="25" rx="7" '
                     f'fill="#1c3048" stroke="{stroke}" stroke-width="1"/>')
        parts.append(text(left + 8, y + 18, label, size=12, color=stroke, weight=700, mono=True))
        if omitted:
            parts.append(f'<path d="M{left + 7} {y + 21}L{left + 63} {y + 4}" '
                         'stroke="#ec7f77" stroke-width="2"/>')
    return "\n".join(parts)


def output_panel(panel: Panel) -> str:
    x, color = panel.x, panel.color
    omitted = panel.variant == "vtu"
    return "\n".join((
        f'<rect x="{x}" y="445" width="365" height="331" rx="22" fill="{PANEL}" '
        'stroke="#385573" stroke-width="1.5"/>',
        f'<rect x="{x}" y="445" width="365" height="7" rx="3.5" fill="{color}"/>',
        text(x + 25, 480, panel.step, size=13, color=color, weight=700, spacing=1),
        text(x + 25, 513, panel.filename, size=24, weight=700),
        mesh(x + 76, 554, panel.variant),
        text(x + 25, 681, "OTHER BDF RECORDS", size=12, color=MUTED, weight=700, spacing=1),
        record_pills(x + 25, 691, color=color, omitted=omitted),
        text(x + 25, 750, panel.summary, size=14, color=MUTED),
    ))


def render() -> str:
    parts = [
        '<svg xmlns="http://www.w3.org/2000/svg" width="1240" height="820" '
        'viewBox="0 0 1240 820" role="img" aria-labelledby="title description">',
        '<title id="title">How caxifer changes a BDF mesh and its surrounding data</title>',
        '<desc id="description">A schematic BDF quad mesh branches into three outputs. The BDF copy has identical geometry and records. The edited BDF moves one mesh node but keeps other records. The VTU retains the original geometry and IDs while solver records are omitted and reported.</desc>',
        '<defs><linearGradient id="background" x1="0" y1="0" x2="1" y2="1">'
        '<stop offset="0" stop-color="#122238"/>'
        f'<stop offset="1" stop-color="{BACKGROUND}"/>'
        '</linearGradient></defs>',
        '<rect width="1240" height="820" rx="26" fill="url(#background)"/>',
        text(50, 58, "The mesh tells the story", size=32, weight=700),
        text(51, 87, "One preserved BDF source. Three different output guarantees.",
             size=16, color=MUTED),
        '<rect x="365" y="122" width="510" height="242" rx="23" '
        f'fill="{PANEL}" stroke="#f4ad5a" stroke-width="2"/>',
        '<rect x="365" y="122" width="510" height="7" rx="3.5" fill="#f4ad5a"/>',
        text(391, 159, "SOURCE DOCUMENT", size=13, color="#f4ad5a", weight=700, spacing=1),
        text(391, 191, "model.bdf", size=25, weight=700),
        mesh(408, 221, "source"),
        text(663, 217, "SOURCE RECORDS", size=12, color=MUTED, weight=700, spacing=1),
        '<rect x="661" y="234" width="188" height="91" rx="12" fill="#20354d"/>',
        text(675, 258, "PSHELL    MAT1", size=14, color="#f6ca87", weight=700, mono=True),
        text(675, 284, "FORCE     SPC1", size=14, color="#f6ca87", weight=700, mono=True),
        text(675, 309, "+ comments, source bytes", size=12, color=MUTED),
        '<path d="M620 364V403 M232 403H1006" fill="none" stroke="#6885a2" '
        'stroke-width="2.5" stroke-linecap="round"/>',
    ]
    for panel in PANELS:
        center = panel.x + 182
        parts.extend((
            f'<path d="M{center} 403V429" stroke="{panel.color}" stroke-width="3"/>',
            f'<path d="M{center - 7} 429l7 12 7-12z" fill="{panel.color}"/>',
            output_panel(panel),
        ))
    parts.extend((
        '<path d="M50 792H1190" stroke="#304761" stroke-width="1"/>',
        text(51, 811, "Schematic mesh: the copy and VTU keep source geometry; only the edited BDF moves a node.",
             size=13, color=MUTED),
        '</svg>',
    ))
    return "\n".join(parts) + "\n"


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
