use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::core::{Dataset, Error, Mesh, Result};
use crate::formats::{bdf, exodus, frd, inp, msh, op2, pch, stl, su2, unv, vtk, vtu};

use super::{AssumptionKind, Format, Omission, Options, ReadResult, Stage};

/// Bound non-BDF file reads with both a metadata check and an actual read cap.
/// The second check handles files growing between the metadata and read calls.
fn read_limited(path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
    // Metadata catches ordinary oversize files without allocating for them.
    if std::fs::metadata(path)?.len() > max_bytes as u64 {
        return Err(Error::new(
            "E_LIMIT",
            format!("input exceeds {max_bytes} bytes"),
        ));
    }

    // The extra byte detects a file that grew after the metadata check.
    let mut bytes = Vec::new();
    File::open(path)?
        .take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        return Err(Error::new(
            "E_LIMIT",
            format!("input exceeds {max_bytes} bytes"),
        ));
    }
    Ok(bytes)
}

/// Decode a bounded text input with its format-specific UTF-8 error.
fn read_text(
    path: &Path,
    max_bytes: usize,
    code: &'static str,
    message: &'static str,
) -> Result<String> {
    String::from_utf8(read_limited(path, max_bytes)?).map_err(|_| Error::new(code, message))
}

/// Load a Nastran result companion mesh and report source projection losses.
/// BDF grid output frames are checked; other formats require the caller's
/// explicit basic-frame assertion because that metadata is unavailable.
fn read_result_mesh(
    path: &Path,
    options: &Options,
    result: Format,
) -> Result<(Mesh, Vec<Omission>)> {
    let format = Format::from_input_path(path)?;
    match format {
        Format::Bdf => {
            // BDF exposes GRID output frames, so verify them directly.
            let projection = bdf::mesh::read_from(File::open(path)?, options.max_bytes)?;
            if projection.has_nonbasic_output_frame {
                return Err(Error::new(
                    if result == Format::Pch {
                        "E_PCH"
                    } else {
                        "E_OP2"
                    },
                    "nonbasic GRID CD requires displacement frame transformation",
                ));
            }

            // Keep source losses from the companion deck visible in the
            // result conversion report.
            let omissions = projection
                .omissions
                .into_iter()
                .map(|item| {
                    Omission::new(
                        Stage::Source,
                        format!("BDF {} × {}: {}", item.category, item.count, item.detail),
                    )
                })
                .collect();
            Ok((projection.mesh, omissions))
        }
        Format::Op2 | Format::Pch => Err(Error::new(
            "E_USAGE",
            "result companion mesh must be a mesh-bearing format",
        )),
        _ => {
            // Other mesh formats carry no GRID CD, requiring an explicit
            // caller assertion before pairing them with Nastran displacements.
            if !options.assume_basic_frame {
                return Err(Error::new(
                    "E_USAGE",
                    "non-BDF result mesh lacks GRID CD; set Options.assume_basic_frame only when coordinates and displacements use the basic frame",
                ));
            }

            // A companion supplies geometry only; result-selection options
            // must not be reapplied while reading its mesh-bearing file.
            let mesh_options = Options {
                input_format: None,
                mesh: None,
                assume_basic_frame: false,
                subcase: None,
                step: None,
                ..options.clone()
            };
            let read = read_path(path, &mesh_options)?;
            if read.generated_point_ids {
                return Err(Error::new(
                    "E_USAGE",
                    "result companion mesh has assigned point IDs; original node IDs are required",
                ));
            }

            // A companion contributes geometry only; report ignored fields.
            let mut omissions = read
                .omissions
                .into_iter()
                .map(|item| {
                    Omission::new(
                        item.stage,
                        format!("companion {}: {}", format.name(), item.detail),
                    )
                })
                .collect::<Vec<_>>();
            if !read.dataset.fields.is_empty() {
                omissions.push(Omission::new(
                    Stage::Source,
                    format!(
                        "{} numeric field(s) in companion mesh ignored",
                        read.dataset.fields.len()
                    ),
                ));
            }
            omissions.push(Omission::assumed(
                AssumptionKind::BasicFrame,
                format!("companion {} has no GRID CD; basic-frame coordinates and {} displacements asserted by caller", format.name(), result.name().to_ascii_uppercase()),
            ));
            Ok((read.dataset.mesh, omissions))
        }
    }
}

