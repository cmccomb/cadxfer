//! Native 32-bit OP2 real displacement adapter.
//! A matching mesh with original node IDs is required. Coordinates and
//! displacements must be in the basic frame; unsupported tables fail explicitly.
mod binary;

use super::nastran_result;
use crate::core::{Dataset, Error, Field, FieldLocation, Mesh, Result};

/// Decode one real six-component OUGV1 displacement table from OP2 bytes.
/// `step` is a zero-based index within the subcase.
/// `mesh` must match the result node IDs; its coordinates and the result
/// components must be in the basic frame. This function does not transform
/// frames or verify mesh coordinates against OP2. The bool reports whether the
/// OP2 title marks a synthetic all-zero table.
///
/// The returned [`Dataset`] uses the supplied mesh and selected displacement
/// values. Only 32-bit Fortran records and real SORT1 OUGV1 tables are decoded.
///
/// # Errors
///
/// Returns an error for invalid mesh data, malformed or unsupported OP2 records,
/// or a displacement table that does not match the mesh.
pub fn read_displacements(
    bytes: &[u8],
    mesh: &Mesh,
    subcase: Option<i64>,
    step: Option<usize>,
) -> Result<(Dataset, bool)> {
    let selected = binary::decode(bytes, subcase, step)?;
    let dataset = nastran_result::displacement_dataset(
        mesh,
        &selected.rows,
        6,
        selected.subcase,
        selected.step,
        Some(selected.time),
        "E_OP2",
    )?;
    Ok((dataset, selected.assumed_zero))
}

