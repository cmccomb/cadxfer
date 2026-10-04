# Artwork

| File | Purpose |
| --- | --- |
| `logo.svg` | Editable CAE Ferris mark used by the repository README. |
| `conversion-flow.svg` | Editable flowchart of format groups and supported conversion directions. |
| `rustacean-flat-happy.svg` | Retained upstream Ferris artwork. |

## CAE Ferris mark

`logo.svg` is the source of truth. It contains vector paths and shapes, with no
embedded raster image or font. The README displays it at 320 pixels wide. Its
`viewBox="0 35 1200 925"` crops the illustration without a wordmark.

The five file cards represent a cube, triangular mesh, gear, mechanical part,
and result graph. Their centers sit 28 degrees apart on a circle centered at
`(620, 650)` with radius `500`. The six arrow shafts use that same circle.

SVG paint order controls the overlaps:

1. The left paper stack covers the start of the incoming arrow.
2. The middle arrows and first four file cards sit above the left stack.
3. Ferris sits behind the right paper stack.
4. The outgoing arrow sits above the right stack. The graph card covers the
   arrow at its start.
5. The laptop covers Ferris's lower body.

The paper stacks and laptop are vector paths traced from generated artwork.
Keep the groups and their order when editing the mark. Check the complete
illustration and a 320-pixel preview on light and dark backgrounds.

## Conversion flowchart

`conversion-flow.svg` groups formats by the roles used under `src/formats/`.
The arrows show reader and writer availability for each group. OP2 is readable
and writable with a companion mesh; PCH is read-only. The diagram describes
supported projections, so it does not promise lossless transfer. Keep its
accessible title and description current when adapter capabilities change.
Review it at full size and at the README's typical displayed width.
The README image URL names a commit so cached branch images do not show an old
diagram. Update that commit in `README.md` after changing the SVG.

## Ferris source and license

`rustacean-flat-happy.svg` is the unmodified Happy Ferris SVG from
[rustacean.net](https://rustacean.net/assets/rustacean-flat-happy.svg), retrieved
October 4, 2026. The site identifies Karen Rustad Tölva as the creator and
publishes Ferris under CC0. `logo.svg` and `conversion-flow.svg` embed Ferris
vector paths. Keep the upstream file for provenance; edit the composed paths
in the relevant SVG.
