# CLI contract

`cadxfer --help` lists all commands. Paths can occur before or after options; use
`--` for paths beginning with a hyphen. `--xyz` consumes three values, so negative
coordinates do not need escaping. Filenames use OS-native strings internally;
JSON/human display of non-UTF-8 paths is lossy, not an exact path serialization.

| Command | Result |
| --- | --- |
| `formats` | Actual read/write capabilities, not a roadmap |
| `info INPUT` | Record counts and whether geometry projection is available |
| `validate INPUT` | Geometry-subset diagnostics |
| `roundtrip INPUT OUTPUT` | Accepted source bytes reproduced unchanged |
| `set-grid INPUT OUTPUT --id ID --xyz X Y Z` | Native-frame GRID coordinate edit |
| `convert INPUT OUTPUT.vtu --geometry-only` | Explicit geometry projection |

Native BDF input extensions are `.bdf`, `.nas`, `.dat`, `.pch`, case-insensitive.
`--from bdf` permits another filename. This is explicit selection, not reliable
file-content autodetection. A `.dat` extension alone does not prove a file is BDF.
There is no OP2/FRD reader or VTU reader in this version.

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