/// Emit one real Nastran displacement table in native Rust. A recognized
/// three-component displacement is promoted to six components with typed 0.0
/// rotations only when `zero_missing_rotations` is true. The OP2 carries no mesh.
/// `assumed_zero` marks a synthetic, non-solver table in the OP2 title.
///
/// A malformed field or empty mesh is rejected before encoding. The caller
/// must save both the OP2 and a matching mesh for later reading.
///
/// # Examples
///
/// ```
/// use caexfer::core::{Dataset, Field, FieldLocation};
/// use caexfer::formats::op2;
/// let field = Field {
///     name: "DISP".into(), location: FieldLocation::Point,
///     components: vec!["T1".into(), "T2".into(), "T3".into()],
///     values: vec![], step: None, time: None,
/// };
/// let error = op2::write_displacements(&Dataset::default(), &field, 1, false, false).unwrap_err();
/// assert_eq!(error.code, "E_OP2"); // A complete mesh and values are required.
/// ```
///
/// # Errors
///
/// Returns an error for invalid or unrepresentable displacement data, including
/// a count or value outside the supported 32-bit OP2 representation.
#[allow(clippy::cast_possible_truncation)] // OP2 stores float32; range and underflow are checked.
pub fn write_displacements(
    dataset: &Dataset,
    field: &Field,
    subcase: i64,
    zero_missing_rotations: bool,
    assumed_zero: bool,
) -> Result<Vec<u8>> {
    // Reject incomplete or nonfinite data before building an OP2 table.
    dataset.mesh.validate()?;
    dataset.mesh.require_no_sets("E_OP2")?;
    if dataset.mesh.points.is_empty()
        || field.values.len()
            != dataset
                .mesh
                .points
                .len()
                .saturating_mul(field.components.len())
        || field.time.is_some_and(|value| !value.is_finite())
        || !field.values.iter().all(|value| value.is_finite())
    {
        return Err(Error::new(
            "E_OP2",
            "OP2 output requires complete finite displacement values for every mesh node",
        ));
    }
    // The OP2 subcase header holds a positive signed 32-bit integer.
    if subcase <= 0 || subcase > i64::from(i32::MAX) {
        return Err(Error::new(
            "E_OP2",
            "subcase must be a positive 32-bit integer",
        ));
    }
    // Float32 conversion must not overflow or erase a nonzero time value.
    if field
        .time
        .is_some_and(|value| !(value as f32).is_finite() || (value != 0.0 && (value as f32) == 0.0))
    {
        return Err(Error::new("E_OP2", "time is outside the OP2 float32 range"));
    }
    // This writer accepts nodal translations with optional rotations.
    if field.location != FieldLocation::Point || !matches!(field.components.len(), 3 | 6) {
        return Err(Error::new(
            "E_OP2",
            "OP2 output requires a 3- or 6-component nodal displacement",
        ));
    }
    // Treat absent rotations as zero only with the caller's explicit assertion.
    if field.components.len() == 3 && !zero_missing_rotations {
        return Err(Error::new(
            "E_OP2",
            "three-component displacement has unknown rotations; pass --zero-missing-rotations only if R1/R2/R3 are known to be zero",
        ));
    }

    // A synthetic provenance marker is valid only for an actually all-zero
    // six-component table, never for measured or partially known data.
    if assumed_zero
        && (field.components.len() != 6 || field.values.iter().any(|value| *value != 0.0))
    {
        return Err(Error::new(
            "E_OP2",
            "assumed-zero OP2 output requires six zero components per node",
        ));
    }
    // Accept source labels and the normalized label produced by result readers.
    let name = field.name.to_ascii_uppercase();
    let named_displacement = name == "DISP"
        || name == "DISPLACEMENT"
        || name
            .strip_prefix("DISPLACEMENT_SUBCASE_")
            .is_some_and(|suffix| suffix.parse::<i64>().is_ok_and(|value| value > 0));
    if !named_displacement {
        return Err(Error::new(
            "E_OP2",
            "OP2 output requires a field named DISP or DISPLACEMENT",
        ));
    }

    // Preserve companion-mesh order and original GRID IDs in the table rows.
    let mut rows = Vec::with_capacity(dataset.mesh.points.len());
    for (i, point) in dataset.mesh.points.iter().enumerate() {
        // The low decimal digit stores the OP2 device code.
        if point.id > ((i32::MAX - 2) / 10) as u64 {
            return Err(Error::new(
                "E_OP2",
                "OP2 node ID exceeds 32-bit encoded range",
            ));
        }
        let mut values = [0_f32; 6];
        for (slot, value) in values
            .iter_mut()
            .zip(&field.values[i * field.components.len()..(i + 1) * field.components.len()])
        {
            // Float32 overflow and underflow would alter result meaning.
            if !(*value as f32).is_finite() {
                return Err(Error::new(
                    "E_OP2",
                    "displacement exceeds OP2 float32 range",
                ));
            }
            if *value != 0.0 && (*value as f32) == 0.0 {
                return Err(Error::new(
                    "E_OP2",
                    "nonzero displacement would underflow to zero in OP2 float32",
                ));
            }
            *slot = *value as f32;
        }
        rows.push((point.id, values));
    }
    // Encode only after all rows have passed the range and completeness checks.
    binary::encode(
        &rows,
        i32::try_from(subcase).map_err(|_| Error::new("E_OP2", "subcase exceeds 32-bit range"))?,
        field.time.map(|value| value as f32),
        assumed_zero,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Point;

    /// Return a valid displacement dataset and its field for writer checks.
    fn sample() -> (Dataset, Field) {
        let dataset = Dataset {
            mesh: Mesh {
                points: vec![Point {
                    id: 1,
                    position: [0.0; 3],
                }],
                cells: Vec::new(),
                ..Mesh::default()
            },
            fields: Vec::new(),
        };
        let field = Field {
            name: "DISP".into(),
            location: FieldLocation::Point,
            components: vec!["D1".into(), "D2".into(), "D3".into()],
            values: vec![1.0, 0.0, 0.0],
            step: None,
            time: None,
        };
        (dataset, field)
    }

    /// Do not invent rotational displacement components during OP2 writing.
    #[test]
    fn unknown_rotations_are_not_filled_implicitly() {
        let (dataset, field) = sample();
        let error = write_displacements(&dataset, &field, 1, false, false).unwrap_err();
        assert!(error.message.contains("unknown rotations"));
    }

    /// Validate the public displacement field before encoding binary records.
    #[test]
    fn malformed_public_field_fails_before_encoding() {
        let (dataset, mut field) = sample();
        field.values.pop();
        assert_eq!(
            write_displacements(&dataset, &field, 1, true, false)
                .unwrap_err()
                .code,
            "E_OP2"
        );
    }

    /// Require real zero values before labeling an OP2 table synthetic zero.
    #[test]
    fn synthetic_provenance_requires_actual_zero_values() {
        let (dataset, mut field) = sample();
        field
            .components
            .extend(["R1".into(), "R2".into(), "R3".into()]);
        field.values.extend([0.0, 0.0, 0.0]);
        assert_eq!(
            write_displacements(&dataset, &field, 1, false, true)
                .unwrap_err()
                .code,
            "E_OP2"
        );
    }

    /// Reject IDs and numbers outside the supported OP2 binary layout.
    #[test]
    fn writer_rejects_values_and_ids_that_binary_records_cannot_represent() {
        let (dataset, field) = sample();
        for subcase in [0, i64::from(i32::MAX) + 1] {
            let error = write_displacements(&dataset, &field, subcase, true, false).unwrap_err();
            assert!(error.message.contains("subcase"));
        }

        let mut timed = field.clone();
        timed.time = Some(1e100);
        assert!(
            write_displacements(&dataset, &timed, 1, true, false)
                .unwrap_err()
                .message
                .contains("time")
        );

        let mut wrong_location = field.clone();
        wrong_location.location = FieldLocation::Cell;
        assert!(
            write_displacements(&dataset, &wrong_location, 1, true, false)
                .unwrap_err()
                .message
                .contains("nodal")
        );

        let mut wrong_name = field.clone();
        wrong_name.name = "TEMPERATURE".into();
        assert!(
            write_displacements(&dataset, &wrong_name, 1, true, false)
                .unwrap_err()
                .message
                .contains("named DISP")
        );

        let mut large_id = dataset.clone();
        large_id.mesh.points[0].id = i32::MAX as u64;
        assert!(
            write_displacements(&large_id, &field, 1, true, false)
                .unwrap_err()
                .message
                .contains("node ID")
        );

        let mut too_large = field.clone();
        too_large.values[0] = 1e100;
        assert!(
            write_displacements(&dataset, &too_large, 1, true, false)
                .unwrap_err()
                .message
                .contains("float32 range")
        );

        let mut too_small = field;
        too_small.values[0] = 1e-100;
        assert!(
            write_displacements(&dataset, &too_small, 1, true, false)
                .unwrap_err()
                .message
                .contains("underflow")
        );
    }
}
