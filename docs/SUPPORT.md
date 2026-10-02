# 0.1.0 support contract

## Three different promises

1. **Source preservation.** For input that `Document::parse` accepts, writing an
   unchanged document reproduces its bytes exactly. This includes unknown card
   contents, comments, CRLF/LF mixtures, and an absent final newline. It does not
   mean every dialect or malformed file is accepted.
2. **Typed access.** GRID records have a typed view. Blank coordinate fields
   default to zero only in the supported scope. GRDSET or any INCLUDE disables
   typed GRID interpretation because those records may supply defaults. The
   reported coordinates remain in the GRID's native CP frame.
3. **Geometry projection.** Only the geometry subset below is interpreted.
   Unknown geometry, unresolved frames/defaults/includes, and unsupported
   higher-order connectivity fail. The API returns an omission report; the CLI
   additionally requires `--geometry-only`.

These are intentionally not described by one generic `read/write: yes` flag.

## BDF syntax

Small fixed fields are eight bytes wide; large fields are sixteen bytes wide,
with eight-byte keyword/continuation slots. Free fields preserve empty slots.
The parser recognizes commas in the first ten bytes as free format, and `*`
markers as large format. Free large-field physical lines contain four data
fields; ordinary free-field lines contain eight. Missing fields are padded at
physical-line boundaries rather than shifting the next continuation's fields.

Continuation records must be adjacent apart from blank/comment lines. Blank,
`+`, and `*` continuation heads are supported. Explicit trailing/head labels
must match literally, including their prefix. Dialects using numerically
matched labels without the same prefix are outside this version's scope.

Single full-deck `BEGIN BULK` and punch-style input are supported. Executive and
case-control text is retained, not interpreted. Superelement sections and
multiple bulk sections fail. Text after ENDDATA is retained but not interpreted.
INCLUDE text, including quoted multiline paths, is retained; no path is opened.

ASCII data fields are required. `$` comments can contain arbitrary bytes.
UTF-8 BOMs, tab-expanded fields, vendor `#`/`//` comments, replication syntax,
compressed input, and non-adjacent continuation matching are not implemented.
Fixed-format text after column 80 is retained but not interpreted as a field.

Finite Nastran real forms include ordinary decimal/E notation, D notation, and
implicit exponents such as `1.2-3`. NaN and infinity are rejected. Integer
identifiers use positive u64 storage; vendor-specific ID ceilings are not
validated. Full numerical/card-schema validation is not provided.

## Geometry subset

| Source entry | Exported topology | Notes |
| --- | --- | --- |
| GRID | point | CP=0 and SEID=0 required for projection |
| CROD | 2-node line | Explicit PID required |
| CONROD | 2-node line | MID/section data omitted; no property ID |
| CBAR, CBEAM | 2-node line | Endpoint geometry only; orientation/offsets omitted |
| CTRIA3 | 3-node triangle | Section/orientation/thickness attributes omitted |
| CQUAD4 | 4-node quad | Section/orientation/thickness attributes omitted |
| CTETRA | 4-node tetrahedron | Extra node/data fields reject projection |
| CHEXA | 8-node hexahedron | Extra node/data fields reject projection |
| CPENTA | 6-node wedge | Extra node/data fields reject projection |
| CPYRAM | 5-node pyramid | Extra node/data fields reject projection |

Only the literal `CPYRAM` name is recognized, not every vendor alias. Explicit
PID is required where applicable rather than guessing dialect-specific defaults.
Node order for these linear families is retained and mapped to zero-based point
indices. No higher-order node permutations or cell quality repairs are attempted.
Points and cells are emitted in numeric ID order for reproducibility.

Duplicate IDs, missing GRID references, repeated node indices in an element,
invalid numbers, nonbasic CP, nonzero SEID, INCLUDE, and GRDSET are errors.
Collapsed/degenerate solids that intentionally repeat nodes are not supported.
Geometric degeneracy, signed volumes, inversion, Jacobians, physical units,
material validity, and solution correctness are not checked.

A finite allowlist of familiar nongeometry cards is retained but treated as
opaque, including material, property, load, constraint, table, and coordinate
system records. The exact allowlist is `opaque_nongeometry` in `model.rs`.
These records produce warnings and appear in the omission report. A coordinate
system card can be ignored only when every exported GRID explicitly or by the
supported default uses the basic frame; nonzero CP is always an error.
Any card outside the geometry subset and this allowlist blocks projection.

## Editing

`set_grid_coordinates` changes X1/X2/X3 in the native CP frame. It does not
translate the model in global coordinates or update loads/boundaries. The
method requires GRID interpretation to be unambiguous and therefore refuses
decks with INCLUDE or GRDSET. Existing GRID numeric fields must parse.

Only coordinate field byte ranges change. Unknown cards, comments, and all
other fields remain identical. Free-field surrounding whitespace is retained;
fixed fields retain their widths. Fixed-width edits that would require rounding
fail with E_FIELD_WIDTH. Physically omitted/truncated fixed fields are not
expanded. All three coordinate edits commit together after reparsing; any error
leaves the original document unchanged.

## VTU

Write-only ASCII UnstructuredGrid, one Piece, Float64 coordinates, Int64
connectivity/offsets, UInt8 cell types, and UInt64 original IDs. A missing
property ID is encoded as zero, which is invalid as a normal positive PID.
No arbitrary user strings enter XML tags/attributes. No binary/compressed VTU,
parallel files, scalar/vector/tensor result fields, or VTU reader is included.

## Resource and storage limits

The BDF document and its indices are resident in memory. This is not streaming,
lazy, zero-copy-from-disk, or mmap I/O. The original bytes are retained rather
than duplicated into per-field strings, but the parser creates a line index and
field index, and parsing/edits can temporarily duplicate the source buffer.
Defaults: 256 MiB input, 1 MiB per line, two million physical lines, two million
cards, and 65,536 indexed fields per card. These are rejection thresholds, not
a hard total-memory budget. Use OS process limits for hostile or very large data.

The CLI creates a temporary file in the destination directory, flushes/syncs it,
then installs a new destination via a hard link. Existing destinations are never
replaced. A filesystem without hard-link support receives an explicit error;
there is no unsafe fallback. This provides staged no-clobber writes, not a
cross-platform transactional filesystem or guaranteed power-loss durability.
The library's stream writers leave persistence policy to their callers.
