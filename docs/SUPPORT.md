# 0.1.0 support contract

## Three different promises

1. **Source preservation.** For input that `Document::parse` accepts, writing the
   parsed document reproduces its bytes exactly. This includes unknown card
   contents, comments, CRLF/LF mixtures, and an absent final newline. It does not
   mean every dialect or malformed file is accepted.
2. **Typed access.** GRID records have a typed view. Blank coordinate fields
   default to zero only in the supported scope. GRDSET or any INCLUDE disables
   typed GRID interpretation because those records may supply defaults. The
   reported coordinates remain in the GRID's native CP frame.
3. **Geometry projection.** Only the geometry subset below is interpreted.
   Unknown geometry, unresolved frames/defaults/includes, and unsupported
   higher-order connectivity fail. The API returns an omission report; the CLI
   additionally requires `--accept-projection`.

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

## Conversion formats

All `convert` operations require `--accept-projection`, including routes that carry
supported numeric fields. This acknowledges projection from a native source to
our linear mesh and numeric-field subset. The CLI emits source and destination
omissions. The BDF library `Document` retains source bytes and can write them
unchanged; conversion always works from the supported projection.

### VTU

Read/write one ASCII VTK XML `UnstructuredGrid` piece with the seven supported
linear VTK cell types. Float32/Float64 point/cell numeric arrays are read; the
writer uses Float64. Original IDs use UInt64 `nastran_node_id` and
`nastran_element_id` arrays. If absent on input, sequential IDs are assigned.
Zero in `nastran_property_id` means no property ID. The writer retains component
names and optional step/time through caexfer XML attributes. Binary, compressed,
appended, parallel, multi-piece, and `FieldData` layouts are rejected or outside
the reader's scope. XML tags, attributes, and comments are tokenized before
geometry is read. This is a bounded XML subset, not a full VTK XML parser.

### Legacy VTK

Read/write ASCII legacy VTK `UNSTRUCTURED_GRID` with the seven supported
linear cell types, complete point/cell numeric arrays, and original IDs in
`nastran_node_id` and `nastran_element_id` integer arrays. The reader accepts
classic counted `CELLS` and VTK 5.1 offsets/connectivity layout, plus `FIELD`,
default-lookup `SCALARS`, and `VECTORS` data. If original IDs are absent,
one-based IDs are assigned and reported. A zero `nastran_property_id` means
no property ID. The writer emits version 2.0 syntax and Float64 values.
Component labels and step/time metadata are reported as destination omissions.
Binary data, nondefault lookup tables, structured grids, polydata, and
higher-order cells fail explicitly. CI checks output with VTK 9.6.1 itself.

### Gmsh MSH

Read/write ASCII MSH 4.1 and 2.2 with the seven supported linear element
families and complete numeric `NodeData`/`ElementData` blocks. Input dialect is
detected from `$MeshFormat`; output defaults to 4.1, and `--msh-version 2.2`
selects the flat 2.2 dialect. The 2.2 writer requires node/element IDs within
the signed 32-bit range. 2.2 element tag lists are reported as source omissions;
they are not interpreted as solver regions.
Node and element tags remain integer IDs. Binary MSH, high-order types,
parametric nodes and partial result blocks fail. Extra sections such as physical
names or entity metadata are reported as omissions by the CLI. MSH field values,
time and step are retained when supplied. A field without step metadata uses
MSH step 0 with an omission note. Per-component names and BDF property IDs do
not have mappings in this exporter. The writer declares one discrete entity per
occupied element dimension and classifies nodes on the highest dimension; for a
nodes-only mesh it declares one point entity per node. These entities carry no
physical groups or boundary topology. CI checks that Gmsh can import and resave
mixed-dimension and nodes-only exports without changing IDs or connectivity.

### Abaqus/CalculiX INP

Read global `*NODE` and supported linear `*ELEMENT, TYPE=...` blocks. Node IDs
and element IDs remain intact. `*PART`, `*ASSEMBLY`, `*INSTANCE`, `*SYSTEM`, and
`*INCLUDE`, `*NGEN`, and `*ELGEN` require expansion/scoping and fail. Other
keyword blocks are omitted from the mesh projection and listed by keyword. The
writer emits only nodes and elements, without properties, materials, sets, loads,
steps or results. It is not
a runnable solver deck. `C3D5` is an Abaqus pyramid type; not every solver
accepts every emitted element type.

### CalculiX FRD

Read ASCII short/long fixed-record blocks for supported linear elements and
complete nodal fields. Material-independent results and material-dependent
results with exactly one material per node are supported. Component labels,
field values and available time/step metadata are retained. Binary blocks,
higher-order cells and multiple materials at a node fail. When a field has
multiple steps, select `--step N` for VTU output; MSH can carry multiple blocks.
The FRD writer emits long-format ASCII mesh and complete nodal fields for the
six supported FRD element types. Five-node pyramids, field/component labels
longer than eight ASCII characters, and IDs over ten digits fail explicitly.
Cell-associated fields have no direct FRD nodal meaning and are omitted by the
CLI with a report. Properties have no mesh mapping. ASCII `E12.5` rounds
coordinates and results to six significant digits; the CLI reports this.
Missing result time/step metadata is written as zero and reported. FRD output
does not assert that a solver computed any included values.

