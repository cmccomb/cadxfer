//! Human-facing command help. Keep option descriptions beside the CLI parser.

/// Show the command hierarchy and the shortest useful next step.
pub(super) const OVERVIEW: &str = "caexfer: exchange finite-element meshes and results

USAGE
  caexfer --help
  caexfer --version
  caexfer --formats
  caexfer COMMAND [OPTIONS]

COMMANDS
  validate  Check supported data, counts, and omissions
  convert   Write a supported projection to a new file

Run 'caexfer COMMAND --help' for options and examples.

GLOBAL OPTIONS
  -h, --help     Show this help
  -V, --version  Show the version
  -f, --formats  List supported formats and read/write capabilities
";

/// Explain validation and when source omissions make it fail.
const VALIDATE: &str = "caexfer validate: check supported data, counts, and omissions

USAGE
  caexfer validate INPUT [OPTIONS]

INPUT OPTIONS
  --from FORMAT         Override the input extension; see 'caexfer --formats'
  --max-bytes N         Limit input size in bytes (default: 268435456 / 256 MiB)
  --mesh FILE           Companion mesh for OP2/PCH results
  --assume-basic-frame  Assert basic frame with a non-BDF companion mesh
  --subcase N           Select an OP2/PCH displacement subcase
  --step N              Select OP2/PCH/Exodus step by zero-based index, or FRD step number

VALIDATION OPTIONS
  --strict              Fail when the source has reported omissions

GENERAL OPTIONS
  --json                Return the report as JSON (schema_version=1)
  --                    Treat following arguments as paths
  -h, --help            Show this command's help

EXAMPLES
  caexfer validate model.bdf
  caexfer validate model.bdf --strict --json
  caexfer validate results.op2 --mesh model.bdf --subcase 1
";

/// Explain conversion in the order users choose source, destination, and receipt.
const CONVERT: &str = "caexfer convert: write a supported projection to a new file

USAGE
  caexfer convert INPUT OUTPUT --accept-projection [OPTIONS]

INPUT OPTIONS
  --from FORMAT              Override the input extension; see 'caexfer --formats'
  --max-bytes N              Limit input size in bytes (default: 268435456 / 256 MiB)
  --mesh FILE                Companion mesh for OP2/PCH results
  --assume-basic-frame       Assert basic frame with a non-BDF companion mesh
  --subcase N                Select an OP2/PCH displacement subcase
  --step N                   Select OP2/PCH/Exodus step by zero-based index, or FRD step number

DESTINATION OPTIONS
  --accept-projection        Required: acknowledge reported omissions and assumptions
  --msh-version 2.2|4.1      MSH output dialect (default: 4.1)
  --mesh-out FILE            Write a companion mesh alongside OP2 output
  --zero-missing-rotations   Assert missing R1/R2/R3 are zero for OP2 output
  --assume-zero-displacement Create synthetic zero OP2 from BDF/INP; no solver runs

GENERAL OPTIONS
  --json                     Return the report as JSON (schema_version=1)
  --                         Treat following arguments as paths
  -h, --help                 Show this command's help

OUTPUT is selected by its extension. Existing files are never overwritten.

EXAMPLES
  caexfer convert model.bdf model.vtu --accept-projection
  caexfer convert results.op2 results.vtu --mesh model.bdf --accept-projection
  caexfer convert model.msh model-2.2.msh --msh-version 2.2 --accept-projection
";

/// Return the help for the selected command; parsing ensures it is recognized.
pub(super) fn for_command(command: &str) -> &'static str {
    match command {
        "validate" => VALIDATE,
        "convert" => CONVERT,
        _ => OVERVIEW,
    }
}