/// Read a supported source file into a mesh and fields, reporting omitted data.
/// OP2/PCH input requires `options.mesh`. A non-BDF
/// companion additionally requires `options.assume_basic_frame`.
///
/// # Errors
///
/// Returns an option or format error, an I/O or size-limit error, or a source
/// reader error when the requested projection cannot be represented safely.
///
#[allow(clippy::too_many_lines)] // Format-specific read branches share projection reporting.
pub fn read_path(path: &Path, options: &Options) -> Result<ReadResult> {
    // An explicit source format overrides suffix-based selection.
    let format = options
        .input_format
        .map_or_else(|| Format::from_input_path(path), Ok)?;

    // Reject result-selection and frame options on formats that cannot use
    // them before opening or parsing any source file.
    if !matches!(format, Format::Op2 | Format::Pch)
        && (options.mesh.is_some() || options.subcase.is_some())
    {
        return Err(Error::new(
            "E_USAGE",
            "mesh and subcase apply only to OP2/PCH input",
        ));
    }
    if !matches!(format, Format::Op2 | Format::Pch) && options.assume_basic_frame {
        return Err(Error::new(
            "E_USAGE",
            "basic-frame assertion applies only to OP2/PCH input",
        ));
    }
    if !matches!(
        format,
        Format::Op2 | Format::Pch | Format::Frd | Format::Exodus
    ) && options.step.is_some()
    {
        return Err(Error::new(
            "E_USAGE",
            "step applies only to OP2, PCH, FRD, or Exodus input",
        ));
    }
    let mut generated_point_ids = false;
    let (dataset, omissions, assumed_zero) = match format {
        Format::Bdf => {
            // BDF projection can lose solver cards while retaining geometry.
            let projection = bdf::mesh::read_from(File::open(path)?, options.max_bytes)?;
            let omissions = projection
                .omissions
                .into_iter()
                .map(|item| {
                    Omission::new(
                        Stage::Source,
                        format!("{} × {}: {}", item.category, item.count, item.detail),
                    )
                })
                .collect();
            (
                Dataset {
                    mesh: projection.mesh,
                    fields: Vec::new(),
                },
                omissions,
                false,
            )
        }
        Format::Vtu => {
            let text = read_text(path, options.max_bytes, "E_VTU", "VTU must be UTF-8 XML")?;
            let projection = vtu::read_projection(&text)?;
            generated_point_ids = projection.generated_point_ids;
            let mut omissions = Vec::new();
            if projection.generated_point_ids {
                omissions.push(Omission::new(
                    Stage::Source,
                    "VTU has no original point IDs; assigned one-based IDs",
                ));
            }
            if projection.generated_cell_ids {
                omissions.push(Omission::new(
                    Stage::Source,
                    "VTU has no original cell IDs; assigned one-based IDs",
                ));
            }
            (projection.dataset, omissions, false)
        }
        Format::Vtk => {
            let text = read_text(
                path,
                options.max_bytes,
                "E_VTK",
                "legacy VTK must be ASCII text",
            )?;
            let projection = vtk::read_projection(&text)?;
            generated_point_ids = projection.generated_point_ids;
            let mut omissions = Vec::new();
            if projection.generated_point_ids {
                omissions.push(Omission::new(
                    Stage::Source,
                    "legacy VTK has no original point IDs; assigned one-based IDs",
                ));
            }
            if projection.generated_cell_ids {
                omissions.push(Omission::new(
                    Stage::Source,
                    "legacy VTK has no original cell IDs; assigned one-based IDs",
                ));
            }
            (projection.dataset, omissions, false)
        }
        Format::Stl => {
            let bytes = read_limited(path, options.max_bytes)?;
            let projection = stl::read_projection(&bytes)?;
            generated_point_ids = true;
            let mut omissions = vec![Omission::new(
                Stage::Source,
                "STL has no node or element IDs; assigned one-based facet-local IDs without welding",
            )];
            if projection.normals > 0 {
                omissions.push(Omission::new(
                    Stage::Source,
                    format!(
                        "{} STL facet normal(s) not retained as numeric fields",
                        projection.normals
                    ),
                ));
            }
            if projection.attributed_facets > 0 {
                omissions.push(Omission::new(
                    Stage::Source,
                    format!(
                        "{} STL facet(s) have nonstandard attribute bytes that were omitted",
                        projection.attributed_facets
                    ),
                ));
            }
            (projection.dataset, omissions, false)
        }
        Format::Su2 => {
            let text = read_text(
                path,
                options.max_bytes,
                "E_SU2",
                "SU2 mesh must be UTF-8 ASCII",
            )?;
            let projection = su2::read_projection(&text)?;
            generated_point_ids = true;
            let omissions = vec![Omission::new(
                Stage::Source,
                "SU2 uses positional connectivity; assigned one-based node and element IDs",
            )];
            (projection.dataset, omissions, false)
        }
        Format::Unv => {
            let text = read_text(path, options.max_bytes, "E_UNV", "UNV must be UTF-8 ASCII")?;
            let projection = unv::read_projection(&text)?;
            let mut omissions = projection
                .omitted_datasets
                .into_iter()
                .map(|number| Omission::new(Stage::Source, format!("UNV dataset {number} omitted")))
                .collect::<Vec<_>>();
            if projection.tagged_elements > 0 {
                omissions.push(Omission::new(
                    Stage::Source,
                    format!("{} UNV element header(s) have entity, physical, or color tags without a mapping", projection.tagged_elements),
                ));
            }
            (projection.dataset, omissions, false)
        }
        Format::Exodus => {
            let bytes = read_limited(path, options.max_bytes)?;
            let projection = exodus::read_projection(&bytes, options.step)?;
            generated_point_ids = projection.generated_point_ids;
            let omissions = projection
                .omissions
                .into_iter()
                .map(|detail| Omission::new(Stage::Source, detail))
                .collect();
            (projection.dataset, omissions, false)
        }
        Format::Msh => {
            // The MSH reader handles geometry and data; other sections are
            // recorded as source omissions for the caller to review.
            let text = read_text(path, options.max_bytes, "E_MSH", "MSH must be UTF-8 ASCII")?;
            let projection = msh::read_projection(&text)?;
            let mut omissions = text
                .lines()
                .filter_map(|line| line.trim().strip_prefix('$'))
                .filter(|name| !name.starts_with("End"))
                .filter(|section| {
                    !matches!(
                        *section,
                        "MeshFormat" | "Nodes" | "Elements" | "NodeData" | "ElementData" | "PhysicalNames"
                    )
                })
                .map(|section| {
                    Omission::new(
                        Stage::Source,
                        if section == "Entities" {
                            "MSH $Entities bounds and CAD topology are not retained; physical group membership is projected".to_owned()
                        } else {
                            format!("MSH ${section} section is not represented in the projection")
                        },
                    )
                })
                .collect::<Vec<_>>();
            if projection.tagged_elements > 0 {
                omissions.push(Omission::new(
                    Stage::Source,
                    format!(
                        "{} MSH 2.2 element tag list(s) have geometrical or other metadata without a mapping",
                        projection.tagged_elements
                    ),
                ));
            }
            if projection.generated_group_names > 0 {
                omissions.push(Omission::new(
                    Stage::Source,
                    format!("{} MSH physical group(s) have no source name; assigned physical_<dimension>_<tag> names", projection.generated_group_names),
                ));
            }
            (projection.dataset, omissions, false)
        }
        Format::Inp => {
            let text = read_text(path, options.max_bytes, "E_INP", "INP must be UTF-8 text")?;
            let parsed = inp::read(&text)?;
            let omissions = parsed
                .omitted_keywords
                .into_iter()
                .map(|keyword| {
                    Omission::new(
                        Stage::Source,
                        format!("INP keyword *{keyword} is absent from mesh projection"),
                    )
                })
                .collect();
            (
                Dataset {
                    mesh: parsed.mesh,
                    fields: Vec::new(),
                },
                omissions,
                false,
            )
        }
        Format::Frd => {
            // Step selection filters complete fields after parsing the file.
            let bytes = read_limited(path, options.max_bytes)?;
            let mut dataset = frd::read(&bytes)?;
            if let Some(step) = options.step {
                let step = i64::try_from(step)
                    .map_err(|_| Error::new("E_FRD", "selected step exceeds Int64"))?;
                dataset.fields.retain(|field| field.step == Some(step));
                if dataset.fields.is_empty() {
                    return Err(Error::new("E_FRD", "selected step has no fields"));
                }
            }
            let mut omissions = Vec::new();
            if bytes.windows(2).any(|pair| pair == b"1U" || pair == b"1P") {
                omissions.push(Omission::new(
                    Stage::Source,
                    "FRD user/model parameter metadata is not represented in the projection",
                ));
            }
            (dataset, omissions, false)
        }
        Format::Op2 => {
            // OP2 result tables carry no complete matching mesh on their own.
            let mesh_path = options
                .mesh
                .as_deref()
                .ok_or_else(|| Error::new("E_USAGE", "OP2 requires --mesh matching mesh file"))?;
            let (mesh, mut omissions) = read_result_mesh(mesh_path, options, Format::Op2)?;
            let bytes = read_limited(path, options.max_bytes)?;
            let (dataset, assumed_zero) =
                op2::read_displacements(&bytes, &mesh, options.subcase, options.step)?;

            // Preserve both explicit synthetic provenance and the results
            // omitted by the selected displacement-table projection.
            if assumed_zero {
                omissions.push(Omission::assumed(
                    AssumptionKind::SyntheticZero,
                    "OP2 title marks this as a synthetic all-zero displacement table, not solver results",
                ));
            }
            omissions.push(Omission::new(
                Stage::Source,
                "OP2 result tables other than the selected real displacement table are not exported",
            ));
            (dataset, omissions, assumed_zero)
        }
        Format::Pch => {
            let mesh_path = options
                .mesh
                .as_deref()
                .ok_or_else(|| Error::new("E_USAGE", "PCH requires --mesh matching mesh file"))?;
            let (mesh, mut omissions) = read_result_mesh(mesh_path, options, Format::Pch)?;
            let source = read_text(path, options.max_bytes, "E_PCH", "PCH must be ASCII text")?;
            let projection = pch::read(&source, &mesh, options.subcase, options.step)?;
            if projection.skipped_blocks > 0 {
                omissions.push(Omission::new(
                    Stage::Source,
                    format!(
                        "{} other PCH result block header(s) were not projected",
                        projection.skipped_blocks
                    ),
                ));
            }
            if source.contains("$TITLE")
                || source.contains("$SUBTITLE")
                || source.contains("$LABEL")
            {
                omissions.push(Omission::new(
                    Stage::Source,
                    "PCH title, subtitle, and label metadata are not represented",
                ));
            }
            (projection.dataset, omissions, false)
        }
    };
    Ok(ReadResult {
        format,
        dataset,
        omissions,
        generated_point_ids,
        assumed_zero,
    })
}
