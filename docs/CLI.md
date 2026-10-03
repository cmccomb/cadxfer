# CLI contract

Install with `cargo install --git https://github.com/cmccomb/caexfer.git --locked`
(Rust 1.85+). In a checkout, `cargo run --locked -- COMMAND ...` runs the same
CLI without installation. Routes that do not read or write OP2 need no Python.

```sh
caexfer formats
caexfer info model.bdf
caexfer validate model.bdf --json
caexfer convert model.bdf model.vtu --accept-projection
caexfer convert results.op2 results.vtu --mesh model.bdf --accept-projection
caexfer convert results.op2 results.vtu --mesh model.msh \
  --assume-basic-frame --accept-projection
caexfer convert results.frd results.op2 --zero-missing-rotations \
  --mesh-out results-mesh.bdf --accept-projection
```

The OP2 example requires pyNastran in the selected Python interpreter.
Install it in that interpreter (for example, `python3 -m pip install pyNastran`),
then pass `--python PATH` if `python3` is not the right environment.
Use a fresh output filename for each command.

`caexfer --help` lists all commands. Paths can occur before or after options; use
`--` for paths beginning with a hyphen. Filenames use OS-native strings internally;
JSON/human display of non-UTF-8 paths is lossy, not an exact path serialization.

| Command | Result |
| --- | --- |
| `formats` | Actual read/write capabilities, not a roadmap |
| `info INPUT` | Record counts and whether geometry projection is available |
| `validate INPUT` | BDF geometry diagnostics or supported mesh/field subset checks |
| `convert INPUT OUTPUT --accept-projection` | Explicit mesh/field projection to BDF, VTU, MSH, INP, FRD, or OP2 when the source has suitable results |

Input extensions are `.bdf`, `.nas`, `.dat`, `.pch`, `.vtu`, `.msh`, `.inp`,
`.frd`, and `.op2` (case-insensitive). `--from FORMAT` overrides the extension;
this is not content autodetection. Output extension selects the writer.
`--mesh FILE` supplies geometry when reading OP2. It accepts BDF, VTU, MSH,
INP, or FRD with the same node IDs as the displacement table. BDF input
verifies `GRID CD=0`. The other formats do not carry that check, so
`--assume-basic-frame` explicitly asserts that both mesh coordinates and OP2
displacements use the basic frame. Any fields in the companion file are ignored.
OP2 output carries one displacement table and needs a separately retained
matching mesh. `--mesh-out FILE` optionally writes a geometry-only companion
in BDF, VTU, MSH, INP, or FRD format. `--python PATH` selects an interpreter
with pyNastran installed; `CAEXFER_PYTHON` is the fallback. `--subcase N` and
zero-based `--step N` select an OP2 result. For FRD, `--step N` selects the
step number. These selection options also work with `info` and `validate`.
`--zero-missing-rotations` is an OP2-output-only assertion that absent
R1/R2/R3 in a three-component displacement are known float zero. Without it,
that conversion fails rather than filling unknown results.
`--assume-zero-displacement` accepts BDF or INP input and creates a synthetic
static OP2 displacement table with six float zeros per node. It requires
pyNastran, labels the OP2 title as assumed data, and never runs a solver.
For INP, retain the source mesh or use `--mesh-out` to export a companion.

`--json` produces one JSON object on stdout, with schema_version=1. Human-readable
conversion omission reports go to stderr; JSON conversion reports place them in
an `omissions` array with each item's `stage` (`source`, `destination`, or
`assumption`) and detail. A paired export adds `mesh_output` with its path,
format, and separate omissions. JSON error output also goes to stdout. Schema version 1
is intentionally small; consumers should tolerate new keys.

Exit 0 means the requested scoped operation succeeded. `info` succeeds after
native parsing even when geometry cannot be projected; consult its report.
Exit 1 means I/O, parsing, projection, or validation failed. `validate
--strict` also exits 1 on a warning. Exit 2 means invalid CLI usage, including
conversion without `--accept-projection`. Use stable diagnostic `code` values rather
than parsing human descriptions. The version's code list can grow.

The CLI never overwrites an output path and deliberately has no `--force` option
in 0.1.0. Choose a new filename and review the result. File output requires a
filesystem supporting hard links. Library writers can be used with other
persistence policies if the application supplies them deliberately.
For paired output, the CLI stages both files before installing either. Two
different filenames cannot be committed atomically: if installing the second
fails after the OP2 is installed, the error names the OP2 file that remains.
