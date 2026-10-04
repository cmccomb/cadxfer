//! Read supported ASCII legacy VTK datasets.

use crate::core::{Cell, CellKind, Dataset, Error, Field, FieldLocation, Mesh, Point, Result};
use std::collections::BTreeSet;
use std::str::SplitWhitespace;

/// A legacy VTK projection and whether missing source IDs needed allocation.
#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    /// Mesh and complete numeric fields.
    pub dataset: Dataset,

    /// True when the file had no `nastran_node_id` array.
    pub generated_point_ids: bool,

    /// True when the file had no `nastran_element_id` array.
    pub generated_cell_ids: bool,
}

/// Cursor over ASCII tokens after the four-line file header.
struct Tokens<'a> {
    /// Whitespace-delimited source tokens.
    words: std::iter::Peekable<SplitWhitespace<'a>>,

    /// Upper bound for any declared item count.
    limit: usize,
}

impl<'a> Tokens<'a> {
    /// Read one required token.
    fn next(&mut self, what: &str) -> Result<&'a str> {
        self.words
            .next()
            .ok_or_else(|| err(format!("missing {what}")))
    }

    /// Read one typed token.
    fn number<T: std::str::FromStr>(&mut self, what: &str) -> Result<T> {
        self.next(what)?
            .parse()
            .map_err(|_| err(format!("invalid {what}")))
    }

    /// Consume a required keyword.
    fn expect(&mut self, keyword: &str) -> Result<()> {
        if self.next(keyword)?.eq_ignore_ascii_case(keyword) {
            Ok(())
        } else {
            Err(err(format!("expected {keyword}")))
        }
    }

    /// Reject a count too large for the source byte length.
    fn count(&mut self, what: &str) -> Result<usize> {
        let count = self.number(what)?;
        if count > self.limit {
            return Err(err(format!("{what} exceeds input size")));
        }
        Ok(count)
    }
}

/// Construct a format-specific diagnostic.
pub(super) fn err(message: impl Into<String>) -> Error {
    Error::new("E_VTK", message)
}

/// Decode a supported VTK linear cell number.
fn cell_kind(number: u32) -> Result<CellKind> {
    match number {
        3 => Ok(CellKind::Line2),
        5 => Ok(CellKind::Triangle3),
        9 => Ok(CellKind::Quad4),
        10 => Ok(CellKind::Tet4),
        12 => Ok(CellKind::Hex8),
        13 => Ok(CellKind::Wedge6),
        14 => Ok(CellKind::Pyramid5),
        _ => Err(err(format!("unsupported VTK cell type {number}"))),
    }
}

/// Read one finite numeric array without converting IDs through floating point.
#[allow(clippy::cast_precision_loss)] // Integer magnitude is bounded to f64's exact integer range.
fn floats(tokens: &mut Tokens<'_>, count: usize, kind: &str) -> Result<Vec<f64>> {
    let float_type = matches!(kind.to_ascii_lowercase().as_str(), "float" | "double");
    if !float_type
        && !matches!(
            kind.to_ascii_lowercase().as_str(),
            "int"
                | "unsigned_int"
                | "long"
                | "unsigned_long"
                | "long_long"
                | "unsigned_long_long"
                | "short"
                | "unsigned_short"
                | "char"
                | "unsigned_char"
        )
    {
        return Err(err(format!("unsupported VTK numeric type {kind}")));
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let value: f64 = if float_type {
            tokens.number("numeric value")?
        } else {
            let integer: i128 = tokens.number("integer field value")?;
            if integer.unsigned_abs() > 9_007_199_254_740_992 {
                return Err(err("integer field value cannot be represented exactly"));
            }
            integer as f64
        };
        if !value.is_finite() {
            return Err(err("nonfinite VTK value"));
        }
        values.push(value);
    }
    Ok(values)
}

/// Read a reserved original-ID array as integers.
fn ids(tokens: &mut Tokens<'_>, count: usize, kind: &str) -> Result<Vec<u64>> {
    if !integer_id_type(kind) {
        return Err(err("original IDs require an integer VTK type"));
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let value: u64 = tokens.number("original ID")?;
        if value == 0 {
            return Err(err("original IDs must be positive"));
        }
        values.push(value);
    }
    Ok(values)
}

