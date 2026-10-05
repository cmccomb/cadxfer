use std::collections::BTreeSet;
use std::io::Write;

use crate::core::{Error, FieldLocation, Mesh, Result, VoxelGrid, boundary_surface};
use crate::formats::{bdf, exodus, frd, inp, msh, op2, stl, su2, unv, vox, vti, vtk, vtu};

use super::{AssumptionKind, ConversionReport, Format, Omission, Options, ReadResult, Stage};

/// Remove unmapped solver property IDs and return the number removed.
fn omit_property_ids(mesh: &mut Mesh) -> usize {
    mesh.cells
        .iter_mut()
        .map(|cell| usize::from(cell.property_id.take().is_some()))
        .sum()
}

/// Write a previously read source in another supported format.
///
/// OP2 output requires exactly one nodal displacement field. A caller may
/// supply a synthetic field, but must set
/// `source.assumed_zero` to mark that provenance in the OP2 title.
///
/// # Errors
///
/// Returns a validation, representability, adapter, or output-stream error
/// for the selected destination format.
///
#[allow(clippy::too_many_lines)] // Destination branches share omission and assumption tracking.
pub fn convert(
    mut source: ReadResult,
    target: Format,
    options: &Options,
    mut writer: impl Write,
) -> Result<ConversionReport> {
    let voxel_size_allowed = match (source.format, target) {
        (Format::Vti | Format::Vox, _) | (Format::Stl, Format::Stl | Format::Op2 | Format::Pch) => {
            false
        }
        (_, Format::Vti | Format::Vox) | (Format::Stl, _) => true,
        _ => false,
    };
    if options.voxel_size.is_some() && !voxel_size_allowed {
        return Err(Error::new(
            "E_USAGE",
            "--voxel-size applies to mesh/STL to VTI/VOX or STL to a volume mesh",
        ));
    }
    if options.smooth_iterations > 50
        || (options.smooth_iterations > 0
            && !(target == Format::Stl && source.voxel_grid.is_some()))
    {
        return Err(Error::new(
            "E_USAGE",
            "smooth_iterations must be 1-50 and applies only to VTI/VOX to STL",
        ));
    }
    if target == Format::Stl && source.voxel_grid.is_some() {
        source.dataset.mesh = source
            .voxel_grid
            .as_ref()
            .ok_or_else(|| Error::new("E_VOXEL", "missing voxel grid"))?
            .surface_mesh()?;
        if options.smooth_iterations > 0 {
            crate::core::smooth_surface(&mut source.dataset.mesh, options.smooth_iterations)?;
            source.omissions.push(Omission::new(
                Stage::Destination,
                format!("voxel STL surface smoothed for {} Taubin cycle(s); geometry no longer exactly follows voxel boundaries", options.smooth_iterations),
            ));
        }
        source.omissions.push(Omission::new(
            Stage::Destination,
            if options.smooth_iterations > 0 {
                "exposed voxel faces triangulated before smoothing; STL detail remains limited by grid resolution"
            } else {
                "exposed voxel faces triangulated; stair-step surface retains grid resolution"
            },
        ));
    } else if target == Format::Stl
        && source
            .dataset
            .mesh
            .cells
            .iter()
            .any(|c| c.kind != crate::core::CellKind::Triangle3)
    {
        let volume = source
            .dataset
            .mesh
            .cells
            .iter()
            .any(|c| c.kind.dimension() == 3);
        let lower = source
            .dataset
            .mesh
            .cells
            .iter()
            .filter(|c| c.kind.dimension() < 3)
            .count();
        let properties = source
            .dataset
            .mesh
            .cells
            .iter()
            .filter(|c| c.property_id.is_some())
            .count();
        let node_sets = source.dataset.mesh.node_sets.len();
        let cell_sets = source.dataset.mesh.cell_sets.len();
        source.dataset.mesh = boundary_surface(&source.dataset.mesh)?;
        for (count, label) in [
            (properties, "property ID(s)"),
            (node_sets, "named node set(s)"),
            (cell_sets, "named cell set(s)"),
        ] {
            if count > 0 {
                source.omissions.push(Omission::new(
                    Stage::Destination,
                    format!("{count} {label} omitted during STL boundary extraction"),
                ));
            }
        }
        source.omissions.push(Omission::new(Stage::Destination,
            if volume { format!("external volume faces triangulated for STL; {lower} lower-dimensional cell(s) excluded") }
            else { "quadrilateral faces triangulated for STL".to_owned() }));
    } else if source.format == Format::Stl
        && options.voxel_size.is_some()
        && !matches!(target, Format::Stl | Format::Vti | Format::Vox)
    {
        let grid = VoxelGrid::from_surface(
            &source.dataset.mesh,
            options
                .voxel_size
                .ok_or_else(|| Error::new("E_USAGE", "missing voxel size"))?,
        )?;
        source.dataset.mesh = grid.volume_mesh()?;
        source.omissions.push(Omission::new(
            Stage::Destination,
            "STL surface sampled at voxel centers and emitted as Hex8 volume cells",
        ));
    }
    // Count fields before destination-specific filtering; the report describes
    // the source dataset as well as any losses during output.
    let source_fields = source.dataset.fields.len();
    let mut voxel_summary = None;
    let dataset = &mut source.dataset;
    let omissions = &mut source.omissions;
    if target != Format::Su2 {
        if !dataset.mesh.node_sets.is_empty() {
            omissions.push(Omission::new(
                Stage::Destination,
                format!(
                    "{} named node set(s) omitted by {}",
                    dataset.mesh.node_sets.len(),
                    target.name()
                ),
            ));
        }
        if !dataset.mesh.cell_sets.is_empty() {
            omissions.push(Omission::new(
                Stage::Destination,
                format!(
                    "{} named cell set(s) omitted by {}",
                    dataset.mesh.cell_sets.len(),
                    target.name()
                ),
            ));
        }
        dataset.mesh.node_sets.clear();
        dataset.mesh.cell_sets.clear();
    }
    match target {
        Format::Vti | Format::Vox => {
            let grid = if let Some(grid) = &source.voxel_grid {
                if options.voxel_size.is_some() {
                    return Err(Error::new(
                        "E_USAGE",
                        "--voxel-size applies only to mesh or surface input",
                    ));
                }
                grid.clone()
            } else {
                let spacing = options.voxel_size.ok_or_else(|| {
                    Error::new("E_USAGE", "mesh to voxel conversion requires --voxel-size")
                })?;
                omissions.push(Omission::new(Stage::Destination,
                    "closed surface sampled at voxel centers; features below grid spacing may disappear"));
                let surface = if dataset.mesh.cells.iter().any(|c| c.kind.dimension() == 3) {
                    let excluded = dataset
                        .mesh
                        .cells
                        .iter()
                        .filter(|c| c.kind.dimension() < 3)
                        .count();
                    omissions.push(Omission::new(
                        Stage::Destination,
                        format!("external faces of volume cells extracted before voxel-center sampling; {excluded} lower-dimensional cell(s) excluded"),
                    ));
                    boundary_surface(&dataset.mesh)?
                } else {
                    dataset.mesh.clone()
                };
                VoxelGrid::from_surface(&surface, spacing)?
            };
            if dataset.mesh.cells.iter().any(|c| c.property_id.is_some()) {
                omissions.push(Omission::new(
                    Stage::Destination,
                    "mesh property IDs have no binary occupancy mapping",
                ));
            }
            if !dataset.fields.is_empty() {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!(
                        "{} numeric field(s) omitted from binary occupancy",
                        dataset.fields.len()
                    ),
                ));
            }
            if target == Format::Vox {
                if grid
                    .origin
                    .iter()
                    .any(|&v| v.to_bits() != 0.0_f64.to_bits())
                    || grid.spacing.to_bits() != 1.0_f64.to_bits()
                {
                    omissions.push(Omission::new(
                        Stage::Destination,
                        "VOX stores grid indices only; physical origin and spacing are omitted",
                    ));
                }
                vox::write(&grid, &mut writer)?;
            } else {
                vti::write(&grid, &mut writer)?;
            }
            voxel_summary = Some((grid.dims, grid.occupied.iter().filter(|&&v| v != 0).count()));
        }
        Format::Vtu => {
            // VTU has one array per location and name; multiple source steps
            // require the caller to select a step first.
            let mut seen = BTreeSet::new();
            for field in &dataset.fields {
                if !seen.insert((field.location as u8, field.name.clone())) {
                    return Err(Error::new(
                        "E_VTU",
                        "multiple steps of one field; choose --step",
                    ));
                }
            }
            vtu::write_data(dataset, &mut writer)?;
        }
        Format::Vtk => {
            for field in &dataset.fields {
                if field.step.is_some() || field.time.is_some() {
                    omissions.push(Omission::new(
                        Stage::Destination,
                        format!(
                            "field {} loses step/time metadata in legacy VTK",
                            field.name
                        ),
                    ));
                }
                if field
                    .components
                    .iter()
                    .enumerate()
                    .any(|(index, name)| name != &format!("C{}", index + 1))
                {
                    omissions.push(Omission::new(
                        Stage::Destination,
                        format!("field {} loses component labels in legacy VTK", field.name),
                    ));
                }
            }
            vtk::write_data(dataset, &mut writer)?;
        }
        Format::Stl => {
            if !dataset.fields.is_empty() {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!(
                        "{} numeric field(s) omitted from STL triangle surface",
                        dataset.fields.len()
                    ),
                ));
            }
            if omit_property_ids(&mut dataset.mesh) > 0 {
                omissions.push(Omission::new(
                    Stage::Destination,
                    "property IDs have no STL mapping",
                ));
            }
            omissions.push(Omission::new(
                Stage::Destination,
                "STL has no node or element IDs or shared-vertex identity; binary coordinates use float32",
            ));
            dataset.fields.clear();
            stl::write_data(dataset, &mut writer)?;
        }
        Format::Su2 => {
            if !dataset.fields.is_empty() {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!(
                        "{} numeric field(s) omitted from SU2 mesh",
                        dataset.fields.len()
                    ),
                ));
            }
            if !dataset.mesh.node_sets.is_empty() {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!(
                        "{} named node set(s) have no SU2 marker mapping",
                        dataset.mesh.node_sets.len()
                    ),
                ));
            }
            let dimension = dataset
                .mesh
                .cells
                .iter()
                .map(|cell| cell.kind.dimension())
                .max()
                .unwrap_or(0);
            let unrepresented = dataset
                .mesh
                .cell_sets
                .iter()
                .filter(|set| set.dimension != dimension.saturating_sub(1))
                .count();
            if unrepresented > 0 {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!("{unrepresented} cell set(s) are not SU2 boundary markers"),
                ));
            }
            dataset.mesh.node_sets.clear();
            dataset
                .mesh
                .cell_sets
                .retain(|set| set.dimension == dimension.saturating_sub(1));
            if omit_property_ids(&mut dataset.mesh) > 0 {
                omissions.push(Omission::new(
                    Stage::Destination,
                    "property IDs have no SU2 mapping",
                ));
            }
            omissions.push(Omission::new(
                Stage::Destination,
                "SU2 uses positional connectivity; original node and element IDs are not encoded",
            ));
            dataset.fields.clear();
            su2::write_data(dataset, &mut writer)?;
        }
        Format::Unv => {
            if !dataset.fields.is_empty() {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!(
                        "{} numeric field(s) omitted from UNV geometry",
                        dataset.fields.len()
                    ),
                ));
            }
            let properties = omit_property_ids(&mut dataset.mesh);
            if properties > 0 {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!("{properties} property ID(s) omitted from UNV geometry"),
                ));
            }
            dataset.fields.clear();
            unv::write_data(dataset, &mut writer)?;
        }
        Format::Exodus => {
            let properties = omit_property_ids(&mut dataset.mesh);
            if properties > 0 {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!("{properties} solver property ID(s) have no Exodus block mapping"),
                ));
            }
            omissions.push(Omission::new(
                Stage::Destination,
                "Exodus element block IDs are generated from cell topology; no source block identity is inferred",
            ));
            exodus::write_data(dataset, &mut writer)?;
        }
        Format::Msh => {
            // MSH stores numeric tuples but loses these component labels and
            // this exporter's unmapped property IDs.
            let missing_steps = dataset
                .fields
                .iter()
                .filter(|field| field.step.is_none())
                .count();
            if missing_steps > 0 {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!("{missing_steps} field(s) without step metadata use MSH step 0"),
                ));
            }
            let labels = dataset
                .fields
                .iter()
                .filter(|field| {
                    field
                        .components
                        .iter()
                        .enumerate()
                        .any(|(i, name)| name != &format!("C{}", i + 1))
                })
                .count();
            if labels > 0 {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!("{labels} field(s) lose component labels in MSH NodeData/ElementData"),
                ));
            }
            let props = omit_property_ids(&mut dataset.mesh);
            if props > 0 {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!("{props} property IDs have no MSH entity mapping in this exporter"),
                ));
            }
            msh::write_version(
                dataset,
                options.msh_version.unwrap_or_default(),
                &mut writer,
            )?;
        }
        Format::Inp | Format::Bdf => {
            // These writers emit geometry-only solver input, so result fields
            // remain accounted for in the report instead of silently vanishing.
            if !dataset.fields.is_empty() {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!(
                        "{} numeric field(s) omitted from geometry-only solver input",
                        dataset.fields.len()
                    ),
                ));
            }
            if target == Format::Inp {
                // The bounded INP writer has no property definition mapping.
                let props = omit_property_ids(&mut dataset.mesh);
                if props > 0 {
                    omissions.push(Omission::new(
                        Stage::Destination,
                        format!("{props} property IDs omitted from INP"),
                    ));
                }
                inp::write(&dataset.mesh, &mut writer)?;
            } else {
                // BDF requires a PID on these element cards; the writer uses
                // placeholder 1 where the source mesh has none.
                let missing = dataset
                    .mesh
                    .cells
                    .iter()
                    .filter(|cell| cell.property_id.is_none())
                    .count();
                if missing > 0 {
                    omissions.push(Omission::new(
                        Stage::Destination,
                        format!("{missing} BDF element(s) use placeholder PID 1; no property cards are emitted"),
                    ));
                }
                bdf::mesh::write(&dataset.mesh, &mut writer)?;
            }
        }
        Format::Frd => {
            // The supported FRD result blocks are nodal, so cell fields must
            // be removed before calling its writer.
            let before = dataset.fields.len();
            dataset
                .fields
                .retain(|field| field.location == FieldLocation::Point);
            if dataset.fields.len() != before {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!(
                        "{} cell field(s) have no direct FRD nodal representation",
                        before - dataset.fields.len()
                    ),
                ));
            }
            for field in &mut dataset.fields {
                // FRD's displacement name and metadata layout differ from
                // the generic field model; record each normalization.
                if field.name.starts_with("DISPLACEMENT_SUBCASE_") {
                    omissions.push(Omission::new(
                        Stage::Destination,
                        format!(
                            "field {} is named DISP in FRD; subcase name is not retained",
                            field.name
                        ),
                    ));
                    field.name = "DISP".into();
                }
                if field.step.is_none() || field.time.is_none() {
                    omissions.push(Omission::new(
                        Stage::Destination,
                        format!(
                            "field {} uses FRD step/time 0 where metadata is absent",
                            field.name
                        ),
                    ));
                }
            }
            if omit_property_ids(&mut dataset.mesh) > 0 {
                omissions.push(Omission::new(
                    Stage::Destination,
                    "BDF property IDs have no direct FRD mesh mapping",
                ));
            }
            omissions.push(Omission::new(
                Stage::Destination,
                "FRD ASCII E12.5 rounds coordinates and field values to six significant digits",
            ));
            frd::write(dataset, &mut writer)?;
        }
        Format::Op2 => {
            // The adapter writes a single real nodal displacement table.
            let matches: Vec<_> = dataset
                .fields
                .iter()
                .filter(|field| {
                    let name = field.name.to_ascii_uppercase();
                    field.location == FieldLocation::Point
                        && (name == "DISP"
                            || name == "DISPLACEMENT"
                            || name.starts_with("DISPLACEMENT_SUBCASE_"))
                        && matches!(field.components.len(), 3 | 6)
                })
                .collect();
            if matches.len() != 1 {
                return Err(Error::new(
                    "E_OP2",
                    "OP2 output requires exactly one 3- or 6-component nodal DISP field; select one result step",
                ));
            }
            let field = matches[0];

            // Recover a subcase encoded in the field name, defaulting to 1
            // for generic DISP names.
            let subcase = field
                .name
                .to_ascii_uppercase()
                .strip_prefix("DISPLACEMENT_SUBCASE_")
                .and_then(|text| text.parse().ok())
                .unwrap_or(1);
            if field.components.len() == 3 && options.zero_missing_rotations {
                omissions.push(Omission::assumed(AssumptionKind::ZeroRotations, "rotational displacement components R1/R2/R3 filled with typed float 0.0 by explicit request"));
            }
            if field.step.is_some_and(|step| step != 0) {
                omissions.push(Omission::new(
                    Stage::Destination,
                    "source step number is not encoded in the one-step OP2 table",
                ));
            }
            if dataset.fields.len() > 1 {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!(
                        "{} other numeric field(s) omitted from OP2 displacement output",
                        dataset.fields.len() - 1
                    ),
                ));
            }
            omissions.push(Omission::new(
                Stage::Destination,
                "OP2 contains no mesh; retain or export a matching companion mesh",
            ));
            omissions.push(Omission::new(
                Stage::Destination,
                "OP2 real displacement values use float32 precision",
            ));

            // The adapter builds the binary table before it reaches the
            // caller's output stream.
            let bytes = op2::write_displacements(
                dataset,
                field,
                subcase,
                options.zero_missing_rotations,
                source.assumed_zero,
            )?;
            writer.write_all(&bytes)?;
        }
        Format::Pch => return Err(Error::new("E_FORMAT", "PCH writing is not supported")),
    }
    Ok(ConversionReport {
        points: dataset.mesh.points.len(),
        cells: dataset.mesh.cells.len(),
        fields: source_fields,
        voxel_grid: voxel_summary,
        omissions: source.omissions,
        mesh_output: None,
    })
}
