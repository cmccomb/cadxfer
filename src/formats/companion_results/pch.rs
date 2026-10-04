//! Bounded real SORT1 Nastran punch displacement reader.
//!
//! Only GRID displacement rows with three translations and optional three
//! rotations are projected. A matching basic-frame mesh is required.

use super::nastran_result;
use crate::core::{Dataset, Error, Mesh, Result};
use std::collections::BTreeMap;

/// One selected punch result and count of other result blocks skipped.
#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    /// Companion mesh and one normalized displacement field.
    pub dataset: Dataset,

    /// Number of other result-block headers not projected.
    pub skipped_blocks: usize,
}

/// One GRID row before insertion into a complete result step.
struct Row {
    /// Original positive GRID identifier.
    id: u64,

    /// Three or six displacement values.
    values: [f64; 6],

    /// Number of known components.
    width: usize,
}

/// One real displacement block awaiting selection.
struct Block {
    /// Positive subcase from the block header.
    subcase: Option<i64>,

    /// Optional transient time from the block header.
    time: Option<f64>,

    /// Whether `$REAL OUTPUT` was seen.
    real_output: bool,

    /// Number of components, consistent across rows.
    width: Option<usize>,

    /// Values keyed by original GRID ID.
    rows: BTreeMap<u64, [f64; 6]>,

    /// Row waiting for its optional `-CONT-` rotations.
    pending: Option<Row>,
}

impl Block {
    /// Start a new block with no asserted result metadata.
    fn new() -> Self {
        Self {
            subcase: None,
            time: None,
            real_output: false,
            width: None,
            rows: BTreeMap::new(),
            pending: None,
        }
    }

    /// Insert the pending row after checking component width and ID uniqueness.
    fn flush(&mut self) -> Result<()> {
        if let Some(row) = self.pending.take() {
            if self.width.is_some_and(|width| width != row.width) {
                return Err(err("mixed three- and six-component PCH displacement rows"));
            }
            self.width = Some(row.width);
            if self.rows.insert(row.id, row.values).is_some() {
                return Err(err("duplicate PCH displacement GRID"));
            }
        }
        Ok(())
    }

    /// Require complete metadata and push a finished nonempty result step.
    fn finish(mut self, cases: &mut BTreeMap<i64, Vec<Self>>) -> Result<()> {
        self.flush()?;
        if self.rows.is_empty() {
            return Err(err("empty PCH displacement block"));
        }
        if !self.real_output {
            return Err(err("PCH displacement block lacks $REAL OUTPUT"));
        }
        let subcase = self
            .subcase
            .ok_or_else(|| err("PCH displacement block lacks subcase"))?;
        cases.entry(subcase).or_default().push(self);
        Ok(())
    }
}

/// Construct a PCH-specific diagnostic.
fn err(message: impl Into<String>) -> Error {
    Error::new("E_PCH", message)
}

/// Parse one finite real with Nastran `D` or conventional `E` exponent.
fn real(token: &str) -> Result<f64> {
    let normalized = token.replace(['D', 'd'], "E");
    let value: f64 = normalized
        .parse()
        .map_err(|_| err("invalid PCH real value"))?;
    if !value.is_finite() {
        return Err(err("nonfinite PCH real value"));
    }
    Ok(value)
}

/// Parse a valued `$KEY = value` header, ignoring the optional sequence column.
fn header_value<'a>(line: &'a str, name: &str) -> Result<&'a str> {
    let (_, tail) = line
        .split_once('=')
        .ok_or_else(|| err(format!("missing {name} value")))?;
    tail.split_whitespace()
        .next()
        .ok_or_else(|| err(format!("missing {name} value")))
}

/// Parse one first-line GRID record with three translations.
fn row(line: &str) -> Result<Row> {
    let words: Vec<_> = line.split_whitespace().collect();
    if !matches!(words.len(), 5 | 6) || words[1] != "G" {
        return Err(err(
            "unsupported PCH displacement row; expected GRID and T1/T2/T3",
        ));
    }
    let id: u64 = words[0].parse().map_err(|_| err("invalid PCH GRID ID"))?;
    if id == 0 {
        return Err(err("PCH GRID ID must be positive"));
    }
    if words.len() == 6 {
        words[5]
            .parse::<u64>()
            .map_err(|_| err("invalid PCH sequence"))?;
    }
    let mut values = [0.; 6];
    for index in 0..3 {
        values[index] = real(words[index + 2])?;
    }
    Ok(Row {
        id,
        values,
        width: 3,
    })
}

