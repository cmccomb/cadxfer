# CLI contract

`caexfer --help` lists all commands. Paths can occur before or after options; use
`--` for paths beginning with a hyphen. `--xyz` consumes three values, so negative
coordinates do not need escaping. Filenames use OS-native strings internally;
JSON/human display of non-UTF-8 paths is lossy, not an exact path serialization.

| Command | Result |
| --- | --- |
| `formats` | Actual read/write capabilities, not a roadmap |
| `info INPUT` | Record counts and whether geometry projection is available |
| `validate INPUT` | BDF geometry diagnostics or supported mesh/field subset checks |
| `roundtrip INPUT OUTPUT` | Source bytes reproduced unchanged (only BDF parses first) |
| `set-grid INPUT OUTPUT --id ID --xyz X Y Z` | Native-frame GRID coordinate edit |
| `convert INPUT OUTPUT --geometry-only` | Explicit mesh/field projection to BDF, VTU, MSH or INP |

Input extensions are `.bdf`, `.nas`, `.dat`, `.pch`, `.vtu`, `.msh`, `.inp`,
`.frd`, and `.op2` (case-insensitive). `--from FORMAT` overrides the extension;
this is not content autodetection. Output extension selects the writer.
`--mesh BDF` supplies geometry for OP2. `--python PATH` selects an interpreter
with pyNastran installed; `CAEXFER_PYTHON` is the fallback. `--subcase N` and
zero-based `--step N` select an OP2 result. For FRD, `--step N` selects the
step number. These selection options also work with `info` and `validate`.

`--json` produces one JSON object on stdout, with schema_version=1. Human-readable
conversion omission reports go to stderr; JSON conversion reports place them in
an `omissions` array. JSON error output also goes to stdout. Schema version 1
is intentionally small; consumers should tolerate new keys.

Exit 0 means the requested scoped operation succeeded. `info` succeeds after
native parsing even when geometry cannot be projected; consult its report.
Exit 1 means I/O, parsing, editing, projection, or validation failed. `validate
--strict` also exits 1 on a warning. Exit 2 means invalid CLI usage, including
conversion without `--geometry-only`. Use stable diagnostic `code` values rather
than parsing human descriptions. The version's code list can grow.

The CLI never overwrites an output path and deliberately has no `--force` option
in 0.1.0. Choose a new filename and review the result. File output requires a
filesystem supporting hard links. Library writers can be used with other
persistence policies if the application supplies them deliberately.
