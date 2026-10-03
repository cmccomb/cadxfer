//! Optional pyNastran-backed OP2 displacement adapter.
//! A matching basic-frame BDF mesh is required; no binary record guesswork.
use caexfer_core::{Dataset, Error, Field, FieldLocation, Mesh, Result};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Decode one real six-component displacement table using the installed
/// pyNastran Python package. `step` is a zero-based index within the subcase.
/// The bool reports whether the OP2 title marks a synthetic all-zero table.
pub fn read_displacements(
    path: &Path,
    mesh: &Mesh,
    python: &Path,
    subcase: Option<i64>,
    step: Option<usize>,
) -> Result<(Dataset, bool)> {
    mesh.validate()?;
    let output = Command::new(python)
        .arg("-c")
        .arg(include_str!("op2_extract.py"))
        .arg(path)
        .arg(subcase.map_or_else(|| "-".into(), |v| v.to_string()))
        .arg(step.map_or_else(|| "-".into(), |v| v.to_string()))
        .output()
        .map_err(|e| Error::new("E_OP2", format!("cannot launch Python: {e}")))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let last = detail
            .lines()
            .last()
            .unwrap_or("pyNastran extraction failed");
        return Err(Error::new("E_OP2", last));
    }
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|_| Error::new("E_OP2", "extractor returned non-UTF-8"))?;
    let mut lines = text.lines();
    let header = lines
        .next()
        .ok_or_else(|| Error::new("E_OP2", "empty extractor output"))?;
    let parts: Vec<&str> = header.split('\t').collect();
    if parts.len() != 6 || parts[0] != "OK" {
        return Err(Error::new("E_OP2", "invalid extractor header"));
    }
    let assumed_zero = match parts[5] {
        "0" => false,
        "1" => true,
        _ => return Err(Error::new("E_OP2", "invalid assumed-zero provenance flag")),
    };
    let count: usize = parts[1]
        .parse()
        .map_err(|_| Error::new("E_OP2", "invalid result count"))?;
    let result_subcase: i64 = parts[2]
        .parse()
        .map_err(|_| Error::new("E_OP2", "invalid subcase"))?;
    let result_step: i64 = parts[3]
        .parse()
        .map_err(|_| Error::new("E_OP2", "invalid result step"))?;
    let time: f64 = parts[4]
        .parse()
        .map_err(|_| Error::new("E_OP2", "invalid result time"))?;
    let mut rows = BTreeMap::<u64, [f64; 6]>::new();
    for line in lines {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 7 {
            return Err(Error::new("E_OP2", "invalid result row"));
        }
        let id: u64 = cols[0]
            .parse()
            .map_err(|_| Error::new("E_OP2", "invalid node ID"))?;
        let mut vals = [0.; 6];
        for i in 0..6 {
            vals[i] = cols[i + 1]
                .parse()
                .map_err(|_| Error::new("E_OP2", "invalid displacement"))?;
        }
        if rows.insert(id, vals).is_some() {
            return Err(Error::new("E_OP2", "duplicate result node"));
        }
    }
    if rows.len() != count || rows.len() != mesh.points.len() {
        return Err(Error::new(
            "E_OP2",
            "displacement nodes do not match mesh nodes",
        ));
    }
    let mut values = Vec::with_capacity(6 * count);
    for point in &mesh.points {
        values.extend(rows.get(&point.id).ok_or_else(|| {
            Error::new("E_OP2", format!("no displacement for GRID {}", point.id))
        })?);
    }
    let dataset = Dataset {
        mesh: mesh.clone(),
        fields: vec![Field {
            name: format!("DISPLACEMENT_SUBCASE_{result_subcase}"),
            location: FieldLocation::Point,
            components: ["T1", "T2", "T3", "R1", "R2", "R3"]
                .map(str::to_owned)
                .to_vec(),
            values,
            step: Some(result_step),
            time: Some(time),
        }],
    };
    dataset.validate()?;
    Ok((dataset, assumed_zero))
}

