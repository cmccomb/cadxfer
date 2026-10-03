#!/usr/bin/env python3
"""Generate the README's format-comparison SVG using only Python's stdlib."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from html import escape
from pathlib import Path
from xml.etree import ElementTree

OUTPUT = Path(__file__).resolve().parents[1] / "assets/conversion-flow.svg"
BACKGROUND = "#0b1423"
PANEL = "#17283e"
WHITE = "#f5f8ff"
MUTED = "#b6c7d9"
DIM = "#758ba3"


@dataclass(frozen=True)
class Card:
    x: int
    y: int
    label: str
    filename: str
    accent: str
    visual: str
    note: str


FORMATS = (
    Card(45, 118, "BDF", "mesh.bdf", "#69dfc0", "bdf", "Solver cards omitted"),
    Card(435, 118, "VTU", "mesh.vtu", "#ffae75", "vtu", "Point and cell arrays"),
    Card(825, 118, "MSH 4.1", "mesh.msh", "#b9a0ff", "msh", "Component labels can be lost"),
    Card(45, 373, "INP", "mesh.inp", "#75d2d6", "inp", "Properties and results omitted"),
    Card(435, 373, "FRD", "results.frd", "#f28eaa", "frd", "ASCII values rounded"),
    Card(825, 373, "OP2", "results.op2", "#f6c76d", "op2", "Optional separate mesh export"),
)


def text(x: int, y: int, value: str, *, size: int = 16, color: str = WHITE,
         weight: int = 400, mono: bool = False, spacing: int = 0) -> str:
    family = "Menlo, Consolas, monospace" if mono else "Arial, Helvetica, sans-serif"
    return (f'<text x="{x}" y="{y}" fill="{color}" font-family="{family}" '
            f'font-size="{size}" font-weight="{weight}" letter-spacing="{spacing}">'
            f'{escape(value)}</text>')


def mesh(x: int, y: int, accent: str, variant: str) -> str:
    """Show the same linear topology in each mesh-bearing representation."""
    points = {}
    for row in range(4):
        for col in range(5):
            px, py = x + col * 37 + row * 7, y + row * 28 - col * 3
            points[col, row] = px, py

    parts = ['<g aria-hidden="true">']
    for row in range(3):
        for col in range(4):
            corners = (points[col, row], points[col + 1, row],
                       points[col + 1, row + 1], points[col, row + 1])
            coords = " ".join(f"{px},{py}" for px, py in corners)
            fill = "#b45e3d" if variant == "vtu" and (col + row) % 2 else "#37546b"
            opacity = ".45" if variant in {"vtu", "frd"} else ".24"
            parts.append(f'<polygon points="{coords}" fill="{fill}" fill-opacity="{opacity}" '
                         f'stroke="{accent}" stroke-width="1.5" stroke-linejoin="round"/>')
    for (col, row), (px, py) in points.items():
        color = ("#ffe2a5" if (col + row) % 3 == 0 else accent) if variant == "frd" else accent
        parts.append(f'<circle cx="{px}" cy="{py}" r="2.6" fill="{color}"/>')
    if variant == "msh":
        for col, row, label in ((0, 0, "10"), (2, 0, "20"), (4, 3, "30")):
            px, py = points[col, row]
            label_y = py + 13 if row == 0 else py - 7
            parts.append(text(px + 5, label_y, label, size=10, color=WHITE, mono=True))
    parts.append("</g>")
    return "\n".join(parts)


def op2_table(x: int, y: int, accent: str) -> str:
    """Align each displacement component under its own centered heading."""
    columns = (58, 82, 106, 130, 154, 178)

    def cell(offset: int, baseline: int, value: str) -> str:
        return f'<text x="{x + offset}" y="{y + baseline}">{value}</text>'

    parts = [
        f'<rect x="{x}" y="{y}" width="195" height="104" rx="10" '
        'fill="#21344b" stroke="#49627b"/>',
        '<g font-family="Menlo, Consolas, monospace" font-size="11" text-anchor="middle">',
        f'<text x="{x + 26}" y="{y + 22}" fill="{MUTED}">ID</text>',
        f'<g fill="{accent}">',
        ''.join(cell(offset, 22, label) for offset, label in zip(columns[:3], ("T1", "T2", "T3"))),
        ''.join(cell(offset, 22, label) for offset, label in zip(columns[3:], ("R1", "R2", "R3"))),
        '</g>',
        f'<path d="M{x + 10} {y + 29}H{x + 185}" stroke="#49627b"/>',
        f'<g fill="{WHITE}">',
    ]
    for baseline, node_id in ((53, "10"), (79, "20")):
        parts.append(cell(26, baseline, node_id))
        parts.append(''.join(cell(offset, baseline, "0") for offset in columns[:3]))
        parts.append(''.join(cell(offset, baseline, "0") for offset in columns[3:]))
    parts.extend(('</g>', '</g>'))
    return "\n".join(parts)


def card(card: Card) -> str:
    x, y = card.x, card.y
    art = (op2_table(x + 25, y + 83, card.accent) if card.visual == "op2"
           else mesh(x + 30, y + 87, card.accent, card.visual))
    right = x + 244
    return "\n".join((
        f'<rect x="{x}" y="{y}" width="365" height="218" rx="20" '
        f'fill="{PANEL}" stroke="#3c5875" stroke-width="1.3"/>',
        f'<rect x="{x + 24}" y="{y + 10}" width="317" height="5" rx="2.5" '
        f'fill="{card.accent}"/>',
        text(x + 24, y + 37, card.label, size=13, color=card.accent, weight=700, spacing=1),
        text(x + 24, y + 68, card.filename, size=23, weight=700),
        art,
        f'<path d="M{x + 231} {y + 82}V{y + 181}" stroke="#3c5875"/>',
        text(right, y + 106, "TOPOLOGY", size=11, color=DIM, weight=700, spacing=1),
        text(right, y + 128, "not stored" if card.visual == "op2" else "same cells",
             size=13, color=WHITE),
        text(right, y + 154, "DATA", size=11, color=DIM, weight=700, spacing=1),
        text(right, y + 176, "DISP only" if card.visual == "op2" else
            "nodal" if card.visual == "frd" else
            "mesh only" if card.visual in {"bdf", "inp"} else "numeric",
             size=13, color=card.accent),
        f'<path d="M{x + 24} {y + 189}H{x + 341}" stroke="#38516b"/>',
        text(x + 24, y + 210, card.note, size=13, color=MUTED),
    ))


def render() -> str:
    parts = [
        '<svg xmlns="http://www.w3.org/2000/svg" width="1240" height="637" '
        'viewBox="0 0 1240 637" role="img" aria-labelledby="title description">',
        '<title id="title">Six engineering format representations</title>',
        '<desc id="description">The cards show BDF and INP geometry decks; '
        'VTU and MSH meshes with numeric fields; '
        'FRD mesh with nodal fields; and an OP2 displacement table with an optional separate mesh. '
        'Mesh-bearing representations share the same topology.</desc>',
        '<defs><linearGradient id="background" x1="0" y1="0" x2="1" y2="1">'
        '<stop offset="0" stop-color="#122238"/>'
        f'<stop offset="1" stop-color="{BACKGROUND}"/>'
        '</linearGradient></defs>',
        '<rect width="1240" height="637" rx="24" fill="url(#background)"/>',
        text(45, 49, "SIX FORMAT REPRESENTATIONS", size=22, weight=700),
        text(46, 76, "Mesh-bearing outputs retain cell topology; carried fields depend on the input.",
             size=15, color=MUTED),
        text(46, 97, "Routes and conditions are specified in the matrix below.",
             size=13, color=DIM),
    ]
    parts.extend(card(item) for item in FORMATS)
    parts.extend((
        '<path d="M45 613H1190" stroke="#304761"/>',
        text(46, 630, "Schematic only: field availability depends on the source; OP2 needs pyNastran and a matching mesh for reading.",
             size=12, color=MUTED),
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