/// Check a legacy integer type used for entity identifiers.
fn integer_id_type(kind: &str) -> bool {
    matches!(
        kind.to_ascii_lowercase().as_str(),
        "unsigned_long_long" | "unsigned_long" | "unsigned_int" | "long_long" | "long" | "int"
    )
}

/// Read one point or cell attribute section.
fn attributes(
    tokens: &mut Tokens<'_>,
    location: FieldLocation,
    count: usize,
    fields: &mut Vec<Field>,
    original_ids: &mut Option<Vec<u64>>,
    property_ids: &mut Option<Vec<u64>>,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    while let Some(&keyword) = tokens.words.peek() {
        if keyword.eq_ignore_ascii_case("POINT_DATA") || keyword.eq_ignore_ascii_case("CELL_DATA") {
            break;
        }
        let keyword = tokens.next("attribute keyword")?;
        if keyword.eq_ignore_ascii_case("FIELD") {
            let _group = tokens.next("field group name")?;
            let arrays = tokens.count("field array count")?;
            for _ in 0..arrays {
                let name = tokens.next("array name")?;
                let components = tokens.count("component count")?;
                let tuples = tokens.count("tuple count")?;
                let kind = tokens.next("array type")?;
                if components == 0 || tuples != count {
                    return Err(err("field array has wrong tuple count or zero components"));
                }
                let length = tuples
                    .checked_mul(components)
                    .ok_or_else(|| err("field size overflow"))?;
                if length > tokens.limit {
                    return Err(err("field size exceeds input"));
                }
                add_array(
                    tokens,
                    location,
                    name,
                    components,
                    length,
                    kind,
                    fields,
                    original_ids,
                    property_ids,
                    &mut seen,
                )?;
            }
        } else if keyword.eq_ignore_ascii_case("SCALARS") {
            let name = tokens.next("scalar name")?;
            let kind = tokens.next("scalar type")?;
            let components = if tokens
                .words
                .peek()
                .is_some_and(|word| word.eq_ignore_ascii_case("LOOKUP_TABLE"))
            {
                1
            } else {
                tokens.count("scalar components")?
            };
            if !(1..=4).contains(&components) {
                return Err(err("legacy SCALARS supports one to four components"));
            }
            tokens.expect("LOOKUP_TABLE")?;
            tokens.expect("default")?;
            let length = count
                .checked_mul(components)
                .ok_or_else(|| err("scalar size overflow"))?;
            add_array(
                tokens,
                location,
                name,
                components,
                length,
                kind,
                fields,
                original_ids,
                property_ids,
                &mut seen,
            )?;
        } else if keyword.eq_ignore_ascii_case("VECTORS") {
            let name = tokens.next("vector name")?;
            let kind = tokens.next("vector type")?;
            let length = count
                .checked_mul(3)
                .ok_or_else(|| err("vector size overflow"))?;
            add_array(
                tokens,
                location,
                name,
                3,
                length,
                kind,
                fields,
                original_ids,
                property_ids,
                &mut seen,
            )?;
        } else {
            return Err(err(format!("unsupported VTK attribute {keyword}")));
        }
    }
    Ok(())
}

/// Attach an attribute to its entity location, reserving integer ID arrays.
#[allow(clippy::too_many_arguments)] // Array metadata and three destinations are kept together.
fn add_array(
    tokens: &mut Tokens<'_>,
    location: FieldLocation,
    name: &str,
    components: usize,
    length: usize,
    kind: &str,
    fields: &mut Vec<Field>,
    original_ids: &mut Option<Vec<u64>>,
    property_ids: &mut Option<Vec<u64>>,
    seen: &mut BTreeSet<String>,
) -> Result<()> {
    if !seen.insert(name.to_owned()) {
        return Err(err(format!("duplicate VTK array {name}")));
    }
    if (location == FieldLocation::Point && name == "nastran_node_id")
        || (location == FieldLocation::Cell && name == "nastran_element_id")
    {
        if components != 1 || original_ids.is_some() {
            return Err(err("invalid original ID array"));
        }
        *original_ids = Some(ids(tokens, length, kind)?);
    } else if location == FieldLocation::Cell && name == "nastran_property_id" {
        if components != 1 || property_ids.is_some() || !integer_id_type(kind) {
            return Err(err("invalid property ID array"));
        }
        // Zero is the documented sentinel for an absent property.
        let mut values = Vec::with_capacity(length);
        for _ in 0..length {
            values.push(tokens.number("property ID")?);
        }
        *property_ids = Some(values);
    } else {
        let values = floats(tokens, length, kind)?;
        fields.push(Field {
            name: name.to_owned(),
            location,
            components: (1..=components).map(|index| format!("C{index}")).collect(),
            values,
            step: None,
            time: None,
        });
    }
    Ok(())
}