/// Fill three rotations from a continuation row.
fn continuation(line: &str, pending: &mut Row) -> Result<()> {
    let words: Vec<_> = line.split_whitespace().collect();
    if !matches!(words.len(), 4 | 5) || words[0] != "-CONT-" || pending.width != 3 {
        return Err(err("invalid PCH displacement continuation"));
    }
    if words.len() == 5 {
        words[4]
            .parse::<u64>()
            .map_err(|_| err("invalid PCH sequence"))?;
    }
    for index in 0..3 {
        pending.values[index + 3] = real(words[index + 1])?;
    }
    pending.width = 6;
    Ok(())
}

/// Read one real SORT1 displacement subcase/step against a matching mesh.
///
/// The file may contain several real displacement blocks; the caller selects
/// one subcase and zero-based step when ambiguity exists. Other result types
/// are counted as omissions. Complex, modal, SORT2 and unsupported row layouts
/// fail explicitly when encountered in a displacement block.
///
/// # Errors
///
/// Returns an error for malformed/unsupported displacement blocks, ambiguous
/// selection, or a GRID set that differs from the companion mesh.
#[allow(clippy::too_many_lines)] // Header state and row continuation are parsed together.
pub fn read(
    source: &str,
    mesh: &Mesh,
    subcase: Option<i64>,
    step: Option<usize>,
) -> Result<Projection> {
    if !source.is_ascii() || source.as_bytes().contains(&0) {
        return Err(err("PCH must be ASCII text without NUL bytes"));
    }
    let mut cases: BTreeMap<i64, Vec<Block>> = BTreeMap::new();
    let mut active: Option<Block> = None;
    let mut skipped_blocks = 0usize;
    for (index, physical) in source.lines().enumerate() {
        let line = physical.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('$') {
            if line.starts_with("$TITLE")
                || line.starts_with("$SUBTITLE")
                || line.starts_with("$LABEL")
            {
                if line.starts_with("$TITLE") {
                    if let Some(block) = active.take() {
                        block
                            .finish(&mut cases)
                            .map_err(|error| error.at(index + 1))?;
                    }
                }
            } else if line.starts_with("$DISPLACEMENTS") {
                if let Some(block) = active.take() {
                    block
                        .finish(&mut cases)
                        .map_err(|error| error.at(index + 1))?;
                }
                active = Some(Block::new());
            } else if let Some(block) = active.as_mut() {
                if line.starts_with("$REAL OUTPUT") {
                    if line.contains("SEID") {
                        return Err(
                            err("superelement PCH displacement frame is unsupported").at(index + 1)
                        );
                    }
                    block.real_output = true;
                } else if line.starts_with("$SUBCASE ID") {
                    let value: i64 = header_value(line, "subcase")?
                        .parse()
                        .map_err(|_| err("invalid PCH subcase"))?;
                    if value <= 0 || block.subcase.replace(value).is_some() {
                        return Err(err("invalid or duplicate PCH subcase header").at(index + 1));
                    }
                } else if line.starts_with("$TIME") {
                    let time = real(header_value(line, "time")?)?;
                    if !block.rows.is_empty() || block.pending.is_some() {
                        let mut next = Block::new();
                        next.subcase = block.subcase;
                        next.real_output = block.real_output;
                        let old = active
                            .take()
                            .ok_or_else(|| err("missing active PCH block"))?;
                        old.finish(&mut cases)
                            .map_err(|error| error.at(index + 1))?;
                        next.time = Some(time);
                        active = Some(next);
                    } else {
                        block.time = Some(time);
                    }
                } else if line.starts_with("$COMPLEX")
                    || line.starts_with("$FREQUENCY")
                    || line.starts_with("$EIGENVALUE")
                    || line.starts_with("$POINT ID")
                    || line.starts_with("$LOAD FACTOR")
                    || line.starts_with("$SORT2")
                {
                    return Err(err("unsupported PCH displacement result variant").at(index + 1));
                } else {
                    let old = active
                        .take()
                        .ok_or_else(|| err("missing active PCH block"))?;
                    old.finish(&mut cases)
                        .map_err(|error| error.at(index + 1))?;
                    skipped_blocks += 1;
                }
            } else if !line.starts_with("$REAL OUTPUT") && !line.starts_with("$SUBCASE ID") {
                skipped_blocks += 1;
            }
            continue;
        }
        let Some(block) = active.as_mut() else {
            continue;
        };
        if !block.real_output || block.subcase.is_none() {
            return Err(
                err("PCH displacement row precedes real-output or subcase header").at(index + 1),
            );
        }
        if line.starts_with("-CONT-") {
            continuation(
                line,
                block
                    .pending
                    .as_mut()
                    .ok_or_else(|| err("orphan PCH continuation"))?,
            )
            .map_err(|error| error.at(index + 1))?;
        } else {
            block.flush().map_err(|error| error.at(index + 1))?;
            block.pending = Some(row(line).map_err(|error| error.at(index + 1))?);
        }
    }
    if let Some(block) = active {
        block.finish(&mut cases)?;
    }
    let selected_subcase = match subcase {
        Some(value) => value,
        None if cases.len() == 1 => *cases
            .keys()
            .next()
            .ok_or_else(|| err("missing PCH subcase"))?,
        None if cases.is_empty() => {
            return Err(err("PCH contains no supported displacement block"));
        }
        None => return Err(err("multiple PCH displacement subcases; select --subcase")),
    };
    let blocks = cases.get_mut(&selected_subcase).ok_or_else(|| {
        err(format!(
            "PCH subcase {selected_subcase} has no displacement block"
        ))
    })?;
    let selected_step = match step {
        Some(value) => value,
        None if blocks.len() == 1 => 0,
        None => return Err(err("multiple PCH displacement steps; select --step")),
    };
    if selected_step >= blocks.len() {
        return Err(err("PCH result step is out of range"));
    }
    let selected = blocks.swap_remove(selected_step);
    let dataset = nastran_result::displacement_dataset(
        mesh,
        &selected.rows,
        selected
            .width
            .ok_or_else(|| err("PCH displacement block lacks values"))?,
        selected_subcase,
        selected_step,
        selected.time,
        "E_PCH",
    )?;
    Ok(Projection {
        dataset,
        skipped_blocks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Point;

    /// Build a companion mesh with the exact GRID IDs needed by each table.
    fn mesh(ids: &[u64]) -> Mesh {
        Mesh {
            points: ids
                .iter()
                .map(|&id| Point {
                    id,
                    position: [0.; 3],
                })
                .collect(),
            cells: vec![],
            ..Mesh::default()
        }
    }

    /// Decode a published MSC PCH displacement layout against a matching mesh.
    #[test]
    fn published_msc_displacement_excerpt_decodes() {
        let source = include_str!("../../../tests/fixtures/msc-reference-displacement.pch");
        let result = read(source, &mesh(&[101]), None, None).unwrap();
        assert_eq!(result.dataset.fields[0].components.len(), 6);
        assert!((result.dataset.fields[0].values[1] - 0.000_999_407_5).abs() < 1e-12);
        assert_eq!(result.dataset.fields[0].time, None);
    }

    /// Select one PCH subcase and time step without mixing result rows.
    #[test]
    fn subcase_and_time_step_selection_are_explicit() {
        let source = include_str!("../../../tests/fixtures/pch-multiple.pch");
        assert!(
            read(source, &mesh(&[10, 20]), None, None)
                .unwrap_err()
                .message
                .contains("multiple PCH displacement subcases")
        );
        assert!(
            read(source, &mesh(&[10, 20]), Some(2), None)
                .unwrap_err()
                .message
                .contains("multiple PCH displacement steps")
        );
        let selected = read(source, &mesh(&[20, 10]), Some(2), Some(1)).unwrap();
        assert_eq!(selected.dataset.fields[0].time, Some(0.5));
        assert_eq!(selected.dataset.fields[0].step, Some(1));
        assert_eq!(
            &selected.dataset.fields[0].values[..6],
            &[21., 24., 27., 30., 33., 36.]
        );
    }

    /// Reject PCH displacement tables outside the supported real SORT1 subset.
    #[test]
    fn unsupported_and_incomplete_displacements_fail() {
        let source = include_str!("../../../tests/fixtures/msc-reference-displacement.pch");
        assert!(
            read(
                &source.replace("$REAL OUTPUT", "$COMPLEX OUTPUT"),
                &mesh(&[101]),
                None,
                None
            )
            .is_err()
        );
        assert!(read(&source.replace("101 G", "101 S"), &mesh(&[101]), None, None).is_err());
        assert!(
            read(
                &source.replace("-CONT-", "101 G"),
                &mesh(&[101]),
                None,
                None
            )
            .is_err()
        );
        assert!(read(source, &mesh(&[101, 102]), None, None).is_err());
        assert!(
            read(
                &source.replace("9.994075E-04", "NaN"),
                &mesh(&[101]),
                None,
                None
            )
            .is_err()
        );
    }

    /// Report skipped PCH result blocks instead of treating them as displacement.
    #[test]
    fn unrelated_result_block_is_reported() {
        let source = format!(
            "$STRESSES\n101 0.5\n{}",
            include_str!("../../../tests/fixtures/msc-reference-displacement.pch")
        );
        let projection = read(&source, &mesh(&[101]), None, None).unwrap();
        assert_eq!(projection.skipped_blocks, 1);
    }

    /// Reject malformed PCH headers and GRID displacement rows.
    #[test]
    fn invalid_headers_and_grid_records_are_rejected() {
        let source = include_str!("../../../tests/fixtures/msc-reference-displacement.pch");
        for (old, new) in [
            ("$REAL OUTPUT", "$SORT2"),
            ("$REAL OUTPUT", "$FREQUENCY = 1.0"),
            ("$REAL OUTPUT", "$REAL OUTPUT SEID = 1"),
            ("$SUBCASE ID = 1", "$SUBCASE ID = nope"),
            ("$SUBCASE ID = 1", "$SUBCASE ID = 0"),
            ("101 G", "0 G"),
            ("101 G", "101 X"),
            ("-CONT- 0.000000E+00", "-CONT- NaN"),
        ] {
            assert!(source.contains(old));
            let changed = source.replacen(old, new, 1);
            assert_eq!(
                read(&changed, &mesh(&[101]), None, None).unwrap_err().code,
                "E_PCH"
            );
        }
        let duplicate = source.replace("$SUBCASE ID = 1", "$SUBCASE ID = 1\n$SUBCASE ID = 1");
        assert_eq!(
            read(&duplicate, &mesh(&[101]), None, None)
                .unwrap_err()
                .code,
            "E_PCH"
        );
        let absent = source.replace("$SUBCASE ID = 1\n", "");
        assert_eq!(
            read(&absent, &mesh(&[101]), None, None).unwrap_err().code,
            "E_PCH"
        );
        let orphan = source.replace("101 G 0.000000E+00 9.994075E-04 0.000000E+00\n", "");
        assert_eq!(
            read(&orphan, &mesh(&[101]), None, None).unwrap_err().code,
            "E_PCH"
        );
    }

    /// Refuse transient frames with missing GRID rows rather than merging steps.
    #[test]
    fn incomplete_transient_steps_do_not_mix_grid_values() {
        let source = include_str!("../../../tests/fixtures/pch-multiple.pch");
        let mixed_width = source.replacen("-CONT- 4.0 5.0 6.0\n", "", 1);
        assert_eq!(
            read(&mixed_width, &mesh(&[10, 20]), Some(1), None)
                .unwrap_err()
                .code,
            "E_PCH"
        );
        let duplicate = source.replacen("20 G 7.0 8.0 9.0", "10 G 7.0 8.0 9.0", 1);
        assert_eq!(
            read(&duplicate, &mesh(&[10, 20]), Some(1), None)
                .unwrap_err()
                .code,
            "E_PCH"
        );
        let invalid_time = source.replace("$TIME = 0.5", "$TIME = NaN");
        assert_eq!(
            read(&invalid_time, &mesh(&[10, 20]), Some(2), Some(1))
                .unwrap_err()
                .code,
            "E_PCH"
        );
        assert_eq!(
            read(source, &mesh(&[10, 20]), Some(2), Some(99))
                .unwrap_err()
                .code,
            "E_PCH"
        );
        assert_eq!(
            read(source, &mesh(&[10, 20]), Some(99), Some(0))
                .unwrap_err()
                .code,
            "E_PCH"
        );
    }
}
