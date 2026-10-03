//! Shared association of Nastran GRID displacement rows with a companion mesh.
use crate::core::{Dataset, Error, Field, FieldLocation, Mesh, Result};
use std::collections::BTreeMap;

/// Build one normalized displacement field after a format-specific decoder.
pub(crate) fn displacement_dataset(
    mesh: &Mesh,
    rows: &BTreeMap<u64, [f64; 6]>,
    width: usize,
    subcase: i64,
    step: usize,
    time: Option<f64>,
    code: &'static str,
) -> Result<Dataset> {
    mesh.validate()?;
    if !matches!(width, 3 | 6) || rows.len() != mesh.points.len() {
        return Err(Error::new(
            code,
            "displacement nodes do not match mesh nodes",
        ));
    }
    let capacity = mesh
        .points
        .len()
        .checked_mul(width)
        .ok_or_else(|| Error::new(code, "displacement value count overflows"))?;
    let mut values = Vec::with_capacity(capacity);
    for point in &mesh.points {
        let row = rows
            .get(&point.id)
            .ok_or_else(|| Error::new(code, format!("no displacement for GRID {}", point.id)))?;
        values.extend_from_slice(&row[..width]);
    }
    let step = i64::try_from(step).map_err(|_| Error::new(code, "result step exceeds Int64"))?;
    let components = ["T1", "T2", "T3", "R1", "R2", "R3"]
        .into_iter()
        .take(width)
        .map(str::to_owned)
        .collect();
    let dataset = Dataset {
        mesh: mesh.clone(),
        fields: vec![Field {
            name: format!("DISPLACEMENT_SUBCASE_{subcase}"),
            location: FieldLocation::Point,
            components,
            values,
            step: Some(step),
            time,
        }],
    };
    dataset.validate()?;
    Ok(dataset)
}