/// Emit one real Nastran displacement table through pyNastran. A recognized
/// three-component displacement is promoted to six components with typed 0.0
/// rotations only when `zero_missing_rotations` is true. The OP2 carries no mesh.
/// `assumed_zero` marks a synthetic, non-solver table in the OP2 title.
pub fn write_displacements(
    dataset: &Dataset,
    field: &Field,
    python: &Path,
    subcase: i64,
    zero_missing_rotations: bool,
    assumed_zero: bool,
) -> Result<Vec<u8>> {
    dataset.mesh.validate()?;
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
    if subcase <= 0 || subcase > i32::MAX as i64 {
        return Err(Error::new(
            "E_OP2",
            "subcase must be a positive 32-bit integer",
        ));
    }
    if field
        .time
        .is_some_and(|value| !(value as f32).is_finite() || (value != 0.0 && (value as f32) == 0.0))
    {
        return Err(Error::new("E_OP2", "time is outside the OP2 float32 range"));
    }
    if field.location != FieldLocation::Point || !matches!(field.components.len(), 3 | 6) {
        return Err(Error::new(
            "E_OP2",
            "OP2 output requires a 3- or 6-component nodal displacement",
        ));
    }
    if field.components.len() == 3 && !zero_missing_rotations {
        return Err(Error::new(
            "E_OP2",
            "three-component displacement has unknown rotations; pass --zero-missing-rotations only if R1/R2/R3 are known to be zero",
        ));
    }
    if assumed_zero
        && (field.components.len() != 6 || field.values.iter().any(|value| *value != 0.0))
    {
        return Err(Error::new(
            "E_OP2",
            "assumed-zero OP2 output requires six zero components per node",
        ));
    }
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
    let mut source = format!(
        "{}\t{}\t{}\t{}\t{}\n",
        dataset.mesh.points.len(),
        subcase,
        field
            .time
            .map_or("-".to_string(), |value| value.to_string()),
        field.components.len(),
        if assumed_zero { 1 } else { 0 }
    );
    for (i, point) in dataset.mesh.points.iter().enumerate() {
        if point.id > i32::MAX as u64 {
            return Err(Error::new(
                "E_OP2",
                "OP2 node IDs must fit signed 32-bit integers",
            ));
        }
        source.push_str(&point.id.to_string());
        for value in &field.values[i * field.components.len()..(i + 1) * field.components.len()] {
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
            source.push('\t');
            source.push_str(&format!("{value:.17e}"));
        }
        source.push('\n');
    }
    let mut child = Command::new(python)
        .arg("-c")
        .arg(include_str!("op2_write.py"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::new("E_OP2", format!("cannot launch Python: {e}")))?;
    child.stdin.take().unwrap().write_all(source.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(Error::new(
            "E_OP2",
            detail.lines().last().unwrap_or("pyNastran writing failed"),
        ));
    }
    if output.stdout.is_empty() {
        return Err(Error::new("E_OP2", "pyNastran produced an empty OP2"));
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use caexfer_core::Point;

    fn sample() -> (Dataset, Field) {
        let dataset = Dataset {
            mesh: Mesh {
                points: vec![Point {
                    id: 1,
                    position: [0.0; 3],
                }],
                cells: Vec::new(),
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

    #[test]
    fn unknown_rotations_are_not_filled_implicitly() {
        let (dataset, field) = sample();
        let error = write_displacements(&dataset, &field, Path::new("python3"), 1, false, false)
            .unwrap_err();
        assert!(error.message.contains("unknown rotations"));
    }

    #[test]
    fn malformed_public_field_fails_before_launch() {
        let (dataset, mut field) = sample();
        field.values.pop();
        assert_eq!(
            write_displacements(&dataset, &field, Path::new("python3"), 1, true, false)
                .unwrap_err()
                .code,
            "E_OP2"
        );
    }

    #[test]
    fn synthetic_provenance_requires_actual_zero_values() {
        let (dataset, mut field) = sample();
        field
            .components
            .extend(["R1".into(), "R2".into(), "R3".into()]);
        field.values.extend([0.0, 0.0, 0.0]);
        assert_eq!(
            write_displacements(&dataset, &field, Path::new("python3"), 1, false, true)
                .unwrap_err()
                .code,
            "E_OP2"
        );
    }
}