### Nastran OP2

The native Rust OP2 adapter decodes one real six-component SORT1 OUGV1
displacement table from 32-bit Fortran records. The tested solver and
independently generated fixtures use little endian records. Pass
`--mesh FILE` with a matching BDF, VTU, VTK, MSH, INP, or FRD mesh and, if needed,
`--subcase N` and zero-based `--step N`. Result node IDs must match the companion mesh exactly;
the OP2 result table cannot verify companion coordinates or connectivity.
A BDF must project to a basic-frame mesh and all GRID CD values must be zero.
The other formats do not encode GRID CD; they require `--assume-basic-frame`,
which explicitly asserts that both coordinates and displacements are in the
basic frame. Companion result fields are ignored. Complex results, other OP2
tables, nonbasic result frames, multiple unselected subcases/steps, and
embedded-geometry recovery are outside
this read route. The OP2 writer accepts one named `DISP`/`DISPLACEMENT` nodal
field with three or six real components. Three-component displacements have
unknown rotations and fail by default. `--zero-missing-rotations` sets R1/R2/R3
to typed float `0.0` only when the caller asserts they are known zero, and
reports the fill. OP2 has six numeric slots, not a typed null marker. The writer
emits a single static or one-time transient MSC-style real displacement table
in Rust. CI independently rereads written files with pyNastran. Values use float32;
nonzero values that would underflow to zero, overflowing values, and node IDs
outside signed 32-bit range fail. The output OP2 contains no mesh, so retain a
matching mesh or use `--mesh-out FILE` to create a geometry-only companion in
BDF, VTU, VTK, MSH, INP, or FRD. The companion carries no result fields; BDF and
INP companions are not runnable solver decks. Each output is staged before
either is installed, and existing output paths are never overwritten. Because
two file installs cannot be atomic, a failed second install can leave the OP2
file in place; the CLI error identifies it. Other fields and a nonzero source
step number are omitted with reports. A mesh-only BDF/INP cannot generate an OP2
result from analysis. With `--assume-zero-displacement`, a BDF or INP mesh can
generate a **synthetic** static table with T1/T2/T3/R1/R2/R3 all set to float
`0.0` at every node. The OP2 title says `CAEXFER ASSUMED ZERO DISPLACEMENT -
NOT SOLVER RESULTS`, and the CLI reports the same assumption. This does not
infer a solution or verify consistency with loads, constraints, or prescribed
motions in the input. INP users may retain the source mesh or export a companion.
BDF synthetic output requires basic-frame GRID CD=0 so caexfer can reread it
with that BDF. A rewritten synthetic OP2 retains its provenance title; other
format projections report that provenance but do not encode it in their fields.
CI also checks the included solver-produced fixture and separately authored
multiple-subcase and multiple-step fixtures.

### Nastran PCH

The native Rust PCH reader accepts ASCII real SORT1 `$DISPLACEMENTS` GRID rows
with three translations and optional three rotations. It accepts one selected
positive subcase and zero-based step; multiple unselected subcases or steps
fail. `$TIME` supplies transient time metadata. A matching companion mesh is
required and GRID IDs must match exactly. The same BDF `GRID CD=0` check and
non-BDF `--assume-basic-frame` assertion as OP2 apply. Titles, subtitles,
labels, and other result blocks are reported as omitted. Complex, modal,
SORT2, superelement, unsupported row layouts, and malformed results fail.
PCH does not embed mesh geometry and has no writer in this release. The
fixture from the MSC reference manual checks the supported row layout; it is
an excerpt, not a complete solver-generated PCH qualification file.

### Geometry-only BDF output

`convert` can emit basic-frame GRID and linear element cards. It preserves known
property IDs or uses placeholder PID 1 when absent. No property, material,
load, constraint or case-control cards are generated. Use the BDF library
`Document::write_to` when source preservation matters.

## Resource and storage limits

The BDF document and its indices are resident in memory. This is not streaming,
lazy, zero-copy-from-disk, or mmap I/O. The original bytes are retained rather
than duplicated into per-field strings, but the parser creates a line index and
field index, and parsing can temporarily duplicate the source buffer.
Defaults: 256 MiB input, 1 MiB per line, two million physical lines, two million
cards, and 65,536 indexed fields per card. These are rejection thresholds, not
a hard total-memory budget. Use OS process limits for hostile or very large data.

The CLI creates a temporary file in the destination directory, flushes/syncs it,
then installs a new destination via a hard link. Existing destinations are never
replaced. A filesystem without hard-link support receives an explicit error;
there is no unsafe fallback. This provides staged no-clobber writes, not a
cross-platform transactional filesystem or guaranteed power-loss durability.
The library's stream writers leave persistence policy to their callers.
