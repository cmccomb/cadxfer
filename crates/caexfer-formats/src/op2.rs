//! Optional pyNastran-backed OP2 displacement adapter.
//! A matching basic-frame BDF mesh is required; no binary record guesswork.
use caexfer_core::{Dataset, Error, Field, FieldLocation, Mesh, Result};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

/// Decode one real six-component displacement table using the installed
/// pyNastran Python package. `step` is a zero-based index within the subcase.
pub fn read_displacements(
    path: &Path,
    mesh: &Mesh,
    python: &Path,
    subcase: Option<i64>,
    step: Option<usize>,
) -> Result<Dataset> {
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
    if parts.len() != 5 || parts[0] != "OK" {
        return Err(Error::new("E_OP2", "invalid extractor header"));
    }
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
    Ok(dataset)
}
