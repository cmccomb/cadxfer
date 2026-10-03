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

## Conversion formats

All `convert` operations require `--geometry-only`, including routes that carry
supported numeric fields. This acknowledges projection from a native source to
our linear mesh and numeric-field subset. The CLI emits source and destination
omissions. Native `roundtrip` copies bytes unchanged; for BDF, the document is
parsed and rewritten from its retained source buffer. Other formats are copied
as opaque bytes by `roundtrip`.

### VTU

Read/write one ASCII VTK XML `UnstructuredGrid` piece with the seven supported
linear VTK cell types. Float32/Float64 point/cell numeric arrays are read; the
writer uses Float64. Original IDs use UInt64 `nastran_node_id` and
`nastran_element_id` arrays. If absent on input, sequential IDs are assigned.
Zero in `nastran_property_id` means no property ID. The writer retains component
names and optional step/time through caexfer XML attributes. Binary, compressed,
appended, parallel, multi-piece, and `FieldData` layouts are rejected or outside
the reader's scope. This is a bounded XML subset, not a full VTK XML parser.

### Gmsh MSH

Read/write ASCII MSH 4.1 with nonparametric node blocks, the seven supported
linear element families, and complete numeric `NodeData`/`ElementData` blocks.
Node and element tags remain integer IDs. Binary MSH, high-order types,
parametric nodes and partial result blocks fail. Extra sections such as physical
names or entity metadata are reported as omissions by the CLI. MSH field values,
time and step are retained when supplied. A field without step metadata uses
MSH step 0 with an omission note. Per-component names and BDF property IDs do
not have mappings in this exporter.

### Abaqus/CalculiX INP

Read global `*NODE` and supported linear `*ELEMENT, TYPE=...` blocks. Node IDs
and element IDs remain intact. `*PART`, `*ASSEMBLY`, `*INSTANCE`, `*SYSTEM`, and
`*INCLUDE` require expansion/scoping and fail. Other keyword blocks are omitted
from the mesh projection and listed by keyword. The writer emits only nodes and
elements, without properties, materials, sets, loads, steps or results. It is not
a runnable solver deck. `C3D5` is an Abaqus pyramid type; not every solver
accepts every emitted element type.

### CalculiX FRD

Read ASCII short/long fixed-record blocks for supported linear elements and
complete nodal fields. Material-independent results and material-dependent
results with exactly one material per node are supported. Component labels,
field values and available time/step metadata are retained. Binary blocks,
higher-order cells and multiple materials at a node fail. When a field has
multiple steps, select `--step N` for VTU output; MSH can carry multiple blocks.
No FRD writer is supplied.

### Nastran OP2

The OP2 adapter uses an installed pyNastran Python interpreter to decode one
real six-component displacement table. Pass `--mesh model.bdf` and, if needed,
`--python PATH`, `--subcase N`, and zero-based `--step N`. The BDF must project to
a basic-frame mesh, all GRID CD values must be zero, and result node IDs must
match exactly. Complex results, other OP2 tables, nonbasic result frames,
multiple unselected subcases/steps, and embedded-geometry recovery are outside
this route. No OP2 writer is supplied. The included real fixture permits an
optional pyNastran-backed interoperability check.

### Geometry-only BDF output

`convert` can emit basic-frame GRID and linear element cards. It preserves known
property IDs or uses placeholder PID 1 when absent. No property, material,
load, constraint or case-control cards are generated. Use the native BDF
`roundtrip` or `set-grid` commands when source preservation matters.

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