/// Decode either classic counted cells or VTK 5.1 offsets/connectivity cells.
fn cells(tokens: &mut Tokens<'_>) -> Result<Vec<Vec<usize>>> {
    tokens.expect("CELLS")?;
    let first = tokens.count("cell count")?;
    let second = tokens.count("cell list size")?;
    if tokens
        .words
        .peek()
        .is_some_and(|word| word.eq_ignore_ascii_case("OFFSETS"))
    {
        if first == 0 {
            return Err(err("offset list must contain at least one entry"));
        }
        tokens.expect("OFFSETS")?;
        let integer_type = tokens.next("offset type")?;
        if !matches!(
            integer_type.to_ascii_lowercase().as_str(),
            "vtktypeint64" | "int" | "long" | "long_long"
        ) {
            return Err(err("unsupported VTK offsets type"));
        }
        let mut offsets = Vec::with_capacity(first);
        for _ in 0..first {
            offsets.push(tokens.number::<usize>("cell offset")?);
        }
        if offsets[0] != 0
            || offsets.last().copied() != Some(second)
            || offsets
                .windows(2)
                .any(|pair| pair[0] >= pair[1] || pair[1] - pair[0] > 8)
        {
            return Err(err("invalid VTK cell offsets"));
        }
        tokens.expect("CONNECTIVITY")?;
        let integer_type = tokens.next("connectivity type")?;
        if !matches!(
            integer_type.to_ascii_lowercase().as_str(),
            "vtktypeint64" | "int" | "long" | "long_long"
        ) {
            return Err(err("unsupported VTK connectivity type"));
        }
        let mut indices = Vec::with_capacity(second);
        for _ in 0..second {
            indices.push(tokens.number::<usize>("point index")?);
        }
        Ok(offsets
            .windows(2)
            .map(|pair| indices[pair[0]..pair[1]].to_vec())
            .collect())
    } else {
        let mut connections = Vec::with_capacity(first);
        let mut consumed = 0usize;
        for _ in 0..first {
            let size = tokens.count("cell width")?;
            if size > 8 {
                return Err(err("unsupported higher-order cell width"));
            }
            consumed = consumed
                .checked_add(size + 1)
                .ok_or_else(|| err("cell list overflow"))?;
            if consumed > second {
                return Err(err("cell list exceeds declared size"));
            }
            let mut connection = Vec::with_capacity(size);
            for _ in 0..size {
                connection.push(tokens.number("point index")?);
            }
            connections.push(connection);
        }
        if consumed != second {
            return Err(err("cell list size mismatch"));
        }
        Ok(connections)
    }
}

