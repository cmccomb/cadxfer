use super::{Error, Mesh, Result};

/// Association of a numeric field with mesh entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldLocation {
    /// One value tuple per mesh point.
    Point,

    /// One value tuple per mesh cell.
    Cell,
}

/// One field at one step. Values are interleaved by entity and component.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Field name, such as `DISP`.
    pub name: String,

    /// Entity type to which values belong.
    pub location: FieldLocation,

    /// Ordered component names, such as `T1`, `T2`, `T3`.
    pub components: Vec<String>,

    /// Entity-major values: all components for entity 0, then entity 1, etc.
    pub values: Vec<f64>,

    /// Optional source step number; interpretation depends on the format.
    pub step: Option<i64>,

    /// Optional source time; units are not inferred.
    pub time: Option<f64>,
}

/// Mesh plus fields. Solver loads, constraints and material laws are outside this type.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Dataset {
    /// Geometry and original node/element identifiers.
    pub mesh: Mesh,

    /// Complete numeric fields associated with points or cells.
    pub fields: Vec<Field>,
}

impl Dataset {
    /// Check mesh invariants, field tuple lengths, and finite numeric values.
    /// Values are grouped by entity, with components in the declared order.
    /// This does not validate solver physics or format-specific representability.
    ///
    /// # Errors
    ///
    /// Returns a mesh validation error, or an error for incomplete, unnamed,
    /// or nonfinite numeric fields.
    ///
    pub fn validate(&self) -> Result<()> {
        // A field is meaningful only against a structurally valid mesh.
        self.mesh.validate()?;
        for field in &self.fields {
            if field.name.is_empty() || field.components.is_empty() {
                return Err(Error::new(
                    "E_FIELD",
                    "field name and components must be nonempty",
                ));
            }

            // Values are stored entity-major, so tuple width times the
            // location's entity count determines the only valid array length.
            let entities = match field.location {
                FieldLocation::Point => self.mesh.points.len(),
                FieldLocation::Cell => self.mesh.cells.len(),
            };
            if field.values.len() != entities.saturating_mul(field.components.len()) {
                return Err(Error::new(
                    "E_FIELD",
                    format!("field {} has an invalid value count", field.name),
                ));
            }

            // Reject nonfinite payloads and time metadata before serialization.
            if !field.values.iter().all(|value| value.is_finite())
                || field.time.is_some_and(|time| !time.is_finite())
            {
                return Err(Error::new(
                    "E_NONFINITE",
                    format!("field {} contains a nonfinite value", field.name),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Dataset, Field, FieldLocation, Mesh};
    use crate::core::Point;

    /// Reject malformed field metadata, tuple lengths, and nonfinite values.
    #[test]
    fn field_validation_rejects_incomplete_or_nonfinite_data() {
        let mut dataset = Dataset {
            mesh: Mesh {
                points: vec![Point {
                    id: 1,
                    position: [0.0; 3],
                }],
                ..Mesh::default()
            },
            fields: vec![Field {
                name: "DISP".into(),
                location: FieldLocation::Point,
                components: vec!["T1".into()],
                values: vec![0.0],
                step: None,
                time: None,
            }],
        };
        assert!(dataset.validate().is_ok());

        dataset.fields[0].name.clear();
        assert_eq!(dataset.validate().unwrap_err().code, "E_FIELD");
        dataset.fields[0].name = "DISP".into();
        dataset.fields[0].components.clear();
        assert_eq!(dataset.validate().unwrap_err().code, "E_FIELD");
        dataset.fields[0].components.push("T1".into());
        dataset.fields[0].values.clear();
        assert_eq!(dataset.validate().unwrap_err().code, "E_FIELD");
        dataset.fields[0].values.push(f64::INFINITY);
        assert_eq!(dataset.validate().unwrap_err().code, "E_NONFINITE");
        dataset.fields[0].values[0] = 0.0;
        dataset.fields[0].time = Some(f64::NAN);
        assert_eq!(dataset.validate().unwrap_err().code, "E_NONFINITE");
    }
}
