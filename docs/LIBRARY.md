# Rust library guide

`caexfer` is one Rust package with a library and a CLI. It requires Rust 1.86
or newer. Add the published library with `cargo add caexfer`.

The public library has two file operations:

| Operation | Result |
| --- | --- |
| `caexfer::validate(input, &options)` | Supported-subset counts, source omissions, and a `passed` verdict |
| `caexfer::convert(input, output, &options)` | A newly installed output file and a typed conversion report |

The `core`, `conversion`, and format adapter modules are implementation details.
Both operations infer formats from filename extensions; set `Options.input_format`
when the input suffix is ambiguous. The output suffix chooses the writer.
`Format`, `MshVersion`, `Options`, report types, and `Error` are public support
types. See [format limits](SUPPORT.md) for what each route can carry.

## Validate a file

```rust
use caexfer::{Options, Result, validate};

fn main() -> Result<()> {
    let report = validate("model.bdf", &Options::default())?;
    println!("{} points, {} cells, {} fields", report.points, report.cells, report.fields);
    for omission in &report.omissions {
        eprintln!("Omission: {}", omission.detail);
    }
    Ok(())
}
```

Ordinary validation passes when the source can be projected into caexfer's
supported mesh-and-fields subset, even if the report lists omissions or
assumptions. For OP2/PCH with a non-BDF companion mesh, validation reports an
unverified basic-frame assumption without requiring acceptance. Set
`Options.strict = true` to make any notice set `report.passed = false`.
Parse, I/O, size-limit, and unsafe-projection failures return an `Error`. A
passing report does not validate a full solver model.

## Convert a file

```rust
use caexfer::{Options, Result, convert};

fn main() -> Result<()> {
    let report = convert("results.frd", "results.vtu", &Options {
        accept_omissions: true,
        ..Options::default()
    })?;
    for item in &report.omissions {
        eprintln!("{}: {}", item.stage.name(), item.detail);
    }
    Ok(())
}
```

Conversion writes to an exclusive temporary file beside the destination,
flushes and syncs it, then installs a new destination name. It never overwrites
an existing path. Source, conversion, or acceptance failures leave no output
file. The destination filesystem must support hard links. `Error.code` is the
stable machine-readable failure identifier; message wording is not stable.

The report records source and destination omissions and explicit assumptions.
The default refuses to install a conversion with any unaccepted notice.
`accept_omissions` accepts source and destination losses. A specific assumption
requires its matching option: `assume_basic_frame`, `zero_missing_rotations`, or
`accept_synthetic_zero`. `accept_all` accepts every notice. These options do not
override parsing, coordinate-system, representability, or I/O errors.

## Options for special routes

| Option | Purpose |
| --- | --- |
| `input_format` | Override source extension detection with a `Format` value |
| `mesh` | Matching mesh path for OP2 or PCH input; original node IDs are required |
| `assume_basic_frame` | Accept basic-frame coordinates and displacements for conversion with a non-BDF companion |
| `subcase`, `step` | Select an OP2/PCH result; `step` also selects an FRD or Exodus step |
| `msh_version` | Select MSH 2.2 output; default is 4.1 |
| `max_bytes` | Bound the input and companion mesh reads; default is 256 MiB |
| `zero_missing_rotations` | Accept float zero for missing OP2 R1/R2/R3 components |
| `accept_synthetic_zero` | Accept a labeled all-zero OP2 result from result-free BDF or INP geometry |
| `mesh_output` | Write a separate geometry companion beside OP2 output |

For example, PCH results need a matching BDF mesh and may need result selection:

```rust
use caexfer::{Options, Result, convert};

fn main() -> Result<()> {
    let report = convert("results.pch", "displacements.vtu", &Options {
        mesh: Some("model.bdf".into()),
        subcase: Some(1),
        accept_omissions: true,
        ..Options::default()
    })?;
    println!("{} fields", report.fields);
    Ok(())
}
```

When `mesh_output` is set for OP2 output, both files are staged before either
is installed. The returned `ConversionReport.mesh_output` contains the companion
path, format, and separate omissions. Two file installs cannot be atomic: an
error during the second install can leave the first completed OP2 file in place,
and the error names it.

BDF output and INP output are geometry exchange decks, not complete runnable
solver models. No units are inferred. Keep the source file whenever omitted
solver data, precision, metadata, or frame information matters.