/// Read one ASCII legacy VTK unstructured grid and report allocated IDs.
///
/// # Errors
///
/// Returns an error for unsupported syntax, invalid counts or connectivity,
/// nonfinite values, duplicate IDs, or unsupported topology.
#[allow(clippy::too_many_lines)] // A legacy grid has ordered geometry and attribute sections.
pub fn read_projection(source: &str) -> Result<Projection> {
    if !source.is_ascii() || source.as_bytes().contains(&0) {
        return Err(err("legacy VTK input must be ASCII without NUL bytes"));
    }
    let mut lines = source.lines();
    let signature = lines.next().ok_or_else(|| err("missing VTK header"))?;
    if !signature.trim_end().starts_with("# vtk DataFile Version ") {
        return Err(err("unsupported VTK file header"));
    }
    let _title = lines.next().ok_or_else(|| err("missing VTK title"))?;
    if !lines
        .next()
        .ok_or_else(|| err("missing VTK encoding"))?
        .trim()
        .eq_ignore_ascii_case("ASCII")
    {
        return Err(err("only ASCII legacy VTK is supported"));
    }
    let body = lines.collect::<Vec<_>>().join("\n");
    let mut tokens = Tokens {
        words: body.split_whitespace().peekable(),
        limit: source.len() / 2,
    };
    tokens.expect("DATASET")?;
    tokens.expect("UNSTRUCTURED_GRID")?;
    tokens.expect("POINTS")?;
    let point_count = tokens.count("point count")?;
    let point_type = tokens.next("point type")?;
    if !matches!(point_type.to_ascii_lowercase().as_str(), "float" | "double") {
        return Err(err("POINTS require float or double coordinates"));
    }
    let coordinate_count = point_count
        .checked_mul(3)
        .ok_or_else(|| err("coordinate count overflow"))?;
    if coordinate_count > tokens.limit {
        return Err(err("coordinate count exceeds input"));
    }
    let coords = floats(&mut tokens, coordinate_count, point_type)?;
    let connections = cells(&mut tokens)?;
    let cell_count = connections.len();
    tokens.expect("CELL_TYPES")?;
    if tokens.count("cell type count")? != cell_count {
        return Err(err("cell type count mismatch"));
    }
    let mut kinds = Vec::with_capacity(cell_count);
    for _ in 0..cell_count {
        kinds.push(cell_kind(tokens.number("cell type")?)?);
    }
    let mut fields = Vec::new();
    let mut point_ids = None;
    let mut cell_ids = None;
    let mut properties = None;
    let mut locations = BTreeSet::new();
    while let Some(&keyword) = tokens.words.peek() {
        let location = if keyword.eq_ignore_ascii_case("POINT_DATA") {
            FieldLocation::Point
        } else if keyword.eq_ignore_ascii_case("CELL_DATA") {
            FieldLocation::Cell
        } else {
            return Err(err(format!("unsupported VTK section {keyword}")));
        };
        if !locations.insert(location as u8) {
            return Err(err("duplicate VTK data section"));
        }
        tokens.next("data section")?;
        let expected = if location == FieldLocation::Point {
            point_count
        } else {
            cell_count
        };
        if tokens.count("data tuple count")? != expected {
            return Err(err("data tuple count mismatch"));
        }
        let original = if location == FieldLocation::Point {
            &mut point_ids
        } else {
            &mut cell_ids
        };
        attributes(
            &mut tokens,
            location,
            expected,
            &mut fields,
            original,
            &mut properties,
        )?;
    }
    let generated_point_ids = point_ids.is_none();
    let generated_cell_ids = cell_ids.is_none();
    let point_ids = point_ids.unwrap_or_else(|| (1..=point_count as u64).collect());
    let cell_ids = cell_ids.unwrap_or_else(|| (1..=cell_count as u64).collect());
    let points = coords
        .chunks_exact(3)
        .zip(point_ids)
        .map(|(coord, id)| Point {
            id,
            position: [coord[0], coord[1], coord[2]],
        })
        .collect();
    let cells = connections
        .into_iter()
        .zip(kinds)
        .zip(cell_ids)
        .enumerate()
        .map(|(index, ((connectivity, kind), id))| {
            if connectivity.len() != kind.node_count() {
                return Err(err("cell arity does not match CELL_TYPES"));
            }
            Ok(Cell {
                id,
                kind,
                connectivity,
                property_id: properties
                    .as_ref()
                    .and_then(|ids| ids.get(index))
                    .copied()
                    .filter(|id| *id != 0),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let dataset = Dataset {
        mesh: Mesh {
            points,
            cells,
            ..Mesh::default()
        },
        fields,
    };
    dataset.validate()?;
    Ok(Projection {
        dataset,
        generated_point_ids,
        generated_cell_ids,
    })
}

/// Read one ASCII legacy VTK unstructured grid.
///
/// IDs absent from the source are allocated deterministically from one. Use
/// [`read_projection`] when this distinction matters to the caller.
///
/// # Errors
///
/// Returns an error for malformed or unsupported input.
#[cfg(test)]
pub fn read(source: &str) -> Result<Dataset> {
    Ok(read_projection(source)?.dataset)
}
