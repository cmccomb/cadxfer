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
    topology: str
    data: str
    note: str


FORMATS = (
    Card(45, 118, "BDF", "mesh.bdf", "#69dfc0", "bdf", "linear cells", "mesh only", "Native document keeps source bytes"),
    Card(435, 118, "INP", "mesh.inp", "#75d2d6", "inp", "linear cells", "mesh only", "Sets and solver data are omitted"),
    Card(825, 118, "UNV", "mesh.unv", "#c2b4f9", "unv", "linear cells", "mesh only", "Original labels; no pyramids"),
    Card(45, 373, "VTU", "mesh.vtu", "#ffae75", "vtu", "linear cells", "numeric", "Point and cell arrays"),
    Card(435, 373, "VTK legacy", "mesh.vtk", "#9fdaa5", "vtk", "linear cells", "numeric", "ASCII arrays; metadata can be lost"),
    Card(825, 373, "MSH 4.1 / 2.2", "mesh.msh", "#b9a0ff", "msh", "linear cells", "numeric", "Physical groups read; sets not written"),
    Card(45, 628, "STL", "surface.stl", "#efab7c", "stl", "triangles", "none", "IDs assigned on read; binary float32"),
    Card(435, 628, "SU2", "mesh.su2", "#70d7bc", "su2", "cells+markers", "none", "Named, oriented boundary markers"),
    Card(825, 628, "Exodus II", "results.exo", "#e7c675", "exodus", "linear blocks", "scalar+time", "Classic NetCDF-3; complete fields"),
    Card(45, 883, "FRD", "results.frd", "#f28eaa", "frd", "linear cells", "nodal", "ASCII values rounded"),
    Card(435, 883, "OP2", "results.op2", "#f6c76d", "op2", "separate", "DISP only", "Matching mesh and node IDs required"),
    Card(825, 883, "PCH", "results.pch", "#daa8f5", "op2", "separate", "DISP only", "Read-only; matching mesh required"),
)


def text(x: int, y: int, value: str, *, size: int = 16, color: str = WHITE,
         weight: int = 400, mono: bool = False, spacing: int = 0) -> str:
    family = "Menlo, Consolas, monospace" if mono else "Arial, Helvetica, sans-serif"
    return (f'<text x="{x}" y="{y}" fill="{color}" font-family="{family}" '
            f'font-size="{size}" font-weight="{weight}" letter-spacing="{spacing}">'
            f'{escape(value)}</text>')


def mesh(x: int, y: int, accent: str, variant: str) -> str:
    """Show representative topology without implying every format carries it."""
    if variant == "stl":
        return "\n".join((
            '<g aria-hidden="true">',
            f'<path d="M{x} {y + 91}L{x + 77} {y + 3}L{x + 99} {y + 91}Z" '
            f'fill="#37546b" stroke="{accent}" stroke-width="2"/>',
            f'<path d="M{x + 105} {y + 91}L{x + 120} {y + 3}L{x + 195} {y + 91}Z" '
            f'fill="#37546b" stroke="{accent}" stroke-width="2"/>',
            '</g>',
        ))
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
            fill = "#b45e3d" if variant in {"vtu", "exodus"} and (col + row) % 2 else "#37546b"
            opacity = ".45" if variant in {"vtu", "frd", "exodus"} else ".24"
            parts.append(f'<polygon points="{coords}" fill="{fill}" fill-opacity="{opacity}" '
                         f'stroke="{accent}" stroke-width="1.5" stroke-linejoin="round"/>')
    for (col, row), (px, py) in points.items():
        color = ("#ffe2a5" if (col + row) % 3 == 0 else accent) if variant == "frd" else accent
        parts.append(f'<circle cx="{px}" cy="{py}" r="2.6" fill="{color}"/>')
    if variant in {"msh", "unv"}:
        for col, row, label in ((0, 0, "10"), (2, 0, "20"), (4, 3, "30")):
            px, py = points[col, row]
            label_y = py + 13 if row == 0 else py - 7
            parts.append(text(px + 5, label_y, label, size=10, color=WHITE, mono=True))
    if variant == "su2":
        first = points[0, 0]
        last = points[4, 0]
        parts.append(f'<path d="M{first[0]} {first[1]}L{last[0]} {last[1]}" '
                     'stroke="#ffe2a5" stroke-width="5" stroke-linecap="round"/>')
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
        text(right, y + 128, card.topology, size=13, color=WHITE),
        text(right, y + 154, "DATA", size=11, color=DIM, weight=700, spacing=1),
        text(right, y + 176, card.data, size=13, color=card.accent),
        f'<path d="M{x + 24} {y + 189}H{x + 341}" stroke="#38516b"/>',
        text(x + 24, y + 210, card.note, size=13, color=MUTED),
    ))


def render() -> str:
    parts = [
        '<svg xmlns="http://www.w3.org/2000/svg" width="1240" height="1147" '
        'viewBox="0 0 1240 1147" role="img" aria-labelledby="title description">',
        '<title id="title">Twelve engineering format representations</title>',
        '<desc id="description">The cards show BDF, INP, and UNV geometry; '
        'VTU, legacy VTK, MSH, and classic Exodus meshes with supported fields; '
        'STL triangle facets; SU2 boundary markers; FRD nodal results; '
        'and OP2 and PCH displacements requiring separate meshes.</desc>',
        '<defs><linearGradient id="background" x1="0" y1="0" x2="1" y2="1">'
        '<stop offset="0" stop-color="#122238"/>'
        f'<stop offset="1" stop-color="{BACKGROUND}"/>'
        '</linearGradient></defs>',
        '<rect width="1240" height="1147" rx="24" fill="url(#background)"/>',
        text(45, 49, "TWELVE FORMAT REPRESENTATIONS", size=22, weight=700),
        text(46, 76, "Each adapter carries a documented subset of mesh identity, topology, and results.",
             size=15, color=MUTED),
        text(46, 97, "See the format limits for supported routes and reported losses.",
             size=13, color=DIM),
    ]
    parts.extend(card(item) for item in FORMATS)
    parts.extend((
        '<path d="M45 1123H1190" stroke="#304761"/>',
        text(46, 1140, "Schematic only: field availability depends on the source; OP2 and PCH need matching meshes.",
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
